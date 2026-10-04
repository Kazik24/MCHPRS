use crate::chat::ColorCode;
use crate::config::CONFIG;
use crate::player::PacketSender;
use crate::player::Player;
use crate::plot::PlotWorld;
use crate::plot::PLOT_BLOCK_HEIGHT;
use crate::redstone;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
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
    if item_in_hand == Some(Item::Stick) {
        //debug info about blocks
        player.send_color_message(
            ColorCode::DarkAqua,
            format_args!("Block at ({}, {}, {}):\n    {block:?}", pos.x, pos.y, pos.z),
        );

        let power_desc = BlockFace::values()
            .map(|face| {
                let power = redstone::get_redstone_power(block, world, pos, face);
                let name = format!("{face:?}").chars().next().unwrap_or('-');
                format!("{name}: {power:>2}")
            })
            .join(", ");
        player.send_color_message(
            ColorCode::Gold,
            format_args!("  Redstone power: {power_desc}"),
        );
        match world.get_block_entity(pos) {
            Some(entity) => {
                player.send_color_message(
                    ColorCode::Aqua,
                    format_args!("  Block entity:\n    {entity:?}"),
                );
            }
            None => {}
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
            ActionResult::Success
        }
        Block::Lever { mut lever } => {
            lever.powered = !lever.powered;
            world.set_block(pos, Block::Lever { lever });
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
        Block::RedstoneWire { wire } => redstone::wire::on_use(wire, world, pos),
        Block::SeaPickle { pickles } => {
            if let Some(Item::SeaPickle {}) = item_in_hand {
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
                redstone::noteblock::play_note(world, pos, instrument, (note + 1) % 25);
            }
            ActionResult::Success
        }
        b if b.has_block_entity() => {
            // Open container
            let block_entity = world.get_block_entity(pos);
            if let Some(BlockEntity::Container { inventory, ty, .. }) = block_entity {
                player.open_container(inventory, *ty);
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
        Item::Stone {} => Block::Stone {},
        Item::Glass {} => Block::Glass {},
        Item::Sandstone {} => Block::Sandstone {},
        Item::SeaPickle {} => Block::SeaPickle { pickles: 1 },
        Item::Wool { color } => Block::Wool { color },
        Item::Furnace {} => Block::Furnace {},
        Item::StonePressurePlate {} => Block::StonePressurePlate { powered: false },
        Item::Lever {} => {
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
        Item::RedstoneTorch {} => match context.block_face {
            BlockFace::Top | BlockFace::Bottom => Block::RedstoneTorch { lit: true },
            face => Block::RedstoneWallTorch {
                lit: true,
                facing: face.unwrap_direction(),
            },
        },
        Item::TripwireHook {} => match context.block_face {
            BlockFace::Bottom | BlockFace::Top => Block::Air {},
            direction => Block::TripwireHook {
                direction: direction.unwrap_direction(),
            },
        },
        Item::StoneButton {} => {
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
        Item::RedstoneLamp {} => Block::RedstoneLamp {
            lit: redstone::redstone_lamp_should_be_lit(world, pos),
        },
        Item::RedstoneBlock {} => Block::RedstoneBlock {},
        Item::Hopper {} => Block::Hopper {},
        Item::Terracotta {} => Block::Terracotta {},
        Item::ColoredTerracotta { color } => Block::ColoredTerracotta { color },
        Item::Concrete { color } => Block::Concrete { color },
        Item::Repeater {} => Block::RedstoneRepeater {
            repeater: redstone::repeater::get_state_for_placement(
                world,
                pos,
                context.player.get_direction().opposite(),
            ),
        },
        Item::Comparator {} => Block::RedstoneComparator {
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
        Item::Redstone {} => Block::RedstoneWire {
            wire: redstone::wire::get_state_for_placement(world, pos),
        },
        Item::Barrel {} => Block::Barrel {},
        Item::Target {} => Block::Target {},
        Item::StainedGlass { color } => Block::StainedGlass { color },
        Item::SmoothStoneSlab {} => Block::SmoothStoneSlab {},
        Item::QuartzSlab {} => Block::QuartzSlab {},
        Item::IronTrapdoor {} => match context.block_face {
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
        Item::NoteBlock {} => Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 0,
            powered: false,
        },
        Item::Clay {} => Block::Clay {},
        Item::GoldBlock {} => Block::GoldBlock {},
        Item::PackedIce {} => Block::PackedIce {},
        Item::BoneBlock {} => Block::BoneBlock {},
        Item::IronBlock {} => Block::IronBlock {},
        Item::SoulSand {} => Block::SoulSand {},
        Item::Pumpkin {} => Block::Pumpkin {},
        Item::EmeraldBlock {} => Block::EmeraldBlock {},
        Item::HayBlock {} => Block::HayBlock {},
        Item::Sand {} => Block::Sand {},
        Item::Observer {} => Block::Observer {
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
        Item::Snowball {} => Block::PistonHead {
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
    if block.pressure_plate_powered().is_some() {
        return supports_attachment(
            world.get_block(pos.offset(BlockFace::Bottom)),
            BlockFace::Top,
        );
    }

    match block {
        Block::RedstoneWire { .. }
        | Block::RedstoneComparator { .. }
        | Block::RedstoneRepeater { .. }
        | Block::Sign { .. }
        | Block::RedstoneTorch { .. } => {
            let bottom_block = world.get_block(pos.offset(BlockFace::Bottom));
            supports_attachment(bottom_block, BlockFace::Top)
        }
        Block::RedstoneWallTorch { facing, .. } | Block::WallSign { facing, .. } => {
            let parent_block = world.get_block(pos.offset(facing.opposite().block_face()));
            supports_attachment(parent_block, facing.block_face())
        }
        Block::TripwireHook { direction, .. } => {
            let parent_block = world.get_block(pos.offset(direction.opposite().block_face()));
            supports_attachment(parent_block, direction.block_face())
        }
        Block::Lever { lever } => match lever.face {
            LeverFace::Floor => {
                let bottom_block = world.get_block(pos.offset(BlockFace::Bottom));
                supports_attachment(bottom_block, BlockFace::Top)
            }
            LeverFace::Ceiling => {
                let top_block = world.get_block(pos.offset(BlockFace::Top));
                supports_attachment(top_block, BlockFace::Bottom)
            }
            LeverFace::Wall => {
                let parent_block =
                    world.get_block(pos.offset(lever.facing.opposite().block_face()));
                supports_attachment(parent_block, lever.facing.block_face())
            }
        },
        Block::StoneButton { button } => match button.face {
            ButtonFace::Floor => {
                let bottom_block = world.get_block(pos.offset(BlockFace::Bottom));
                supports_attachment(bottom_block, BlockFace::Top)
            }
            ButtonFace::Ceiling => {
                let top_block = world.get_block(pos.offset(BlockFace::Top));
                supports_attachment(top_block, BlockFace::Bottom)
            }
            ButtonFace::Wall => {
                let parent_block =
                    world.get_block(pos.offset(button.facing.opposite().block_face()));
                supports_attachment(parent_block, button.facing.block_face())
            }
        },
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

/// returns true if cancelled
pub fn use_item_on_block(
    item: &ItemStack,
    world: &mut PlotWorld,
    ctx: UseOnBlockContext<'_>,
) -> bool {
    let use_pos = ctx.block_pos;
    let use_block = world.get_block(use_pos);
    let block_pos = ctx.block_pos.offset(ctx.block_face);
    let mut top_pos = ctx.player.pos.block_pos();
    top_pos.y += 1;
    if (block_pos == ctx.player.pos.block_pos() || block_pos == top_pos) && !CONFIG.block_in_hitbox
    {
        return false;
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
        return false;
    }

    if can_place && (0..PLOT_BLOCK_HEIGHT).contains(&block_pos.y) {
        let mut block = get_state_for_placement(world, block_pos, item.item_type, &ctx);
        if let Some(nbt::Value::Compound(props)) =
            item.nbt.as_ref().and_then(|n| n.get("BlockStateTag"))
        {
            block.set_properties(
                props
                    .iter()
                    .filter_map(|(k, v)| {
                        if let nbt::Value::String(s) = v {
                            Some((k.as_str(), s.as_str()))
                        } else {
                            None
                        }
                    })
                    .collect(),
            );
        }

        match block {
            block if block.is_sign() => {
                if !item
                    .nbt
                    .as_ref()
                    .is_some_and(|blob| blob.content.contains_key("BlockEntityTag"))
                {
                    let open_sign_editor = COpenSignEditor {
                        pos: block_pos.packed(),
                    }
                    .encode();
                    ctx.player.client.send_packet(&open_sign_editor);
                }
            }
            _ => {}
        }

        place_in_world(block, world, block_pos, &item.nbt);
        false
    } else {
        true
    }
}
