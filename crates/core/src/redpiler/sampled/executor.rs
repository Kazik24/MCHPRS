use super::program::{ArrangementSource, ComparatorInput};
use super::{Operation, Program, ScratchError};
use mchprs_blocks::blocks::Block;
use mchprs_world::TickPriority;
use std::num::NonZeroUsize;

#[cfg(any(test, feature = "redpiler-trace"))]
macro_rules! runtime_trace {
    ($runtime:expr, $($args:tt)*) => {{
        #[cfg(all(not(test), feature = "redpiler-trace"))]
        tracing::trace!(target: "mchprs_core::redpiler", tick = $runtime.motion_time, phase = $runtime.phase, $($args)*);
    }};
}

#[cfg(any(test, feature = "redpiler-trace"))]
macro_rules! runtime_trace_enabled {
    ($runtime:expr) => {{
        #[cfg(test)]
        { false }
        #[cfg(all(not(test), feature = "redpiler-trace"))]
        { tracing::enabled!(target: "mchprs_core::redpiler", tracing::Level::TRACE) }
    }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PistonEvent {
    Extend,
    Retract,
    EarlyRetract,
}

impl PistonEvent {
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Motion {
    /// index into `Program::pistons` for the actor performing this motion.
    pub(super) piston: usize,
    /// index into `Program::tokens` for the carried token, or `None` for an empty piston.
    pub(super) token: Option<usize>,
    /// projection-cell index where the token is scheduled to land.
    pub(super) destination: usize,
    /// motion-clock tick when the animation starts.
    pub(super) start: u64,
    /// motion-clock tick when the token lands.
    pub(super) landing: u64,
    /// ordering key preserving native piston event ordering.
    pub(super) order: usize,
    /// true while extending and false while retracting.
    pub(super) extending: bool,
    /// indicating the piston base has completed its part of the motion.
    pub(super) piston_done: bool,
    /// indicating the carried token has completed its part of the motion.
    pub(super) token_done: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScheduledComponentTick {
    /// component index into `Program::positions` whose scheduled tick will run.
    pub(super) component: usize,
    /// block identity used to discard stale ticks after the component changes type.
    pub(super) block_type: Option<u32>,
}

pub(crate) struct Runtime {
    // Compiled program and current blocks
    /// immutable compiled topology and instruction data shared with runtime readers.
    pub(super) program: std::sync::Arc<Program>,
    /// component-indexed current block states, parallel to `Program::positions`.
    pub(super) blocks: Vec<Block>,

    // Electrical inputs and wire state
    /// ordinary electrical-link-indexed current source contribution strengths.
    pub(super) contributions: Vec<u8>,
    /// arrangement-link-indexed current contribution level, in 0–15.
    pub(super) arrangement_contributions: Vec<u8>,
    pub(super) routing_attenuations: Vec<Option<u8>>,
    pub(super) route_search: super::routing::Search,
    #[cfg(feature = "redpiler-trace")]
    pub(super) routing_metrics: [u64; 3],
    /// per-component input aggregates for primary and side input channels.
    pub(super) input_levels: Vec<[super::input::InputState; 2]>,
    /// analog strength histograms referenced by `InputState::analog` slots.
    pub(super) input_histograms: Vec<super::input::Histogram>,
    /// component-indexed current comparator output levels, in 0–15.
    pub(super) comparator_outputs: Vec<u8>,
    /// reactive-wire-indexed current wire strengths, in 0–15.
    pub(super) reactive_wire_strengths: Vec<u8>,
    /// watched-wire-indexed last published strength, in 0–15.
    pub(super) watched_strengths: Vec<u8>,

    // Scheduled ticks and update phases
    /// pending component ticks arranged by native delay and priority.
    pub(super) pending: super::scheduler::Scheduler,
    /// component-indexed number of outstanding scheduled ticks.
    pub(super) pending_counts: Vec<usize>,
    /// positions with scheduled passive work that is not tied to a component index.
    pub(super) pending_passive_positions: Vec<mchprs_blocks::BlockPos>,
    /// monotonically increasing execution phase ID used to suppress duplicate updates.
    pub(super) phase: u64,
    /// component-indexed last update phase; zero means not updated in this phase.
    pub(super) updated_in_phase: Vec<u64>,

    // Pistons, tokens, and arrangements
    /// native piston event/motion state captured at compilation and advanced at runtime.
    pub(super) piston_state: mchprs_world::PistonState,
    /// piston-indexed current extended state.
    pub(super) piston_extended: Vec<bool>,
    /// piston-indexed whether a head currently occupies the projected head cell.
    pub(super) piston_head_present: Vec<bool>,
    /// piston-indexed whether the head is participating in an active motion.
    pub(super) piston_head_moving: Vec<bool>,
    /// piston-indexed whether the base is participating in an active motion.
    pub(super) piston_base_moving: Vec<bool>,
    /// token-indexed current world position; `None` means currently carried/in transit.
    pub(super) token_positions: Vec<Option<mchprs_blocks::BlockPos>>,
    /// arrangement-group-indexed encoded active token-position state.
    pub(super) group_arrangements: Vec<usize>,
    /// pending piston index/event pairs in processing order.
    pub(super) piston_events: std::collections::VecDeque<(usize, PistonEvent)>,
    /// piston-indexed bitsets of event kinds already queued for that piston.
    pub(super) queued_piston_events: Vec<u8>,

    // Motion storage and clock
    /// allocated motion records, including records currently active or reusable.
    pub(super) motions: Vec<Motion>,
    /// active indices into `motions` in landing/processing order.
    pub(super) active_motions: std::collections::VecDeque<usize>,
    /// reusable indices into `motions`.
    pub(super) free_motion_slots: Vec<usize>,
    /// piston-indexed active motion index plus one for each piston.
    pub(super) piston_motion: Vec<Option<NonZeroUsize>>,
    /// projection-cell-indexed active motion index plus one occupying each cell.
    pub(super) cell_motion: Vec<Option<NonZeroUsize>>,
    /// elapsed piston-motion clock in ticks.
    pub(super) motion_time: u64,
    /// current half-tick phase for motion progression.
    pub(super) motion_phase: bool,
    /// cursor into active motion ordering for landing processing.
    pub(super) landing_cursor: Option<usize>,

    // Movement working buffers
    /// component indices whose movement-dependent inputs switched this phase.
    pub(super) movement_switched: Vec<usize>,
    /// component-indexed deduplication flags for `movement_switched`.
    pub(super) switched: Vec<bool>,
    /// watched-wire indices whose raw strength changed this phase.
    pub(super) movement_changed_wires: Vec<usize>,
    /// watched-wire-indexed deduplication flags for `movement_changed_wires`.
    pub(super) changed_wires: Vec<bool>,

    // Commands and notes
    /// mutable command-block entities keyed by world position.
    pub(super) command_blocks: std::collections::HashMap<mchprs_blocks::BlockPos, Box<mchprs_blocks::block_entities::CommandBlockEntity>>,
    /// mutable world-level command output state.
    pub(super) command_output_state: Option<crate::world::CommandBlockOutputState>,
    /// command outputs generated during execution and awaiting publication.
    pub(super) command_emissions: Vec<crate::world::CommandBlockEmission>,
    /// queued notes as component, position, instrument, and sound data.
    pub(crate) notes: Vec<(usize, mchprs_blocks::BlockPos, mchprs_blocks::blocks::Instrument, u32)>,
    /// component-indexed flags preventing duplicate note queue entries.
    pub(crate) note_queued: Vec<bool>,

    // Publication
    /// projection-cell indices changed since the last publication.
    pub(super) dirty_cells: Vec<usize>,
    /// projection-cell-indexed flags marking cells that need publication.
    pub(super) dirty: Vec<bool>,
    /// forcing publication to rebuild the complete projected world.
    pub(super) needs_full_publication: bool,

    // Accounting and failure
    /// count of accepted piston actions in the current execution window.
    pub(super) accepted_piston_actions: usize,
    /// count of token movement events processed.
    pub(super) token_movements: usize,
    /// count of completed motions.
    pub(super) motion_completions: usize,
    /// count of executed compiled operations in the current accounting interval.
    pub(super) operations: usize,
    /// count of delivered neighbor/update callbacks in the current accounting interval.
    pub(super) deliveries: usize,
    /// count of scheduled repeater ticks in the current accounting interval.
    pub(super) scheduled_repeater_ticks: usize,
    /// count of operations performed while processing scheduled ticks.
    pub(super) tick_operations: usize,
    /// first runtime failure, retained to stop further state changes safely.
    pub(super) failure: Option<ScratchError>,
}

pub(super) fn compare(rear: u8, side: u8, mode: mchprs_blocks::blocks::ComparatorMode) -> (u8, bool) {
    let subtract = mode == mchprs_blocks::blocks::ComparatorMode::Subtract;
    let powered = rear > 0 && (rear > side || (rear == side && !subtract));
    (
        if side > rear {
            0
        } else if subtract {
            rear - side
        } else {
            rear
        },
        powered,
    )
}

impl Runtime {
    pub(super) fn mark_dirty(&mut self, cell: usize) {
        if !self.dirty[cell] {
            self.dirty[cell] = true;
            self.dirty_cells.push(cell);
        }
    }

    fn set_component(&mut self, component: usize, block: Block) {
        if self.blocks[component] != block {
            let old = self.output(component);
            self.blocks[component] = block;
            self.update_side_inputs(component, old, self.output(component));
            self.mark_dirty(self.program.component_cells[component]);
        }
    }

    fn mark_piston_dirty(&mut self, piston: usize) {
        self.mark_dirty(self.program.component_cells[self.program.piston_bindings[piston].component]);
        self.mark_dirty(self.program.piston_bindings[piston].home_cell);
    }

    pub(super) fn notify_source_watchers(&mut self, source: usize, shape: bool, update: bool) {
        for index in 0..self.program.source_watchers[source].len() {
            let watcher = self.program.source_watchers[source][index];
            if !self.consume_component_operation(watcher) {
                return;
            }
            let delivered = if matches!(self.blocks[watcher], Block::Observer { .. }) {
                shape
            } else {
                update
            };
            #[cfg(any(test, feature = "redpiler-trace"))]
            if runtime_trace_enabled!(self) && delivered {
                runtime_trace!(
                    self,
                    "deliver {:?} -> {:?} shape={shape} update={update}",
                    self.program.positions[source],
                    self.program.positions[watcher]
                );
            }
            if delivered {
                self.update_once(watcher);
            }
        }
    }

    fn notify_home_watchers(&mut self, piston: usize, shape_only: bool) {
        let count = self.program.home_watchers.get(piston).map_or(0, |watchers| watchers.len());
        for index in 0..count {
            let watcher = self.program.home_watchers[piston][index];
            if !self.consume_component_operation(watcher) {
                return;
            }
            if !shape_only || matches!(self.blocks[watcher], Block::Observer { .. }) {
                self.update_once(watcher);
            }
        }
    }
    pub(crate) fn project(&self, position: mchprs_blocks::BlockPos) -> Option<Block> {
        if let Some(&cell) = self.program.projection_indices.get(&position) {
            self.project_cell(cell)
        } else {
            Some(self.program.geometry_indices.get(&position).copied().unwrap_or(Block::Air))
        }
    }

    fn token_at(&self, cell: usize) -> Option<usize> {
        self.program.cell_tokens[cell]
            .iter()
            .copied()
            .find(|&token| self.token_positions[token] == Some(self.program.projection_positions[cell]))
    }

    pub(super) fn project_cell(&self, cell: usize) -> Option<Block> {
        let position = self.program.projection_positions[cell];
        if self.cell_motion[cell].is_some() {
            return None;
        }
        let tokens = &self.program.cell_tokens[cell];
        if let Some(index) = self.token_at(cell) {
            return Some(self.program.tokens[index].block);
        }
        for &index in &self.program.cell_pistons[cell] {
            let piston = &self.program.pistons[index];
            if position == piston.base && self.piston_base_moving[index] {
                return None;
            }
            if position == piston.home {
                if self.piston_head_moving[index] {
                    return None;
                }
                if self.piston_head_present[index] {
                    return Some(Block::PistonHead {
                        head: mchprs_blocks::blocks::RedstonePistonHead {
                            facing: piston.facing,
                            sticky: piston.sticky,
                            short: false,
                        },
                    });
                }
            }
        }
        if tokens.iter().any(|&index| self.program.tokens[index].position == Some(position)) {
            return Some(Block::Air);
        }
        Some(
            self.program.cell_components[cell]
                .map(|index| index.get() - 1)
                .map_or(self.program.cell_geometry[cell], |component| self.blocks[component]),
        )
    }
    fn notify_shape(&mut self, cell: usize) {
        let position = self.program.projection_positions[cell];
        for offset in 0..self.program.cell_observers[cell].len() {
            let index = self.program.cell_observers[cell][offset];
            let block = self.blocks[index];
            if let Block::Observer { observer } = block
                && self.program.positions[index].offset(observer.facing.into()) == position
                && !observer.powered
            {
                if !self.consume_component_operation(index) {
                    return;
                }
                let tick = ScheduledComponentTick {
                    component: index,
                    block_type: Some(block.registry_id()),
                };
                if !self.has_pending(tick) {
                    self.schedule_tick(tick, 1, TickPriority::Normal);
                    #[cfg(any(test, feature = "redpiler-trace"))]
                    if runtime_trace_enabled!(self) {
                        runtime_trace!(self, "observer notified {:?} by shape at {position:?}", self.program.positions[index]);
                    }
                }
            }
        }
    }
    pub(super) fn queue_piston_extension(&mut self, index: usize) {
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
    pub(super) fn add_test_motion(&mut self, motion: Motion) {
        self.insert_motion(motion).expect("test motion capacity");
        self.active_motions
            .make_contiguous()
            .sort_by_key(|&slot| (self.motions[slot].landing, self.motions[slot].order));
    }

    pub(super) fn update_piston(&mut self, component: usize) {
        let Some(index) = self.program.piston_by_component[component].map(|index| index.get() - 1) else {
            return;
        };
        if self.piston_base_moving[index] {
            return;
        }
        let powered = self.has_input(component);
        #[cfg(any(test, feature = "redpiler-trace"))]
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
            #[cfg(any(test, feature = "redpiler-trace"))]
            if runtime_trace_enabled!(self) {
                runtime_trace!(self, "piston request {:?} extending={powered}", self.program.pistons[index].base);
            }
        }
    }

    fn run_piston_events(&mut self) {
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
                #[cfg(any(test, feature = "redpiler-trace"))]
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
                    #[cfg(any(test, feature = "redpiler-trace"))]
                    if runtime_trace_enabled!(self) {
                        runtime_trace!(self, "token {token} departs {:?} for {:?}", piston.far, piston.home);
                    }
                    self.set_token_pos(token, None);
                }
                #[cfg(any(test, feature = "redpiler-trace"))]
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
                #[cfg(any(test, feature = "redpiler-trace"))]
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
                #[cfg(any(test, feature = "redpiler-trace"))]
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
                #[cfg(any(test, feature = "redpiler-trace"))]
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
                #[cfg(any(test, feature = "redpiler-trace"))]
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
            #[cfg(any(test, feature = "redpiler-trace"))]
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

    pub(super) fn finish_piston_part(&mut self, slot: usize, interrupted: bool) {
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

    fn tick_motions(&mut self) {
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
                #[cfg(any(test, feature = "redpiler-trace"))]
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
    fn consume_operation(&mut self, position: mchprs_blocks::BlockPos) -> bool {
        self.consume_operation_with(|_| position)
    }

    fn consume_component_operation(&mut self, component: usize) -> bool {
        self.consume_operation_with(|program| program.positions[component])
    }

    fn consume_operation_with(&mut self, position: impl FnOnce(&super::program::Program) -> mchprs_blocks::BlockPos) -> bool {
        if self.failure.is_some() {
            return false;
        }
        if self.tick_operations >= 1_000_000 {
            self.failure = Some(ScratchError {
                position: Some(position(&self.program)),
                reason: "runtime semantic operation limit exceeded",
            });
            return false;
        }
        self.tick_operations += 1;
        true
    }

    pub(super) fn input_strength(&self, target: usize) -> u8 {
        let input = &self.input_levels[target][0];
        if input.analog != u32::MAX {
            return input.maximum(&self.input_histograms);
        }
        let fixed = self.program.incoming[target]
            .iter()
            .copied()
            .filter(|&index| self.program.effective_links[index])
            .map(|index| self.contributions[index]);
        let arranged = self
            .program
            .arrangement_links
            .iter()
            .enumerate()
            .filter(|(_, link)| link.target == target && !link.side)
            .map(|(index, _)| self.arrangement_contributions[index]);
        fixed.chain(arranged).max().unwrap_or(0)
    }

    pub(super) fn has_input(&self, target: usize) -> bool {
        self.input_levels[target][0].active != 0
    }

    fn set_contribution(&mut self, index: usize, value: u8) {
        let old = self.contributions[index];
        self.contributions[index] = value;
        if self.program.effective_links[index] {
            self.input_levels[self.program.links[index].target][0].change(old, value, &mut self.input_histograms);
        }
    }

    fn set_arrangement_contribution(&mut self, index: usize, target: usize, side: bool, value: u8) {
        let old = self.arrangement_contributions[index];
        if old == value { return; }
        self.arrangement_contributions[index] = value;
        self.input_levels[target][usize::from(side)].change(old, value, &mut self.input_histograms);
    }

    fn update_side_inputs(&mut self, source: usize, old: u8, new: u8) {
        if old == new {
            return;
        }
        for index in 0..self.program.side_outputs[source].len() {
            let (target, attenuation) = self.program.side_outputs[source][index];
            self.input_levels[target][1].change(
                old.saturating_sub(attenuation),
                new.saturating_sub(attenuation),
                &mut self.input_histograms,
            );
        }
    }

    pub(super) fn arrangement_contribution(&self, index: usize) -> (usize, bool, u8) {
        let link = &self.program.arrangement_links[index];
        let value = if link.active[self.group_arrangements[link.group]] {
            let attenuation = self.routing_attenuations.get(index).copied().flatten().unwrap_or(link.attenuation);
            self.arrangement_output(link.source).saturating_sub(attenuation)
        } else {
            0
        };
        (link.target, link.side, value)
    }

    fn routed_values(&mut self, changed: Option<usize>) -> Result<Vec<(usize, u8)>, ScratchError> {
        let program = std::sync::Arc::clone(&self.program);
        let Some(graph) = &program.routing else { return Ok(Vec::new()); };
        let readers: Vec<_> = changed.map_or_else(|| (0..graph.reader_count()).collect(), |token| graph.readers_for_token(token).to_vec());
        #[cfg(feature = "redpiler-trace")]
        if changed.is_some() {
            self.routing_metrics[0] += 1;
            self.routing_metrics[1] += readers.len() as u64;
        }
        self.route_search.remaining = changed.map(|_| 1_000_000usize.saturating_sub(self.tick_operations));
        self.route_search.spent = 0;
        let mut updates = Vec::new();
        for reader in readers {
            let values = graph.reader_sources(reader, |super::routing::Axis::Token(token, _)| {
                self.token_positions[token].and_then(|pos| program.tokens[token].cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1)
            }, &mut self.route_search);
            if changed.is_some() {
                #[cfg(feature = "redpiler-trace")]
                { self.routing_metrics[2] += self.route_search.spent as u64; }
                self.tick_operations += self.route_search.spent;
                self.operations += self.route_search.spent;
            }
            self.route_search.spent = 0;
            let values = values?;
            for &index in graph.links(reader) {
                let link = &program.arrangement_links[index];
                updates.push((index, values.get(&(link.source, link.side)).copied().unwrap_or(u8::MAX)));
            }
        }
        Ok(updates)
    }

    pub(super) fn initialize_routing(&mut self) {
        match self.routed_values(None) {
            Ok(values) => for (index, attenuation) in values { self.routing_attenuations[index] = Some(attenuation); },
            Err(error) => self.failure = Some(error),
        }
    }

    fn refresh_routing(&mut self, token: usize, switched: &mut Vec<usize>) {
        if self.program.routing.as_ref().is_none_or(|graph| graph.readers_for_token(token).is_empty()) { return; }
        let updates = match self.routed_values(Some(token)) {
            Ok(values) => values,
            Err(error) => { self.failure = Some(error); return; }
        };
        for (index, attenuation) in updates {
            let old = self.routing_attenuations[index].unwrap_or(u8::MAX);
            if old == attenuation { continue; }
            let link = self.program.arrangement_links[index].clone();
            let strength = self.arrangement_output(link.source);
            if old != u8::MAX {
                if !self.consume_component_operation(link.target) { return; }
                self.routing_attenuations[index] = Some(u8::MAX);
                self.set_arrangement_contribution(index, link.target, link.side, 0);
                self.operations += 1;
            }
            if attenuation != u8::MAX {
                if !self.consume_component_operation(link.target) { return; }
                self.routing_attenuations[index] = Some(attenuation);
                self.set_arrangement_contribution(index, link.target, link.side, strength.saturating_sub(attenuation));
                self.operations += 1;
            }
            if strength > old || strength > attenuation { self.updated_in_phase[link.target] = 0; }
            if !matches!(self.blocks[link.target], Block::Piston { .. }) && !self.switched[link.target] {
                self.switched[link.target] = true;
                switched.push(link.target);
            }
        }
    }

    pub(super) fn arrangement_output(&self, source: ArrangementSource) -> u8 {
        match source {
            ArrangementSource::Component(node) => self.output(node),
            ArrangementSource::Token(token) => {
                if matches!(self.program.tokens[token].block, Block::RedstoneBlock) {
                    15
                } else {
                    0
                }
            }
        }
    }

    pub(super) fn set_token_pos(&mut self, token: usize, position: Option<mchprs_blocks::BlockPos>) {
        let old = self.token_positions[token];
        if old == position {
            return;
        }
        let Some(cell_pos) = position.or(old) else {
            return;
        };
        let Some(cell) = self.program.tokens[token].cells.iter().position(|&at| at == cell_pos) else {
            self.failure = Some(ScratchError {
                position: Some(cell_pos),
                reason: "unsupported token position",
            });
            return;
        };
        if old.is_some() && position.is_some() {
            self.failure = Some(ScratchError {
                position: Some(cell_pos),
                reason: "token relocation requires departure before arrival",
            });
            return;
        }
        self.token_positions[token] = position;
        self.mark_dirty(self.program.token_cells[token][cell]);
        let mut switched = std::mem::take(&mut self.movement_switched);
        let mut changed_wires = std::mem::take(&mut self.movement_changed_wires);
        self.run_move_steps(token, cell, position.is_some(), &mut switched, &mut changed_wires);
        for &node in switched.iter() {
            self.switched[node] = false;
        }
        for &wire in &changed_wires {
            self.changed_wires[wire] = false;
        }
        switched.clear();
        changed_wires.clear();
        self.movement_switched = switched;
        self.movement_changed_wires = changed_wires;
    }

    fn run_move_steps(&mut self, token: usize, cell: usize, arriving: bool, switched: &mut Vec<usize>, changed_wires: &mut Vec<usize>) {
        self.refresh_routing(token, switched);
        if self.failure.is_some() { return; }
        for &component in &self.program.token_piston_components[token] {
            self.updated_in_phase[component] = 0;
        }
        let transitions = self.program.movement_cells.get(token).and_then(|cells| cells.get(cell)).cloned().unwrap_or(0..0);
        for index in transitions {
            let record = &self.program.movement_transitions[index];
            let before = self.group_arrangements[record.group];
            let after = if arriving { before.checked_add(record.delta) } else { before.checked_sub(record.delta) };
            let Some(after) = after.filter(|&state| before < record.states && state < record.states) else {
                self.failure = Some(ScratchError { position: self.program.tokens[token].position, reason: "invalid movement transition" });
                return;
            };
            let state = if arriving { after } else { before };
            let base = if arriving { record.arrive } else { record.depart };
            let flips = self.program.movement_ranges[base + state].clone();
            self.group_arrangements[record.group] = after;
            for index in flips {
                let flip = self.program.movement_flips[index];
                if !self.consume_component_operation(flip.target) {
                    return;
                }
                let strength = self.arrangement_output(flip.source);
                self.set_arrangement_contribution(
                    flip.link,
                    flip.target,
                    flip.side,
                    if flip.entering { strength.saturating_sub(flip.attenuation) } else { 0 },
                );
                if strength > flip.attenuation {
                    self.updated_in_phase[flip.target] = 0;
                }
                // WithMoveStep reserves -2 for pistons: flips do not deliver them.
                if !matches!(self.blocks[flip.target], Block::Piston { .. }) && !self.switched[flip.target] {
                    self.switched[flip.target] = true;
                    switched.push(flip.target);
                }
                self.operations += 1;
            }
        }
        let steps = self
            .program
            .move_steps
            .get(token)
            .and_then(|cells| cells.get(cell))
            .map_or(0..0, |edges| edges[usize::from(arriving)].clone());
        // Movement callbacks read the new wire level even when that wire is not a piston watcher.
        for &node in switched.iter() {
            if matches!(self.blocks[node], Block::RedstoneWire { .. }) {
                let strength = self.input_strength(node);
                self.refresh_wire_power(node, strength);
            }
        }
        for index in steps {
            let step = &self.program.movement_steps[index];
            let node = step.node;
            let wire = step.wire;
            let watcher = step.watcher;
            let deliver = self.switched[node]
                || watcher
                || self.program.movement_wires[step.wires.clone()]
                    .iter()
                    .any(|&wire| self.changed_wires[wire]);
            if !self.consume_component_operation(node) {
                return;
            }
            if wire {
                if self.switched[node] {
                    self.switched[node] = false;
                    if self.mark_changed_wire(node) {
                        self.changed_wires[node] = true;
                        changed_wires.push(node);
                    }
                }
            } else if deliver {
                self.switched[node] = false;
                if matches!(self.blocks[node], Block::Observer { .. }) && !watcher {
                    self.update_wire_watcher(node);
                } else {
                    self.update_once(node);
                }
            }
            self.operations += 1;
        }
        for &node in switched.iter() {
            if !self.switched[node] {
                continue;
            }
            if !self.consume_component_operation(node) {
                return;
            }
            if self.program.watched_by_component[node].is_some() {
                if self.mark_changed_wire(node) {
                    self.changed_wires[node] = true;
                    changed_wires.push(node);
                }
            } else if matches!(self.blocks[node], Block::RedstoneWire { .. }) {
                let strength = self.input_strength(node);
                self.refresh_wire_power(node, strength);
            } else {
                self.update_once(node);
            }
        }
        for &wire in changed_wires.iter() {
            if let Some(index) = self.program.watched_by_component[wire].map(|index| index.get() - 1) {
                for offset in 0..self.program.watched_wires[index].watchers.len() {
                    let watcher = self.program.watched_wires[index].watchers[offset];
                    if !self.consume_component_operation(watcher) {
                        return;
                    }
                    self.update_wire_watcher(watcher);
                }
            }
        }
    }

    fn mark_changed_wire(&mut self, wire: usize) -> bool {
        let Some(index) = self.program.watched_by_component[wire].map(|index| index.get() - 1) else {
            return false;
        };
        let strength = self.input_strength(wire);
        self.refresh_wire_power(wire, strength);
        if strength == self.watched_strengths[index] {
            return false;
        }
        self.watched_strengths[index] = strength;
        true
    }

    pub(super) fn refresh_wire_power(&mut self, wire: usize, strength: u8) {
        let Block::RedstoneWire { wire: mut state } = self.blocks[wire] else {
            unreachable!()
        };
        let previous = state.power;
        state.power = strength;
        self.set_component(wire, Block::RedstoneWire { wire: state });
        if previous == strength {
            return;
        }
        for offset in 0..self.program.outgoing[wire].len() {
            let index = self.program.outgoing[wire][offset];
            let link = self.program.links[index];
            let contribution = strength.saturating_sub(link.attenuation);
            let old = self.contributions[index];
            self.set_contribution(index, contribution);
            if (old > 0) != (contribution > 0)
                || (old != contribution
                    && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                        || self.program.watched_by_component[link.target].is_some()))
            {
                self.updated_in_phase[link.target] = 0;
            }
            #[cfg(any(test, feature = "redpiler-trace"))]
            if runtime_trace_enabled!(self) && old != contribution {
                runtime_trace!(
                    self,
                    "link {:?} -> {:?} strength={old}->{contribution}",
                    self.program.positions[wire],
                    self.program.positions[link.target]
                );
            }
        }
        for offset in 0..self.program.outgoing_arrangements[wire].len() {
            let index = self.program.outgoing_arrangements[wire][offset];
            let (target, side, contribution) = self.arrangement_contribution(index);
            let old = self.arrangement_contributions[index];
            self.set_arrangement_contribution(index, target, side, contribution);
            if (old > 0) != (contribution > 0)
                || (old != contribution
                    && (matches!(self.blocks[target], Block::RedstoneComparator { .. }) || self.program.watched_by_component[target].is_some()))
            {
                self.updated_in_phase[target] = 0;
            }
        }
    }

    // shortcut: receiver signatures omit geometry-dependent fixed links, extend arrangement groups to all readers.
    fn token_dependency_signature(&self, changed: usize, target: usize, position: Option<mchprs_blocks::BlockPos>) -> Result<Vec<(ArrangementSource, u8)>, ScratchError> {
        let token_position = |token: usize| if token == changed { position } else { self.token_positions[token] };
        if let Some(graph) = &self.program.routing && let Some(reader) = graph.reader_for_component(target) {
            return graph.reader_sources(reader, |super::routing::Axis::Token(token, _)| {
                token_position(token).and_then(|pos| self.program.tokens[token].cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1)
            }, &mut super::routing::Search::default()).map(|sources| sources.into_iter().filter_map(|((source, side), attenuation)| (!side).then_some((source, attenuation))).collect());
        }
        let mut result = Vec::new();
        let arrangements: Vec<_> = self.program.input_arrangements.iter().filter(|group| group.target == target).collect();
        for group in &arrangements {
            let mut index = 0;
            let mut stride = 1;
            for (axis, &token) in group.tokens.iter().enumerate() {
                let record = &self.program.tokens[token];
                let digit = token_position(token)
                    .and_then(|pos| record.cells.iter().position(|&cell| cell == pos))
                    .map_or(0, |cell| cell + 1);
                index += group.digit(axis, digit) * stride;
                stride *= group.radix(axis, record.cells.len() + 1);
            }
            result.extend(group.inputs[index].iter().copied());
        }
        for (token, record) in self.program.tokens.iter().enumerate() {
            if arrangements
                .iter()
                .any(|group| group.conditional_sources.contains(&ArrangementSource::Token(token)))
            {
                continue;
            }
            let Some(cell) = token_position(token).and_then(|pos| record.cells.iter().position(|&cell| cell == pos)) else {
                continue;
            };
            if let Some(inputs) = self.program.token_sources.get(token).and_then(|cells| cells.get(cell)) {
                result.extend(
                    inputs
                        .iter()
                        .filter_map(|&(receiver, attenuation)| (receiver == target).then_some((ArrangementSource::Token(token), attenuation))),
                );
            }
        }
        result.sort_unstable();
        Ok(result)
    }

    pub(super) fn output(&self, source: usize) -> u8 {
        match self.blocks[source] {
            block if block.pressure_plate_powered().is_some() => u8::from(block.pressure_plate_powered() == Some(true)) * 15,
            Block::RedstoneBlock => {
                if self.program.movable_sources[source] {
                    0
                } else {
                    15
                }
            }
            Block::Lever { lever } => {
                if lever.powered {
                    15
                } else {
                    0
                }
            }
            Block::StoneButton { button } => {
                if button.powered {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneComparator { comparator } => {
                if comparator.powered {
                    self.comparator_outputs[source]
                } else {
                    0
                }
            }
            Block::RedstoneWire { wire } => wire.power,
            Block::Observer { observer } => {
                if observer.powered {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => {
                if lit {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneRepeater { repeater } => {
                if repeater.powered {
                    15
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub(super) fn begin_phase(&mut self) {
        self.phase = self.phase.wrapping_add(1);
        if self.phase == 0 {
            self.updated_in_phase.fill(0);
            self.phase = 1;
        }
    }

    pub(super) fn dispatch_receiver(&mut self, receiver: usize) {
        self.deliveries += 1;
        let powered = self.has_input(receiver);
        let Block::RedstoneLamp { lit } = self.blocks[receiver] else {
            unreachable!()
        };
        if powered && !lit {
            self.set_component(receiver, Block::RedstoneLamp { lit: true });
            self.notify_source_watchers(receiver, true, false);
        } else if !powered && lit {
            let tick = ScheduledComponentTick {
                component: receiver,
                block_type: Some(self.blocks[receiver].registry_id()),
            };
            if !self.has_pending(tick) {
                self.schedule_tick(tick, 2, TickPriority::Normal);
            }
        }
    }

    fn update_hopper(&mut self, component: usize) {
        let Block::Hopper { facing, .. } = self.blocks[component] else {
            unreachable!()
        };
        let enabled = !self.has_input(component);
        let updated = Block::Hopper { facing, enabled };
        if self.blocks[component] != updated {
            self.set_component(component, updated);
        }
    }

    fn update_trapdoor(&mut self, component: usize) {
        let Block::IronTrapdoor { facing, half, .. } = self.blocks[component] else {
            unreachable!()
        };
        let updated = Block::IronTrapdoor {
            facing,
            half,
            powered: self.has_input(component),
        };
        if self.blocks[component] != updated {
            self.set_component(component, updated);
        }
    }

    fn update_note_block(&mut self, component: usize) {
        let Block::NoteBlock { note, powered, .. } = self.blocks[component] else {
            unreachable!()
        };
        let powered_now = self.has_input(component);
        if powered == powered_now {
            return;
        }
        let below = self
            .project(self.program.positions[component].offset(mchprs_blocks::BlockFace::Bottom))
            .unwrap_or(Block::Air);
        let instrument = mchprs_blocks::blocks::Instrument::from_block_below(below);
        let pos = self.program.positions[component];
        // Capture native playback eligibility and instrument at the rising edge.
        if powered_now && !self.note_queued[component] && self.project(pos.offset(mchprs_blocks::BlockFace::Top)) == Some(Block::Air) {
            self.note_queued[component] = true;
            self.notes.push((component, pos, instrument, note));
        }
        self.set_component(
            component,
            Block::NoteBlock {
                instrument,
                note,
                powered: powered_now,
            },
        );
    }

    fn update_copper_bulb(&mut self, component: usize) {
        let block = self.blocks[component];
        let Some((mut lit, powered)) = block.copper_bulb_state() else {
            unreachable!()
        };
        let powered_now = self.has_input(component);
        if powered == powered_now {
            return;
        }
        if powered_now {
            lit = !lit;
        }
        self.set_component(component, block.with_copper_bulb_state(lit, powered_now).unwrap());
        for index in 0..self.program.comparators_by_override[component].len() {
            let comparator = self.program.comparators_by_override[component][index];
            self.update_comparator(comparator);
        }
        self.notify_source_watchers(component, true, true);
    }

    pub(super) fn update_once(&mut self, receiver: usize) {
        if self.updated_in_phase[receiver] == self.phase {
            #[cfg(any(test, feature = "redpiler-trace"))]
            if runtime_trace_enabled!(self) {
                runtime_trace!(self, "watcher suppressed {:?}", self.program.positions[receiver]);
            }
            return;
        }
        self.updated_in_phase[receiver] = self.phase;
        #[cfg(any(test, feature = "redpiler-trace"))]
        if runtime_trace_enabled!(self) {
            runtime_trace!(self, "callback {:?}", self.program.positions[receiver]);
        }
        self.update(receiver);
    }

    pub(super) fn update(&mut self, receiver: usize) {
        match self.blocks[receiver] {
            Block::RedstoneComparator { .. } => self.update_comparator(receiver),
            Block::RedstoneLamp { .. } => self.dispatch_receiver(receiver),
            Block::Hopper { .. } => self.update_hopper(receiver),
            Block::IronTrapdoor { .. } => self.update_trapdoor(receiver),
            Block::NoteBlock { .. } => self.update_note_block(receiver),
            block if block.is_command_block() => crate::redstone::command_block::update(self, self.program.positions[receiver]),
            block if block.is_copper_bulb() => self.update_copper_bulb(receiver),
            Block::Piston { .. } => self.update_piston(receiver),
            Block::RedstoneRepeater { .. } => self.update_repeater(receiver),
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => self.update_torch(receiver),
            Block::Observer { observer } if !observer.powered => {
                let tick = ScheduledComponentTick {
                    component: receiver,
                    block_type: Some(self.blocks[receiver].registry_id()),
                };
                if !self.has_pending(tick) {
                    self.schedule_tick(tick, 1, TickPriority::Normal);
                }
            }
            _ => {}
        }
    }

    pub(super) fn check_watched_wire(&mut self, record_index: usize) {
        let wire = self.program.watched_wires[record_index].wire;
        let new_strength = self.input_strength(wire);
        if new_strength == self.watched_strengths[record_index] {
            return;
        }
        #[cfg(any(test, feature = "redpiler-trace"))]
        if runtime_trace_enabled!(self) {
            runtime_trace!(
                self,
                "watched wire {:?} strength={}->{}",
                self.program.positions[wire],
                self.watched_strengths[record_index],
                new_strength
            );
        }
        self.watched_strengths[record_index] = new_strength;
        let watcher_count = self.program.watched_wires[record_index].watchers.len();
        for index in 0..watcher_count {
            let watcher = self.program.watched_wires[record_index].watchers[index];
            self.update_wire_watcher(watcher);
        }
    }

    fn update_wire_watcher(&mut self, receiver: usize) {
        if let Block::Observer { observer } = self.blocks[receiver] {
            // Dust power callbacks have no direction; only a powered observer may release.
            if observer.powered {
                let tick = ScheduledComponentTick { component: receiver, block_type: Some(self.blocks[receiver].registry_id()) };
                if !self.has_pending(tick) { self.schedule_tick(tick, 1, TickPriority::Normal); }
            }
        } else { self.update_once(receiver); }
    }

    pub(super) fn execute_output_program(&mut self, component: usize, powered: bool) {
        let reactive = self.program.has_reactive;
        let operations = self.program.output_programs[component][usize::from(powered)].clone();
        if reactive {
            for index in operations.clone() {
                let operation = self.program.operations[index];
                match operation {
                    Operation::Contribution(index) => {
                        if !self.consume_component_operation(component) {
                            return;
                        }
                        let link = self.program.links[index];
                        let value = self.output(link.source).saturating_sub(link.attenuation);
                        if self.contributions[index] != value {
                            self.updated_in_phase[link.target] = 0;
                        }
                        self.set_contribution(index, value);
                        self.operations += 1;
                    }
                    Operation::ArrangementContribution(index) => {
                        if !self.consume_component_operation(component) {
                            return;
                        }
                        let (target, side, value) = self.arrangement_contribution(index);
                        if self.arrangement_contributions[index] != value {
                            self.updated_in_phase[target] = 0;
                        }
                        self.set_arrangement_contribution(index, target, side, value);
                        self.operations += 1;
                    }
                    _ => {}
                }
            }
        }
        for index in operations {
            let operation = self.program.operations[index];
            if reactive {
                match operation {
                    Operation::Contribution(_) | Operation::ArrangementContribution(_) => continue,
                    Operation::UpdateLamp(node)
                    | Operation::UpdateHopper(node)
                    | Operation::UpdateTrapdoor(node)
                    | Operation::UpdateCommandBlock(node)
                    | Operation::UpdateNoteBlock(node)
                    | Operation::UpdateCopperBulb(node)
                    | Operation::UpdateRepeater(node)
                    | Operation::UpdateComparator(node)
                    | Operation::UpdateTorch(node)
                        if self.program.reactive[node] =>
                    {
                        continue;
                    }
                    _ => {}
                }
            }
            if !self.consume_component_operation(component) {
                return;
            }
            match operation {
                Operation::ArrangementContribution(index) => {
                    let (target, side, contribution) = self.arrangement_contribution(index);
                    if (self.arrangement_contributions[index] > 0) != (contribution > 0)
                        || (self.arrangement_contributions[index] != contribution
                            && (matches!(self.blocks[target], Block::RedstoneComparator { .. })
                                || self.program.watched_by_component[target].is_some()))
                    {
                        self.updated_in_phase[target] = 0;
                    }
                    self.set_arrangement_contribution(index, target, side, contribution);
                }
                Operation::Contribution(link_index) => {
                    let link = self.program.links[link_index];
                    let old_contribution = self.contributions[link_index];
                    let contribution = self.output(link.source).saturating_sub(link.attenuation);
                    self.set_contribution(link_index, contribution);
                    #[cfg(any(test, feature = "redpiler-trace"))]
                    if runtime_trace_enabled!(self) && old_contribution != contribution {
                        runtime_trace!(
                            self,
                            "link {:?} -> {:?} strength={old_contribution}->{contribution}",
                            self.program.positions[link.source],
                            self.program.positions[link.target]
                        );
                    }
                    if (old_contribution > 0) != (contribution > 0)
                        || (old_contribution != contribution
                            && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                                || self.program.watched_by_component[link.target].is_some()))
                    {
                        self.updated_in_phase[link.target] = 0;
                    }
                }
                Operation::RefreshWire(slot) => {
                    let strength = self.input_strength(slot);
                    self.refresh_wire_power(slot, strength);
                }
                Operation::CheckWatchedWire(record) => self.check_watched_wire(record),
                Operation::UpdateLamp(slot) => self.dispatch_receiver(slot),
                Operation::UpdateHopper(slot) => self.update_hopper(slot),
                Operation::UpdateTrapdoor(slot) => self.update_trapdoor(slot),
                Operation::UpdateCommandBlock(slot) => crate::redstone::command_block::update(self, self.program.positions[slot]),
                Operation::UpdateNoteBlock(slot) => self.update_note_block(slot),
                Operation::UpdateCopperBulb(slot) => self.update_copper_bulb(slot),
                Operation::UpdateRepeater(slot) => self.update_repeater(slot),
                Operation::UpdatePiston(slot) => self.update_piston(slot),
                Operation::UpdateTorch(slot) => self.update_torch(slot),
                Operation::UpdateComparator(slot) => self.update_comparator(slot),
            }
            self.operations += 1;
        }
        if reactive {
            self.update_reactive(component);
        }
    }

    pub(super) fn side_strength(&self, component: usize) -> u8 {
        self.input_levels[component][1].maximum(&self.input_histograms)
    }

    fn comparator_result(&self, component: usize) -> (u8, bool) {
        let Block::RedstoneComparator { comparator } = self.blocks[component] else {
            unreachable!()
        };
        let record = &self.program.comparators[self.program.comparator_by_component[component].map(|index| index.get() - 1).unwrap()];
        let override_level = |input: ComparatorInput| match input {
            ComparatorInput::Static(level) => level,
            ComparatorInput::CommandBlock(source) => self
                .command_blocks
                .get(&self.program.positions[source])
                .map_or(0, |entity| entity.success_count.clamp(0, 15) as u8),
            ComparatorInput::CopperBulb(source) => {
                if self.blocks[source].copper_bulb_state().unwrap().0 {
                    15
                } else {
                    0
                }
            }
        };
        let mut rear = self.input_strength(component);
        if let Some(input) = record.rear_override {
            rear = override_level(input);
        } else if rear < 15 {
            if let Some(input) = record.far_override {
                rear = override_level(input);
            }
        }
        compare(rear, self.side_strength(component), comparator.mode)
    }

    pub(super) fn update_comparator(&mut self, component: usize) {
        let Block::RedstoneComparator { comparator } = self.blocks[component] else {
            unreachable!()
        };
        let (level, powered) = self.comparator_result(component);
        if level == self.comparator_outputs[component] && powered == comparator.powered {
            return;
        }
        let tick = ScheduledComponentTick {
            component: component,
            block_type: Some(self.blocks[component].registry_id()),
        };
        if !self.has_pending(tick) {
            let diode =
                self.program.comparators[self.program.comparator_by_component[component].map(|index| index.get() - 1).unwrap()].output_is_diode;
            self.schedule_tick(tick, 1, if diode { TickPriority::High } else { TickPriority::Normal });
        }
    }

    fn tick_comparator(&mut self, component: usize) {
        let Block::RedstoneComparator { mut comparator } = self.blocks[component] else {
            unreachable!()
        };
        let from = self.output(component);
        let (level, powered) = self.comparator_result(component);
        let previous = self.comparator_outputs[component];
        self.comparator_outputs[component] = level;
        self.update_side_inputs(component, from, self.output(component));
        if previous != level {
            self.mark_dirty(self.program.component_cells[component]);
        }
        if previous == level && comparator.mode == mchprs_blocks::blocks::ComparatorMode::Subtract {
            return;
        }
        let shape = comparator.powered != powered;
        comparator.powered = powered;
        self.set_component(component, Block::RedstoneComparator { comparator });
        if from != self.output(component) {
            self.execute_output_program(component, self.output(component) > from);
        } else {
            self.update_reactive(component);
        }
        self.notify_source_watchers(component, shape, true);
    }

    fn update_reactive(&mut self, source: usize) {
        for offset in 0..self.program.reactive_by_source[source].len() {
            let index = self.program.reactive_by_source[source][offset];
            let mut strength = 0;
            for offset in 0..self.program.reactive_wires[index].sources.len() {
                let (node, attenuation) = self.program.reactive_wires[index].sources[offset];
                if !self.consume_component_operation(node) {
                    return;
                }
                let level = if matches!(self.blocks[node], Block::RedstoneBlock) {
                    15
                } else {
                    self.output(node)
                };
                strength = strength.max(level.saturating_sub(attenuation));
                self.operations += 1;
            }
            if self.reactive_wire_strengths[index] == strength {
                continue;
            }
            self.reactive_wire_strengths[index] = strength;
            for offset in 0..self.program.reactive_wires[index].recipients.len() {
                let target = self.program.reactive_wires[index].recipients[offset];
                if !self.consume_component_operation(target) {
                    return;
                }
                self.update(target);
            }
        }
        for offset in 0..self.program.reactive_neighbours[source].len() {
            let target = self.program.reactive_neighbours[source][offset];
            if !self.consume_component_operation(target) {
                return;
            }
            self.update(target);
        }
    }

    fn update_torch(&mut self, component: usize) {
        let lit = match self.blocks[component] {
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
            _ => return,
        };
        if lit == (self.has_input(component)) {
            let tick = ScheduledComponentTick {
                component: component,
                block_type: Some(self.blocks[component].registry_id()),
            };
            if !self.has_pending(tick) {
                self.schedule_tick(tick, 1, TickPriority::Normal);
            }
        }
    }

    fn tick_torch(&mut self, component: usize) {
        let lit = match self.blocks[component] {
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
            _ => return,
        };
        if lit != (self.has_input(component)) {
            return;
        }
        let block = match self.blocks[component] {
            Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit: !lit },
            Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { lit: !lit, facing },
            _ => unreachable!(),
        };
        self.set_component(component, block);
        self.execute_output_program(component, !lit);
        self.notify_source_watchers(component, true, true);
    }

    pub(super) fn update_repeater(&mut self, component: usize) {
        let Block::RedstoneRepeater { mut repeater } = self.blocks[component] else {
            unreachable!()
        };
        if self.program.repeater_by_component[component]
            .map(|index| index.get() - 1)
            .is_some_and(|index| self.program.repeaters[index].side_sources.is_some())
        {
            let locked = self.input_levels[component][1].active != 0;
            if repeater.locked != locked {
                repeater.locked = locked;
                self.set_component(component, Block::RedstoneRepeater { repeater });
                self.notify_source_watchers(component, true, false);
            }
        }
        if repeater.locked {
            #[cfg(any(test, feature = "redpiler-trace"))]
            if runtime_trace_enabled!(self) {
                runtime_trace!(
                    self,
                    "repeater callback {:?} powered={} locked=true rear={} side={} pending={:?}",
                    self.program.positions[component],
                    repeater.powered,
                    self.input_strength(component),
                    self.side_strength(component),
                    self.pending_entries()
                        .into_iter()
                        .filter(|entry| entry.pos == self.program.positions[component])
                        .collect::<Vec<_>>()
                );
            }
            return;
        }
        let rear_powered = self.has_input(component);
        #[cfg(any(test, feature = "redpiler-trace"))]
        if runtime_trace_enabled!(self) {
            runtime_trace!(
                self,
                "repeater callback {:?} powered={} locked=false rear={} side={} pending={:?}",
                self.program.positions[component],
                repeater.powered,
                self.input_strength(component),
                self.side_strength(component),
                self.pending_entries()
                    .into_iter()
                    .filter(|entry| entry.pos == self.program.positions[component])
                    .collect::<Vec<_>>()
            );
        }
        if rear_powered == repeater.powered {
            return;
        }
        let tick = ScheduledComponentTick {
            component: component,
            block_type: Some(self.blocks[component].registry_id()),
        };
        if self.has_pending(tick) {
            return;
        }
        let output_is_diode = self.program.repeater_by_component[component]
            .map(|index| index.get() - 1)
            .is_some_and(|index| self.program.repeaters[index].output_is_diode);
        let priority = if output_is_diode {
            TickPriority::Highest
        } else if !rear_powered {
            TickPriority::Higher
        } else {
            TickPriority::High
        };
        self.schedule_tick(tick, repeater.delay as usize, priority);
        #[cfg(any(test, feature = "redpiler-trace"))]
        if runtime_trace_enabled!(self) {
            let _remaining = self
                .pending_entries()
                .into_iter()
                .find(|entry| entry.pos == self.program.positions[component])
                .map(|entry| entry.ticks_left);
            runtime_trace!(
                self,
                "schedule repeater {:?} remaining={_remaining:?} priority={priority:?}",
                self.program.positions[component]
            );
        }
    }

    pub(crate) fn set_input(&mut self, source_index: usize, powered: bool) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let Some(&source) = self.program.sources.get(source_index) else {
            return Err(ScratchError {
                position: None,
                reason: "runtime set_input source index out of bounds",
            });
        };
        match self.blocks[source] {
            Block::Lever { lever } if lever.powered != powered => {}
            Block::StoneButton { button } if button.powered != powered => {}
            Block::Lever { .. } | Block::StoneButton { .. } => return Ok(()),
            _ => unreachable!(),
        }
        self.begin_phase();
        match self.blocks[source] {
            Block::Lever { mut lever } => {
                lever.powered = powered;
                self.set_component(source, Block::Lever { lever });
            }
            Block::StoneButton { mut button } => {
                button.powered = powered;
                self.set_component(source, Block::StoneButton { button });
                if powered {
                    let tick = ScheduledComponentTick {
                        component: source,
                        block_type: Some(self.blocks[source].registry_id()),
                    };
                    self.schedule_tick(tick, 10, TickPriority::Normal);
                }
            }
            _ => unreachable!(),
        }
        #[cfg(any(test, feature = "redpiler-trace"))]
        if runtime_trace_enabled!(self) {
            runtime_trace!(self, "input {:?} powered={powered}", self.program.positions[source]);
        }
        self.execute_output_program(source, powered);
        self.notify_source_watchers(source, true, true);
        self.failure.clone().map_or(Ok(()), Err)
    }

    fn has_pending(&self, tick: ScheduledComponentTick) -> bool {
        debug_assert_eq!(tick.block_type, Some(self.blocks[tick.component].registry_id()));
        self.pending_counts[tick.component] != 0
    }

    fn schedule_tick(&mut self, tick: ScheduledComponentTick, delay: usize, priority: TickPriority) {
        self.schedule_half_tick(tick, delay * 2, priority);
    }

    pub(super) fn schedule_half_tick(&mut self, tick: ScheduledComponentTick, delay: usize, priority: TickPriority) {
        if let Err(reason) = self.pending.schedule_half_tick(tick, delay, priority) {
            self.failure = Some(ScratchError {
                position: Some(self.program.positions[tick.component]),
                reason,
            });
        } else {
            self.pending_counts[tick.component] += 1;
        }
    }

    pub(super) fn pending_position(&self, component: usize) -> mchprs_blocks::BlockPos {
        self.program
            .positions
            .get(component)
            .copied()
            .unwrap_or_else(|| self.pending_passive_positions[component - self.blocks.len()])
    }

    pub(super) fn drain_ticks(&mut self) {
        while let Some(tick) = self.pending.pop_first() {
            let position = self.pending_position(tick.component);
            if tick.component < self.blocks.len() && tick.block_type == Some(self.blocks[tick.component].registry_id()) {
                self.pending_counts[tick.component] -= 1;
            }
            if !self.consume_operation(position) {
                return;
            }
            #[cfg(any(test, feature = "redpiler-trace"))]
            if runtime_trace_enabled!(self) {
                runtime_trace!(self, "due tick {position:?} type={:?}", tick.block_type);
            }
            let component = tick.component;
            if component < self.blocks.len() {
                if tick.block_type != Some(self.blocks[component].registry_id()) {
                    continue;
                }
                match self.blocks[component] {
                    block if block.is_command_block() => crate::redstone::command_block::tick(self, position),
                    Block::RedstoneComparator { .. } => self.tick_comparator(component),
                    Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => self.tick_torch(component),
                    Block::RedstoneLamp { .. } => self.dispatch_receiver_tick(component),
                    Block::StoneButton { mut button } if button.powered => {
                        button.powered = false;
                        self.set_component(component, Block::StoneButton { button });
                        self.execute_output_program(component, false);
                        self.notify_source_watchers(component, true, true);
                    }
                    Block::RedstoneRepeater { .. } => {
                        self.scheduled_repeater_ticks += 1;
                        self.tick_repeater(component);
                    }
                    Block::Observer { mut observer } => {
                        observer.powered = !observer.powered;
                        #[cfg(any(test, feature = "redpiler-trace"))]
                        if runtime_trace_enabled!(self) {
                            runtime_trace!(self, "observer tick {:?} powered={}", self.program.positions[component], observer.powered);
                        }
                        self.set_component(component, Block::Observer { observer });
                        self.notify_source_watchers(component, true, false);
                        if observer.powered {
                            self.schedule_tick(tick, 1, TickPriority::Normal);
                        }
                        self.execute_output_program(component, observer.powered);
                        self.notify_source_watchers(component, false, true);
                    }
                    _ => {}
                }
                self.operations += 1;
            }
        }
    }

    fn tick_repeater(&mut self, component: usize) {
        #[cfg(any(test, feature = "redpiler-trace"))]
        if runtime_trace_enabled!(self) {
            runtime_trace!(self, "repeater tick {:?}", self.program.positions[component]);
        }
        let Block::RedstoneRepeater { mut repeater } = self.blocks[component] else {
            unreachable!()
        };
        if repeater.locked {
            return;
        }
        let rear_powered = self.has_input(component);
        if repeater.powered {
            if rear_powered {
                return;
            }
            repeater.powered = false;
            self.set_component(component, Block::RedstoneRepeater { repeater });
            self.execute_output_program(component, false);
            self.notify_source_watchers(component, true, true);
        } else {
            if !rear_powered {
                let tick = ScheduledComponentTick {
                    component: component,
                    block_type: Some(self.blocks[component].registry_id()),
                };
                self.schedule_tick(tick, repeater.delay as usize, TickPriority::Higher);
            }
            repeater.powered = true;
            self.set_component(component, Block::RedstoneRepeater { repeater });
            self.execute_output_program(component, true);
            self.notify_source_watchers(component, true, true);
        }
    }

    pub(super) fn dispatch_receiver_tick(&mut self, receiver: usize) {
        if !self.has_input(receiver) && matches!(self.blocks[receiver], Block::RedstoneLamp { lit: true }) {
            self.set_component(receiver, Block::RedstoneLamp { lit: false });
            self.notify_source_watchers(receiver, true, false);
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.motion_time += 1;
        self.begin_phase();
        self.drain_ticks();
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.pending.end_last_tick_move_next();
        self.drain_ticks();
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if !self.program.pistons.is_empty() {
            self.begin_phase();
            self.run_piston_events();
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.begin_phase();
            self.tick_motions();
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
        }
        self.tick_operations = 0;
        Ok(())
    }

    pub(crate) fn component_count(&self) -> usize {
        self.program.positions.len()
    }

    pub(crate) fn accepts_control(&self, pos: mchprs_blocks::BlockPos) -> bool {
        self.program.sources.iter().any(|&source| self.program.positions[source] == pos)
    }

    pub(crate) fn use_control(&mut self, pos: mchprs_blocks::BlockPos) -> Result<(), ScratchError> {
        let Some(source) = self.program.sources.iter().position(|&source| self.program.positions[source] == pos) else {
            return Ok(());
        };
        match self.blocks[self.program.sources[source]] {
            Block::Lever { lever } => self.set_input(source, !lever.powered),
            Block::StoneButton { .. } => self.set_input(source, true),
            _ => unreachable!(),
        }
    }

    pub(crate) fn projected_blocks(&self) -> Vec<(mchprs_blocks::BlockPos, Block)> {
        self.program
            .projection_positions
            .iter()
            .enumerate()
            .filter_map(|(cell, &pos)| self.project_cell(cell).map(|block| (pos, block)))
            .collect()
    }
}
