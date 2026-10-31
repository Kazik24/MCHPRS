use super::*;

mod cache;
pub(super) use cache::{PistonIndex, TickIndex};

use mchprs_world::AdvancePhase;

impl PlotWorld {
    /// Find the next operation in this game tick. All stepping commands share
    /// these phases, so pausing between operations does not alter their order.
    fn prepare_operation(&mut self) -> bool {
        loop {
            match self.piston_state.phase {
                AdvancePhase::BetweenTicks => {
                    self.piston_state.logical_tick += 1;
                    self.waveform.tick(self.piston_state.logical_tick, true);
                    self.piston_state.scheduled_advanced = false;
                    self.piston_state.phase = AdvancePhase::ScheduledTicks;
                }
                AdvancePhase::ScheduledTicks => {
                    if !self.to_be_ticked.this_tick_ref().is_empty() {
                        return true;
                    }
                    if !self.piston_state.scheduled_advanced {
                        self.to_be_ticked.end_last_tick_move_next();
                        self.piston_state.scheduled_advanced = true;
                    } else {
                        self.piston_state.phase = AdvancePhase::PistonEvents;
                    }
                }
                AdvancePhase::PistonEvents => {
                    if !self.piston_state.events.is_empty() {
                        return true;
                    }
                    self.piston_state.movement_work.clear();
                    self.piston_state
                        .movement_work
                        .extend(self.piston_state.motions.iter().map(|m| (m.pos, m.identity)));
                    self.piston_state.movement_cursor = 0;
                    self.piston_state.phase = AdvancePhase::MovingEntities;
                }
                AdvancePhase::MovingEntities => {
                    if self.piston_state.movement_cursor < self.piston_state.movement_work.len() {
                        return true;
                    }
                    self.piston_state.movement_work.clear();
                    self.piston_state.movement_cursor = 0;
                    self.piston_state.phase = AdvancePhase::BetweenTicks;
                    self.waveform.tick(self.piston_state.logical_tick, false);
                    return false;
                }
            }
        }
    }

    fn advance_operation(&mut self) -> bool {
        if !self.prepare_operation() {
            return false;
        }
        match self.piston_state.phase {
            AdvancePhase::ScheduledTicks => {
                let tick = self.to_be_ticked.this_tick().pop_first().unwrap();
                #[cfg(test)]
                redstone::instant_piston_tests::record_operation(
                    self,
                    "scheduled_tick",
                    serde_json::json!({"pos":tick.pos,"block_type":tick.block_type}),
                );
                self.tick_index.popped(tick);
                let block = self.get_block(tick.pos);
                if tick.block_type == Some(block.registry_id()) {
                    redstone::tick(block, self, tick.pos);
                }
            }
            AdvancePhase::PistonEvents => {
                let event = self.piston_state.events.pop_front().unwrap();
                self.piston_index.get_mut().popped(event);
                redstone::piston::execute_event(self, event);
            }
            AdvancePhase::MovingEntities => {
                let (pos, identity) = self.piston_state.movement_work[self.piston_state.movement_cursor];
                self.piston_state.movement_cursor += 1;
                #[cfg(test)]
                redstone::instant_piston_tests::record_operation(self, "motion", serde_json::json!([pos, identity]));
                redstone::piston::tick_motion(self, pos, identity);
            }
            AdvancePhase::BetweenTicks => unreachable!(),
        }
        true
    }

    fn current_phase_operation_count(&self) -> usize {
        match self.piston_state.phase {
            AdvancePhase::ScheduledTicks => self.to_be_ticked.this_tick_ref().len(),
            AdvancePhase::PistonEvents => self.piston_state.events.len(),
            AdvancePhase::MovingEntities => self.piston_state.movement_work.len() - self.piston_state.movement_cursor,
            AdvancePhase::BetweenTicks => 0,
        }
    }

    pub fn tick_interpreted(&mut self) {
        self.record_tick();
        while self.advance_operation() {}
    }

    pub fn nanotick_advance(&mut self, amount: u32) {
        // Production commands reject this; protect other callers as well.
        if self.history.enabled() {
            return;
        }
        for _ in 0..amount {
            if !self.prepare_operation() {
                continue;
            }
            let count = self.current_phase_operation_count();
            for _ in 0..count {
                self.advance_operation();
            }
        }
    }

    pub fn picotick_advance(&mut self, amount: u32) {
        if self.history.enabled() {
            return;
        }
        for _ in 0..amount {
            self.advance_operation();
        }
    }
}
