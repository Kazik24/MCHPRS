//! Server-owned container inventory operations. Client hashes are predictions, not items.
use mchprs_blocks::block_entities::{ContainerType, InventoryEntry};
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::BlockPos;
use mchprs_network::packets::{PacketEncoderExt, SlotData};

pub(crate) fn eat_cake(world: &mut impl crate::world::World, pos: BlockPos) {
    use mchprs_blocks::blocks::Block;
    if let Block::Cake { bites } = world.get_block(pos) {
        if bites < 6 {
            world.set_block(pos, Block::Cake { bites: bites + 1 });
            crate::redstone::update_surrounding_blocks(world, pos);
        } else {
            crate::interaction::destroy(Block::Cake { bites }, world, pos);
        }
    }
}

pub(crate) fn set_barrel_open(world: &mut impl crate::world::World, pos: BlockPos, open: bool) {
    if let mchprs_blocks::blocks::Block::Barrel {
        facing,
        open: previous,
    } = world.get_block(pos)
    {
        if previous == open {
            return;
        }
        world.set_block(pos, mchprs_blocks::blocks::Block::Barrel { facing, open });
        // Protocol 770 sound registry: block.barrel.close/open; category BLOCKS.
        world.play_sound(pos, if open { 130 } else { 129 }, 4, 0.5, 0.95);
        crate::redstone::update_surrounding_blocks(world, pos);
    }
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
    ((fullness / slots.len() as f32 * 14.0).floor() as u8
        + u8::from(slots.iter().any(Option::is_some)))
    .min(15)
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
    kind: u8,
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

fn furnace_items(key: &str, item: &ItemStack) -> bool {
    static RULES: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let rules = RULES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../mc_data/1.21.5/furnace_menu.json"))
            .expect("checked-in Java furnace menu rules")
    });
    rules[key]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name.as_str() == Some(item.item_type.get_name()))
}

fn may_place(ty: ContainerType, slot: usize, item: &ItemStack) -> bool {
    ty != ContainerType::Furnace
        || match slot {
            1 => furnace_items("fuel", item) || item.item_type.get_name() == "bucket",
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

fn pickup(slot: &mut Option<ItemStack>, cursor: &mut Option<ItemStack>, right: bool, limit: u8) {
    match (slot.as_mut(), cursor.as_mut()) {
        (Some(item), None) => {
            let amount = if right {
                item.count.div_ceil(2)
            } else {
                item.count
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
            let amount = item.count.min(if right { 1 } else { limit });
            let mut placed = item.clone();
            placed.count = amount;
            item.count -= amount;
            *slot = Some(placed);
            if item.count == 0 {
                *cursor = None;
            }
        }
        (Some(dest), Some(source)) if same_item(dest, source) => {
            let amount = source
                .count
                .min(if right { 1 } else { source.count })
                .min(limit.saturating_sub(dest.count));
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
    if mode != 5 {
        menu.drag = None;
    }
    match mode {
        0 if (0..=1).contains(&button) => {
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
                    pickup(&mut slots[i], &mut menu.cursor, button == 1, limit);
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
                if button == 0 {
                    menu.cursor = None;
                } else if let Some(item) = &mut menu.cursor {
                    item.count -= 1;
                    if item.count == 0 {
                        menu.cursor = None;
                    }
                }
            }
        }
        1 if (0..=1).contains(&button) => {
            if let Some(i) = index {
                let mut item = slots[i].take();
                let range: Vec<_> = if menu.ty == ContainerType::Furnace && i >= size {
                    if item
                        .as_ref()
                        .is_some_and(|item| furnace_items("smeltable", item))
                    {
                        vec![0]
                    } else if item
                        .as_ref()
                        .is_some_and(|item| furnace_items("fuel", item))
                    {
                        vec![1]
                    } else if i < size + 27 {
                        (size + 27..slots.len()).collect()
                    } else {
                        (size..size + 27).collect()
                    }
                } else if i < size {
                    (size..slots.len()).rev().collect()
                } else {
                    (0..size)
                        .filter(|&slot| menu.ty != ContainerType::Furnace || slot != 2)
                        .collect()
                };
                insert_with_rules(slots, &mut item, &range, Some(menu.ty));
                slots[i] = item;
            }
        }
        2 if button == 2 && creative => {
            if let Some(i) = index {
                if menu.cursor.is_none() {
                    menu.cursor = slots[i].clone().map(|mut item| {
                        item.count = max_count(&item);
                        item
                    });
                }
            }
        }
        3 if (0..=8).contains(&button) || button == 40 => {
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
        4 if (0..=1).contains(&button) && menu.cursor.is_none() => {
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
        5 if (0..=10).contains(&button) => {
            let kind = (button as u8 >> 2) & 3;
            let phase = button as u8 & 3;
            if kind > 2 || (kind == 2 && !creative) || menu.cursor.is_none() {
                menu.drag = None;
                return;
            }
            match phase {
                0 => {
                    menu.drag = Some(Drag {
                        kind,
                        slots: Vec::new(),
                    })
                }
                1 => {
                    if let (Some(i), Some(drag), Some(item)) = (index, &mut menu.drag, &menu.cursor)
                    {
                        if drag.kind != kind {
                            menu.drag = None;
                            return;
                        }
                        if !drag.slots.contains(&i)
                            && may_place(menu.ty, i, item)
                            && (kind == 2 || item.count as usize > drag.slots.len())
                            && slots[i].as_ref().is_none_or(|s| {
                                same_item(s, item) && s.count < slot_limit(Some(menu.ty), i, s)
                            })
                        {
                            drag.slots.push(i);
                        }
                    }
                }
                2 => {
                    if let Some(drag) = menu.drag.take() {
                        if drag.kind != kind || drag.slots.is_empty() {
                            return;
                        }
                        let Some(mut item) = menu.cursor.take() else {
                            return;
                        };
                        let each = if kind == 0 {
                            item.count as usize / drag.slots.len()
                        } else {
                            1
                        } as u8;
                        for i in drag.slots {
                            if slots[i].as_ref().is_some_and(|s| !same_item(s, &item)) {
                                continue;
                            }
                            let present = slots[i].as_ref().map_or(0, |s| s.count);
                            let room = slot_limit(Some(menu.ty), i, &item).saturating_sub(present);
                            let amount = if kind == 2 {
                                room
                            } else {
                                each.min(item.count).min(room)
                            };
                            if amount > 0 {
                                let mut placed = item.clone();
                                placed.count = present + amount;
                                slots[i] = Some(placed);
                                if kind != 2 {
                                    item.count -= amount;
                                }
                            }
                        }
                        if item.count > 0 {
                            menu.cursor = Some(item);
                        }
                    }
                }
                _ => menu.drag = None,
            }
        }
        6 if (0..=1).contains(&button) => {
            if let Some(cursor) = &mut menu.cursor {
                let indices: Vec<_> = if button == 0 {
                    (0..slots.len()).collect()
                } else {
                    (0..slots.len()).rev().collect()
                };
                for partial_only in [true, false] {
                    for &i in &indices {
                        if let Some(item) = &mut slots[i] {
                            if same_item(item, cursor)
                                && (!partial_only || item.count < max_count(item))
                            {
                                let amount = item
                                    .count
                                    .min(max_count(cursor).saturating_sub(cursor.count));
                                cursor.count += amount;
                                item.count -= amount;
                                if item.count == 0 {
                                    slots[i] = None;
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => menu.drag = None,
    }
}

#[cfg(test)]
mod tests;
