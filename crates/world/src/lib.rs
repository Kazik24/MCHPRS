use mchprs_blocks::BlockPos;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TickPriority {
    Highest = 0,
    Higher = 1,
    High = 2,
    Normal = 3,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct TickEntry {
    pub ticks_left: u32,
    pub tick_priority: TickPriority,
    pub pos: BlockPos,
}
