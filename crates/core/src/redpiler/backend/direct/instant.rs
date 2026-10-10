//! Dependency-driven logical piston execution over frozen input/state snapshots.
//! Reset exports settled geometry and resumes owned native reset callbacks.
use super::node::{NodeId, Nodes};
use crate::redpiler::backend::BackendError;
use crate::redpiler::instant::boolean::{Expr, GeometryPart, Variable};
use crate::redpiler::instant::program::PreparedInstant;
use crate::redpiler::instant::sampling::SamplingSource;
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstonePistonHead};
use mchprs_blocks::BlockPos;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

mod logical;
mod timing;

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
    published_memory: Vec<bool>,
    memory_geometry: FxHashMap<BlockPos, Observation>,
    geometry_index: FxHashMap<BlockPos, (Observation, bool)>,
    sampling: Vec<SamplingEvent>,
    activation_wires: Vec<ActivationWire>,
    activations: VecDeque<crate::redpiler::instant::activation::Delivery>,
    deferred_sources: bool,
    #[cfg(test)]
    activation_trace: Vec<crate::redpiler::instant::activation::Delivery>,
    sampling_groups: Vec<Vec<usize>>,
    sampling_pending: bool,
    observations: Vec<(BlockPos, Observation, Block)>,
    observations_dirty: bool,
    boundaries: Vec<timing::Boundary>,
    boundary_index: Vec<Option<usize>>,
    reset_owners: Vec<Vec<usize>>,
    next_boundary: Option<u64>,
    pending_launch: bool,
    in_tick: bool,
    bank_deadline: Option<u64>,
    bank_values: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Observation {
    Base(usize),
    Near(usize),
    Far(usize),
}

struct ActivationWire {
    terms: Vec<(usize, Term)>,
    previous: u8,
    deliveries: Vec<crate::redpiler::instant::activation::Delivery>,
}

struct SamplingEvent {
    targets: Vec<SamplingTarget>,
    source: SampleSource,
    writer: SampleWriter,
    previous: u8,
    delivered: bool,
}

struct SamplingTarget {
    actor: usize,
    requires_extended: bool,
    eligible: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SampleWriter {
    Generator(usize),
    Power(BlockPos),
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
    Committed(usize),
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
    #[cfg(test)]
    pub(super) fn activation_states(&self) -> Vec<(BlockPos, bool)> {
        self.program
            .activation
            .actors
            .iter()
            .map(|&actor| (self.program.pistons[actor].pos, self.fired[actor]))
            .collect()
    }

    #[cfg(test)]
    pub(super) fn geometry_state(
        &self,
        pos: BlockPos,
    ) -> Option<(usize, bool, bool, bool, Option<&'static str>, Block)> {
        let &(observation, _) = self.geometry_index.get(&pos)?;
        let actor = match observation {
            Observation::Base(actor) | Observation::Near(actor) | Observation::Far(actor) => actor,
        };
        let mut pending = vec![self.program.logic.responses[actor]];
        let mut seen = FxHashSet::default();
        let mut decisions = Vec::new();
        while let Some(root) = pending.pop() {
            if !seen.insert(root) {
                continue;
            }
            if let Some(decision) = self.program.logic.arena.decision(root) {
                if let Variable::Actuator(dependency) = decision.variable {
                    pending.push(self.program.logic.responses[dependency]);
                }
                decisions.push((root, decision, match decision.variable {
                    Variable::Actuator(dependency) => Some((self.program.pistons[dependency].pos, self.fired[dependency])),
                    _ => None,
                }));
                pending.extend([decision.low, decision.high]);
            }
        }
        if self.elapsed <= 1 {
            eprintln!("response pos={:?} root={} decisions={:?}", pos,
                self.program.logic.responses[actor], decisions);
        }
        let phase = self.boundary_index[actor].map(|index| match self.boundaries[index].phase {
            timing::Phase::Extended => "extended",
            timing::Phase::Retracting => "retracting",
            timing::Phase::Retracted => "retracted",
            timing::Phase::Extending => "extending",
        });
        Some((
            actor,
            self.fired[actor],
            self.program.activation.actors.contains(&actor),
            self.memory_actors[actor],
            phase,
            self.observed_block(observation),
        ))
    }

    #[cfg(test)]
    pub(super) fn output_states(
        &self,
        consumer: BlockPos,
        nodes: &Nodes,
    ) -> Option<Vec<String>> {
        let mut result = Vec::new();
        for (port_index, port) in self.program.logic.outputs.iter().enumerate() {
            for (term_index, term) in port.terms.iter().enumerate() {
                if port.consumer != consumer {
                    continue;
                }
                let geometry_guard =
                    self.program
                        .logic
                        .arena
                        .evaluate(term.guard, |variable| match variable {
                            Variable::Geometry { actor, part } => self.geometry(actor, part),
                            _ => unreachable!("output guards depend only on compiled geometry"),
                        });
                let runtime_term = &self.outputs[port_index].terms[term_index];
                let source_power = runtime_term
                    .source
                    .map_or(15, |source| nodes[source].output_power);
                let mut dependencies = Vec::new();
                let mut pending = vec![term.guard];
                let mut seen = FxHashSet::default();
                while let Some(root) = pending.pop() {
                    if !seen.insert(root) {
                        continue;
                    }
                    if let Some(decision) = self.program.logic.arena.decision(root) {
                        if let Variable::Geometry { actor, part } = decision.variable {
                            dependencies.push((
                                self.program.pistons[actor].pos,
                                part,
                                self.geometry(actor, part),
                                self.fired[actor],
                            ));
                        }
                        pending.extend([decision.low, decision.high]);
                    }
                }
                result.push(format!(
                    "source={:?} power={} attenuation={} guard={} output={} dependencies={:?}",
                    term.source, source_power, term.attenuation, geometry_guard,
                    nodes[self.outputs[port_index].node].output_power, dependencies
                ));
            }
        }
        (!result.is_empty()).then_some(result)
    }

    #[cfg(test)]
    pub(super) fn take_activation_trace(
        &mut self,
    ) -> Vec<crate::redpiler::instant::activation::Delivery> {
        std::mem::take(&mut self.activation_trace)
    }

    pub(super) fn mark_source_dirty(&mut self, source: NodeId) {
        if let Some(state) = &mut self.logical {
            state.mark_dirty(Input::Source(source));
        }
    }

    /// Bind prepared positions and expressions to backend nodes before activation.
    /// Missing bindings reject the staged runtime instead of publishing partial state.
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
            let memory = program
                .clocked
                .iter()
                .flat_map(|c| &c.memory)
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
            let node = *bindings
                .get(&pos)
                .ok_or(BackendError::MissingInstantBinding { pos })?;
            // These electrical supplies can never change. Their graph nodes and
            // the program's full geometry/restoration aliases remain intact.
            if matches!(
                supply,
                Supply::Wave {
                    initial: false,
                    near: None,
                    ..
                }
            ) && nodes[node].output_power == 0
            {
                continue;
            }
            aliases.push((node, supply));
        }
        let decisions = program
            .logic
            .arena
            .nodes
            .iter()
            .map(|d| {
                let (input, threshold) = match d.variable {
                    Variable::Signal { pos, threshold, .. } => (
                        Input::Source(
                            *sources
                                .get(&pos)
                                .ok_or(BackendError::MissingInstantBinding { pos })?,
                        ),
                        threshold,
                    ),
                    Variable::Memory(actor)
                        if program
                            .clocked
                            .iter()
                            .flat_map(|c| &c.memory)
                            .chain(&program.independent_memory)
                            .any(|m| m.actor == actor) =>
                    {
                        (Input::Memory(actor), 0)
                    }
                    Variable::Actuator(actor) if actor < program.logic.responses.len() => (
                        if program.activation.actors.contains(&actor) {
                            Input::Committed(actor)
                        } else {
                            Input::Response(actor)
                        },
                        0,
                    ),
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
        let mut output_sources: FxHashSet<NodeId> = outputs
            .iter()
            .flat_map(|output| output.terms.iter().filter_map(|term| term.source))
            .collect();
        let mut memory_actors = vec![false; program.logic.responses.len()];
        for cell in program
            .clocked
            .iter()
            .flat_map(|c| &c.memory)
            .chain(&program.independent_memory)
        {
            memory_actors[cell.actor] = true;
        }
        let mut sampling_roots = Vec::new();
        let mut sampling = Vec::new();
        for event in &program.sampling {
            let (source, writer, previous) = match &event.source {
                SamplingSource::Generator(actor) => (
                    SampleSource::Generator(*actor),
                    SampleWriter::Generator(*actor),
                    u8::from(!program.pistons[*actor].piston.extended) * 15,
                ),
                SamplingSource::Power {
                    initial,
                    terms,
                    writer,
                    ..
                } => {
                    let terms = terms
                        .iter()
                        .map(|term| {
                            let root = sampling_roots.len();
                            sampling_roots.push(term.guard);
                            Ok((
                                root,
                                Term {
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
                                },
                            ))
                        })
                        .collect::<Result<_, BackendError>>()?;
                    (
                        SampleSource::Power(terms),
                        SampleWriter::Power(*writer),
                        *initial,
                    )
                }
            };
            sampling.push(SamplingEvent {
                source,
                writer,
                previous,
                delivered: false,
                targets: event
                    .targets
                    .iter()
                    .map(|target| SamplingTarget {
                        actor: target.actor,
                        requires_extended: target.requires_extended,
                        eligible: false,
                    })
                    .collect(),
            });
        }
        output_sources.extend(sampling.iter().flat_map(|event| {
            match &event.source {
                SampleSource::Power(terms) => terms
                    .iter()
                    .filter_map(|(_, term)| term.source)
                    .collect::<Vec<_>>(),
                SampleSource::Generator(_) => Vec::new(),
            }
        }));
        let mut writers: Vec<(SampleWriter, Vec<usize>)> = Vec::new();
        for (index, event) in sampling.iter().enumerate() {
            if let Some((_, events)) = writers
                .iter_mut()
                .find(|(writer, _)| *writer == event.writer)
            {
                events.push(index);
            } else {
                writers.push((event.writer, vec![index]));
            }
        }
        let sampling_groups = writers.into_iter().map(|(_, events)| events).collect();
        let mut activation_wires = Vec::new();
        for wire in &program.activation.wires {
            let mut terms = Vec::new();
            for term in &wire.terms {
                let root = sampling_roots.len();
                sampling_roots.push(term.guard);
                terms.push((
                    root,
                    Term {
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
                    },
                ));
            }
            activation_wires.push(ActivationWire {
                terms,
                previous: wire.initial,
                deliveries: wire.deliveries.clone(),
            });
        }
        let logical = Some(logical::State::bind(
            &decisions,
            program.logic.responses.clone(),
            &program.logic.response_order,
            outputs
                .iter()
                .flat_map(|output| output.terms.iter().map(|term| term.guard))
                .collect(),
            sampling_roots,
        )?);
        if let Some(state) = &logical {
            if let Some(source) = state
                .source_nodes()
                .chain(output_sources.iter().copied())
                .find(|&source| matches!(nodes[source].ty, super::node::NodeType::Wire))
            {
                let pos = bindings
                    .iter()
                    .find_map(|(&pos, &node)| (node == source).then_some(pos))
                    .ok_or(BackendError::InvalidInstantProgram)?;
                return Err(BackendError::LogicalWireInput { pos });
            }
        }
        aliases.retain(|(node, _)| {
            !nodes[*node].updates.is_empty()
                || logical
                    .as_ref()
                    .unwrap()
                    .source_nodes()
                    .any(|source| source == *node)
                || output_sources.contains(node)
        });
        let fired: Vec<_> = program
            .pistons
            .iter()
            .map(|piston| !piston.piston.extended)
            .collect();
        let mut memory = vec![false; program.logic.responses.len()];
        let mut memory_geometry = FxHashMap::default();
        for cell in program
            .clocked
            .iter()
            .flat_map(|c| &c.memory)
            .chain(&program.independent_memory)
        {
            memory[cell.actor] = cell.initial;
            for (pos, observation) in [
                (cell.base, Observation::Base(cell.actor)),
                (
                    program.pistons[cell.actor].head,
                    Observation::Near(cell.actor),
                ),
                (cell.far, Observation::Far(cell.actor)),
            ] {
                memory_geometry.insert(pos, observation);
            }
        }
        let mut group_fired = vec![false; program.groups.len()];
        for (actor, &value) in fired.iter().enumerate() {
            group_fired[actor_groups[actor]] |= value;
        }
        let mut geometry_masks = vec![0u8; fired.len()];
        for (actor, part) in logical
            .as_ref()
            .unwrap()
            .outputs
            .geometry_inputs()
            .chain(logical.as_ref().unwrap().sampling.geometry_inputs())
        {
            geometry_masks[actor] |= 1 << part as u8;
        }
        for &(node, ref supply) in &aliases {
            if nodes[node].updates.is_empty() {
                continue;
            }
            match supply {
                Supply::Wave { group, near, .. } => {
                    let part = if near.is_some() {
                        GeometryPart::NearPayload
                    } else {
                        GeometryPart::FarPayload
                    };
                    for &actor in &program.groups[*group] {
                        geometry_masks[actor] |= 1 << part as u8;
                    }
                }
                Supply::Memory { actor, far } => {
                    geometry_masks[*actor] |= 1
                        << (if *far {
                            GeometryPart::FarPayload
                        } else {
                            GeometryPart::NearPayload
                        }) as u8;
                }
            }
        }
        for (actor, piston) in program.pistons.iter().enumerate() {
            if program.activation.actors.contains(&actor)
                || program.activation.pose_deliveries.contains_key(&piston.pos)
                || program
                    .activation
                    .pose_deliveries
                    .contains_key(&piston.head)
            {
                geometry_masks[actor] = 31;
            }
        }
        for members in &program.groups {
            let mask = members
                .iter()
                .fold(0, |mask, &actor| mask | geometry_masks[actor]);
            for &actor in members {
                geometry_masks[actor] |= mask;
            }
        }
        let mut reset_owners = vec![Vec::new(); fired.len()];
        for group in &program.reset_groups {
            for &actor in &group.actors {
                reset_owners[actor].push(group.owner);
            }
        }
        if let Some(clock) = &program.clocked {
            reset_owners[clock.clock].push(clock.clock);
        }
        for actor in 0..fired.len() {
            reset_owners[actor].sort_unstable();
            reset_owners[actor].dedup();
        }
        let mut boundary_index = vec![None; fired.len()];
        let mut boundaries = Vec::new();
        for actor in 0..fired.len() {
            if geometry_masks[actor] != 0 {
                boundary_index[actor] = Some(boundaries.len());
                boundaries.push(timing::Boundary::new(
                    actor,
                    reset_owners[actor].clone(),
                    fired[actor],
                ));
            }
        }
        let bank_values = vec![
            false;
            program
                .clocked
                .as_ref()
                .map_or(0, |clock| clock.memory.len())
        ];
        let geometry_candidates: Vec<_> = program
            .pistons
            .iter()
            .enumerate()
            .map(|(actor, piston)| {
                (
                    piston.pos,
                    piston.head,
                    (piston.head != piston.payload
                        && program.payloads[actor_groups[actor]] != Block::Air)
                        .then_some(piston.payload),
                )
            })
            .collect();
        let geometry_index = Self::build_geometry_index(&geometry_candidates);
        let mut runtime = Self {
            logical,
            fired,
            published_memory: memory.clone(),
            memory,
            memory_geometry,
            geometry_index,
            group_fired,
            program,
            aliases,
            elapsed: 0,
            outputs,
            output_sources,
            actor_groups,
            memory_actors,
            sampling,
            activation_wires,
            activations: VecDeque::new(),
            deferred_sources: false,
            #[cfg(test)]
            activation_trace: Vec::new(),
            sampling_groups,
            sampling_pending: false,
            observations: Vec::new(),
            observations_dirty: false,
            boundaries,
            boundary_index,
            reset_owners,
            next_boundary: None,
            pending_launch: false,
            in_tick: false,
            bank_deadline: None,
            bank_values,
        };
        // Compilation establishes event baselines; activation itself is not a sample.
        let mut state = runtime.logical.take().unwrap();
        state
            .responses
            .capture(|input, threshold| runtime.read_input(input, threshold, nodes));
        state
            .outputs
            .capture(|input, threshold| runtime.read_input(input, threshold, nodes));
        state
            .sampling
            .capture(|input, threshold| runtime.read_input(input, threshold, nodes));
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
        stats.memory_cells += self.program.clocked.as_ref().map_or(0, |c| c.memory.len())
            + self.program.independent_memory.len();
        stats.decisions += self.program.logic.arena.nodes.len();
        stats.response_roots += self.program.logic.responses.len();
        stats.output_ports += self.outputs.len();
        stats.output_terms += self
            .outputs
            .iter()
            .map(|output| output.terms.len())
            .sum::<usize>();
        let state = self.logical.as_ref().unwrap();
        let (responses, response_inputs) = state.responses.compile_counts();
        let (outputs, output_inputs) = state.outputs.compile_counts();
        let (sampling, sampling_inputs) = state.sampling.compile_counts();
        stats.logical_response_decisions += responses;
        stats.logical_output_decisions += outputs + sampling;
        stats.logical_input_bindings += response_inputs + output_inputs + sampling_inputs;
    }

    pub(super) fn has_pending_samples(&self) -> bool {
        self.sampling_pending || !self.activations.is_empty() || self.deferred_sources
    }

    pub(super) fn begin_tick(&mut self) {
        self.elapsed += 1;
        self.in_tick = true;
    }

    pub(super) fn end_tick(&mut self) {
        self.in_tick = false;
    }

    pub(super) fn source_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.logical.iter().flat_map(|state| {
            state
                .source_nodes()
                .chain(self.output_sources.iter().copied())
        })
    }

    #[cfg(test)]
    pub(crate) fn logical_stats(&self) -> Option<(u64, u64, Vec<(BlockPos, bool)>)> {
        self.logical.as_ref().map(|state| {
            (
                state.responses.evaluations()
                    + state.outputs.evaluations()
                    + state.sampling.evaluations(),
                state.samples,
                self.program
                    .clocked
                    .iter()
                    .flat_map(|clock| &clock.memory)
                    .chain(&self.program.independent_memory)
                    .map(|cell| (cell.base, self.memory[cell.actor]))
                    .collect(),
            )
        })
    }

    fn read_input(&self, input: Input, threshold: u8, nodes: &Nodes) -> bool {
        match input {
            Input::Source(source) => nodes[source].output_power > threshold,
            Input::Memory(actor) => self.memory[actor],
            Input::Response(actor) | Input::Committed(actor) => self.fired[actor],
            Input::Geometry { actor, part } => self.geometry(actor, part),
        }
    }

    fn sample_strength(event: &SamplingEvent, state: &mut logical::State, nodes: &Nodes) -> u8 {
        match &event.source {
            SampleSource::Generator(actor) => u8::from(state.responses.evaluate(*actor)) * 15,
            SampleSource::Power(terms) => terms
                .iter()
                .filter_map(|(root, term)| {
                    state.sampling.evaluate(*root).then(|| {
                        term.source
                            .map_or(15, |id| nodes[id].output_power)
                            .saturating_sub(term.attenuation)
                    })
                })
                .max()
                .unwrap_or(0),
        }
    }

    /// Commit due bank writes, capture inputs, and deliver one sampling writer.
    /// Targets of that writer read the same old bank; later writers see its commit.
    /// Electrical responses and scheduled geometry then publish their new strengths.
    pub(super) fn advance(
        &mut self,
        nodes: &Nodes,
        mut sources_changed: bool,
        clock_event: bool,
    ) -> Vec<(NodeId, u8)> {
        self.deferred_sources |= sources_changed;
        if let Some(event) = self.activations.pop_front() {
            #[cfg(test)]
            self.activation_trace.push(event);
            let eligible =
                !event.requires_extended || self.geometry(event.actor, GeometryPart::Head);
            if eligible {
                let mut state = self.logical.take().unwrap();
                state
                    .responses
                    .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
                let value = state.responses.evaluate(event.actor);
                if self.fired[event.actor] != value {
                    self.fired[event.actor] = value;
                    state.mark_dirty(Input::Committed(event.actor));
                    for &actor in &self.program.groups[self.actor_groups[event.actor]] {
                        state.mark_geometry_actor_dirty(actor);
                    }
                }
                self.logical = Some(state);
                if self.in_tick {
                    self.advance_boundaries(true);
                } else {
                    self.pending_launch = true;
                }
                self.queue_activations(nodes);
            }
            return self.supply_changes(nodes);
        }
        sources_changed = std::mem::take(&mut self.deferred_sources);
        let boundary_due = clock_event
            && self
                .next_boundary
                .is_some_and(|deadline| deadline <= self.elapsed);
        let bank_due = clock_event
            && self
                .bank_deadline
                .is_some_and(|deadline| deadline <= self.elapsed);
        let launch_due = clock_event && self.pending_launch;
        let state = self.logical.as_ref().unwrap();
        let due = clock_event
            && state
                .next_sample
                .is_some_and(|deadline| deadline <= self.elapsed);
        if state.initialized && !sources_changed && !due && !self.sampling_pending && !bank_due {
            if boundary_due || launch_due {
                if self.advance_boundaries(launch_due) {
                    self.queue_activations(nodes);
                    return self.supply_changes(nodes);
                }
            }
            return Vec::new();
        }
        let mut state = self.logical.take().unwrap();
        if bank_due {
            self.bank_deadline = None;
            for (cell, &value) in self
                .program
                .clocked
                .as_ref()
                .unwrap()
                .memory
                .iter()
                .zip(&self.bank_values)
            {
                let previous = self.memory[cell.actor];
                self.memory[cell.actor] = value;
                if value != previous {
                    state.mark_dirty(Input::Memory(cell.actor));
                    for &actor in &self.program.groups[self.actor_groups[cell.actor]] {
                        state.mark_geometry_actor_dirty(actor);
                    }
                }
            }
            #[cfg(test)]
            {
                state.samples += 1;
            }
        }
        if state.initialized {
            state
                .responses
                .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
        } else {
            state
                .responses
                .capture(|input, threshold| self.read_input(input, threshold, nodes));
        }
        let sample = if let Some(clock) = self.program.clocked.as_ref().map(|clock| clock.clock) {
            // Control pose follows the source independently of bank sampling;
            // stopping the clock preserves memory and the last data response.
            let active = state.responses.evaluate(clock);
            let control_group = self.actor_groups[clock];
            let control_changed = self.fired[clock] != active;
            self.fired[clock] = active;
            self.group_fired[control_group] = active;
            if control_changed {
                state.mark_dirty(Input::Response(clock));
                for &actor in &self.program.groups[control_group] {
                    state.mark_geometry_actor_dirty(actor);
                }
            }
            if !active {
                state.next_sample = None;
                false
            } else {
                state.next_sample.is_none() || due
            }
        } else {
            !state.initialized || sources_changed
        };
        if sample {
            // Every response reads the same frozen old bank before any write.
            let mut changed_actors = Vec::new();
            state.responses.evaluate_dirty(|actor, value| {
                if self.program.activation.actors.contains(&actor) {
                    return;
                }
                if self.fired[actor] != value {
                    changed_actors.push(actor);
                }
                self.fired[actor] = value;
            });
            for actor in changed_actors {
                state.mark_dirty(Input::Response(actor));
                state.mark_dirty(Input::Committed(actor));
                for &member in &self.program.groups[self.actor_groups[actor]] {
                    state.mark_geometry_actor_dirty(member);
                }
            }
            self.group_fired.fill(false);
            for (actor, &fired) in self.fired.iter().enumerate() {
                self.group_fired[self.actor_groups[actor]] |= fired;
            }
            if let Some(clock) = &self.program.clocked {
                for (cell, value) in clock.memory.iter().zip(&mut self.bank_values) {
                    *value = self.fired[cell.actor];
                }
                let launch = self.elapsed + u64::from(!self.in_tick);
                self.bank_deadline = Some(launch + 2);
                state.next_sample = Some(launch + 6);
            }
        }
        if state.initialized {
            state
                .sampling
                .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
        } else {
            state
                .sampling
                .capture(|input, threshold| self.read_input(input, threshold, nodes));
        }
        for event in &mut self.sampling {
            let strength = Self::sample_strength(event, &mut state, nodes);
            event.delivered |= std::mem::replace(&mut event.previous, strength) != strength;
        }
        if let Some(events) = self
            .sampling_groups
            .iter()
            .find(|events| events.iter().any(|&event| self.sampling[event].delivered))
        {
            // All notifications delivered by one source read one old bank.
            // Separate source events read the preceding event's committed bank.
            state
                .responses
                .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
            for &event in events {
                let notification = &mut self.sampling[event];
                if notification.delivered {
                    for target in &mut notification.targets {
                        target.eligible = !target.requires_extended || !self.memory[target.actor];
                        if target.eligible {
                            let value = state.responses.evaluate(target.actor);
                            if self.fired[target.actor] != value {
                                let group = self.actor_groups[target.actor];
                                state.mark_dirty(Input::Response(target.actor));
                                for &actor in &self.program.groups[group] {
                                    state.mark_geometry_actor_dirty(actor);
                                }
                            }
                            self.fired[target.actor] = value;
                        }
                    }
                }
            }
            #[cfg(test)]
            let mut sampled = false;
            for &event in events {
                let notification = &mut self.sampling[event];
                if notification.delivered {
                    for target in &notification.targets {
                        if target.eligible {
                            let changed = self.memory[target.actor] != self.fired[target.actor];
                            self.memory[target.actor] = self.fired[target.actor];
                            if changed {
                                let group = self.actor_groups[target.actor];
                                state.mark_dirty(Input::Memory(target.actor));
                                for &actor in &self.program.groups[group] {
                                    state.mark_geometry_actor_dirty(actor);
                                }
                            }
                            #[cfg(test)]
                            {
                                sampled = true;
                            }
                        }
                    }
                    notification.delivered = false;
                }
            }
            #[cfg(test)]
            {
                state.samples += u64::from(sampled);
            }
            state
                .responses
                .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
            let mut changed_actors = Vec::new();
            state.responses.evaluate_dirty(|actor, value| {
                if !self.memory_actors[actor] && !self.program.activation.actors.contains(&actor) {
                    if self.fired[actor] != value {
                        changed_actors.push(actor);
                    }
                    self.fired[actor] = value;
                }
            });
            for actor in changed_actors {
                state.mark_dirty(Input::Response(actor));
                state.mark_dirty(Input::Committed(actor));
                for &member in &self.program.groups[self.actor_groups[actor]] {
                    state.mark_geometry_actor_dirty(member);
                }
            }
        }
        self.sampling_pending = self.sampling.iter().any(|event| event.delivered);
        self.group_fired.fill(false);
        for (actor, &fired) in self.fired.iter().enumerate() {
            self.group_fired[self.actor_groups[actor]] |= if self.memory_actors[actor] {
                self.memory[actor]
            } else {
                fired
            };
        }
        state.initialized = true;
        self.observations_dirty = true;
        self.logical = Some(state);
        if clock_event {
            self.advance_boundaries(true);
        } else {
            self.pending_launch = true;
        }
        self.queue_activations(nodes);
        self.supply_changes(nodes)
    }

    fn queue_activations(&mut self, nodes: &Nodes) {
        let mut state = self.logical.take().unwrap();
        state
            .sampling
            .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
        for wire in &mut self.activation_wires {
            let strength = wire
                .terms
                .iter()
                .filter_map(|(root, term)| {
                    state.sampling.evaluate(*root).then(|| {
                        term.source
                            .map_or(15, |id| nodes[id].output_power)
                            .saturating_sub(term.attenuation)
                    })
                })
                .max()
                .unwrap_or(0);
            let previous = std::mem::replace(&mut wire.previous, strength);
            if previous != strength {
                self.activations.extend(wire.deliveries.iter().copied());
            }
        }
        self.logical = Some(state);
    }

    fn advance_boundaries(&mut self, launch: bool) -> bool {
        self.pending_launch = false;
        let clock_active = self
            .program
            .clocked
            .as_ref()
            .is_none_or(|clock| self.fired[clock.clock]);
        let mut changed = false;
        let mut changed_groups = Vec::new();
        let mut notifications = Vec::new();
        for boundary in &mut self.boundaries {
            let previous = boundary.phase;
            if launch {
                let requested = if self.memory_actors[boundary.actor] {
                    self.memory[boundary.actor]
                } else {
                    clock_active && self.fired[boundary.actor]
                };
                boundary.request(requested, self.elapsed);
            }
            let resetting =
                clock_active && boundary.reset_owners.iter().any(|&owner| self.fired[owner]);
            boundary.advance(self.elapsed, resetting);
            if previous != boundary.phase {
                changed = true;
                let piston = &self.program.pistons[boundary.actor];
                match boundary.phase {
                    timing::Phase::Retracting | timing::Phase::Retracted => {
                        notifications.extend([piston.pos, piston.head])
                    }
                    timing::Phase::Extending => {
                        notifications.extend([piston.head, piston.head, piston.pos])
                    }
                    timing::Phase::Extended => notifications.push(piston.head),
                }
                let group = self.actor_groups[boundary.actor];
                if !changed_groups.contains(&group) {
                    changed_groups.push(group);
                }
            }
        }
        self.next_boundary = self
            .boundaries
            .iter()
            .filter_map(|boundary| boundary.deadline)
            .min();
        self.observations_dirty |= changed;
        if let Some(state) = &mut self.logical {
            for group in changed_groups {
                for &actor in &self.program.groups[group] {
                    state.mark_geometry_actor_dirty(actor);
                }
            }
        }
        for pos in notifications {
            if let Some(deliveries) = self.program.activation.pose_deliveries.get(&pos) {
                self.activations.extend(deliveries.iter().copied());
            }
        }
        changed
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
                            self.geometry(actor, GeometryPart::NearPayload)
                        } else {
                            initial
                                && self.geometry(
                                    self.program.groups[group][0],
                                    GeometryPart::FarPayload,
                                )
                        }
                    }
                    Supply::Memory { actor, far } => {
                        self.geometry(
                            actor,
                            if far {
                                GeometryPart::FarPayload
                            } else {
                                GeometryPart::NearPayload
                            },
                        ) && self.program.payloads[self.actor_groups[actor]] == Block::RedstoneBlock
                    }
                };
                let strength = if powered { 15 } else { 0 };
                (nodes[*id].output_power != strength).then_some((*id, strength))
            })
            .collect();
        changes.extend(self.output_changes(nodes));
        changes
    }

    fn geometry(&self, actor: usize, part: GeometryPart) -> bool {
        if let Some(index) = self.boundary_index[actor] {
            if part == GeometryPart::FarPayload && !self.memory_actors[actor] {
                return self.program.groups[self.actor_groups[actor]]
                    .iter()
                    .all(|&member| {
                        self.boundary_index[member].map_or(!self.fired[member], |index| {
                            self.boundaries[index].geometry(part)
                        })
                    });
            }
            if part == GeometryPart::NearPayload {
                return self.boundaries[index].geometry(part)
                    && self.near_owner(self.actor_groups[actor]) == Some(actor);
            }
            return self.boundaries[index].geometry(part);
        }
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
        self.program.groups[group].iter().copied().find(|&actor| {
            if self.memory_actors[actor] {
                self.memory[actor]
            } else {
                self.fired[actor]
            }
        })
    }

    fn observed_block(&self, observation: Observation) -> Block {
        match observation {
            Observation::Base(actor) => {
                let mut piston = self.program.pistons[actor].piston;
                if self.geometry(actor, GeometryPart::MovingBase) {
                    return Block::MovingPiston {
                        moving: piston.into(),
                    };
                }
                piston.extended = !self.geometry(actor, GeometryPart::RetractedBase);
                Block::Piston { piston }
            }
            Observation::Near(actor) => {
                let piston = &self.program.pistons[actor];
                if self.geometry(actor, GeometryPart::Head) {
                    Block::PistonHead {
                        head: RedstonePistonHead {
                            facing: piston.piston.facing,
                            sticky: piston.piston.sticky,
                            short: false,
                        },
                    }
                } else if self.geometry(actor, GeometryPart::NearPayload) {
                    self.program.payloads[self.actor_groups[actor]]
                } else if self.boundary_index[actor].is_some_and(|index| {
                    self.boundaries[index].phase == timing::Phase::Extending
                        || (self.boundaries[index].phase == timing::Phase::Retracting
                            && self.program.payloads[self.actor_groups[actor]] != Block::Air
                            && self.near_owner(self.actor_groups[actor]) == Some(actor))
                }) {
                    Block::MovingPiston {
                        moving: piston.piston.into(),
                    }
                } else {
                    Block::Air
                }
            }
            Observation::Far(actor) => {
                if self.geometry(actor, GeometryPart::FarPayload) {
                    self.program.payloads[self.actor_groups[actor]]
                } else if let Some(owner) =
                    self.near_owner(self.actor_groups[actor]).filter(|&owner| {
                        self.boundary_index[owner].is_some_and(|index| {
                            self.boundaries[index].phase == timing::Phase::Extending
                        })
                    })
                {
                    Block::MovingPiston {
                        moving: self.program.pistons[owner].piston.into(),
                    }
                } else {
                    Block::Air
                }
            }
        }
    }

    pub(super) fn memory_block_at(&self, pos: BlockPos) -> Option<Block> {
        self.memory_geometry
            .get(&pos)
            .map(|&observation| self.committed_block(observation))
    }

    fn committed_block(&self, observation: Observation) -> Block {
        let actor = match observation {
            Observation::Base(actor) | Observation::Near(actor) | Observation::Far(actor) => actor,
        };
        let piston = self.program.pistons[actor].piston;
        let payload = self.program.payloads[self.actor_groups[actor]];
        match observation {
            Observation::Base(_) => Block::Piston {
                piston: piston.extend(!self.memory[actor]),
            },
            Observation::Near(_) if self.memory[actor] => payload,
            Observation::Near(_) => Block::PistonHead {
                head: piston.extend(true).into(),
            },
            Observation::Far(_) if self.memory[actor] => Block::Air,
            Observation::Far(_) => payload,
        }
    }

    pub(super) fn flush_memory(&mut self, world: &mut impl World) {
        for cell in self
            .program
            .clocked
            .iter()
            .flat_map(|clock| &clock.memory)
            .chain(&self.program.independent_memory)
        {
            if self.published_memory[cell.actor] == self.memory[cell.actor] {
                continue;
            }
            let piston = &self.program.pistons[cell.actor];
            for (pos, observation) in [
                (cell.base, Observation::Base(cell.actor)),
                (piston.head, Observation::Near(cell.actor)),
                (cell.far, Observation::Far(cell.actor)),
            ] {
                world.set_block(pos, self.committed_block(observation));
            }
            self.published_memory[cell.actor] = self.memory[cell.actor];
        }
    }

    pub(super) fn watch_geometry(&mut self, pos: BlockPos) -> bool {
        let Some(&(observation, ambiguous)) = self.geometry_index.get(&pos) else {
            return false;
        };
        if ambiguous {
            tracing::debug!(
                "overlapping instant geometry at {:?}; preserving first match",
                pos
            );
        }
        let actor = match observation {
            Observation::Base(actor) | Observation::Near(actor) | Observation::Far(actor) => actor,
        };
        let actors = match observation {
            Observation::Far(_) => self.program.groups[self.actor_groups[actor]].clone(),
            _ => vec![actor],
        };
        for actor in actors {
            if self.boundary_index[actor].is_none() {
                self.boundary_index[actor] = Some(self.boundaries.len());
                self.boundaries.push(timing::Boundary::new(
                    actor,
                    self.reset_owners[actor].clone(),
                    self.fired[actor],
                ));
            }
        }
        self.observations
            .push((pos, observation, self.observed_block(observation)));
        true
    }

    fn index_geometry(
        index: &mut FxHashMap<BlockPos, (Observation, bool)>,
        pos: BlockPos,
        observation: Observation,
    ) {
        if let Some((first, ambiguous)) = index.get_mut(&pos) {
            *ambiguous |= *first != observation;
        } else {
            index.insert(pos, (observation, false));
        }
    }

    fn build_geometry_index(
        candidates: &[(BlockPos, BlockPos, Option<BlockPos>)],
    ) -> FxHashMap<BlockPos, (Observation, bool)> {
        let mut index = FxHashMap::default();
        for (actor, &(base, _, _)) in candidates.iter().enumerate() {
            Self::index_geometry(&mut index, base, Observation::Base(actor));
        }
        for (actor, &(_, head, far)) in candidates.iter().enumerate() {
            Self::index_geometry(&mut index, head, Observation::Near(actor));
            if let Some(far) = far {
                Self::index_geometry(&mut index, far, Observation::Far(actor));
            }
        }
        index
    }

    pub(super) fn geometry_changes(&mut self) -> Vec<BlockPos> {
        let mut changes = Vec::new();
        if !std::mem::replace(&mut self.observations_dirty, false) {
            return changes;
        }
        for index in 0..self.observations.len() {
            let (pos, observation, previous) = self.observations[index];
            let block = self.observed_block(observation);
            if previous != block {
                self.observations[index].2 = block;
                changes.push(pos);
            }
        }
        changes
    }

    pub(super) fn output_changes(&mut self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        let mut state = self.logical.take().unwrap();
        state
            .outputs
            .capture_dirty(|input, threshold| self.read_input(input, threshold, nodes));
        let mut root = 0;
        let changes = self
            .outputs
            .iter()
            .filter_map(|output| {
                let mut strength = 0;
                for term in &output.terms {
                    let guard = root;
                    root += 1;
                    let candidate = term
                        .source
                        .map_or(15, |source| nodes[source].output_power)
                        .saturating_sub(term.attenuation);
                    if candidate > strength && state.outputs.evaluate(guard) {
                        strength = candidate;
                    }
                }
                (strength != nodes[output.node].output_power).then_some((output.node, strength))
            })
            .collect();
        self.logical = Some(state);
        changes
    }

    pub(super) fn materialize(mut self, world: &mut impl World) {
        if self
            .program
            .clocked
            .as_ref()
            .is_some_and(|clock| !self.fired[clock.clock])
        {
            self.fired.fill(false);
        }
        for boundary in &mut self.boundaries {
            let retracted = if self.memory_actors[boundary.actor] {
                self.memory[boundary.actor]
            } else {
                self.fired[boundary.actor]
            };
            *boundary = timing::Boundary::new(boundary.actor, Vec::new(), retracted);
        }
        self.group_fired.fill(false);
        for (actor, &fired) in self.fired.iter().enumerate() {
            self.group_fired[self.actor_groups[actor]] |= if self.memory_actors[actor] {
                self.memory[actor]
            } else {
                fired
            };
        }
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
            if payload == Block::Air {
                continue;
            }
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
        // Restore settled electrical paths before any native callback can read
        // the region; propagating updates here would expose partial geometry.
        let mut wires = Vec::with_capacity(self.program.logic.handoff_wires.len());
        for (pos, terms) in &self.program.logic.handoff_wires {
            let pos = *pos;
            if let Block::RedstoneWire { mut wire } = world.get_block(pos) {
                wire = crate::redstone::wire::get_regulated_sides(wire, world, pos);
                wire.power = terms
                    .iter()
                    .filter(|term| {
                        self.program
                            .logic
                            .arena
                            .evaluate(term.guard, |variable| match variable {
                                Variable::Geometry { actor, part } => self.geometry(actor, part),
                                Variable::Memory(actor) => self.memory[actor],
                                Variable::Actuator(actor) => self.fired[actor],
                                Variable::Signal { pos, threshold, .. } => {
                                    crate::redstone::source_strength(
                                        world.get_block(pos),
                                        world,
                                        pos,
                                    ) > threshold
                                }
                                _ => unreachable!(
                                    "validated logical handoff has no sampled local inputs"
                                ),
                            })
                    })
                    .map(|term| {
                        term.source
                            .map_or(15, |source| {
                                crate::redstone::source_strength(
                                    world.get_block(source),
                                    world,
                                    source,
                                )
                            })
                            .saturating_sub(term.attenuation)
                    })
                    .max()
                    .unwrap_or(0);
                wires.push((pos, wire));
            }
        }
        for (pos, wire) in wires {
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        self.resume_reset_protocol(world);
    }

    /// Start native callbacks after the whole settled snapshot has been written.
    /// Only active reset owners resume; stored BUD bits wait for their native writer.
    fn resume_reset_protocol(&self, world: &mut impl World) {
        let owners: FxHashSet<_> = self
            .program
            .reset_groups
            .iter()
            .map(|group| group.owner)
            .chain(self.program.clocked.iter().map(|clock| clock.clock))
            .filter(|&actor| self.fired[actor] && !self.memory_actors[actor])
            .map(|actor| self.program.pistons[actor].pos)
            .collect();
        for &(pos, block, _) in &self.program.template {
            if !self.program.owned.contains(&pos) {
                continue;
            }
            if let Block::Observer { observer } = block {
                if owners.contains(&pos.offset(observer.facing.into())) {
                    crate::redstone::update(
                        world.get_block(pos),
                        world,
                        pos,
                        Some(observer.facing.into()),
                    );
                }
            }
        }
        for piston in &self.program.pistons {
            if owners.contains(&piston.pos) {
                if let Block::Piston { piston: state } = world.get_block(piston.pos) {
                    crate::redstone::piston::update_piston_state(world, state, piston.pos);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Observation, Runtime};
    use mchprs_blocks::BlockPos;

    #[test]
    fn geometry_index_matches_legacy_lookup_and_marks_overlaps() {
        let positions = [
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 0, 0),
            BlockPos::new(2, 0, 0),
            BlockPos::new(3, 0, 0),
        ];
        let candidates = [
            (positions[0], positions[1], Some(positions[2])),
            (positions[1], positions[3], Some(positions[2])),
        ];
        let index = Runtime::build_geometry_index(&candidates);
        for pos in positions {
            let expected = candidates
                .iter()
                .enumerate()
                .find(|(_, (base, _, _))| *base == pos)
                .map(|(actor, _)| Observation::Base(actor))
                .or_else(|| {
                    candidates
                        .iter()
                        .enumerate()
                        .find_map(|(actor, (_, head, far))| {
                            if *head == pos {
                                Some(Observation::Near(actor))
                            } else if *far == Some(pos) {
                                Some(Observation::Far(actor))
                            } else {
                                None
                            }
                        })
                });
            assert_eq!(index.get(&pos).map(|entry| entry.0), expected);
        }
        assert_eq!(index[&positions[1]], (Observation::Base(1), true));
        assert_eq!(index[&positions[2]], (Observation::Far(0), true));
    }
}
