//! Private sampled reference compiler and runtime.
#![allow(dead_code)]
#[cfg(test)]
#[path = "sampled/adder_tests.rs"]
mod adder_tests;
mod compiler;
#[cfg(test)]
mod cpu_trace;
mod executor;
mod group_probe;
#[cfg(test)]
mod incremental_input_tests;
mod input;
mod program;
#[cfg(test)]
mod runtime_index_tests;
#[cfg(test)]
#[path = "sampled/reviewed_fixes_tests.rs"]
mod reviewed_fixes_tests;
mod scheduler;
#[cfg(test)]
mod tests;

use super::scratch::ScratchWorld;
pub(super) use super::scratch::{ScratchEntry, ScratchError};
#[cfg(test)]
use crate::world::World;
#[cfg(test)]
use compiler::{PistonActor, assign_piston_groups, compile, discover_piston_actors};
pub(super) use executor::Runtime;
pub(super) use program::Program;
#[cfg(test)]
use program::WatchedWire;
use program::{ElectricalLink, Operation};
use mchprs_blocks::{
    block_entities::MovingPistonEntity,
    blocks::{Block, RedstonePistonHead},
};

pub(super) use compiler::compile_monitored as compile_program;

pub(super) type ExportKey = (usize, u64, bool, mchprs_blocks::BlockPos);

pub(super) struct MovingPart {
    pub(super) key: ExportKey,
    pub(super) pos: mchprs_blocks::BlockPos,
    pub(super) block: mchprs_blocks::blocks::Block,
    pub(super) entity: mchprs_blocks::block_entities::MovingPistonEntity,
    pub(super) progress: f32,
}

impl Program {
    pub(super) fn component_count(&self) -> usize {
        self.positions.len()
    }
}

pub(super) fn start_runtime(
    program: Program,
    ticks: Vec<mchprs_world::TickEntry>,
    piston: mchprs_world::PistonState,
) -> Result<Runtime, ScratchError> {
    let mut runtime = program.into_runtime_prepared();
    runtime.set_pending(ticks)?;
    runtime.piston_state = piston;
    Ok(runtime)
}

impl Runtime {
    pub(super) fn set_pending(&mut self, ticks: Vec<mchprs_world::TickEntry>) -> Result<(), ScratchError> {
        // Pending work has a fixed admission limit; exhaustion is an execution error.
        let limit = self
            .blocks
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(ticks.len()))
            .ok_or(ScratchError {
                position: None,
                reason: "runtime scheduled tick capacity overflow",
            })?
            .max(64);
        self.pending.reset(limit);
        self.pending_counts.fill(0);
        self.pending_passive_positions.clear();
        let mut indices = self.program.component_indices.clone();
        for tick in ticks {
            let component = *indices.entry(tick.pos).or_insert_with(|| {
                let id = self.blocks.len() + self.pending_passive_positions.len();
                self.pending_passive_positions.push(tick.pos);
                id
            });
            self.pending
                .schedule_half_tick(
                    executor::ScheduledComponentTick {
                        component,
                        block_type: tick.block_type,
                    },
                    tick.ticks_left as usize,
                    tick.tick_priority,
                )
                .map_err(|reason| ScratchError {
                    position: Some(tick.pos),
                    reason,
                })?;
            if component < self.blocks.len() && tick.block_type == Some(self.blocks[component].registry_id()) {
                self.pending_counts[component] += 1;
            }
        }
        Ok(())
    }

    pub(super) fn pending_entries(&self) -> Vec<mchprs_world::TickEntry> {
        self.pending
            .iter()
            .map(|(tick, delay, priority)| mchprs_world::TickEntry {
                pos: self.pending_position(tick.component),
                block_type: tick.block_type,
                ticks_left: delay as u32,
                tick_priority: priority,
            })
            .collect()
    }

    pub(super) fn has_motions(&self) -> bool {
        !self.active_motions.is_empty()
    }

    pub(super) fn piston_state(&self) -> mchprs_world::PistonState {
        self.piston_state.clone()
    }

    pub(super) fn motion_time(&self) -> u64 {
        self.motion_time
    }

    pub(super) fn publication_changes(&self, full: bool) -> impl Iterator<Item = (mchprs_blocks::BlockPos, mchprs_blocks::blocks::Block, Option<u8>)> + '_ {
        self.dirty_cells.iter().filter_map(move |&cell| {
            let pos = self.program.projection_positions[cell];
            let block = if full {
                self.project_cell(cell)?
            } else {
                let component = self.program.cell_components[cell]?;
                if self.program.piston_by_component[component].is_some() {
                    return None;
                }
                self.blocks[component]
            };
            let comparator = self.program.cell_components[cell]
                .filter(|&component| self.program.comparator_by_component[component].is_some())
                .map(|component| self.comparator_outputs[component]);
            Some((pos, block, comparator))
        })
    }

    pub(super) fn prepare_publication(&mut self, io_only: bool) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if !io_only && self.needs_full_publication {
            self.dirty_cells.clear();
            self.dirty_cells.extend(0..self.dirty.len());
            self.dirty.fill(true);
        }
        Ok(())
    }

    pub(super) fn mark_published(&mut self, io_only: bool) {
        for &cell in &self.dirty_cells {
            self.dirty[cell] = false;
        }
        self.dirty_cells.clear();
        self.needs_full_publication = io_only;
    }

    pub(super) fn comparator_outputs(&self) -> Vec<(mchprs_blocks::BlockPos, u8)> {
        self.program
            .comparators
            .iter()
            .map(|record| (self.program.positions[record.component], self.comparator_outputs[record.component]))
            .collect()
    }

    #[cfg(test)]
    pub(super) fn moving_parts(&self) -> Result<Vec<MovingPart>, ScratchError> {
        let mut parts = Vec::new();
        self.write_moving_parts(&mut parts, &mut std::collections::HashSet::new())?;
        Ok(parts)
    }

    pub(super) fn motion_part_capacity(&self) -> usize {
        self.motions.capacity() * 2
    }

    pub(super) fn write_moving_parts(
        &self,
        parts: &mut Vec<MovingPart>,
        positions: &mut std::collections::HashSet<mchprs_blocks::BlockPos>,
    ) -> Result<(), ScratchError> {
        parts.clear();
        positions.clear();
        for &slot in &self.active_motions {
            let motion = &self.motions[slot];
            if motion.piston_done && motion.token_done {
                continue;
            }
            let piston = &self.program.pistons[motion.piston];
            let remaining = motion.landing.saturating_sub(self.motion_time);
            let progress = if remaining >= 2 { 0.5 } else { 1.0 };
            let mut add = |pos: mchprs_blocks::BlockPos, block: Block, source: bool| {
                let mut entity = MovingPistonEntity {
                    facing: piston.facing.into(),
                    progress: 0,
                    block_state: block.get_id(),
                    extending: motion.extending,
                    source,
                };
                entity.set_progress(progress);
                parts.push(MovingPart {
                    key: (motion.piston, motion.landing, motion.extending, pos),
                    pos,
                    block: Block::MovingPiston {
                        moving: mchprs_blocks::blocks::RedstoneMovingPiston {
                            facing: piston.facing,
                            sticky: piston.sticky,
                        },
                    },
                    entity,
                    progress,
                });
            };
            if motion.extending {
                if !motion.token_done {
                    if let Some(token) = motion.token {
                        add(
                            self.program.projection_positions[motion.destination],
                            self.program.tokens[token].block,
                            false,
                        );
                    }
                }
                if !motion.piston_done {
                    add(
                        piston.home,
                        Block::PistonHead {
                            head: RedstonePistonHead {
                                facing: piston.facing,
                                sticky: piston.sticky,
                                short: false,
                            },
                        },
                        true,
                    );
                }
            } else {
                if !motion.piston_done {
                    let component = self.program.piston_bindings[motion.piston].component;
                    let Block::Piston { piston: mut base } = self.blocks[component] else {
                        return Err(ScratchError {
                            position: Some(piston.base),
                            reason: "piston base missing during motion export",
                        });
                    };
                    base.extended = false;
                    add(piston.base, Block::Piston { piston: base }, true);
                }
                if !motion.token_done {
                    if let Some(token) = motion.token {
                        add(
                            self.program.projection_positions[motion.destination],
                            self.program.tokens[token].block,
                            false,
                        );
                    }
                }
            }
        }
        if parts.iter().any(|part| !positions.insert(part.pos)) {
            return Err(ScratchError {
                position: None,
                reason: "overlapping piston motion parts cannot be exported",
            });
        }
        Ok(())
    }

    pub(super) fn pending_piston_events(&self) -> Vec<mchprs_world::PistonEvent> {
        self.pending_piston_event_iter().collect()
    }

    pub(super) fn owns_piston(&self, pos: mchprs_blocks::BlockPos) -> bool {
        self.program
            .component_indices
            .get(&pos)
            .is_some_and(|&component| self.program.piston_by_component[component].is_some())
    }

    pub(super) fn logical_tick(&self) -> u64 {
        self.piston_state.logical_tick.saturating_add(self.motion_time)
    }

    pub(super) fn pending_piston_event_iter(&self) -> impl Iterator<Item = mchprs_world::PistonEvent> + '_ {
        self.piston_events.iter().map(|&(index, event)| {
            let piston = &self.program.pistons[index];
            mchprs_world::PistonEvent {
                pos: piston.base,
                sticky: piston.sticky,
                facing: piston.facing.into(),
                action: match event {
                    executor::PistonEvent::Extend => mchprs_world::PistonAction::Extend,
                    executor::PistonEvent::Retract => mchprs_world::PistonAction::Retract,
                    executor::PistonEvent::EarlyRetract => mchprs_world::PistonAction::RetractWithoutPull,
                },
            }
        })
    }
}
#[cfg(test)]
use mchprs_blocks::{
    BlockFacing, BlockPos,
    blocks::LeverFace,
};
