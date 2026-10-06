//! Ordinary ticks consume the visible far-supply waveform. Internal response
//! work is a Boolean program; physical replay is confined to interpreter handoff.
use super::node::{NodeId, Nodes};
use crate::plot::{PlotWorld, PLOT_BLOCK_WIDTH, PLOT_WIDTH};
use crate::redpiler::backend::BackendError;
use crate::redpiler::instant::boolean::{Expr, Variable, TRUE};
use crate::redpiler::instant::program::PreparedInstant;
use crate::world::storage::Chunk;
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;

pub(super) struct Runtime {
    program: PreparedInstant,
    sources: FxHashMap<BlockPos, NodeId>,
    aliases: Vec<(NodeId, Supply)>,
    fired: Vec<bool>,
    phase: u8,
    repeated: bool,
    elapsed: u64,
    ready: FxHashMap<BlockPos, u8>,
    epoch: FxHashMap<BlockPos, u8>,
    actions: Vec<(BlockPos, u8)>,
    epoch_actions: Vec<(BlockPos, u8)>,
    decisions: Vec<Decision>,
    memory: Vec<bool>,
    moving_memory: Vec<bool>,
    replay_memory: Vec<bool>,
    previous_wave_memory: Vec<bool>,
}

struct Decision {
    input: Input,
    threshold: u8,
    low: Expr,
    high: Expr,
}

enum Input {
    Source(NodeId),
    Memory(usize),
}
enum Supply {
    Wave { group: usize, initial: bool },
    Memory { actor: usize, far: bool },
}

impl Runtime {
    pub(super) fn bind(
        mut program: PreparedInstant,
        bindings: FxHashMap<BlockPos, NodeId>,
        nodes: &Nodes,
    ) -> Result<Self, BackendError> {
        let mut sources = FxHashMap::default();
        for &pos in program.logic.sources.iter().chain(&program.controls) {
            sources.insert(
                pos,
                *bindings
                    .get(&pos)
                    .ok_or(BackendError::MissingInstantBinding { pos })?,
            );
        }
        let mut aliases = Vec::new();
        for &(group, pos, initial) in &program.aliases {
            let memory = program.clocked.as_ref().and_then(|c| {
                c.memory
                    .iter()
                    .find(|m| program.groups[group].contains(&m.actor))
            });
            let supply = if let Some(cell) = memory {
                Supply::Memory {
                    actor: cell.actor,
                    far: pos == cell.far,
                }
            } else {
                Supply::Wave { group, initial }
            };
            aliases.push((
                *bindings
                    .get(&pos)
                    .ok_or(BackendError::MissingInstantBinding { pos })?,
                supply,
            ));
        }
        let ready = sources
            .iter()
            .map(|(&pos, &id)| (pos, nodes[id].output_power))
            .collect();
        let decisions = program
            .logic
            .arena
            .nodes
            .iter()
            .map(|d| {
                let (input, threshold) = match d.variable {
                    Variable::Signal { pos, threshold, .. } => {
                        (Input::Source(sources[&pos]), threshold)
                    }
                    Variable::Memory(actor)
                        if program
                            .clocked
                            .as_ref()
                            .is_some_and(|c| c.memory.iter().any(|m| m.actor == actor)) =>
                    {
                        (Input::Memory(actor), 0)
                    }
                    _ => return Err(BackendError::InvalidInstantProgram),
                };
                Ok(Decision {
                    input,
                    threshold,
                    low: d.low,
                    high: d.high,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        program.logic.arena = Default::default();
        Ok(Self {
            fired: vec![false; program.logic.responses.len()],
            memory: vec![false; program.logic.responses.len()],
            moving_memory: vec![false; program.logic.responses.len()],
            replay_memory: vec![false; program.logic.responses.len()],
            previous_wave_memory: vec![false; program.logic.responses.len()],
            program,
            sources,
            aliases,
            phase: 0,
            repeated: false,
            elapsed: 0,
            epoch: FxHashMap::default(),
            ready,
            actions: Vec::new(),
            epoch_actions: Vec::new(),
            decisions,
        })
    }

    pub(super) fn observe_action(&mut self, pos: BlockPos, strength: u8) {
        if !self.sources.contains_key(&pos) {
            return;
        }
        // Inputs are frozen during reset by the operating contract. Retaining
        // the latest action per source bounds handoff storage for invalid use.
        self.actions.retain(|&(p, _)| p != pos);
        self.actions.push((pos, strength));
    }

    pub(super) fn advance(&mut self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        self.elapsed += 1;
        if self.phase == 0 || self.phase == 6 {
            for (&root, value) in self.program.logic.responses.iter().zip(&mut self.fired) {
                let mut id = root;
                while id > TRUE {
                    let d = &self.decisions[(id - 2) as usize];
                    let high = match d.input {
                        Input::Source(source) => nodes[source].output_power > d.threshold,
                        Input::Memory(actor) => self.memory[actor],
                    };
                    id = if high { d.high } else { d.low };
                }
                *value = id == TRUE;
            }
            let active = self
                .program
                .clocked
                .as_ref()
                .map_or_else(|| self.fired.iter().any(|&f| f), |c| self.fired[c.clock]);
            if active {
                if self.program.clocked.is_some() {
                    self.replay_memory.clone_from(&self.previous_wave_memory);
                    self.previous_wave_memory.clone_from(&self.memory);
                }
                if self.phase == 0 {
                    self.epoch = self
                        .sources
                        .iter()
                        .map(|(&pos, &id)| (pos, nodes[id].output_power))
                        .collect();
                    self.epoch_actions = std::mem::take(&mut self.actions);
                    self.repeated = false;
                } else {
                    self.repeated = true;
                }
                self.phase = 1;
            } else {
                self.phase = 0;
                self.ready = self
                    .sources
                    .iter()
                    .map(|(&pos, &id)| (pos, nodes[id].output_power))
                    .collect();
                self.actions.clear();
            }
        } else {
            self.phase += 1;
        }
        if let Some(clocked) = &self.program.clocked {
            if self.phase == 3 {
                for cell in &clocked.memory {
                    self.moving_memory[cell.actor] =
                        self.memory[cell.actor] != self.fired[cell.actor];
                    self.memory[cell.actor] = self.fired[cell.actor];
                }
            }
            if self.phase == 5 {
                self.moving_memory.fill(false);
            }
        }
        self.aliases
            .iter()
            .map(|(id, supply)| {
                let powered = match *supply {
                    Supply::Wave { group, initial } => {
                        let low = self.phase != 0
                            && self.phase != 6
                            && self.program.groups[group].iter().any(|&p| self.fired[p]);
                        initial && !low
                    }
                    Supply::Memory { actor, far } => {
                        !self.moving_memory[actor] && self.memory[actor] != far
                    }
                };
                (*id, if powered { 15 } else { 0 })
            })
            .collect()
    }

    pub(super) fn materialize<W: World>(self, world: &mut W) {
        let first = self.program.bounds.0;
        let plot_x = first.x.div_euclid(PLOT_BLOCK_WIDTH);
        let plot_z = first.z.div_euclid(PLOT_BLOCK_WIDTH);
        let chunks = (0..PLOT_WIDTH)
            .flat_map(|x| {
                (0..PLOT_WIDTH)
                    .map(move |z| Chunk::empty(plot_x * PLOT_WIDTH + x, plot_z * PLOT_WIDTH + z))
            })
            .collect();
        let mut replay = PlotWorld::from_chunks(plot_x, plot_z, chunks, Default::default());
        for &(pos, block, ref entity) in &self.program.template {
            replay.set_block(pos, block);
            if let Some(entity) = entity {
                replay.set_block_entity(pos, entity.clone());
            }
        }
        if let Some(clocked) = &self.program.clocked {
            for cell in &clocked.memory {
                if !self.replay_memory[cell.actor] {
                    continue;
                }
                let Block::Piston { mut piston } = replay.get_block(cell.base) else {
                    unreachable!()
                };
                piston.extended = false;
                replay.set_block(cell.base, Block::Piston { piston });
                replay.set_block(cell.near, Block::RedstoneBlock);
                replay.set_block(cell.far, Block::Air);
            }
            for &pos in &self.program.logic.wires {
                let block = replay.get_block(pos);
                crate::redstone::update(block, &mut replay, pos, None);
            }
        }
        for (&pos, &strength) in &self.ready {
            apply_source(&mut replay, pos, strength);
        }
        // Prepared data can alter dust without firing an instant. Bring that
        // entry snapshot to a quiescent extended state before replaying launch.
        for _ in 0..32 {
            replay.retain_tick_requests(|pos| self.program.owned.contains(&pos));
            if replay.scheduler().iter_entries().next().is_none()
                && replay.piston_state().events.is_empty()
                && replay.piston_state().motions.is_empty()
            {
                break;
            }
            replay.tick_interpreted();
        }
        if self.phase != 0 {
            for &(pos, strength) in &self.epoch_actions {
                apply_source(&mut replay, pos, strength);
            }
            for (&pos, &strength) in &self.epoch {
                // Timed ordinary sources need the same launch values even when
                // they were updated by a backend tick rather than player use.
                apply_source(&mut replay, pos, strength);
            }
            let ticks = self.phase as usize + if self.repeated { 6 } else { 0 };
            for _ in 0..ticks {
                replay.retain_tick_requests(|pos| self.program.owned.contains(&pos));
                replay.tick_interpreted();
            }
        } else {
            for &(pos, strength) in &self.actions {
                apply_source(&mut replay, pos, strength);
            }
        }
        for &pos in &self.program.owned {
            world.set_block(pos, replay.get_block(pos));
            world.delete_block_entity(pos);
            if let Some(entity) = replay.get_block_entity(pos) {
                world.set_block_entity(pos, entity.clone());
            }
        }
        let mut state = replay.piston_state().clone();
        let time = self.program.logical_tick.wrapping_add(self.elapsed);
        for motion in &mut state.motions {
            motion.last_tick = time.wrapping_sub(state.logical_tick.wrapping_sub(motion.last_tick));
        }
        state.logical_tick = time;
        state.next_identity = state.next_identity.max(world.piston_state().next_identity);
        *world.piston_state_mut() = state;
        for tick in replay
            .scheduler()
            .iter_entries()
            .filter(|t| self.program.owned.contains(&t.pos))
        {
            world.schedule_half_tick(tick.pos, tick.ticks_left, tick.tick_priority);
        }
        // Ordinary values and work belong to the live backend. The replay is
        // used only for region geometry, motion entities and reset work.
    }
}

fn apply_source(world: &mut PlotWorld, pos: BlockPos, strength: u8) {
    let mut block = world.get_block(pos);
    if crate::redstone::source_strength(block, world, pos) == strength {
        return;
    }
    if let Some(plate) = block.with_pressure_plate_power(strength != 0) {
        block = plate;
    } else if let Some(powered) = crate::redpiler::block_powered_mut(&mut block) {
        *powered = strength != 0;
    } else {
        return;
    }
    if matches!(block, Block::RedstoneComparator { .. }) {
        world.set_block_entity(
            pos,
            mchprs_blocks::block_entities::BlockEntity::Comparator {
                output_strength: strength,
            },
        );
    }
    world.set_block(pos, block);
    crate::redstone::update_surrounding_blocks(world, pos);
    if let Block::Lever { lever } = block {
        let face = match lever.face {
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        };
        crate::redstone::update_surrounding_blocks(world, pos.offset(face));
    }
}
