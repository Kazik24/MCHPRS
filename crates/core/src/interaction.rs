use crate::chat::ColorCode;
use crate::config::CONFIG;
use crate::messages;
use crate::player::PacketSender;
use crate::player::Player;
use crate::plot::PlotWorld;
use crate::redstone;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::*;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::packets::clientbound::{COpenSignEditor, ClientBoundPacket};
use mchprs_world::TickPriority;

pub fn on_use(
    block: Block,
    world: &mut impl World,
    player: &mut Player,
    pos: BlockPos,
    item_in_hand: Option<Item>,
) -> ActionResult {
    if crate::permissions::dedicated_permissions() {
        let action = if block.is_sign() {
            "sign"
        } else if ContainerType::from_block(block).is_some() {
            "container"
        } else if block.is_command_block() {
            "commandblock"
        } else {
            "interact"
        };
        if !player.has_permission(&format!("mchprs.build.{action}")) {
            return ActionResult::Pass;
        }
    }
    if item_in_hand == Some(Item::Stick) {
        //debug info about blocks
        player.send_color_message(
            ColorCode::DarkAqua,
            messages::block_debug(pos.x, pos.y, pos.z, block),
        );

        let power_desc = BlockFace::values()
            .map(|face| {
                let power = redstone::get_redstone_power(block, world, pos, face);
                let name = format!("{face:?}").chars().next().unwrap_or('-');
                format!("{name}: {power:>2}")
            })
            .join(", ");
        player.send_color_message(ColorCode::Gold, messages::block_debug_power(power_desc));
        if let Some(entity) = world.get_block_entity(pos) {
            player.send_color_message(ColorCode::Aqua, messages::block_debug_entity(entity));
        };
        return ActionResult::Pass;
    }
    match block {
        Block::RedstoneRepeater { repeater } => {
            let mut repeater = repeater;
            repeater.delay += 1;
            if repeater.delay > 4 {
                repeater.delay -= 4;
            }
            world.set_block(pos, Block::RedstoneRepeater { repeater });
            ActionResult::Success
        }
        Block::RedstoneComparator { comparator } => {
            let mut comparator = comparator;
            comparator.mode = comparator.mode.toggle();
            redstone::comparator::tick(comparator, world, pos);
            world.set_block(pos, Block::RedstoneComparator { comparator });
            crate::sound::control_used(world, pos, block, player.uuid);
            ActionResult::Success
        }
        Block::Lever { mut lever } => {
            lever.powered = !lever.powered;
            world.set_block(pos, Block::Lever { lever });
            crate::sound::control_used(world, pos, block, player.uuid);
            redstone::update_surrounding_blocks(world, pos);
            match lever.face {
                LeverFace::Ceiling => {
                    redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                }
                LeverFace::Floor => {
                    redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                }
                LeverFace::Wall => redstone::update_surrounding_blocks(
                    world,
                    pos.offset(lever.facing.opposite().block_face()),
                ),
            }
            ActionResult::Success
        }
        Block::StoneButton { mut button } => {
            if !button.powered {
                button.powered = true;
                world.set_block(pos, Block::StoneButton { button });
                crate::sound::control_used(world, pos, block, player.uuid);
                world.schedule_tick(pos, 10, TickPriority::Normal);
                redstone::update_surrounding_blocks(world, pos);
                match button.face {
                    ButtonFace::Ceiling => {
                        redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                    }
                    ButtonFace::Floor => {
                        redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                    }
                    ButtonFace::Wall => redstone::update_surrounding_blocks(
                        world,
                        pos.offset(button.facing.opposite().block_face()),
                    ),
                }
            }
            ActionResult::Success
        }
        Block::Cake { .. } => {
            crate::container::eat_cake(world, pos);
            ActionResult::Success
        }
        Block::RedstoneWire { wire } => redstone::wire::on_use(wire, world, pos),
        Block::SeaPickle { pickles } => {
            if let Some(Item::SeaPickle) = item_in_hand {
                if pickles < 4 {
                    world.set_block(
                        pos,
                        Block::SeaPickle {
                            pickles: pickles + 1,
                        },
                    );
                }
            }
            ActionResult::Success
        }
        Block::NoteBlock {
            instrument,
            note,
            powered,
        } => {
            world.set_block(
                pos,
                Block::NoteBlock {
                    instrument,
                    note: (note + 1) % 25,
                    powered,
                },
            );
            if redstone::noteblock::is_noteblock_unblocked(world, pos) {
                redstone::noteblock::play_note_for_action(world, pos, instrument, (note + 1) % 25);
            }
            ActionResult::Success
        }
        b if b.is_command_block() => {
            if !player.has_permission("commands.commandblock.edit") {
                player.send_no_permission_message();
                return ActionResult::Success;
            }
            if let Some(entity) = world.get_block_entity(pos) {
                if let Some(nbt) = entity.to_nbt(false) {
                    use mchprs_network::packets::clientbound::CBlockEntityData;
                    player.send_packet(
                        &CBlockEntityData {
                            pos: pos.packed(),
                            ty: entity.ty(),
                            nbt,
                        }
                        .encode(),
                    );
                }
            }
            ActionResult::Success
        }
        b if b.is_sign() => {
            if let Some(BlockEntity::Sign(sign)) = world.get_block_entity(pos) {
                if !sign.waxed {
                    let properties = b.properties();
                    let rotation = if let Some(facing) = properties.get("facing") {
                        match facing.as_str() {
                            "north" => 8,
                            "east" => 12,
                            "south" => 0,
                            "west" => 4,
                            _ => return ActionResult::Pass,
                        }
                    } else {
                        properties
                            .get("rotation")
                            .and_then(|rotation| rotation.parse::<u8>().ok())
                            .unwrap_or(0)
                    };
                    let angle = f64::from(rotation) * std::f64::consts::TAU / 16.0;
                    let dx = player.pos.x - pos.x as f64 - 0.5;
                    let dz = player.pos.z - pos.z as f64 - 0.5;
                    player.send_packet(
                        &COpenSignEditor {
                            pos: pos.packed(),
                            front: -angle.sin() * dx + angle.cos() * dz > 0.0,
                        }
                        .encode(),
                    );
                }
            }
            ActionResult::Success
        }
        b if b.has_block_entity() => {
            // Open container
            if let Some(ty) = ContainerType::from_block(b) {
                // Repair containers from plots saved before empty inventories existed.
                if !matches!(world.get_block_entity(pos), Some(BlockEntity::Container { ty: existing, .. }) if *existing == ty)
                {
                    world.set_block_entity(
                        pos,
                        BlockEntity::Container {
                            inventory: Default::default(),
                            comparator_override: 0,
                            ty,
                        },
                    );
                }
            }
            let block_entity = world.get_block_entity(pos);
            if let Some(BlockEntity::Container { inventory, ty, .. }) = block_entity {
                player.open_container(pos, inventory, *ty);
                world.container_opened(pos, *ty);
                crate::container::set_barrel_open(world, pos, true);
                ActionResult::Success
            } else {
                ActionResult::Pass
            }
        }
        _ => ActionResult::Pass,
    }
}

pub fn get_state_for_placement(
    world: &impl World,
    pos: BlockPos,
    item: Item,
    context: &UseOnBlockContext<'_>,
) -> Block {
    let block = match item {
        Item::Stone => Block::Stone {},
        Item::Glass => Block::Glass {},
        Item::Sandstone => Block::Sandstone {},
        Item::SeaPickle => Block::SeaPickle { pickles: 1 },
        Item::Wool { color } => Block::Wool { color },
        Item::Furnace => Block::Furnace {
            facing: context.player.get_direction().opposite(),
            lit: false,
        },
        Item::StonePressurePlate => Block::StonePressurePlate { powered: false },
        Item::Lever => {
            let lever_face = match context.block_face {
                BlockFace::Top => LeverFace::Floor,
                BlockFace::Bottom => LeverFace::Ceiling,
                _ => LeverFace::Wall,
            };
            let facing = if lever_face == LeverFace::Wall {
                context.block_face.unwrap_direction()
            } else {
                context.player.get_direction()
            };
            Block::Lever {
                lever: Lever::new(lever_face, facing, false),
            }
        }
        Item::RedstoneTorch => match context.block_face {
            BlockFace::Top | BlockFace::Bottom => Block::RedstoneTorch { lit: true },
            face => Block::RedstoneWallTorch {
                lit: true,
                facing: face.unwrap_direction(),
            },
        },
        Item::TripwireHook => match context.block_face {
            BlockFace::Bottom | BlockFace::Top => Block::Air {},
            direction => Block::TripwireHook {
                direction: direction.unwrap_direction(),
            },
        },
        Item::StoneButton => {
            let button_face = match context.block_face {
                BlockFace::Top => ButtonFace::Floor,
                BlockFace::Bottom => ButtonFace::Ceiling,
                _ => ButtonFace::Wall,
            };
            let facing = if button_face == ButtonFace::Wall {
                context.block_face.unwrap_direction()
            } else {
                context.player.get_direction()
            };
            Block::StoneButton {
                button: StoneButton::new(button_face, facing, false),
            }
        }
        Item::RedstoneLamp => Block::RedstoneLamp {
            lit: redstone::redstone_lamp_should_be_lit(world, pos),
        },
        Item::RedstoneBlock => Block::RedstoneBlock {},
        Item::Hopper => Block::Hopper {
            facing: HopperFacing::for_placement(context.block_face),
            enabled: !redstone::redstone_lamp_should_be_lit(world, pos),
        },
        Item::Terracotta => Block::Terracotta {},
        Item::ColoredTerracotta { color } => Block::ColoredTerracotta { color },
        Item::Concrete { color } => Block::Concrete { color },
        Item::Repeater => Block::RedstoneRepeater {
            repeater: redstone::repeater::get_state_for_placement(
                world,
                pos,
                context.player.get_direction().opposite(),
            ),
        },
        Item::Comparator => Block::RedstoneComparator {
            comparator: RedstoneComparator::new(
                context.player.get_direction().opposite(),
                ComparatorMode::Compare,
                false,
            ),
        },
        Item::Sign { sign_type } => match context.block_face {
            BlockFace::Bottom => Block::Air {},
            BlockFace::Top => Block::Sign {
                sign_type,
                rotation: ((180.0 + context.player.yaw).rem_euclid(360.0) * 16.0 / 360.0).round()
                    as u8
                    & 15,
            },
            _ => Block::WallSign {
                sign_type,
                facing: context.block_face.unwrap_direction(),
            },
        },
        Item::Redstone => Block::RedstoneWire {
            wire: redstone::wire::get_state_for_placement(world, pos),
        },
        Item::Barrel => Block::Barrel {
            facing: crate::container::barrel_facing(context.player.yaw, context.player.pitch),
            open: false,
        },
        Item::Target => Block::Target {},
        Item::StainedGlass { color } => Block::StainedGlass { color },
        Item::SmoothStoneSlab => Block::SmoothStoneSlab {},
        Item::QuartzSlab => Block::QuartzSlab {},
        Item::IronTrapdoor => match context.block_face {
            BlockFace::Bottom => Block::IronTrapdoor {
                facing: context.player.get_direction().opposite(),
                half: TrapdoorHalf::Top,
                powered: false,
            },
            BlockFace::Top => Block::IronTrapdoor {
                facing: context.player.get_direction().opposite(),
                half: TrapdoorHalf::Bottom,
                powered: false,
            },
            _ => Block::IronTrapdoor {
                facing: context.block_face.unwrap_direction(),
                half: if context.cursor_y > 0.5 {
                    TrapdoorHalf::Top
                } else {
                    TrapdoorHalf::Bottom
                },
                powered: false,
            },
        },
        Item::NoteBlock => Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 0,
            powered: false,
        },
        Item::Clay => Block::Clay {},
        Item::GoldBlock => Block::GoldBlock {},
        Item::PackedIce => Block::PackedIce {},
        Item::BoneBlock => Block::BoneBlock {},
        Item::IronBlock => Block::IronBlock {},
        Item::SoulSand => Block::SoulSand {},
        Item::Pumpkin => Block::Pumpkin {},
        Item::EmeraldBlock => Block::EmeraldBlock {},
        Item::HayBlock => Block::HayBlock {},
        Item::Sand => Block::Sand {},
        Item::Observer => Block::Observer {
            observer: RedstoneObserver {
                facing: context.player.get_block_facing(),
                powered: false,
            },
        },
        Item::Piston { sticky } => Block::Piston {
            piston: RedstonePiston {
                facing: context.player.get_block_facing().opposite(),
                sticky,
                extended: false,
            },
        },
        Item::Snowball => Block::PistonHead {
            head: RedstonePistonHead {
                facing: context.player.get_block_facing().opposite(),
                sticky: true,
                short: true,
            },
        },
        item if matches!(
            item.get_name(),
            "bamboo_sign" | "cherry_sign" | "mangrove_sign"
        ) =>
        {
            if context.block_face == BlockFace::Bottom {
                return Block::Air;
            }
            let name = item.get_name();
            let name = if context.block_face == BlockFace::Top {
                name.to_owned()
            } else {
                name.replace("_sign", "_wall_sign")
            };
            let mut block = Block::from_name(&name).unwrap();
            if context.block_face == BlockFace::Top {
                let rotation = (((180.0 + context.player.yaw).rem_euclid(360.0) * 16.0 / 360.0)
                    .round() as u8
                    & 15)
                    .to_string();
                block.set_properties(std::collections::HashMap::from([(
                    "rotation",
                    rotation.as_str(),
                )]));
            } else {
                let facing = format!("{:?}", context.block_face.unwrap_direction()).to_lowercase();
                block.set_properties(std::collections::HashMap::from([(
                    "facing",
                    facing.as_str(),
                )]));
            }
            block
        }
        item if item.get_name() == "chest" => Block::Chest {
            chest: Chest {
                facing: context.player.get_direction().opposite(),
                ..Default::default()
            },
        },
        _ => Block::from_name(item.get_name()).unwrap_or(Block::Air {}),
    };
    if is_valid_position(block, world, pos) {
        block
    } else {
        Block::Air {}
    }
}

pub fn place_in_world(
    block: Block,
    world: &mut impl World,
    pos: BlockPos,
    nbt: &Option<nbt::Blob>,
) {
    let previous = world.get_block(pos);
    if block.has_block_entity() {
        if let Some(nbt) = nbt {
            if let Some(nbt::Value::Compound(compound)) = nbt.get("BlockEntityTag") {
                if let Ok(block_entity) = BlockEntity::from_nbt(compound) {
                    world.set_block_entity(pos, block_entity);
                }
            }
        };
    }
    world.set_block(pos, block);
    if block.is_copper_bulb() && previous.registry_id() != block.registry_id() {
        redstone::copper_bulb::update(world, pos);
    }
    if block.is_command_block() {
        redstone::command_block::update(world, pos);
    }
    change_surrounding_blocks(world, pos);
    if let Block::RedstoneWire { .. } = block {
        redstone::update_wire_neighbors(world, pos);
    }
    redstone::update_surrounding_blocks(world, pos);
}

pub fn destroy(block: Block, world: &mut impl World, pos: BlockPos) {
    let counterpart = redstone::piston::remove_owned_parts(world, block, pos);
    if block.has_block_entity() {
        world.delete_block_entity(pos);
    }

    match block {
        Block::RedstoneWire { .. } => {
            world.set_block(pos, Block::Air {});
            change_surrounding_blocks(world, pos);
            redstone::update_wire_neighbors(world, pos);
        }
        Block::Lever { lever } => {
            world.set_block(pos, Block::Air {});
            // This is a horrible idea, don't do this.
            // One day this will be fixed, but for now... too bad!
            match lever.face {
                LeverFace::Ceiling => {
                    change_surrounding_blocks(world, pos.offset(BlockFace::Top));
                    redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                }
                LeverFace::Floor => {
                    change_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                    redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                }
                LeverFace::Wall => {
                    change_surrounding_blocks(
                        world,
                        pos.offset(lever.facing.opposite().block_face()),
                    );
                    redstone::update_surrounding_blocks(
                        world,
                        pos.offset(lever.facing.opposite().block_face()),
                    );
                }
            }
        }
        _ => {
            world.set_block(pos, Block::Air {});
            change_surrounding_blocks(world, pos);
            redstone::update_surrounding_blocks(world, pos);
        }
    }
    if let Some(pos) = counterpart {
        redstone::piston::notify(world, pos);
    }
}

// Center support geometry needed by torches and attachments around moving pistons.
fn supports_attachment(block: Block, face: BlockFace) -> bool {
    match block {
        Block::MovingPiston { .. } => false,
        Block::PistonHead { head } => BlockFace::from(head.facing) == face,
        Block::Piston { piston } if piston.extended => BlockFace::from(piston.facing) != face,
        _ => block.is_cube(),
    }
}

fn supports_dust(world: &impl World, pos: BlockPos) -> bool {
    let block = world.get_block(pos);
    if !matches!(block, Block::MovingPiston { .. }) {
        return supports_attachment(block, BlockFace::Top);
    }
    // The downward source base keeps its upper face throughout retraction.
    // Transported payloads and moving heads do not provide stationary support.
    matches!(
        world.get_block_entity(pos),
        Some(BlockEntity::MovingPiston(entity))
            if entity.source && !entity.extending && entity.facing == BlockFace::Bottom
                && matches!(Block::from_id(entity.block_state), Block::Piston { piston }
                    if piston.facing == mchprs_blocks::BlockFacing::Down && !piston.extended)
    )
}

/// Stationary support used by attachments. Compilation also uses this mapping
/// to reject attachments which would be destroyed by an owned moving block.
pub(crate) fn attachment_support(block: Block, pos: BlockPos) -> Option<(BlockPos, BlockFace)> {
    let face = if block.pressure_plate_powered().is_some() {
        BlockFace::Top
    } else {
        match block {
            Block::RedstoneWire { .. }
            | Block::RedstoneComparator { .. }
            | Block::RedstoneRepeater { .. }
            | Block::Sign { .. }
            | Block::RedstoneTorch { .. } => BlockFace::Top,
            Block::RedstoneWallTorch { facing, .. } | Block::WallSign { facing, .. } => {
                facing.block_face()
            }
            Block::TripwireHook { direction, .. } => direction.block_face(),
            Block::Lever { lever } => match lever.face {
                LeverFace::Floor => BlockFace::Top,
                LeverFace::Ceiling => BlockFace::Bottom,
                LeverFace::Wall => lever.facing.block_face(),
            },
            Block::StoneButton { button } => match button.face {
                ButtonFace::Floor => BlockFace::Top,
                ButtonFace::Ceiling => BlockFace::Bottom,
                ButtonFace::Wall => button.facing.block_face(),
            },
            _ => return None,
        }
    };
    Some((pos.offset(face.opposite()), face))
}

pub fn is_valid_position(block: Block, world: &impl World, pos: BlockPos) -> bool {
    if world.is_cursed() {
        return true;
    }
    if matches!(block, Block::Unknown { .. }) && block.is_sign() {
        let mut support_model = Block::from_name(if block.property("facing").is_some() {
            "oak_wall_sign"
        } else {
            "oak_sign"
        })
        .unwrap();
        let props = block.properties();
        support_model.set_properties(props.iter().map(|(k, v)| (*k, v.as_str())).collect());
        return is_valid_position(support_model, world, pos);
    }
    if matches!(block, Block::RedstoneWire { .. }) {
        return supports_dust(world, pos.offset(BlockFace::Bottom));
    }
    if let Some((support, face)) = attachment_support(block, pos) {
        return supports_attachment(world.get_block(support), face);
    }
    match block {
        Block::PistonHead { head } => {
            matches!(
                world.get_block(pos.offset(BlockFace::from(head.facing).opposite())),
                Block::Piston { piston } if piston.extended && piston.facing == head.facing && piston.sticky == head.sticky
            ) || matches!(
                world.get_block_entity(pos.offset(BlockFace::from(head.facing).opposite())),
                Some(BlockEntity::MovingPiston(e)) if !e.extending && e.source && e.facing == BlockFace::from(head.facing)
            )
        }
        _ => true,
    }
}

pub fn change(block: Block, world: &mut impl World, pos: BlockPos, direction: BlockFace) {
    if !is_valid_position(block, world, pos) {
        destroy(block, world, pos);
        return;
    }
    if let Block::RedstoneWire { wire } = block {
        let new_state = redstone::wire::on_neighbor_changed(wire, world, pos, direction);
        if world.set_block(pos, Block::RedstoneWire { wire: new_state }) {
            redstone::update_wire_neighbors(world, pos);
        }
    }
}

fn change_surrounding_blocks(world: &mut impl World, pos: BlockPos) {
    for direction in &BlockFace::values() {
        let neighbor_pos = pos.offset(*direction);
        let block = world.get_block(neighbor_pos);
        change(block, world, neighbor_pos, *direction);

        // Also change diagonal blocks

        let up_pos = neighbor_pos.offset(BlockFace::Top);
        let up_block = world.get_block(up_pos);
        change(up_block, world, up_pos, *direction);

        let down_pos = neighbor_pos.offset(BlockFace::Bottom);
        let down_block = world.get_block(down_pos);
        change(down_block, world, down_pos, *direction);
    }
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum ActionResult {
    Success,
    Pass,
}

impl ActionResult {
    fn is_success(self) -> bool {
        self == ActionResult::Success
    }
}

pub struct UseOnBlockContext<'a> {
    pub block_pos: BlockPos,
    pub block_face: BlockFace,
    pub player: &'a mut Player,
    pub cursor_y: f32,
}

pub enum ItemUseResult {
    Cancelled,
    Used,
    Placed(BlockPos),
}

/// Distinguishes successful placement from interaction and rejected placement.
pub fn use_item_on_block(
    item: &ItemStack,
    world: &mut PlotWorld,
    ctx: UseOnBlockContext<'_>,
) -> ItemUseResult {
    let use_pos = ctx.block_pos;
    let use_block = world.get_block(use_pos);
    if let Some((transformed, sound, event)) =
        redstone::copper_bulb::item_transform(use_block, item.item_type.get_name())
    {
        if crate::permissions::dedicated_permissions()
            && !ctx.player.has_permission("mchprs.build.interact")
        {
            ctx.player.send_no_permission_message();
            return ItemUseResult::Cancelled;
        }
        if let Some(sound) = sound {
            crate::sound::play(world, use_pos, sound, 1.0, 1.0, Some(ctx.player.uuid));
            world.level_event_for_action(use_pos, event, ctx.player.uuid);
        }
        place_in_world(transformed, world, use_pos, &None);
        if sound.is_none() {
            world.level_event_for_action(use_pos, event, ctx.player.uuid);
        }
        return ItemUseResult::Used;
    }
    let block_pos = ctx.block_pos.offset(ctx.block_face);
    let mut top_pos = ctx.player.pos.block_pos();
    top_pos.y += 1;
    if (block_pos == ctx.player.pos.block_pos() || block_pos == top_pos) && !CONFIG.block_in_hitbox
    {
        return ItemUseResult::Used;
    }
    let can_place = item.item_type.is_block() && world.get_block(block_pos).can_place_block_in();
    if !ctx.player.crouching
        && on_use(
            use_block,
            world,
            ctx.player,
            ctx.block_pos,
            Some(item.item_type),
        )
        .is_success()
    {
        return ItemUseResult::Used;
    }

    if can_place && world.contains_position(block_pos) {
        if crate::permissions::dedicated_permissions()
            && !ctx.player.has_permission("mchprs.build.place")
        {
            ctx.player.send_no_permission_message();
            return ItemUseResult::Cancelled;
        }
        let block = get_state_for_placement(world, block_pos, item.item_type, &ctx);
        let block = apply_item_properties(block, &item.nbt);

        place_in_world(block, world, block_pos, &item.nbt);
        if block.is_sign()
            && !item
                .nbt
                .as_ref()
                .is_some_and(|blob| blob.content.contains_key("BlockEntityTag"))
        {
            // The client needs the sign block entity before it can open its editor.
            world.flush_block_changes();
            ctx.player.send_packet(
                &COpenSignEditor {
                    pos: block_pos.packed(),
                    front: true,
                }
                .encode(),
            );
        }
        crate::sound::placed(world, block_pos, block, ctx.player.uuid);
        ItemUseResult::Placed(block_pos)
    } else {
        ItemUseResult::Cancelled
    }
}

fn apply_item_properties(mut block: Block, nbt: &Option<nbt::Blob>) -> Block {
    if let Some(nbt::Value::Compound(properties)) =
        nbt.as_ref().and_then(|blob| blob.get("BlockStateTag"))
    {
        block.set_properties(
            properties
                .iter()
                .filter_map(|(name, value)| {
                    if let nbt::Value::String(value) = value {
                        Some((name.as_str(), value.as_str()))
                    } else {
                        None
                    }
                })
                .collect(),
        );
    }

    // Slab items always place on top, including items saved with older components.
    block.with_slab_type(SlabType::Top).unwrap_or(block)
}

#[cfg(test)]
mod slab_placement_tests {
    use super::*;

    #[test]
    fn every_slab_item_places_on_top_without_special_components() {
        for &(name, _, _, _, _) in mchprs_blocks::generated::BLOCKS {
            let block = Block::from_name(name).unwrap();
            if block.slab_type().is_none() {
                continue;
            }
            assert!(Item::from_name(name).unwrap().is_block());
            let placed = apply_item_properties(block, &None);
            assert_eq!(placed.slab_type(), Some(SlabType::Top), "{name}");
            assert_eq!(placed.get_name(), name);
            assert!(!placed.is_solid(), "{name} must not conduct redstone");
            assert!(placed.is_transparent(), "{name}");
        }
    }

    #[test]
    fn saved_slab_properties_cannot_override_top_placement() {
        let nbt = Some(nbt::Blob::with_content(std::collections::HashMap::from([
            (
                "BlockStateTag".into(),
                nbt::Value::Compound(std::collections::HashMap::from([
                    ("type".into(), nbt::Value::String("bottom".into())),
                    ("waterlogged".into(), nbt::Value::String("true".into())),
                ])),
            ),
        ])));
        let block = Block::from_name("oak_slab").unwrap();
        let placed = apply_item_properties(block, &nbt);
        assert_eq!(placed.slab_type(), Some(SlabType::Top));
        assert_eq!(placed.property("waterlogged"), Some("true"));
        assert_eq!(
            apply_item_properties(Block::Stone {}, &None),
            Block::Stone {}
        );
    }
}
