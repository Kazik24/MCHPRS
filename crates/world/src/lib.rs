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
    /// Expected block registry ID, independent of mutable state properties.
    pub block_type: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum PistonAction {
    Extend = 0,
    Retract = 1,
    RetractWithoutPull = 2,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct PistonEvent {
    pub pos: BlockPos,
    pub sticky: bool,
    pub facing: mchprs_blocks::BlockFace,
    pub action: PistonAction,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum AdvancePhase {
    #[default]
    BetweenTicks,
    ScheduledTicks,
    PistonEvents,
    MovingEntities,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PistonMotion {
    pub pos: BlockPos,
    pub identity: u64,
    pub progress: f32,
    pub previous_progress: f32,
    pub last_tick: u64,
    pub carried_entity: Option<Box<mchprs_blocks::block_entities::BlockEntity>>,
}

impl PistonMotion {
    /// Advance one half-step; completion is checked before changing progress.
    /// Return the previous progress used by the block entity's interpolation.
    pub fn advance(&mut self, logical_tick: u64) -> (bool, f32) {
        self.last_tick = logical_tick;
        self.previous_progress = self.progress;
        let complete = self.progress >= 1.0;
        if !complete {
            self.progress = (self.progress + 0.5).min(1.0);
        }
        (complete, self.previous_progress)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PistonState {
    pub logical_tick: u64,
    pub phase: AdvancePhase,
    pub scheduled_advanced: bool,
    pub events: std::collections::VecDeque<PistonEvent>,
    /// Ordered sequence; front completion avoids shifting every surviving motion.
    /// Serde retains the same sequence representation as the former Vec.
    pub motions: std::collections::VecDeque<PistonMotion>,
    pub next_identity: u64,
    /// Snapshot of the movement phase; new/replaced entities wait for the next phase.
    pub movement_work: Vec<(BlockPos, u64)>,
    pub movement_cursor: usize,
}
