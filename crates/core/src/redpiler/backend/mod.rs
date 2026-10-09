pub mod direct;
mod native;
mod queue;
pub use queue::*;

fn validate_strengths(graph: &super::compile_graph::CompileGraph) -> Result<(), BackendError> {
    for node in graph.node_weights() {
        let far_input = match node.ty {
            super::compile_graph::NodeType::Comparator { far_input, .. } => far_input,
            _ => None,
        };
        for strength in std::iter::once(node.state.output_strength).chain(far_input) {
            if strength > 15 {
                return Err(BackendError::InvalidStrength {
                    pos: node.block.map(|(pos, _)| pos),
                    strength,
                });
            }
        }
    }
    Ok(())
}

pub(crate) type Runtime = direct::DirectBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    DuplicateInstantChannel {
        pos: mchprs_blocks::BlockPos,
    },
    DuplicateInstantOwner {
        pos: mchprs_blocks::BlockPos,
    },
    InstantRuntimeUnavailable,
    InvalidInstantProgram,
    MissingInstantBinding {
        pos: mchprs_blocks::BlockPos,
    },
    LogicalWireInput {
        pos: mchprs_blocks::BlockPos,
    },
    ObserverGeometryBinding {
        pos: mchprs_blocks::BlockPos,
        bindings: usize,
    },
    InvalidStrength {
        pos: Option<mchprs_blocks::BlockPos>,
        strength: u8,
    },
    TooManyInputs {
        pos: Option<mchprs_blocks::BlockPos>,
        default_inputs: usize,
        side_inputs: usize,
    },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateInstantChannel { pos } => write!(f, "consumer at {pos:?} has duplicate instant channel ownership"),
            Self::DuplicateInstantOwner { pos } => write!(f, "block at {pos:?} belongs to multiple instant assemblies"),
            Self::InstantRuntimeUnavailable => {
                f.write_str("instant graph boundaries require the region runtime")
            }
            Self::InvalidInstantProgram => f.write_str("instant program contains unresolved actuator variables"),
            Self::MissingInstantBinding { pos } => write!(f, "instant port at {pos:?} was lost during graph preparation"),
            Self::LogicalWireInput { pos } => write!(f, "logical input at {pos:?} points to display-only dust instead of a power source"),
            Self::ObserverGeometryBinding { pos, bindings } => write!(f, "observer target at {pos:?} belongs to {bindings} piston regions; expected exactly one"),
            Self::InvalidStrength { pos, strength } => write!(f, "graph node at {pos:?} has invalid strength {strength}; expected 0..15"),
            Self::TooManyInputs { pos, default_inputs, side_inputs } => write!(f, "graph node at {pos:?} has {default_inputs} main and {side_inputs} side inputs; each channel supports at most 255"),
        }
    }
}
impl std::error::Error for BackendError {}
