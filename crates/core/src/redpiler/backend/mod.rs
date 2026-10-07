pub mod direct;
mod queue;
pub use queue::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    InstantRuntimeUnavailable,
    InvalidInstantProgram,
    MissingInstantBinding {
        pos: mchprs_blocks::BlockPos,
    },
    LogicalWireInput {
        pos: mchprs_blocks::BlockPos,
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
            Self::InstantRuntimeUnavailable => {
                f.write_str("instant graph boundaries require the region runtime")
            }
            Self::InvalidInstantProgram => f.write_str("instant program contains unresolved actuator variables"),
            Self::MissingInstantBinding { pos } => write!(f, "instant port at {pos:?} was lost during graph preparation"),
            Self::LogicalWireInput { pos } => write!(f, "logical source at {pos:?} remained a wire display; extract its complete electrical input cone before logical execution"),
            Self::InvalidStrength { pos, strength } => write!(f, "graph node at {pos:?} has invalid strength {strength}; expected 0..15"),
            Self::TooManyInputs { pos, default_inputs, side_inputs } => write!(f, "graph node at {pos:?} has {default_inputs} main and {side_inputs} side inputs; each channel supports at most 255"),
        }
    }
}
impl std::error::Error for BackendError {}
