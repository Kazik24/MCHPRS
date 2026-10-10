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

pub(crate) enum Runtime {
    Direct(direct::DirectBackend),
    Native(native::NativeBackend),
}

impl Runtime {
    pub(crate) fn native(
        world: &impl crate::world::World,
        bounds: (mchprs_blocks::BlockPos, mchprs_blocks::BlockPos),
        graph: &super::compile_graph::CompileGraph,
        ticks: Vec<mchprs_world::TickEntry>,
        monitor: &super::TaskMonitor,
    ) -> Result<Self, super::CompileError> {
        native::NativeBackend::compile(world, bounds, graph, ticks, monitor).map(Self::Native)
    }

    pub(crate) fn node_count(&self) -> usize {
        match self {
            Self::Direct(b) => b.node_count(),
            Self::Native(b) => b.node_count(),
        }
    }

    pub(crate) fn tick(&mut self) {
        match self {
            Self::Direct(b) => b.tick(),
            Self::Native(b) => b.tick(),
        }
    }
    pub(crate) fn tick_with_world(&mut self, world: &mut impl crate::world::World) {
        match self {
            Self::Direct(b) => b.tick_with_world(world),
            Self::Native(b) => b.tick_with_world(world),
        }
    }
    pub(crate) fn flush(&mut self, world: &mut impl crate::world::World, io_only: bool) {
        match self {
            Self::Direct(b) => b.flush(world, io_only),
            Self::Native(b) => b.flush(world, io_only),
        }
    }
    pub(crate) fn reset(&mut self, world: &mut impl crate::world::World, io_only: bool) {
        match self {
            Self::Direct(b) => b.reset(world, io_only),
            Self::Native(b) => b.reset(world, io_only),
        }
    }
    pub(crate) fn on_use_block(&mut self, pos: mchprs_blocks::BlockPos) {
        match self {
            Self::Direct(b) => b.on_use_block(pos),
            Self::Native(b) => b.on_use_block(pos),
        }
    }
    pub(crate) fn set_pressure_plate(&mut self, pos: mchprs_blocks::BlockPos, powered: bool) {
        match self {
            Self::Direct(b) => b.set_pressure_plate(pos, powered),
            Self::Native(b) => b.set_pressure_plate(pos, powered),
        }
    }
    pub(crate) fn inspect(&mut self, pos: mchprs_blocks::BlockPos) {
        match self {
            Self::Direct(b) => b.inspect(pos),
            Self::Native(b) => b.inspect(pos),
        }
    }

    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(mchprs_blocks::BlockPos, u8)> {
        match self {
            Self::Direct(b) => b.ordinary_sources(),
            Self::Native(b) => b.ordinary_sources(),
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn scheduled_ticks(&self) -> Vec<mchprs_world::TickEntry> {
        match self {
            Self::Direct(b) => b.scheduled_ticks(),
            Self::Native(_) => Vec::new(),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn node_state(
        &self,
        pos: mchprs_blocks::BlockPos,
    ) -> Option<(direct::node::NodeType, bool, u8, [u8; 16], [u8; 16])> {
        match self {
            Self::Direct(b) => b.node_state(pos),
            Self::Native(_) => None,
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn incoming_states(
        &self,
        pos: mchprs_blocks::BlockPos,
    ) -> Option<
        Vec<(
            Option<mchprs_blocks::BlockPos>,
            direct::node::NodeType,
            u8,
            bool,
            u8,
        )>,
    > {
        match self {
            Self::Direct(b) => b.incoming_states(pos),
            Self::Native(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
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
            Self::InvalidStrength { pos, strength } => write!(
                f,
                "graph node at {pos:?} has invalid strength {strength}; expected 0..15"
            ),
            Self::TooManyInputs {
                pos,
                default_inputs,
                side_inputs,
            } => write!(
                f,
                "graph node at {pos:?} has {default_inputs} main and {side_inputs} side inputs; each channel supports at most 255"
            ),
        }
    }
}
impl std::error::Error for BackendError {}
