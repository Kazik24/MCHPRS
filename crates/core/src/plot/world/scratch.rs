use crate::world::RedstoneUpdateObservation;
use mchprs_blocks::BlockPos;
use std::{cell::Cell, collections::HashSet};

#[derive(Default)]
pub(super) struct ScratchState {
    pub(super) observations: Option<Vec<RedstoneUpdateObservation>>,
    pub(super) capture_bounds: Option<(BlockPos, BlockPos)>,
    pub(super) read_bounds: Option<(BlockPos, BlockPos)>,
    pub(super) read_chunks: Option<HashSet<(i32, i32)>>,
    pub(super) contract_air: Option<HashSet<BlockPos>>,
    pub(super) boundary_read: Cell<Option<BlockPos>>,
    pub(super) boundary_effect: Cell<Option<BlockPos>>,
    pub(super) touched_blocks: Option<HashSet<BlockPos>>,
    pub(super) touched_entities: Option<HashSet<BlockPos>>,
    pub(super) suppress_commands: bool,
}
