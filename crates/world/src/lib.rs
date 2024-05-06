use mchprs_blocks::BlockPos;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TickPriority {
    Highest = 0,
    Higher = 1,
    High = 2,
    Normal = 3,
}

impl TickPriority {
    #[allow(non_upper_case_globals)]
    /// Indicates that we want to schedule nano-tick
    pub const NanoTick: Self = Self::Normal; //probably the lowest priority is best here
    /// All tick priorities in update order, from highest to lowest
    pub const ALL: [TickPriority; 4] = [
        TickPriority::Highest,
        TickPriority::Higher,
        TickPriority::High,
        TickPriority::Normal,
    ];
    pub const COUNT: usize = Self::ALL.len();
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct TickEntry {
    pub ticks_left: u32,
    pub tick_priority: TickPriority,
    pub pos: BlockPos,
}
