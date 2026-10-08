use mchprs_blocks::blocks::{ComparatorMode, Instrument};
use mchprs_blocks::BlockPos;
use petgraph::stable_graph::{NodeIndex, StableGraph};

pub type NodeIdx = NodeIndex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeType {
    Repeater {
        delay: u8,
        facing_diode: bool,
    },
    Torch,
    /// State-change observation is separate from electrical input links.
    Observer {
        watched: BlockPos,
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
    /// Aggregate electrical input to a recognized actuator. Qualifying updates
    /// remain in the boundary side table, never diode Side links.
    InstantInput {
        piston: usize,
    },
    /// One physical alias of a group's mobile redstone supply. A single payload
    /// can occupy several aliases; none is an immutable constant.
    MobileSource {
        group: usize,
        alias: BlockPos,
    },
    /// Electrical strength at one ordinary consumer input, evaluated by the region.
    InstantOutput {
        port: usize,
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
}

impl NodeState {
    pub fn simple(powered: bool) -> NodeState {
        NodeState {
            powered,
            output_strength: if powered { 15 } else { 0 },
            ..Default::default()
        }
    }

    pub fn repeater(powered: bool, locked: bool) -> NodeState {
        NodeState {
            powered,
            repeater_locked: locked,
            output_strength: if powered { 15 } else { 0 },
            pending_tick: false,
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

#[derive(Debug)]
pub struct CompileNode {
    pub ty: NodeType,
    pub block: Option<(BlockPos, u32)>,
    pub state: NodeState,

    pub is_input: bool,
    pub is_output: bool,
}

impl CompileNode {
    pub fn is_removable(&self) -> bool {
        !self.is_input
            && !self.is_output
            && !self.state.pending_tick
            && !matches!(
                self.ty,
                NodeType::InstantInput { .. }
                    | NodeType::MobileSource { .. }
                    | NodeType::InstantOutput { .. }
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    Default,
    Side,
}

#[derive(Debug)]
pub struct CompileLink {
    pub ty: LinkType,
    /// Signal strength lost between source and target.
    pub attenuation: u8,
}

impl CompileLink {
    pub fn new(ty: LinkType, attenuation: u8) -> CompileLink {
        CompileLink { ty, attenuation }
    }
}

pub type CompileGraph = StableGraph<CompileNode, CompileLink>;

#[derive(Debug)]
pub enum GraphError {
    Cancelled,
    MissingSource {
        pos: BlockPos,
    },
    UnsupportedInstantExport,
    UnsupportedCommandBlockExport,
    UnsupportedObserverExport,
    UnsupportedCopperBulbExport,
    UnsupportedObserverWatch {
        observer: BlockPos,
        watched: BlockPos,
        reason: &'static str,
    },
    Export(std::io::Error),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("graph preparation cancelled"),
            Self::MissingSource { pos } => {
                write!(
                    f,
                    "power source at {pos:?} is missing from the compiled graph"
                )
            }
            Self::UnsupportedInstantExport => {
                f.write_str("instant graph export is not implemented")
            }
            Self::UnsupportedCommandBlockExport => {
                f.write_str("command-block output export is not implemented")
            }
            Self::UnsupportedObserverExport => f.write_str(
                "observer notification links cannot be exported in the electrical graph format",
            ),
            Self::UnsupportedCopperBulbExport => {
                f.write_str("copper-bulb state cannot be exported in the electrical graph format")
            }
            Self::UnsupportedObserverWatch {
                observer,
                watched,
                reason,
            } => write!(f, "observer at {observer:?} watches {watched:?}: {reason}"),
            Self::Export(error) => write!(f, "graph export failed: {error}"),
        }
    }
}
impl std::error::Error for GraphError {}
