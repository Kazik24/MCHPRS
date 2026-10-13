use crate::redstone::{self, comparator, noteblock};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::block_entities::CommandBlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeType {
    Repeater {
        delay: u8,
        facing_diode: bool,
    },
    Torch,
    Observer {
        watched: mchprs_blocks::BlockPos,
    },
    Comparator {
        mode: ComparatorMode,
        far_input: Option<u8>,
        facing_diode: bool,
    },
    Lamp,
    CopperBulb,
    Button,
    Lever,
    PressurePlate,
    Trapdoor,
    Wire,
    Constant,
    CommandBlock {
        repeating: bool,
        chain: bool,
        automatic: bool,
        initial_tick: bool,
    },
    NoteBlock {
        instrument: Instrument,
        note: u32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeState {
    pub powered: bool,
    pub repeater_locked: bool,
    pub output_strength: u8,
    pub pending_tick: bool,
    /// This retains the command block's live entity fields in graph-owned state.
    pub command_entity: Option<CommandBlockEntity>,
    /// This retains the instrument sampled at the most recent noteblock edge.
    pub note_instrument: Option<Instrument>,
}

impl NodeState {
    pub fn simple(powered: bool) -> NodeState {
        NodeState {
            powered,
            output_strength: if powered { 15 } else { 0 },
            command_entity: None,
            note_instrument: None,
            ..Default::default()
        }
    }

    pub fn repeater(powered: bool, locked: bool) -> NodeState {
        NodeState {
            powered,
            repeater_locked: locked,
            output_strength: if powered { 15 } else { 0 },
            pending_tick: false,
            command_entity: None,
            note_instrument: None,
        }
    }

    pub fn with_strength(strength: u8) -> NodeState {
        NodeState {
            output_strength: strength,
            ..Default::default()
        }
    }

    pub fn comparator(powered: bool, strength: u8) -> NodeState {
        NodeState {
            powered,
            output_strength: strength,
            ..Default::default()
        }
    }
}

pub(crate) fn identify_block<W: World>(
    block: Block,
    pos: BlockPos,
    world: &W,
) -> Option<(NodeType, NodeState)> {
    let (ty, state) = match block {
        Block::RedstoneRepeater { repeater } => (
            NodeType::Repeater {
                delay: repeater.delay,
                facing_diode: redstone::is_diode(
                    world.get_block(pos.offset(repeater.facing.opposite().block_face())),
                ),
            },
            NodeState::repeater(repeater.powered, repeater.locked),
        ),
        Block::RedstoneComparator { comparator: state } => (
            NodeType::Comparator {
                mode: state.mode,
                far_input: comparator::get_far_input(world, pos, state.facing),
                facing_diode: redstone::is_diode(
                    world.get_block(pos.offset(state.facing.opposite().block_face())),
                ),
            },
            NodeState::comparator(
                state.powered,
                if let Some(BlockEntity::Comparator { output_strength }) =
                    world.get_block_entity(pos)
                {
                    *output_strength
                } else {
                    0
                },
            ),
        ),
        Block::RedstoneTorch { lit, .. } | Block::RedstoneWallTorch { lit, .. } => {
            (NodeType::Torch, NodeState::simple(lit))
        }
        Block::Observer { observer } => (
            NodeType::Observer {
                watched: pos.offset(observer.facing.into()),
            },
            NodeState::simple(observer.powered),
        ),
        Block::RedstoneWire { wire } => (NodeType::Wire, NodeState::with_strength(wire.power)),
        Block::StoneButton { button } => (NodeType::Button, NodeState::simple(button.powered)),
        Block::RedstoneLamp { lit } => (NodeType::Lamp, NodeState::simple(lit)),
        block if block.is_copper_bulb() => {
            let (lit, powered) = block.copper_bulb_state().unwrap();
            (
                NodeType::CopperBulb,
                NodeState::comparator(powered, redstone::bool_to_ss(lit)),
            )
        }
        Block::Lever { lever } => (NodeType::Lever, NodeState::simple(lever.powered)),
        Block::StonePressurePlate { powered } => {
            (NodeType::PressurePlate, NodeState::simple(powered))
        }
        block if block.pressure_plate_powered().is_some() => (
            NodeType::PressurePlate,
            NodeState::simple(block.pressure_plate_powered().unwrap()),
        ),
        Block::IronTrapdoor { powered, .. } => (NodeType::Trapdoor, NodeState::simple(powered)),
        Block::RedstoneBlock => (NodeType::Constant, NodeState::with_strength(15)),
        Block::NoteBlock { note, powered, .. } => {
            let instrument = noteblock::get_noteblock_instrument(world, pos);
            (
                NodeType::NoteBlock { instrument, note },
                NodeState {
                    note_instrument: Some(instrument),
                    ..NodeState::simple(powered)
                },
            )
        }
        block if block.is_command_block() => {
            let entity = match world.get_block_entity(pos) {
                Some(BlockEntity::CommandBlock(entity)) => Some(entity.as_ref()),
                _ => None,
            };
            (
                NodeType::CommandBlock {
                    repeating: block.get_name() == "repeating_command_block",
                    chain: block.get_name() == "chain_command_block",
                    automatic: entity.is_some_and(|entity| entity.automatic),
                    initial_tick: entity.is_some_and(|entity| {
                        entity.automatic
                            && (entity.last_execution < 0
                                || block.get_name() == "repeating_command_block")
                    }),
                },
                NodeState {
                    powered: entity.is_some_and(|entity| entity.powered),
                    output_strength: comparator::get_override(block, world, pos),
                    command_entity: Some(entity.cloned().unwrap_or_default()),
                    ..Default::default()
                },
            )
        }
        block if comparator::has_override(block) => (
            NodeType::Constant,
            NodeState::with_strength(comparator::get_override(block, world, pos)),
        ),
        _ => return None,
    };
    Some((ty, state))
}
