pub(crate) mod graph;
mod queue;
pub use queue::*;

/// Active compiled backend implementation.
pub(crate) enum Runtime {
    Graph(Box<graph::GraphBackend>),
}

impl Runtime {
    pub(crate) fn node_count(&self) -> usize {
        match self {
            Self::Graph(backend) => backend.node_count(),
        }
    }

    pub(crate) fn kind(&self) -> super::BackendKind {
        super::BackendKind::Graph
    }

    pub(crate) fn graph_execution_counts(&self) -> Option<super::GraphExecutionCounts> {
        match self {
            Self::Graph(backend) => Some(backend.execution_counts()),
        }
    }

    #[cfg(feature = "graph-profile")]
    pub(crate) fn graph_execution_profile(&self) -> Option<super::GraphExecutionProfile> {
        match self {
            Self::Graph(backend) => Some(backend.execution_profile()),
        }
    }

    #[cfg(feature = "graph-profile")]
    pub(crate) fn graph_callback_profile(&self) -> Option<super::GraphCallbackProfile> {
        match self {
            Self::Graph(backend) => Some(backend.callback_execution_profile()),
        }
    }

    #[cfg(feature = "graph-profile")]
    pub(crate) fn graph_callback_inventory(&self) -> Option<serde_json::Value> {
        match self {
            Self::Graph(backend) => Some(backend.callback_research_inventory()),
        }
    }

    #[cfg(feature = "graph-profile")]
    pub(crate) fn dust_candidate_power_samples(&self) -> Vec<(usize, u64)> {
        match self {
            Self::Graph(backend) => backend.dust_candidate_power_samples(),
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), graph::GraphExecutionError> {
        let Self::Graph(backend) = self;
        backend.tick()
    }

    pub(crate) fn tick_with_world(
        &mut self,
        world: &mut impl crate::world::World,
    ) -> Result<(), graph::GraphExecutionError> {
        let Self::Graph(backend) = self;
        backend.tick_with_world(world)
    }

    pub(crate) fn flush(&mut self, world: &mut impl crate::world::World, io_only: bool) {
        let Self::Graph(backend) = self;
        backend.flush(world, io_only);
    }

    pub(crate) fn reset(&mut self, world: &mut impl crate::world::World, _io_only: bool) {
        let Self::Graph(backend) = self;
        backend.reset(world);
    }

    pub(crate) fn on_use_block(&mut self, pos: mchprs_blocks::BlockPos) {
        let Self::Graph(backend) = self;
        if let Err(error) = backend.use_block(pos) {
            tracing::error!(%error, "graph backend control failed");
        }
    }

    pub(crate) fn set_pressure_plate(&mut self, pos: mchprs_blocks::BlockPos, powered: bool) {
        let Self::Graph(backend) = self;
        if let Err(error) = backend.set_pressure_plate(pos, powered) {
            tracing::error!(%error, "graph backend control failed");
        }
    }

    pub(crate) fn inspect(&mut self, pos: mchprs_blocks::BlockPos) {
        let Self::Graph(backend) = self;
        backend.inspect(pos);
    }

    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(mchprs_blocks::BlockPos, u8)> {
        let Self::Graph(backend) = self;
        backend.ordinary_sources()
    }
}
