//! Ordinary ticks consume the visible far-supply waveform. Internal response
//! work is a Boolean program; physical replay is confined to interpreter handoff.
use super::node::{NodeId, Nodes};
use crate::plot::{PlotWorld, PLOT_BLOCK_WIDTH, PLOT_WIDTH};
use crate::redpiler::backend::BackendError;
use crate::redpiler::instant::boolean::{Expr, GeometryPart, Variable, TRUE};
use crate::redpiler::instant::program::PreparedInstant;
use crate::world::storage::Chunk;
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace, RedstonePistonHead};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) struct Runtime {
    program: PreparedInstant,
    sources: FxHashMap<BlockPos, NodeId>,
    aliases: Vec<(NodeId, Supply)>,
    fired: Vec<bool>,
    // Half ticks since launch: 0 ready, 1..2 retracting, 3 near payload,
    // 4..5 extending, 6 reset complete (resample on the next advance).
    phase: u8,
    repeated: bool,
    elapsed: u64,
    ready_inputs: FxHashMap<BlockPos, u8>,
    launch_inputs: FxHashMap<BlockPos, u8>,
    actions: Vec<(BlockPos, u8)>,
    launch_actions: Vec<(BlockPos, u8)>,
    decisions: Vec<Decision>,
    outputs: Vec<Output>,
    output_sources: FxHashSet<NodeId>,
    actor_groups: Vec<usize>,
    group_fired: Vec<bool>,
    memory_actors: Vec<bool>,
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

struct Output {
    node: NodeId,
    terms: Vec<Term>,
}

struct Term {
    guard: Expr,
    source: Option<NodeId>,
    attenuation: u8,
}

enum Input {
    Source(NodeId),
    Memory(usize),
    Geometry { actor: usize, part: GeometryPart },
}
enum Supply {
    Wave {
        group: usize,
        initial: bool,
        near: Option<usize>,
    },
    Memory {
        actor: usize,
        far: bool,
    },
}

impl Runtime {
    pub(super) fn bind(
        mut program: PreparedInstant,
        bindings: &FxHashMap<BlockPos, NodeId>,
        output_bindings: &FxHashMap<usize, NodeId>,
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
                // ponytail: O(n²) alias binding at compile time; index heads if profiling warrants it.
                let near = program.pistons.iter().enumerate().find_map(|(actor, p)| {
                    (p.head == pos
                        && program
                            .aliases
                            .iter()
                            .any(|&(g, _, powered)| g == group && powered))
                    .then_some(actor)
                });
                Supply::Wave {
                    group,
                    initial,
                    near,
                }
            };
            aliases.push((
                *bindings
                    .get(&pos)
                    .ok_or(BackendError::MissingInstantBinding { pos })?,
                supply,
            ));
        }
        let ready_inputs = sources
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
                    Variable::Geometry { actor, part } if actor < program.logic.responses.len() => {
                        (Input::Geometry { actor, part }, 0)
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
        let outputs: Vec<Output> = program
            .logic
            .outputs
            .iter()
            .enumerate()
            .map(|(port, output)| {
                let node = *output_bindings.get(&(port + program.output_offset)).ok_or(
                    BackendError::MissingInstantBinding {
                        pos: output.consumer,
                    },
                )?;
                let terms = output
                    .terms
                    .iter()
                    .map(|term| {
                        Ok(Term {
                            guard: term.guard,
                            source: term
                                .source
                                .map(|pos| {
                                    bindings
                                        .get(&pos)
                                        .copied()
                                        .ok_or(BackendError::MissingInstantBinding { pos })
                                })
                                .transpose()?,
                            attenuation: term.attenuation,
                        })
                    })
                    .collect::<Result<_, BackendError>>()?;
                Ok(Output { node, terms })
            })
            .collect::<Result<_, BackendError>>()?;
        let mut actor_groups = vec![0; program.logic.responses.len()];
        for (group, actors) in program.groups.iter().enumerate() {
            for &actor in actors {
                actor_groups[actor] = group;
            }
        }
        let output_sources = outputs
            .iter()
            .flat_map(|output| output.terms.iter().filter_map(|term| term.source))
            .collect();
        let mut memory_actors = vec![false; program.logic.responses.len()];
        if let Some(clocked) = &program.clocked {
            for cell in &clocked.memory {
                memory_actors[cell.actor] = true;
            }
        }
        program.logic.arena = Default::default();
        Ok(Self {
            fired: vec![false; program.logic.responses.len()],
            memory: vec![false; program.logic.responses.len()],
            moving_memory: vec![false; program.logic.responses.len()],
            replay_memory: vec![false; program.logic.responses.len()],
            previous_wave_memory: vec![false; program.logic.responses.len()],
            group_fired: vec![false; program.groups.len()],
            program,
            sources,
            aliases,
            phase: 0,
            repeated: false,
            elapsed: 0,
            launch_inputs: FxHashMap::default(),
            ready_inputs,
            actions: Vec::new(),
            launch_actions: Vec::new(),
            decisions,
            outputs,
            output_sources,
            actor_groups,
            memory_actors,
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
        if self.program.assume_instant {
            self.advance_ideal(nodes);
            return self.supply_changes(nodes);
        }
        if self.phase == 0 || self.phase == 6 {
            self.sample_responses(nodes);
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
                    self.launch_inputs = self
                        .sources
                        .iter()
                        .map(|(&pos, &id)| (pos, nodes[id].output_power))
                        .collect();
                    self.launch_actions = std::mem::take(&mut self.actions);
                    self.repeated = false;
                } else {
                    self.repeated = true;
                }
                self.phase = 1;
            } else {
                self.phase = 0;
                self.ready_inputs = self
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
        self.supply_changes(nodes)
    }

    fn sample_responses(&mut self, nodes: &Nodes) {
        for actor in 0..self.fired.len() {
            self.fired[actor] = self.evaluate(self.program.logic.responses[actor], nodes);
        }
        self.group_fired.fill(false);
        for (actor, &fired) in self.fired.iter().enumerate() {
            self.group_fired[self.actor_groups[actor]] |= fired;
        }
    }

    fn advance_ideal(&mut self, nodes: &Nodes) {
        if let Some(clock) = self.program.clocked.as_ref().map(|c| c.clock) {
            if !self.evaluate(self.program.logic.responses[clock], nodes) {
                self.phase = 0;
                return; // Stored data and the last sampled output remain held.
            }
            // Keep the observer clock's six-tick cadence, without movement or
            // reset pulses. Every cell reads the old bank before atomic commit.
            if self.phase == 0 || self.phase == 6 {
                self.sample_responses(nodes);
                for cell in &self.program.clocked.as_ref().unwrap().memory {
                    self.memory[cell.actor] = self.fired[cell.actor];
                }
                self.phase = 1;
            } else {
                self.phase += 1;
            }
        } else {
            self.sample_responses(nodes);
        }
    }

    fn supply_changes(&self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        let mut changes: Vec<_> = self
            .aliases
            .iter()
            .map(|(id, supply)| {
                let powered = match *supply {
                    Supply::Wave {
                        group,
                        initial,
                        near,
                    } => {
                        if self.program.assume_instant {
                            if let Some(actor) = near {
                                self.near_owner(group) == Some(actor)
                            } else {
                                initial && !self.group_fired[group]
                            }
                        } else {
                            let low = self.phase != 0 && self.phase != 6 && self.group_fired[group];
                            initial && !low
                        }
                    }
                    Supply::Memory { actor, far } => {
                        !self.moving_memory[actor] && self.memory[actor] != far
                    }
                };
                (*id, if powered { 15 } else { 0 })
            })
            .collect();
        changes.extend(self.output_changes(nodes));
        changes
    }

    fn geometry(&self, actor: usize, part: GeometryPart) -> bool {
        if self.memory_actors[actor] {
            return match part {
                GeometryPart::FarPayload | GeometryPart::Head => {
                    !self.memory[actor] && !self.moving_memory[actor]
                }
                GeometryPart::NearPayload => self.memory[actor] && !self.moving_memory[actor],
                GeometryPart::RetractedBase => self.memory[actor] && !self.moving_memory[actor],
                GeometryPart::MovingBase => self.memory[actor] && self.moving_memory[actor],
            };
        }
        if self.program.assume_instant {
            return match part {
                GeometryPart::FarPayload => !self.group_fired[self.actor_groups[actor]],
                GeometryPart::NearPayload => {
                    self.near_owner(self.actor_groups[actor]) == Some(actor)
                }
                GeometryPart::Head => !self.fired[actor],
                GeometryPart::RetractedBase => self.fired[actor],
                GeometryPart::MovingBase => false,
            };
        }
        let active = self.phase != 0 && self.phase != 6;
        match part {
            GeometryPart::FarPayload => !active || !self.group_fired[self.actor_groups[actor]],
            GeometryPart::NearPayload => self.phase == 3 && self.fired[actor],
            GeometryPart::Head => !active || !self.fired[actor],
            GeometryPart::RetractedBase => self.phase == 3 && self.fired[actor],
            GeometryPart::MovingBase => (1..=2).contains(&self.phase) && self.fired[actor],
        }
    }

    fn near_owner(&self, group: usize) -> Option<usize> {
        self.program.groups[group]
            .iter()
            .copied()
            .find(|&actor| self.fired[actor])
    }

    pub(super) fn output_depends_on(&self, source: NodeId) -> bool {
        self.output_sources.contains(&source)
    }

    fn evaluate(&self, mut root: Expr, nodes: &Nodes) -> bool {
        while root > TRUE {
            let decision = &self.decisions[(root - 2) as usize];
            let high = match decision.input {
                Input::Source(source) => nodes[source].output_power > decision.threshold,
                Input::Memory(actor) => self.memory[actor],
                Input::Geometry { actor, part } => self.geometry(actor, part),
            };
            root = if high { decision.high } else { decision.low };
        }
        root == TRUE
    }

    pub(super) fn output_changes(&self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        self.outputs
            .iter()
            .filter_map(|output| {
                let strength = output
                    .terms
                    .iter()
                    .filter(|term| self.evaluate(term.guard, nodes))
                    .map(|term| {
                        term.source
                            .map_or(15, |source| nodes[source].output_power)
                            .saturating_sub(term.attenuation)
                    })
                    .max()
                    .unwrap_or(0);
                (strength != nodes[output.node].output_power).then_some((output.node, strength))
            })
            .collect()
    }

    pub(super) fn materialize<W: World>(self, world: &mut W) {
        if self.program.assume_instant {
            self.materialize_ideal(world);
            return;
        }
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
        replay.piston_state_mut().next_identity = world.piston_state().next_identity;
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
        for (&pos, &strength) in &self.ready_inputs {
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
            for &(pos, strength) in &self.launch_actions {
                apply_source(&mut replay, pos, strength);
            }
            for (&pos, &strength) in &self.launch_inputs {
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
        let target = world.piston_state_mut();
        target
            .events
            .retain(|event| !self.program.owned.contains(&event.pos));
        target.events.extend(
            state
                .events
                .into_iter()
                .filter(|event| self.program.owned.contains(&event.pos)),
        );
        target
            .motions
            .retain(|motion| !self.program.owned.contains(&motion.pos));
        target.motions.extend(
            state
                .motions
                .into_iter()
                .filter(|motion| self.program.owned.contains(&motion.pos)),
        );
        target.movement_work = target.movement_work[target.movement_cursor..]
            .iter()
            .copied()
            .filter(|(pos, _)| !self.program.owned.contains(pos))
            .chain(
                state
                    .movement_work
                    .into_iter()
                    .skip(state.movement_cursor)
                    .filter(|(pos, _)| self.program.owned.contains(pos)),
            )
            .collect();
        target.movement_cursor = 0;
        target.logical_tick = time;
        target.phase = state.phase;
        target.scheduled_advanced = state.scheduled_advanced;
        target.next_identity = target.next_identity.max(state.next_identity);
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

    fn materialize_ideal(self, world: &mut impl World) {
        // Export current logical occupancy and stored bits, rather than replaying
        // a launch history which this mode deliberately does not implement.
        for &(_, pos, _) in &self.program.aliases {
            world.set_block(pos, Block::Air);
            world.delete_block_entity(pos);
        }
        for (actor, p) in self.program.pistons.iter().enumerate() {
            let retracted = if self.memory_actors[actor] {
                self.memory[actor]
            } else {
                self.fired[actor]
            };
            let mut piston = p.piston;
            piston.extended = !retracted;
            world.set_block(p.pos, Block::Piston { piston });
            if !retracted {
                world.set_block(
                    p.head,
                    Block::PistonHead {
                        head: RedstonePistonHead {
                            facing: piston.facing,
                            sticky: piston.sticky,
                            short: false,
                        },
                    },
                );
            }
        }
        for (group, members) in self.program.groups.iter().enumerate() {
            let p = &self.program.pistons[members[0]];
            let payload = self
                .program
                .template
                .iter()
                .find(|(pos, _, _)| *pos == p.payload)
                .map_or(Block::Air, |(_, block, _)| *block);
            let owner = if self.memory_actors[members[0]] {
                self.memory[members[0]].then_some(members[0])
            } else {
                self.near_owner(group)
            };
            let pos = owner.map_or(p.payload, |actor| self.program.pistons[actor].head);
            world.set_block(pos, payload);
        }
        // Internal reset observers are dormant in the logical snapshot.
        for &(pos, block, _) in &self.program.template {
            if self.program.owned.contains(&pos) && matches!(block, Block::Observer { .. }) {
                world.set_block(pos, block);
            }
        }
        world.piston_state_mut().logical_tick =
            self.program.logical_tick.wrapping_add(self.elapsed);
        for &pos in self
            .program
            .logic
            .wires
            .iter()
            .chain(&self.program.logic.consumer_wires)
        {
            if let Block::RedstoneWire { mut wire } = world.get_block(pos) {
                wire.power = 0;
                world.set_block(pos, Block::RedstoneWire { wire });
            }
        }
        for &pos in self
            .program
            .logic
            .wires
            .iter()
            .chain(&self.program.logic.consumer_wires)
        {
            let block = world.get_block(pos);
            crate::redstone::update(block, world, pos, None);
        }
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
