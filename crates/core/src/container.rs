//! Server-owned container inventory operations. Client hashes are predictions, not items.
use mchprs_blocks::block_entities::{ContainerType, InventoryEntry};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::BlockPos;
use mchprs_network::packets::{PacketEncoderExt, SlotData};

#[derive(PartialEq, Eq)]
enum ClickAction {
    Pickup,
    QuickMove,
    Clone,
    Swap,
    Throw,
    Drag,
    Collect,
}

impl ClickAction {
    fn from_protocol(mode: i32) -> Option<Self> {
        match mode {
            0 => Some(Self::Pickup),
            1 => Some(Self::QuickMove),
            2 => Some(Self::Clone),
            3 => Some(Self::Swap),
            4 => Some(Self::Throw),
            5 => Some(Self::Drag),
            6 => Some(Self::Collect),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MouseButton {
    Left,
    Right,
}

impl MouseButton {
    fn from_protocol(button: i8) -> Option<Self> {
        match button {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(serde::Deserialize)]
struct FurnaceRules {
    fuel: Vec<String>,
    smeltable: Vec<String>,
}

impl FurnaceRules {
    fn get() -> &'static Self {
        static RULES: std::sync::OnceLock<FurnaceRules> = std::sync::OnceLock::new();

        RULES.get_or_init(|| {
            serde_json::from_str(include_str!("../../../mc_data/1.21.5/furnace_menu.json"))
                .expect("checked-in Java furnace menu rules")
        })
    }

    fn is_fuel(&self, item: &ItemStack) -> bool {
        let name = item.item_type.get_name();
        self.fuel.iter().any(|fuel| fuel == name)
    }

    fn is_smeltable(&self, item: &ItemStack) -> bool {
        let name = item.item_type.get_name();
        self.smeltable.iter().any(|smeltable| smeltable == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Even,
    OneEach,
    CreativeFill,
}

enum DragPhase {
    Start,
    AddSlot,
    Finish,
}

pub(crate) fn eat_cake(world: &mut impl crate::world::World, pos: BlockPos) {
    let Block::Cake { bites } = world.get_block(pos) else {
        return;
    };
    if bites >= 6 {
        crate::interaction::destroy(Block::Cake { bites }, world, pos);
        return;
    }

    world.set_block(pos, Block::Cake { bites: bites + 1 });
    crate::redstone::update_surrounding_blocks(world, pos);
}

pub(crate) fn set_barrel_open(world: &mut impl crate::world::World, pos: BlockPos, open: bool) {
    let Block::Barrel {
        facing,
        open: previous,
    } = world.get_block(pos)
    else {
        return;
    };
    if previous == open {
        return;
    }

    world.set_block(pos, Block::Barrel { facing, open });
    crate::sound::play(
        world,
        pos,
        if open {
            "block.barrel.open"
        } else {
            "block.barrel.close"
        },
        0.5,
        0.95,
        None,
    );
    crate::redstone::update_surrounding_blocks(world, pos);
}

/// Java chooses the largest look-vector component, including diagonal vertical views.
pub(crate) fn barrel_facing(yaw: f32, pitch: f32) -> mchprs_blocks::BlockFacing {
    use mchprs_blocks::BlockFacing;
    let yaw = yaw.to_radians();
    let pitch = pitch.to_radians();
    let x = -yaw.sin() * pitch.cos();
    let y = -pitch.sin();
    let z = yaw.cos() * pitch.cos();
    let look = if y.abs() > x.abs().max(z.abs()) {
        if y > 0.0 {
            BlockFacing::Up
        } else {
            BlockFacing::Down
        }
    } else if x.abs() > z.abs() {
        if x > 0.0 {
            BlockFacing::East
        } else {
            BlockFacing::West
        }
    } else if z > 0.0 {
        BlockFacing::South
    } else {
        BlockFacing::North
    };
    look.opposite()
}

pub(crate) fn comparator_strength(slots: &[Option<ItemStack>]) -> u8 {
    let fullness: f32 = slots
        .iter()
        .flatten()
        .map(|item| item.count as f32 / max_count(item) as f32)
        .sum();
    let scaled_fullness = (fullness / slots.len() as f32 * 14.0).floor() as u8;
    let occupied_bonus = u8::from(slots.iter().any(Option::is_some));

    (scaled_fullness + occupied_bonus).min(15)
}

pub(crate) struct OpenContainer {
    pub pos: BlockPos,
    pub ty: ContainerType,
    pub window_id: u8,
    pub state_id: i32,
    pub cursor: Option<ItemStack>,
    pub drag: Option<Drag>,
    pub last_contents: Vec<u8>,
}

pub(crate) struct Drag {
    kind: DragKind,
    slots: Vec<usize>,
}

pub(crate) fn slot_data(item: &ItemStack) -> SlotData {
    SlotData {
        item_id: item.item_type.get_id() as i32,
        item_count: item.count as i8,
        nbt: item.nbt.clone(),
    }
}

pub(crate) fn inventory_slots(entries: &[InventoryEntry], count: usize) -> Vec<Option<ItemStack>> {
    let mut slots = vec![None; count];
    for entry in entries {
        if entry.slot < 0 || entry.slot as usize >= count || entry.count <= 0 {
            continue;
        }
        let nbt = match &entry.nbt {
            Some(bytes) => match nbt::Blob::from_reader(&mut std::io::Cursor::new(bytes)) {
                Ok(blob) => Some(blob),
                Err(_) => continue,
            },
            None => None,
        };
        slots[entry.slot as usize] = Some(ItemStack {
            item_type: Item::from_id(entry.id),
            count: entry.count as u8,
            nbt,
        });
    }
    slots
}

pub(crate) fn inventory_entries(slots: &[Option<ItemStack>]) -> Vec<InventoryEntry> {
    slots
        .iter()
        .enumerate()
        .filter_map(|(slot, item)| {
            let item = item.as_ref()?;
            let nbt = item.nbt.as_ref().map(|blob| {
                let mut bytes = Vec::new();
                blob.to_writer(&mut bytes).expect("validated item NBT");
                bytes
            });
            Some(InventoryEntry {
                slot: slot as i8,
                count: item.count as i8,
                id: item.item_type.get_id(),
                nbt,
            })
        })
        .collect()
}

pub(crate) fn max_count(item: &ItemStack) -> u8 {
    mchprs_network::packets::components::max_stack_size(
        &item.nbt,
        item.item_type.max_stack_size() as u8,
    )
}

fn may_place(ty: ContainerType, slot: usize, item: &ItemStack) -> bool {
    ty != ContainerType::Furnace
        || match slot {
            1 => FurnaceRules::get().is_fuel(item) || item.item_type.get_name() == "bucket",
            2 => false,
            _ => true,
        }
}

fn slot_limit(ty: Option<ContainerType>, slot: usize, item: &ItemStack) -> u8 {
    if ty == Some(ContainerType::Furnace) && slot == 1 && item.item_type.get_name() == "bucket" {
        1
    } else {
        max_count(item)
    }
}

pub(crate) fn same_item(a: &ItemStack, b: &ItemStack) -> bool {
    if a.item_type != b.item_type {
        return false;
    }
    let encode = |item: &ItemStack| {
        let mut bytes = Vec::new();
        let mut slot = slot_data(item);
        slot.item_count = 1;
        bytes.write_slot_data(&Some(slot));
        bytes
    };
    encode(a) == encode(b)
}

pub(crate) fn signature(slots: &[Option<ItemStack>], cursor: &Option<ItemStack>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for item in slots.iter().chain(std::iter::once(cursor)) {
        bytes.write_slot_data(&item.as_ref().map(slot_data));
    }
    bytes
}

/// Merge into occupied slots first, then empty slots, in the requested order.
pub(crate) fn insert(
    slots: &mut [Option<ItemStack>],
    item: &mut Option<ItemStack>,
    indices: &[usize],
) {
    insert_with_rules(slots, item, indices, None);
}

fn insert_with_rules(
    slots: &mut [Option<ItemStack>],
    item: &mut Option<ItemStack>,
    indices: &[usize],
    ty: Option<ContainerType>,
) {
    for empty in [false, true] {
        for &index in indices {
            let Some(source) = item.as_mut() else {
                return;
            };
            if ty.is_some_and(|ty| !may_place(ty, index, source)) {
                continue;
            }
            let limit = slot_limit(ty, index, source);
            let dest = &mut slots[index];
            let amount = match dest {
                Some(dest) if !empty && same_item(dest, source) => {
                    source.count.min(limit.saturating_sub(dest.count))
                }
                None if empty => source.count.min(limit),
                _ => 0,
            };
            if amount == 0 {
                continue;
            }
            if let Some(dest) = dest {
                dest.count += amount;
            } else {
                let mut placed = source.clone();
                placed.count = amount;
                *dest = Some(placed);
            }
            source.count -= amount;
            if source.count == 0 {
                *item = None;
            }
        }
    }
}

fn pickup(
    slot: &mut Option<ItemStack>,
    cursor: &mut Option<ItemStack>,
    button: MouseButton,
    limit: u8,
) {
    match (slot.as_mut(), cursor.as_mut()) {
        (Some(item), None) => {
            let amount = match button {
                MouseButton::Right => item.count.div_ceil(2),
                MouseButton::Left => item.count,
            };
            let mut taken = item.clone();
            taken.count = amount;
            item.count -= amount;
            if item.count == 0 {
                *slot = None;
            }
            *cursor = Some(taken);
        }
        (None, Some(item)) => {
            let placement_limit = match button {
                MouseButton::Right => 1,
                MouseButton::Left => limit,
            };
            let amount = item.count.min(placement_limit);
            let mut placed = item.clone();
            placed.count = amount;
            item.count -= amount;
            *slot = Some(placed);
            if item.count == 0 {
                *cursor = None;
            }
        }
        (Some(dest), Some(source)) if same_item(dest, source) => {
            let requested = match button {
                MouseButton::Right => 1,
                MouseButton::Left => source.count,
            };
            let room = limit.saturating_sub(dest.count);
            let amount = source.count.min(requested).min(room);
            dest.count += amount;
            source.count -= amount;
            if source.count == 0 {
                *cursor = None;
            }
        }
        (Some(_), Some(source)) if source.count <= limit => std::mem::swap(slot, cursor),
        _ => {}
    }
}

/// Container slots are followed by main inventory and hotbar (36 slots).
pub(crate) fn click(
    menu: &mut OpenContainer,
    slots: &mut [Option<ItemStack>],
    offhand: &mut Option<ItemStack>,
    slot: i16,
    button: i8,
    mode: i32,
    creative: bool,
) {
    let size = menu.ty.num_slots() as usize;
    if slots.len() != size + 36 {
        return;
    }
    let index = usize::try_from(slot).ok().filter(|&i| i < slots.len());
    let action = ClickAction::from_protocol(mode);
    if action != Some(ClickAction::Drag) {
        menu.drag = None;
    }
    let Some(action) = action else {
        return;
    };

    match action {
        ClickAction::Pickup => {
            let Some(button) = MouseButton::from_protocol(button) else {
                return;
            };

            if let Some(i) = index {
                if menu
                    .cursor
                    .as_ref()
                    .is_none_or(|item| may_place(menu.ty, i, item))
                {
                    let limit = menu
                        .cursor
                        .as_ref()
                        .or(slots[i].as_ref())
                        .map_or(64, |item| slot_limit(Some(menu.ty), i, item));
                    pickup(&mut slots[i], &mut menu.cursor, button, limit);
                } else if let (Some(dest), Some(cursor)) = (&mut slots[i], &mut menu.cursor) {
                    // Output slots allow collecting into a compatible carried stack.
                    if same_item(dest, cursor) {
                        let amount = dest
                            .count
                            .min(max_count(cursor).saturating_sub(cursor.count));
                        cursor.count += amount;
                        dest.count -= amount;
                        if dest.count == 0 {
                            slots[i] = None;
                        }
                    }
                }
            } else if slot == -999 {
                // MCHPRS has no dropped-item entities; creative outside clicks discard items.
                if button == MouseButton::Left {
                    menu.cursor = None;
                } else if let Some(item) = &mut menu.cursor {
                    item.count -= 1;
                    if item.count == 0 {
                        menu.cursor = None;
                    }
                }
            }
        }
        ClickAction::QuickMove if (0..=1).contains(&button) => {
            if let Some(i) = index {
                let mut item = slots[i].take();
                let range = shift_click_destinations(menu.ty, slots.len(), i, item.as_ref());

                insert_with_rules(slots, &mut item, &range, Some(menu.ty));
                slots[i] = item;
            }
        }
        ClickAction::Clone if button == 2 && creative => {
            if let Some(i) = index {
                if menu.cursor.is_none() {
                    menu.cursor = slots[i].clone().map(|mut item| {
                        item.count = max_count(&item);
                        item
                    });
                }
            }
        }
        ClickAction::Swap if (0..=8).contains(&button) || button == 40 => {
            if let Some(i) = index {
                let hotbar = size + 27 + button.clamp(0, 8) as usize;
                let incoming = if button == 40 {
                    offhand.as_ref()
                } else {
                    slots[hotbar].as_ref()
                };
                if incoming.is_some_and(|item| {
                    !may_place(menu.ty, i, item) || item.count > slot_limit(Some(menu.ty), i, item)
                }) {
                    return;
                }
                if button == 40 {
                    std::mem::swap(&mut slots[i], offhand);
                } else {
                    slots.swap(i, size + 27 + button as usize);
                }
            }
        }
        ClickAction::Throw if (0..=1).contains(&button) && menu.cursor.is_none() => {
            if let Some(i) = index {
                if button == 1 {
                    slots[i] = None;
                } else if let Some(item) = &mut slots[i] {
                    item.count -= 1;
                    if item.count == 0 {
                        slots[i] = None;
                    }
                }
            }
        }
        ClickAction::Drag => {
            drag_click(menu, slots, index, button, creative);
        }
        ClickAction::Collect if (0..=1).contains(&button) => {
            collect_matching_items(slots, &mut menu.cursor, button);
        }
        _ => menu.drag = None,
    }
}

fn shift_click_destinations(
    ty: ContainerType,
    slot_count: usize,
    source: usize,
    item: Option<&ItemStack>,
) -> Vec<usize> {
    let container_size = ty.num_slots() as usize;
    let hotbar_start = container_size + 27;

    if ty == ContainerType::Furnace && source >= container_size {
        if item.is_some_and(|item| FurnaceRules::get().is_smeltable(item)) {
            return vec![0];
        }
        if item.is_some_and(|item| FurnaceRules::get().is_fuel(item)) {
            return vec![1];
        }
        if source < hotbar_start {
            return (hotbar_start..slot_count).collect();
        }

        return (container_size..hotbar_start).collect();
    }

    if source < container_size {
        return (container_size..slot_count).rev().collect();
    }

    (0..container_size)
        .filter(|&slot| ty != ContainerType::Furnace || slot != 2)
        .collect()
}

fn decode_drag_button(button: i8) -> Option<(DragKind, DragPhase)> {
    if !(0..=10).contains(&button) {
        return None;
    }

    let kind = match (button as u8 >> 2) & 3 {
        0 => DragKind::Even,
        1 => DragKind::OneEach,
        2 => DragKind::CreativeFill,
        _ => return None,
    };
    let phase = match button as u8 & 3 {
        0 => DragPhase::Start,
        1 => DragPhase::AddSlot,
        2 => DragPhase::Finish,
        _ => return None,
    };

    Some((kind, phase))
}

fn drag_click(
    menu: &mut OpenContainer,
    slots: &mut [Option<ItemStack>],
    index: Option<usize>,
    button: i8,
    creative: bool,
) {
    let Some((kind, phase)) = decode_drag_button(button) else {
        menu.drag = None;
        return;
    };
    if (kind == DragKind::CreativeFill && !creative) || menu.cursor.is_none() {
        menu.drag = None;
        return;
    }

    match phase {
        DragPhase::Start => {
            menu.drag = Some(Drag {
                kind,
                slots: Vec::new(),
            });
        }
        DragPhase::AddSlot => add_drag_slot(menu, slots, index, kind),
        DragPhase::Finish => finish_drag(menu, slots, kind),
    }
}

fn add_drag_slot(
    menu: &mut OpenContainer,
    slots: &[Option<ItemStack>],
    index: Option<usize>,
    kind: DragKind,
) {
    let (Some(index), Some(drag), Some(item)) = (index, &mut menu.drag, &menu.cursor) else {
        return;
    };
    if drag.kind != kind {
        menu.drag = None;
        return;
    }
    if drag.slots.contains(&index) || !may_place(menu.ty, index, item) {
        return;
    }
    if kind != DragKind::CreativeFill && item.count as usize <= drag.slots.len() {
        return;
    }
    if slots[index].as_ref().is_some_and(|present| {
        !same_item(present, item) || present.count >= slot_limit(Some(menu.ty), index, present)
    }) {
        return;
    }

    drag.slots.push(index);
}

fn finish_drag(menu: &mut OpenContainer, slots: &mut [Option<ItemStack>], kind: DragKind) {
    let Some(drag) = menu.drag.take() else {
        return;
    };
    if drag.kind != kind || drag.slots.is_empty() {
        return;
    }
    let Some(mut item) = menu.cursor.take() else {
        return;
    };
    let each = match kind {
        DragKind::Even => (item.count as usize / drag.slots.len()) as u8,
        DragKind::OneEach | DragKind::CreativeFill => 1,
    };

    for index in drag.slots {
        if slots[index]
            .as_ref()
            .is_some_and(|present| !same_item(present, &item))
        {
            continue;
        }

        let present = slots[index].as_ref().map_or(0, |present| present.count);
        let room = slot_limit(Some(menu.ty), index, &item).saturating_sub(present);
        let amount = match kind {
            DragKind::CreativeFill => room,
            DragKind::Even | DragKind::OneEach => each.min(item.count).min(room),
        };
        if amount == 0 {
            continue;
        }

        let mut placed = item.clone();
        placed.count = present + amount;
        slots[index] = Some(placed);
        if kind != DragKind::CreativeFill {
            item.count -= amount;
        }
    }

    if item.count > 0 {
        menu.cursor = Some(item);
    }
}

fn collect_matching_items(
    slots: &mut [Option<ItemStack>],
    cursor: &mut Option<ItemStack>,
    button: i8,
) {
    let Some(cursor) = cursor else {
        return;
    };
    let indices: Vec<_> = if button == 0 {
        (0..slots.len()).collect()
    } else {
        (0..slots.len()).rev().collect()
    };

    // Collect partial stacks before full stacks, preserving the requested slot order.
    for partial_only in [true, false] {
        for &index in &indices {
            let Some(item) = &mut slots[index] else {
                continue;
            };
            if !same_item(item, cursor) || (partial_only && item.count >= max_count(item)) {
                continue;
            }

            let room = max_count(cursor).saturating_sub(cursor.count);
            let amount = item.count.min(room);
            cursor.count += amount;
            item.count -= amount;
            if item.count == 0 {
                slots[index] = None;
            }
        }
    }
}

#[cfg(test)]
mod tests;
