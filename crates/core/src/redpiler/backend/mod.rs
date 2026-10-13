pub mod direct;
pub(crate) mod graph;
mod native;
mod queue;
pub use queue::*;

fn validate_strengths(
    graph: &super::compile_graph::LegacyCompileGraph,
) -> Result<(), BackendError> {
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
    Graph(Box<graph::GraphBackend>),
}

impl Runtime {
    pub(crate) fn native(
        world: &impl crate::world::World,
        bounds: (mchprs_blocks::BlockPos, mchprs_blocks::BlockPos),
        graph: &super::compile_graph::LegacyCompileGraph,
        ticks: Vec<mchprs_world::TickEntry>,
        monitor: &super::TaskMonitor,
    ) -> Result<Self, super::CompileError> {
        native::NativeBackend::compile(world, bounds, graph, ticks, monitor).map(Self::Native)
    }

    pub(crate) fn node_count(&self) -> usize {
        match self {
            Self::Direct(b) => b.node_count(),
            Self::Native(b) => b.node_count(),
            Self::Graph(b) => b.node_count(),
        }
    }

    pub(crate) fn kind(&self) -> super::BackendKind {
        match self {
            Self::Direct(_) => super::BackendKind::Direct,
            Self::Native(_) => super::BackendKind::Native,
            Self::Graph(_) => super::BackendKind::Graph,
        }
    }

    pub(crate) fn graph_execution_counts(&self) -> Option<super::GraphExecutionCounts> {
        match self {
            Self::Graph(b) => Some(b.execution_counts()),
            _ => None,
        }
    }

    fn tick_graph(backend: &mut graph::GraphBackend) {
        // Validation rejects unsupported components before publication, so an
        // execution error is a defect. Report it; never fall back to physics.
        if let Err(error) = backend.tick() {
            tracing::error!(%error, "graph backend execution failed");
        }
    }

    pub(crate) fn tick(&mut self) {
        match self {
            Self::Direct(b) => b.tick(),
            Self::Native(b) => b.tick(),
            Self::Graph(b) => Self::tick_graph(b),
        }
    }
    pub(crate) fn tick_with_world(&mut self, world: &mut impl crate::world::World) {
        match self {
            Self::Direct(b) => b.tick_with_world(world),
            Self::Native(b) => b.tick_with_world(world),
            Self::Graph(b) => Self::tick_graph(b),
        }
    }
    pub(crate) fn flush(&mut self, world: &mut impl crate::world::World, io_only: bool) {
        match self {
            Self::Direct(b) => b.flush(world, io_only),
            Self::Native(b) => b.flush(world, io_only),
            Self::Graph(b) => b.flush(world, io_only),
        }
    }
    pub(crate) fn reset(&mut self, world: &mut impl crate::world::World, io_only: bool) {
        match self {
            Self::Direct(b) => b.reset(world, io_only),
            Self::Native(b) => b.reset(world, io_only),
            // Continuation needs the whole future, so io_only affects display only.
            Self::Graph(b) => b.reset(world),
        }
    }
    pub(crate) fn on_use_block(&mut self, pos: mchprs_blocks::BlockPos) {
        match self {
            Self::Direct(b) => b.on_use_block(pos),
            Self::Native(b) => b.on_use_block(pos),
            Self::Graph(b) => {
                if let Err(error) = b.use_block(pos) {
                    tracing::error!(%error, "graph backend control failed");
                }
            }
        }
    }
    pub(crate) fn set_pressure_plate(&mut self, pos: mchprs_blocks::BlockPos, powered: bool) {
        match self {
            Self::Direct(b) => b.set_pressure_plate(pos, powered),
            Self::Native(b) => b.set_pressure_plate(pos, powered),
            Self::Graph(b) => {
                if let Err(error) = b.set_pressure_plate(pos, powered) {
                    tracing::error!(%error, "graph backend control failed");
                }
            }
        }
    }
    pub(crate) fn inspect(&mut self, pos: mchprs_blocks::BlockPos) {
        match self {
            Self::Direct(b) => b.inspect(pos),
            Self::Native(b) => b.inspect(pos),
            Self::Graph(b) => b.inspect(pos),
        }
    }

    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(mchprs_blocks::BlockPos, u8)> {
        match self {
            Self::Direct(b) => b.ordinary_sources(),
            Self::Native(b) => b.ordinary_sources(),
            Self::Graph(_) => Vec::new(),
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn scheduled_ticks(&self) -> Vec<mchprs_world::TickEntry> {
        match self {
            Self::Direct(b) => b.scheduled_ticks(),
            Self::Native(_) | Self::Graph(_) => Vec::new(),
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
            Self::Native(_) | Self::Graph(_) => None,
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
            Self::Native(_) | Self::Graph(_) => None,
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
