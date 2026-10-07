//! Dependency-driven logical piston execution over frozen input/state snapshots.
//! Reset exports settled geometry without replaying physical movement.
use super::node::{NodeId, Nodes};
use crate::redpiler::backend::BackendError;
use crate::redpiler::instant::boolean::{Expr, GeometryPart, Variable};
use crate::redpiler::instant::program::PreparedInstant;
use crate::redpiler::instant::sampling::SamplingSource;
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstonePistonHead};
use mchprs_blocks::BlockPos;
use rustc_hash::{FxHashMap, FxHashSet};

mod logical;

pub(super) struct Runtime {
    logical: Option<logical::State>,
    program: PreparedInstant,
    aliases: Vec<(NodeId, Supply)>,
    fired: Vec<bool>,
    elapsed: u64,
    outputs: Vec<Output>,
    output_sources: FxHashSet<NodeId>,
    actor_groups: Vec<usize>,
    group_fired: Vec<bool>,
    memory_actors: Vec<bool>,
    memory: Vec<bool>,
    sampling: Vec<SamplingEvent>,
    observations: Vec<(BlockPos, Block)>,
}

struct SamplingEvent {
    actors: Vec<usize>,
    source: SampleSource,
    previous: u8,
}

enum SampleSource {
    Generator(usize),
    Power(Vec<(usize, Term)>),
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Input {
    Source(NodeId),
    Memory(usize),
    Response(usize),
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
        program: PreparedInstant,
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
            let memory = program.clocked.iter().flat_map(|c| &c.memory)
                .chain(&program.independent_memory)
                .find(|m| program.groups[group].contains(&m.actor));
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
        let decisions = program
            .logic
            .arena
            .nodes
            .iter()
            .map(|d| {
                let (input, threshold) = match d.variable {
                    Variable::Signal { pos, threshold, .. } => {
                        (Input::Source(*sources.get(&pos).ok_or(BackendError::MissingInstantBinding { pos })?), threshold)
                    }
                    Variable::Memory(actor)
                        if program.clocked.iter().flat_map(|c| &c.memory)
                            .chain(&program.independent_memory).any(|m| m.actor == actor) =>
                    {
                        (Input::Memory(actor), 0)
                    }
                    Variable::Actuator(actor) if actor < program.logic.responses.len() => (Input::Response(actor), 0),
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
                                    bindings.get(&pos).copied().ok_or(BackendError::MissingInstantBinding { pos })
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
        let mut output_sources: FxHashSet<NodeId> = outputs
            .iter()
            .flat_map(|output| output.terms.iter().filter_map(|term| term.source))
            .collect();
        let mut memory_actors = vec![false; program.logic.responses.len()];
        for cell in program.clocked.iter().flat_map(|c| &c.memory).chain(&program.independent_memory) {
            memory_actors[cell.actor] = true;
        }
        let mut sampling_roots = Vec::new();
        let mut sampling = Vec::new();
        for event in &program.sampling {
            let (source, previous) = match &event.source {
                SamplingSource::Generator(actor) => (SampleSource::Generator(*actor), u8::from(!program.pistons[*actor].piston.extended) * 15),
                SamplingSource::Power { initial, terms, .. } => {
                    let terms = terms.iter().map(|term| {
                        let root = sampling_roots.len();
                        sampling_roots.push(term.guard);
                        Ok((root, Term {
                            guard: term.guard,
                            source: term.source.map(|pos| bindings.get(&pos).copied()
                                .ok_or(BackendError::MissingInstantBinding { pos })).transpose()?,
                            attenuation: term.attenuation,
                        }))
                    }).collect::<Result<_, BackendError>>()?;
                    (SampleSource::Power(terms), *initial)
                }
            };
            sampling.push(SamplingEvent { source, previous, actors: event.actors.clone() });
        }
        output_sources.extend(sampling.iter().flat_map(|event| match &event.source {
            SampleSource::Power(terms) => terms.iter().filter_map(|(_, term)| term.source).collect::<Vec<_>>(),
            SampleSource::Generator(_) => Vec::new(),
        }));
        let logical = Some(logical::State::bind(&decisions, program.logic.responses.clone(), &program.logic.response_order,
            outputs.iter().flat_map(|output| output.terms.iter().map(|term| term.guard)).collect(), sampling_roots)?);
        if let Some(state) = &logical {
            if let Some(source) = state.source_nodes().chain(output_sources.iter().copied())
                .find(|&source| matches!(nodes[source].ty, super::node::NodeType::Wire))
            {
                let pos = bindings.iter().find_map(|(&pos, &node)| (node == source).then_some(pos))
                    .ok_or(BackendError::InvalidInstantProgram)?;
                return Err(BackendError::LogicalWireInput { pos });
            }
        }
        let fired: Vec<_> = program.pistons.iter()
            .map(|piston| !piston.piston.extended)
            .collect();
        let mut memory = vec![false; program.logic.responses.len()];
        for cell in program.clocked.iter().flat_map(|c| &c.memory).chain(&program.independent_memory) {
            memory[cell.actor] = cell.initial;
        }
        let mut group_fired = vec![false; program.groups.len()];
        for (actor, &value) in fired.iter().enumerate() {
            group_fired[actor_groups[actor]] |= value;
        }
        let mut runtime = Self {
            logical,
            fired,
            memory,
            group_fired,
            program,
            aliases,
            elapsed: 0,
            outputs,
            output_sources,
            actor_groups,
            memory_actors,
            sampling,
            observations: Vec::new(),
        };
        // Compilation establishes event baselines; activation itself is not a sample.
        let mut state = runtime.logical.take().unwrap();
        state.responses.capture(|input, threshold| runtime.read_input(input, threshold, nodes));
        state.sampling.capture(|input, threshold| runtime.read_input(input, threshold, nodes));
        for event in &mut runtime.sampling {
            event.previous = Self::sample_strength(event, &mut state, nodes);
        }
        runtime.logical = Some(state);
        Ok(runtime)
    }

    pub(super) fn collect_statistics(&self, stats: &mut crate::redpiler::RegionStatistics) {
        stats.logical_regions += 1;
        stats.clocked_regions += usize::from(self.program.clocked.is_some());
        stats.pistons += self.program.pistons.len();
        stats.payload_groups += self.program.groups.len();
        stats.memory_cells += self.program.clocked.as_ref().map_or(0, |c| c.memory.len()) + self.program.independent_memory.len();
        stats.decisions += self.program.logic.arena.nodes.len();
        stats.response_roots += self.program.logic.responses.len();
        stats.output_ports += self.outputs.len();
        stats.output_terms += self.outputs.iter().map(|output| output.terms.len()).sum::<usize>();
        let state = self.logical.as_ref().unwrap();
        let (responses, response_inputs) = state.responses.compile_counts();
        let (outputs, output_inputs) = state.outputs.compile_counts();
        let (sampling, sampling_inputs) = state.sampling.compile_counts();
        stats.logical_response_decisions += responses;
        stats.logical_output_decisions += outputs + sampling;
        stats.logical_input_bindings += response_inputs + output_inputs + sampling_inputs;
    }

    pub(super) fn begin_tick(&mut self) {
        self.elapsed += 1;
    }

    pub(super) fn source_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.logical.iter().flat_map(|state|
            state.source_nodes().chain(self.output_sources.iter().copied()))

    }

    #[cfg(test)]
    pub(crate) fn logical_stats(&self) -> Option<(u64, u64, Vec<(BlockPos, bool)>)> {
        self.logical.as_ref().map(|state| (
            state.responses.evaluations() + state.outputs.evaluations() + state.sampling.evaluations(), state.samples,
            self.program.clocked.iter().flat_map(|clock| &clock.memory).chain(&self.program.independent_memory)
                .map(|cell| (cell.base, self.memory[cell.actor])).collect(),
        ))
    }

    fn read_input(&self, input: Input, threshold: u8, nodes: &Nodes) -> bool {
        match input {
            Input::Source(source) => nodes[source].output_power > threshold,
            Input::Memory(actor) => self.memory[actor],
            Input::Response(actor) => self.fired[actor],
            Input::Geometry { actor, part } => self.geometry(actor, part),
        }
    }

    fn sample_strength(event: &SamplingEvent, state: &mut logical::State, nodes: &Nodes) -> u8 {
        match &event.source {
            SampleSource::Generator(actor) => u8::from(state.responses.evaluate(*actor)) * 15,
            SampleSource::Power(terms) => terms.iter().filter_map(|(root, term)| {
                state.sampling.evaluate(*root).then(|| term.source.map_or(15, |id| nodes[id].output_power)
                    .saturating_sub(term.attenuation))
            }).max().unwrap_or(0),
        }
    }

    pub(super) fn advance(&mut self, nodes: &Nodes, sources_changed: bool) -> Vec<(NodeId, u8)> {
        let state = self.logical.as_ref().unwrap();
        let due = state.next_sample.is_some_and(|deadline| deadline <= self.elapsed);
        if state.initialized && !sources_changed && !due {
            return Vec::new();
        }
        let mut state = self.logical.take().unwrap();
        state.responses.capture(|input, threshold| self.read_input(input, threshold, nodes));
        let sample = if let Some(clock) = self.program.clocked.as_ref().map(|clock| clock.clock) {
            // Control pose follows the source independently of bank sampling;
            // stopping the clock preserves memory and the last data response.
            let active = state.responses.evaluate(clock);
            self.fired[clock] = active;
            self.group_fired[self.actor_groups[clock]] = active;
            if !active {
                state.next_sample = None;
                false
            } else { state.next_sample.is_none() || due }
        } else { !state.initialized || sources_changed };
        if sample {
            // Every response reads the same frozen old bank before any write.
            for actor in 0..self.fired.len() { self.fired[actor] = state.responses.evaluate(actor); }
            self.group_fired.fill(false);
            for (actor, &fired) in self.fired.iter().enumerate() {
                self.group_fired[self.actor_groups[actor]] |= fired;
            }
            if let Some(clock) = &self.program.clocked {
                for cell in &clock.memory { self.memory[cell.actor] = self.fired[cell.actor]; }
                #[cfg(test)]
                { state.samples += 1; }
                state.next_sample = Some(self.elapsed + 6);
            }
        }
        for event in 0..self.sampling.len() {
            // A source event is separate from data evaluation. Each event reads
            // one old bank; the next delivered event sees the committed bank.
            state.responses.capture(|input, threshold| self.read_input(input, threshold, nodes));
            state.sampling.capture(|input, threshold| self.read_input(input, threshold, nodes));
            let strength = Self::sample_strength(&self.sampling[event], &mut state, nodes);
            if std::mem::replace(&mut self.sampling[event].previous, strength) == strength {
                continue;
            }
            for &actor in &self.sampling[event].actors {
                self.fired[actor] = state.responses.evaluate(actor);
            }
            for &actor in &self.sampling[event].actors {
                self.memory[actor] = self.fired[actor];
            }
            #[cfg(test)]
            { state.samples += 1; }
            state.responses.capture(|input, threshold| self.read_input(input, threshold, nodes));
            for actor in 0..self.fired.len() {
                if !self.memory_actors[actor] {
                    self.fired[actor] = state.responses.evaluate(actor);
                }
            }
        }
        self.group_fired.fill(false);
        for (actor, &fired) in self.fired.iter().enumerate() {
            self.group_fired[self.actor_groups[actor]] |= if self.memory_actors[actor] {
                self.memory[actor]
            } else { fired };
        }
        state.initialized = true;
        self.logical = Some(state);
        self.supply_changes(nodes)
    }

    fn supply_changes(&mut self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        let mut changes: Vec<_> = self
            .aliases
            .iter()
            .filter_map(|(id, supply)| {
                let powered = match *supply {
                    Supply::Wave {
                        group,
                        initial,
                        near,
                    } => {
                        if let Some(actor) = near {
                            self.near_owner(group) == Some(actor)
                        } else {
                            initial && !self.group_fired[group]
                        }
                    }
                    Supply::Memory { actor, far } => self.memory[actor] != far,
                };
                let strength = if powered { 15 } else { 0 };
                (nodes[*id].output_power != strength).then_some((*id, strength))
            })
            .collect();
        changes.extend(self.output_changes(nodes));
        changes
    }

    fn geometry(&self, actor: usize, part: GeometryPart) -> bool {
        if self.memory_actors[actor] {
            return match part {
                GeometryPart::FarPayload | GeometryPart::Head => !self.memory[actor],
                GeometryPart::NearPayload | GeometryPart::RetractedBase => self.memory[actor],
                GeometryPart::MovingBase => false,
            };
        }
        match part {
            GeometryPart::FarPayload => !self.group_fired[self.actor_groups[actor]],
            GeometryPart::NearPayload => self.near_owner(self.actor_groups[actor]) == Some(actor),
            GeometryPart::Head => !self.fired[actor],
            GeometryPart::RetractedBase => self.fired[actor],
            GeometryPart::MovingBase => false,
        }
    }

    fn near_owner(&self, group: usize) -> Option<usize> {
        self.program.groups[group]
            .iter()
            .copied()
            .find(|&actor| if self.memory_actors[actor] { self.memory[actor] } else { self.fired[actor] })
    }

    fn block_at(&self, pos: BlockPos) -> Option<Block> {
        for (actor, piston) in self.program.pistons.iter().enumerate() {
            if pos == piston.pos {
                let mut state = piston.piston;
                state.extended = !self.geometry(actor, GeometryPart::RetractedBase);
                return Some(Block::Piston { piston: state });
            }
        }
        for (actor, piston) in self.program.pistons.iter().enumerate() {
            if pos == piston.head {
                return Some(if self.geometry(actor, GeometryPart::Head) {
                    Block::PistonHead { head: RedstonePistonHead {
                        facing: piston.piston.facing, sticky: piston.piston.sticky, short: false,
                    }}
                } else if self.geometry(actor, GeometryPart::NearPayload) {
                    self.program.payloads[self.actor_groups[actor]]
                } else { Block::Air });
            }
            let group = self.actor_groups[actor];
            if self.program.payloads[group] != Block::Air && pos == piston.payload {
                return Some(if self.geometry(actor, GeometryPart::FarPayload) {
                    self.program.payloads[group]
                } else { Block::Air });
            }
        }
        None
    }

    pub(super) fn watch_geometry(&mut self, pos: BlockPos) -> bool {
        let Some(block) = self.block_at(pos) else { return false; };
        self.observations.push((pos, block));
        true
    }

    pub(super) fn geometry_changes(&mut self) -> Vec<BlockPos> {
        let mut changes = Vec::new();
        for index in 0..self.observations.len() {
            let (pos, previous) = self.observations[index];
            let block = self.block_at(pos).unwrap();
            if previous != block {
                self.observations[index].1 = block;
                changes.push(pos);
            }
        }
        changes
    }

    pub(super) fn output_depends_on(&self, source: NodeId) -> bool {
        self.output_sources.contains(&source)
    }

    pub(super) fn output_changes(&mut self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        let mut state = self.logical.take().unwrap();
            state.outputs.capture(|input, threshold| self.read_input(input, threshold, nodes));
            let mut root = 0;
            let changes = self.outputs.iter().filter_map(|output| {
                let mut strength = 0;
                for term in &output.terms {
                    let guard = root; root += 1;
                    let candidate = term.source.map_or(15, |source| nodes[source].output_power)
                        .saturating_sub(term.attenuation);
                    if candidate > strength && state.outputs.evaluate(guard) { strength = candidate; }
                }
                (strength != nodes[output.node].output_power).then_some((output.node, strength))
            }).collect();
            self.logical = Some(state);
            changes
    }

    pub(super) fn materialize(self, world: &mut impl World) {
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
            let payload = self.program.payloads[group];
            if payload == Block::Air { continue; }
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
            if self.program.owned.contains(&pos) {
                if let Block::Observer { mut observer } = block {
                    observer.powered = false;
                    world.set_block(pos, Block::Observer { observer });
                }
            }
        }
        world.piston_state_mut().logical_tick =
            self.program.logical_tick.wrapping_add(self.elapsed);
        // All paths were extracted against settled geometry at compilation.
        // Write their values directly: invoking update() here would create a
        // physical reset episode which the logical executor never performed.
        let mut wires = Vec::with_capacity(self.program.logic.handoff_wires.len());
        for (pos, terms) in &self.program.logic.handoff_wires {
            let pos = *pos;
            if let Block::RedstoneWire { mut wire } = world.get_block(pos) {
                wire = crate::redstone::wire::get_regulated_sides(wire, world, pos);
                wire.power = terms.iter().filter(|term| {
                    self.program.logic.arena.evaluate(term.guard, |variable| match variable {
                        Variable::Geometry { actor, part } => self.geometry(actor, part),
                        Variable::Memory(actor) => self.memory[actor],
                        Variable::Signal { pos, threshold, .. } => {
                            crate::redstone::source_strength(world.get_block(pos), world, pos) > threshold
                        }
                        _ => unreachable!("validated logical handoff has no sampled local inputs"),
                    })
                }).map(|term| {
                    term.source.map_or(15, |source| {
                        crate::redstone::source_strength(world.get_block(source), world, source)
                    }).saturating_sub(term.attenuation)
                }).max().unwrap_or(0);
                wires.push((pos, wire));
            }
        }
        for (pos, wire) in wires {
            world.set_block(pos, Block::RedstoneWire { wire });
        }
    }
}

