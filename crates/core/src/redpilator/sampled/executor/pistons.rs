use super::super::ScratchError;
use super::{Motion, PistonEvent, Runtime};
use mchprs_blocks::blocks::Block;
use std::num::NonZeroUsize;

impl Runtime {
    pub(in crate::redpilator::sampled) fn queue_piston_extension(&mut self, index: usize) {
        if self.queued_piston_events[index] & 1 == 0 {
            self.queued_piston_events[index] |= 1;
            self.piston_events.push_back((index, PistonEvent::Extend));
        }
    }

    fn pushing_motion(&self, piston: usize) -> Option<usize> {
        let slot = self.cell_motion[self.program.piston_bindings[piston].far_cell]?.get() - 1;
        let motion = self.motions[slot];
        (motion.extending && self.program.pistons[motion.piston].facing == self.program.pistons[piston].facing).then_some(slot)
    }

    fn is_early_retract(&self, motion: usize) -> bool {
        let motion = self.motions[motion];
        if self.motion_phase && self.motion_time.saturating_sub(motion.start) > 1 {
            self.landing_cursor.is_some_and(|cursor| motion.order < cursor)
        } else {
            true
        }
    }

    fn add_motion(&mut self, piston: usize, token: Option<usize>, destination: usize, extending: bool, piston_part: bool) {
        let landing = self.motion_time + 2;
        let order = self
            .active_motions
            .back()
            .map(|&slot| self.motions[slot])
            .filter(|motion| motion.landing == landing)
            .map_or(0, |motion| motion.order + 1);
        self.insert_motion(Motion {
            piston,
            token,
            destination,
            start: self.motion_time,
            landing,
            order,
            extending,
            piston_done: !piston_part,
            token_done: token.is_none(),
        });
    }

    /// Allocate or reuse a runtime motion slot.
    ///
    /// Inputs: The new motion record and this runtime's active/free slot lists.
    ///
    /// Reject capacity exhaustion, obtain a free or new slot, link it into motion order, and attach it to
    /// the destination cell and piston indexes.
    fn insert_motion(&mut self, motion: Motion) -> Option<usize> {
        let slot = if let Some(slot) = self.free_motion_slots.pop() {
            self.motions[slot] = motion;
            slot
        } else {
            if self.motions.len() == self.motions.capacity() {
                self.failure = Some(ScratchError {
                    position: Some(self.program.pistons[motion.piston].base),
                    reason: "runtime motion capacity exceeded",
                });
                return None;
            }
            self.motions.push(motion);
            self.motions.len() - 1
        };
        if !motion.piston_done {
            self.mark_piston_dirty(motion.piston);
            self.piston_motion[motion.piston] = NonZeroUsize::new(slot + 1);
        }
        if motion.token.is_some() && !motion.token_done {
            debug_assert!(self.cell_motion[motion.destination].is_none());
            self.cell_motion[motion.destination] = NonZeroUsize::new(slot + 1);
            self.mark_dirty(motion.destination);
        }
        self.active_motions.push_back(slot);
        Some(slot)
    }

    #[cfg(test)]
    pub(in crate::redpilator::sampled) fn add_test_motion(&mut self, motion: Motion) {
        self.insert_motion(motion).expect("test motion capacity");
        self.active_motions
            .make_contiguous()
            .sort_by_key(|&slot| (self.motions[slot].landing, self.motions[slot].order));
    }

    /// Decide whether a piston should queue an extension or retraction.
    ///
    /// Inputs: The piston component ID and its current input, geometry and motion state.
    ///
    /// Resolve the piston binding, inspect power and blocked cells, and queue the supported action while
    /// suppressing duplicate pending actions.
    pub(in crate::redpilator::sampled) fn update_piston(&mut self, component: usize) {
        let Some(index) = self.program.piston_by_component[component].map(|index| index.get() - 1) else {
            return;
        };
        if self.piston_base_moving[index] {
            return;
        }
        let powered = self.has_input(component);
        #[cfg(any(test, feature = "redpilator-trace"))]
        if runtime_trace_enabled!(self) {
            let _fixed: Vec<_> = self.program.incoming[component]
                .iter()
                .map(|&link| (self.program.positions[self.program.links[link].source], self.contributions[link]))
                .collect();
            let _token_inputs = self.token_dependency_signature(usize::MAX, component, None);
            runtime_trace!(
                self,
                "piston sample {:?} powered={powered} extended={} fixed={_fixed:?} token_inputs={_token_inputs:?}",
                self.program.pistons[index].base,
                self.piston_extended[index]
            );
        }
        if powered == self.piston_extended[index] {
            return;
        }
        let pushing = self.pushing_motion(index);
        let event = if powered {
            PistonEvent::Extend
        } else if pushing.is_some_and(|motion| self.is_early_retract(motion)) {
            PistonEvent::EarlyRetract
        } else {
            PistonEvent::Retract
        };
        let bit = event.bit();
        if self.queued_piston_events[index] & bit == 0 {
            self.queued_piston_events[index] |= bit;
            self.piston_events.push_back((index, event));
            #[cfg(any(test, feature = "redpilator-trace"))]
            if runtime_trace_enabled!(self) {
                runtime_trace!(self, "piston request {:?} extending={powered}", self.program.pistons[index].base);
            }
        }
    }

    /// Execute queued piston actions in their original order.
    ///
    /// Inputs: This runtime's piston event queue, payload positions and active motions.
    ///
    /// Recheck each action against current state, resolve interrupted or competing movement, then create
    /// extension/retraction motions and deliver their notifications.
    pub(super) fn run_piston_events(&mut self) {
        while let Some((index, event)) = self.piston_events.pop_front() {
            let extending = event == PistonEvent::Extend;
            let piston = self.program.pistons[index];
            if !self.consume_operation(piston.base) {
                return;
            }
            self.queued_piston_events[index] &= !event.bit();
            let component = self.program.piston_bindings[index].component;
            self.updated_in_phase[component] = 0;
            if self.piston_base_moving[index] || (self.has_input(component)) != extending {
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(
                        self,
                        "piston rejected {:?} extending={extending} reason=power_or_base_motion",
                        piston.base
                    );
                }
                continue;
            }
            if !extending {
                let early = event == PistonEvent::EarlyRetract;
                if self.piston_head_moving[index] {
                    if let Some(slot) = self.piston_motion[index] {
                        self.finish_piston_part(slot.get() - 1, true);
                    }
                }
                self.piston_base_moving[index] = true;
                let token = if piston.sticky && !early {
                    self.token_at(self.program.piston_bindings[index].far_cell)
                } else {
                    None
                };
                self.add_motion(index, token, self.program.piston_bindings[index].home_cell, false, true);
                self.notify_source_watchers(component, true, true);
                if let Some(token) = token {
                    self.token_movements += 1;
                    #[cfg(any(test, feature = "redpilator-trace"))]
                    if runtime_trace_enabled!(self) {
                        runtime_trace!(self, "token {token} departs {:?} for {:?}", piston.far, piston.home);
                    }
                    self.set_token_pos(token, None);
                }
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(
                        self,
                        "piston accepted {:?} retract token={token:?} landing={}",
                        piston.base,
                        self.motion_time + 2
                    );
                }
                self.accepted_piston_actions += 1;
                if piston.sticky {
                    if let Some(slot) = self.pushing_motion(index) {
                        self.finish_token_part(slot);
                        self.notify_home_watchers(index, false);
                        continue;
                    }
                }
                if self.piston_head_present[index] {
                    self.piston_head_present[index] = false;
                    self.notify_shape(self.program.piston_bindings[index].home_cell);
                }
                self.notify_home_watchers(index, false);
                continue;
            }
            if self.piston_extended[index] || self.piston_head_present[index] || self.piston_head_moving[index] {
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(self, "piston rejected {:?} extend reason=head_out", piston.base);
                }
                continue;
            }
            if self.program.piston_bindings[index].home_blocked
                || self.cell_motion[self.program.piston_bindings[index].home_cell].is_some()
                || self.program.piston_bindings[index]
                    .home_heads
                    .iter()
                    .any(|&other| self.piston_head_present[other] || self.piston_head_moving[other])
            {
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(self, "piston rejected {:?} extend reason=home_blocked", piston.base);
                }
                continue;
            }
            let token = self.token_at(self.program.piston_bindings[index].home_cell);
            let onward = token.and_then(|_| self.token_at(self.program.piston_bindings[index].far_cell));
            let mate = self.program.piston_bindings[index].mate;
            let mate_home_open = mate.is_some_and(|mate| {
                !self.program.piston_bindings[mate].home_blocked
                    && self.token_at(self.program.piston_bindings[mate].home_cell).is_none()
                    && !self.cell_motion[self.program.piston_bindings[mate].home_cell].is_some()
                    && !self.piston_head_present[mate]
                    && !self.piston_head_moving[mate]
            });
            let far_blocked = self.program.piston_bindings[index].far_blocked
                || self.cell_motion[self.program.piston_bindings[index].far_cell].is_some()
                || self.program.piston_bindings[index]
                    .far_heads
                    .iter()
                    .any(|&other| self.piston_head_present[other] || self.piston_head_moving[other])
                || (onward.is_some() && !mate_home_open);
            if token.is_some() && far_blocked {
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(self, "piston rejected {:?} extend reason=far_blocked", piston.base);
                }
                continue;
            }
            if token.is_some_and(|token| !self.program.tokens[token].cells.contains(&piston.far)) {
                self.failure = Some(ScratchError {
                    position: Some(piston.far),
                    reason: "unsupported token destination",
                });
                return;
            }
            if let Some(onward) = onward {
                let mate = mate.expect("an opposed piston is required to push its mate token onward");
                self.token_movements += 1;
                self.set_token_pos(onward, None);
                self.add_motion(mate, Some(onward), self.program.piston_bindings[mate].home_cell, true, false);
                if self.failure.is_some() {
                    return;
                }
            }
            if let Some(token) = token {
                self.token_movements += 1;
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(self, "token {token} departs {:?} for {:?}", piston.home, piston.far);
                }
                self.set_token_pos(token, None);
            }
            self.piston_extended[index] = true;
            if let Block::Piston { mut piston } = self.blocks[component] {
                piston.extended = true;
                self.set_component(component, Block::Piston { piston });
            }
            self.piston_head_moving[index] = true;
            self.add_motion(index, token, self.program.piston_bindings[index].far_cell, true, true);
            #[cfg(any(test, feature = "redpilator-trace"))]
            if runtime_trace_enabled!(self) {
                runtime_trace!(
                    self,
                    "piston accepted {:?} extend token={token:?} from={:?} to={:?} landing={}",
                    piston.base,
                    piston.home,
                    piston.far,
                    self.motion_time + 2
                );
            }
            self.notify_shape(self.program.piston_bindings[index].home_cell);
            // Empty head movement still delivers neighbor power callbacks.
            self.notify_home_watchers(index, false);
            if token.is_some() {
                self.notify_shape(self.program.piston_bindings[index].far_cell);
            }
            self.notify_source_watchers(component, true, true);
            self.accepted_piston_actions += 1;
        }
    }

    fn finish_token_part(&mut self, slot: usize) {
        let motion = self.motions[slot];
        if let Some(token) = motion.token
            && !motion.token_done
        {
            self.motions[slot].token_done = true;
            self.cell_motion[motion.destination] = None;
            self.set_token_pos(token, Some(self.program.projection_positions[motion.destination]));
        }
    }

    /// Complete the piston-head or base portion of a motion.
    ///
    /// Inputs: The motion slot, an interruption flag and this runtime's piston state.
    ///
    /// Mark that portion finished, restore the stationary head/base flags, dirty the affected cells and
    /// send the required shape and home notifications.
    pub(in crate::redpilator::sampled) fn finish_piston_part(&mut self, slot: usize, interrupted: bool) {
        let motion = self.motions[slot];
        if motion.piston_done {
            return;
        }
        self.motions[slot].piston_done = true;
        self.mark_piston_dirty(motion.piston);
        if self.piston_motion[motion.piston] == NonZeroUsize::new(slot + 1) {
            self.piston_motion[motion.piston] = None;
        }
        let component = self.program.piston_bindings[motion.piston].component;
        let home = self.program.piston_bindings[motion.piston].home_cell;
        self.updated_in_phase[component] = 0;
        if motion.extending {
            self.piston_head_moving[motion.piston] = false;
            self.piston_head_present[motion.piston] = !interrupted;
            self.notify_shape(home);
            self.notify_home_watchers(motion.piston, false);
        } else {
            self.piston_base_moving[motion.piston] = false;
            self.piston_extended[motion.piston] = false;
            if let Block::Piston { mut piston } = self.blocks[component] {
                piston.extended = false;
                self.set_component(component, Block::Piston { piston });
                self.notify_source_watchers(component, true, true);
            }
        }
        if !interrupted {
            self.update_piston(component);
        }
    }

    /// Advance active piston motions and complete due landings.
    ///
    /// Inputs: This runtime's motion clock, ordered active slots and landing cursor.
    ///
    /// Walk active motions in order, finish due piston and token parts, preserve interruption ordering,
    /// and return fully completed slots to the free list.
    pub(super) fn tick_motions(&mut self) {
        self.motion_phase = true;
        // Every motion lands two ticks after insertion, so FIFO is also landing order.
        while let Some(&slot) = self.active_motions.front() {
            let motion = self.motions[slot];
            if motion.landing != self.motion_time {
                break;
            }
            self.landing_cursor = Some(motion.order);
            if !motion.piston_done || !motion.token_done {
                if !self.consume_operation(self.program.pistons[motion.piston].base) {
                    break;
                }
                self.motion_completions += 1;
                if motion.extending {
                    self.finish_token_part(slot);
                    self.finish_piston_part(slot, false);
                } else {
                    self.finish_piston_part(slot, false);
                    self.finish_token_part(slot);
                }
                #[cfg(any(test, feature = "redpilator-trace"))]
                if runtime_trace_enabled!(self) {
                    runtime_trace!(
                        self,
                        "motion complete {:?} token={:?} destination={:?} extending={}",
                        self.program.pistons[motion.piston].base,
                        motion.token,
                        self.program.projection_positions[motion.destination],
                        motion.extending
                    );
                }
            }
            self.active_motions.pop_front();
            self.free_motion_slots.push(slot);
        }
        self.landing_cursor = None;
        self.motion_phase = false;
    }
}
