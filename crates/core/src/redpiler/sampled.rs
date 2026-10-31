//! Private sampled reference compiler and runtime.
#![allow(dead_code)]
#[cfg(test)]
#[path = "sampled/adder_tests.rs"]
mod adder_tests;
mod compiler;
mod executor;
mod group_probe;
mod program;
#[cfg(test)]
mod tests;

pub(super) use super::scratch::{ScratchEntry, ScratchError};
use super::{queue::ScheduledBlockTick, scratch::ScratchWorld};
#[cfg(test)]
use crate::world::World;
#[cfg(test)]
use compiler::{PistonActor, assign_piston_groups, compile, discover_piston_actors};
pub(super) use executor::Runtime;
pub(super) use program::Program;
#[cfg(test)]
use program::WatchedWire;
use program::{ElectricalLink, Operation};

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

pub(super) fn start_runtime(program: Program, ticks: Vec<mchprs_world::TickEntry>, piston: mchprs_world::PistonState) -> Runtime {
    let mut runtime = program.start();
    runtime.set_pending(ticks);
    runtime.piston_state = piston;
    runtime
}

impl Runtime {
    pub(super) fn set_pending(&mut self, ticks: Vec<mchprs_world::TickEntry>) {
        self.pending = ticks.into_iter().collect();
    }

    pub(super) fn pending_entries(&self) -> Vec<mchprs_world::TickEntry> {
        self.pending.iter_entries().collect()
    }

    pub(super) fn has_motions(&self) -> bool {
        !self.motions.is_empty()
    }

    pub(super) fn piston_state(&self) -> mchprs_world::PistonState {
        self.piston_state.clone()
    }

    pub(super) fn motion_time(&self) -> u64 {
        self.motion_time
    }

    pub(super) fn comparator_outputs(&self) -> Vec<(mchprs_blocks::BlockPos, u8)> {
        self.program
            .comparators
            .iter()
            .map(|record| (self.program.positions[record.component], self.comparator_outputs[record.component]))
            .collect()
    }

    pub(super) fn moving_parts(&self) -> Result<Vec<MovingPart>, ScratchError> {
        use mchprs_blocks::{
            block_entities::MovingPistonEntity,
            blocks::{Block, RedstonePistonHead},
        };
        let mut parts = Vec::new();
        for motion in &self.motions {
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
                        add(motion.destination, self.program.tokens[token].block, false);
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
                    let component = self.program.positions.iter().position(|&pos| pos == piston.base).ok_or(ScratchError {
                        position: Some(piston.base),
                        reason: "piston component missing during motion export",
                    })?;
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
                        add(motion.destination, self.program.tokens[token].block, false);
                    }
                }
            }
        }
        let mut positions = std::collections::HashSet::new();
        if parts.iter().any(|part| !positions.insert(part.pos)) {
            return Err(ScratchError {
                position: None,
                reason: "overlapping piston motion parts cannot be exported",
            });
        }
        Ok(parts)
    }

    pub(super) fn pending_piston_events(&self) -> Vec<mchprs_world::PistonEvent> {
        self.piston_events
            .iter()
            .map(|&(index, event)| {
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
            .collect()
    }
}
#[cfg(test)]
use mchprs_blocks::{
    BlockFacing, BlockPos,
    blocks::{Block, LeverFace},
};
