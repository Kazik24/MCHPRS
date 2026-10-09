//! Extract the first response of an acyclic instant network from conditional
//! electrical geometry. Compilation inspects possible occupancy; execution
//! evaluates Boolean functions and never walks blocks or runs piston events.
use super::boolean::{BooleanArena, Expr, GeometryPart, Variable, FALSE, TRUE};
use super::outputs::{supported_payload, OutputPort, PowerTerm};
use crate::redpiler::analysis::{AnalysisReport, PistonDescriptor};
use crate::redpiler::TaskMonitor;
use crate::redstone::{self, power};
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

pub(crate) mod sequential;

#[derive(Debug)]
pub(crate) struct WaveLogic {
    pub arena: BooleanArena,
    pub responses: Vec<Expr>,
    /// Dependency order for local logical responses; roots keep actor order.
    pub response_order: Vec<usize>,
    pub sources: Vec<BlockPos>,
    #[cfg(test)]
    pub response_sources: Vec<BlockPos>,
    pub wires: FxHashSet<BlockPos>,
    pub consumer_wires: FxHashSet<BlockPos>,
    pub outputs: Vec<OutputPort>,
    /// Settled logical dust strengths used only when exporting a snapshot.
    pub handoff_wires: Vec<(BlockPos, Vec<PowerTerm>)>,
    pub follows_payload: Vec<bool>,
    pub context: FxHashSet<BlockPos>,
}

impl WaveLogic {
    pub fn evaluate(&self, read: impl Fn(BlockPos) -> u8) -> Vec<bool> {
        self.evaluate_with_memory(read, |_| false)
    }

    pub fn evaluate_with_memory(
        &self,
        read: impl Fn(BlockPos) -> u8,
        memory: impl Fn(usize) -> bool,
    ) -> Vec<bool> {
        let mut values = vec![false; self.responses.len()];
        for index in 0..self.responses.len() {
            let actor = self.response_order.get(index).copied().unwrap_or(index);
            values[actor] = self
                .arena
                .evaluate(self.responses[actor], |variable| match variable {
                    Variable::Signal { pos, threshold, .. } => read(pos) > threshold,
                    Variable::Actuator(actor) => values[actor],
                    Variable::Geometry { .. } | Variable::Observer(_) | Variable::WireDot(_) => {
                        unreachable!("response contains unsupported physical state variables")
                    }
                    Variable::Memory(actor) => memory(actor),
                });
        }
        values
    }
}

struct Extractor<'a, W: World> {
    world: &'a W,
    report: &'a AnalysisReport,
    monitor: &'a TaskMonitor,
    arena: BooleanArena,
    far: FxHashMap<BlockPos, usize>,
    near: FxHashMap<BlockPos, usize>,
    bases: FxHashMap<BlockPos, usize>,
    group_of: Vec<usize>,
    payloads: Vec<Block>,
    shapes: FxHashMap<(usize, BlockPos), Vec<(Expr, RedstoneWire)>>,
    wires: FxHashSet<BlockPos>,
    sources: FxHashSet<BlockPos>,
    steps: usize,
    signal_order: FxHashMap<BlockPos, usize>,
    output_mode: bool,
    terms: Vec<PowerTerm>,
    context: FxHashSet<BlockPos>,
    memory: FxHashSet<usize>,
    sequential: bool,
    /// Keep each receiving dust strength independent of upstream propagation.
    wire_signals: bool,
    ideal: bool,
    observers: FxHashMap<BlockPos, usize>,
    owned_reset: FxHashSet<BlockPos>,
}

#[cfg(test)]
pub(crate) fn extract(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
) -> Result<WaveLogic, String> {
    extract_with_state(world, report, monitor, FxHashSet::default(), None)
}

#[cfg(test)]
pub(crate) fn extract_with_state(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
    memory: FxHashSet<usize>,
    clock: Option<usize>,
) -> Result<WaveLogic, String> {
    let mut reset = super::sampling::reset_candidates(world, report);
    if let Some(clock) = clock {
        let base = report.pistons[clock].pos;
        reset.extend(report.observers.iter().copied().filter(|&pos| {
            matches!(world.get_block(pos), Block::Observer { observer } if pos.offset(observer.facing.into()) == base)
        }));
    }
    let mut owned = crate::redpiler::analysis::families::reset_internals(&report.recognition);
    owned.extend(reset.iter().copied());
    extract_with_options(
        world,
        report,
        monitor,
        memory,
        clock,
        Some(&owned),
        Some(&reset),
        &[],
        &mut [],
    )
}

pub(crate) fn extract_ideal_with_state(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
    memory: FxHashSet<usize>,
    clock: Option<usize>,
    owned_handoff: &FxHashSet<BlockPos>,
    owned_reset: &FxHashSet<BlockPos>,
    generators: &[usize],
    sampling: &mut [super::sampling::SamplingEvent],
) -> Result<WaveLogic, String> {
    extract_with_options(
        world,
        report,
        monitor,
        memory,
        clock,
        Some(owned_handoff),
        Some(owned_reset),
        generators,
        sampling,
    )
}

fn extract_with_options(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
    memory: FxHashSet<usize>,
    clock: Option<usize>,
    owned_handoff: Option<&FxHashSet<BlockPos>>,
    owned_reset: Option<&FxHashSet<BlockPos>>,
    generators: &[usize],
    sampling: &mut [super::sampling::SamplingEvent],
) -> Result<WaveLogic, String> {
    let handoff = owned_handoff.is_some();
    let actor_limit = if handoff {
        crate::redpiler::analysis::AnalysisLimits::for_budget(monitor.budget_multiplier())
            .max_pistons
    } else {
        1024 * monitor.budget_multiplier()
    };
    if report.pistons.len() > actor_limit {
        return Err(format!("instant actor budget exceeded ({actor_limit})"));
    }
    let mut extractor = Extractor {
        world,
        report,
        monitor,
        arena: BooleanArena::with_budget(monitor.budget_multiplier()),
        far: Default::default(),
        near: Default::default(),
        bases: Default::default(),
        group_of: vec![0; report.pistons.len()],
        payloads: Vec::new(),
        shapes: Default::default(),
        wires: Default::default(),
        sources: Default::default(),
        steps: 0,
        signal_order: Default::default(),
        output_mode: false,
        terms: Vec::new(),
        context: Default::default(),
        memory,
        sequential: false,
        wire_signals: false,
        ideal: handoff,
        observers: Default::default(),
        // The standalone first-response oracle historically excludes reset
        // pulses; executable extraction supplies its certified ownership set.
        owned_reset: owned_reset
            .cloned()
            .unwrap_or_else(|| report.observers.iter().copied().collect()),
    };
    for (group, descriptor) in report.payload_groups.iter().enumerate() {
        let payloads: Vec<_> = descriptor
            .positions
            .iter()
            .filter_map(|&pos| {
                let block = world.get_block(pos);
                supported_payload(block).then_some(block)
            })
            .collect();
        let empty_clock = descriptor.members.len() == 1
            && (Some(descriptor.members[0]) == clock
                || generators.contains(&descriptor.members[0]))
            && payloads.is_empty();
        if payloads.len() != 1 && !empty_clock {
            return Err(format!(
                "payload group {group} needs exactly one supported payload"
            ));
        }
        let payload = payloads.first().copied().unwrap_or(Block::Air);
        extractor.payloads.push(payload);
        for &member in &descriptor.members {
            let p = &report.pistons[member];
            if (!p.piston.extended && !handoff)
                || (!p.piston.sticky && Some(member) != clock && !generators.contains(&member))
                || (!handoff && world.get_block(p.payload) != payload)
            {
                return Err(format!(
                    "piston at {:?} is not a ready single-payload mechanism",
                    p.pos
                ));
            }
            if !empty_clock {
                let far = if handoff {
                    p.head.offset(p.piston.facing.into())
                } else {
                    p.payload
                };
                extractor.far.insert(far, group);
            }
            extractor.near.insert(p.head, member);
            extractor.bases.insert(p.pos, member);
            extractor.group_of[member] = group;
        }
    }
    let mut responses = Vec::with_capacity(report.pistons.len());
    for (actor, piston) in report.pistons.iter().enumerate() {
        let power = extractor.piston_power(actor, piston)?;
        responses.push(extractor.arena.not(power));
    }
    let mut waiting = vec![0; responses.len()];
    let entry = vec![Some(FALSE); responses.len()];
    let follows_payload: Vec<_> = responses
        .iter()
        .map(|&root| extractor.arena.substitute(root, &entry) == FALSE)
        .collect();
    let mut fanout = vec![Vec::new(); responses.len()];
    for (actor, &response) in responses.iter().enumerate() {
        let dependencies = extractor.arena.actuator_dependencies(response);
        waiting[actor] = dependencies.len();
        for dependency in dependencies {
            fanout[dependency].push(actor);
        }
    }
    let mut ready: BinaryHeap<_> = waiting
        .iter()
        .enumerate()
        .filter_map(|(id, &count)| (count == 0).then_some(Reverse(id)))
        .collect();
    let mut resolved = vec![None; responses.len()];
    let mut response_order = Vec::with_capacity(responses.len());
    while let Some(Reverse(actor)) = ready.pop() {
        // Logical execution follows DAG edges instead of expanding every
        // dependency into a global BDD, whose width can grow exponentially.
        let root = if handoff {
            responses[actor]
        } else {
            extractor.arena.substitute(responses[actor], &resolved)
        };
        resolved[actor] = Some(root);
        response_order.push(actor);
        for &target in &fanout[actor] {
            waiting[target] -= 1;
            if waiting[target] == 0 {
                ready.push(Reverse(target));
            }
        }
        extractor.check()?;
    }
    if resolved.iter().any(Option::is_none) {
        let unresolved: Vec<_> = resolved
            .iter()
            .enumerate()
            .filter_map(|(id, value)| value.is_none().then_some(report.pistons[id].pos))
            .collect();
        return Err(format!(
            "power dependency cycle at {unresolved:?}; feedback needs a sampled memory or clock boundary"
        ));
    }
    let mut blocks = Vec::new();
    let output_neighborhoods: Vec<_> = report
        .pistons
        .iter()
        .map(|p| {
            let margin = BlockPos::new(17, 17, 17);
            (p.pos.min(p.payload) - margin, p.pos.max(p.payload) + margin)
        })
        .collect();
    crate::world::for_each_block_optimized(world, report.bounds.0, report.bounds.1, |pos| {
        let block = world.get_block(pos);
        if super::super::analysis::ports::is_consumer(block)
            && output_neighborhoods.iter().any(|&(lo, hi)| {
                pos.x >= lo.x
                    && pos.y >= lo.y
                    && pos.z >= lo.z
                    && pos.x <= hi.x
                    && pos.y <= hi.y
                    && pos.z <= hi.z
            })
        {
            blocks.push((pos, block));
        }
    });
    let wires = extractor.wires.clone();
    #[cfg(test)]
    let mut response_sources: Vec<_> = extractor.sources.iter().copied().collect();
    #[cfg(test)]
    response_sources.sort_by_key(|pos| (pos.y, pos.z, pos.x));
    let mut consumer_wires = FxHashSet::default();
    let mut outputs = Vec::new();
    extractor.output_mode = true;
    for (pos, block) in blocks {
        if let Block::RedstoneComparator { comparator } = block {
            let rear = pos.offset(comparator.facing.block_face());
            let far = rear.offset(comparator.facing.block_face());
            if (extractor.far.contains_key(&rear)
                || extractor.near.contains_key(&rear)
                || extractor.bases.contains_key(&rear))
                && redstone::comparator::has_override(world.get_block(far))
            {
                return Err(format!("comparator at {pos:?} reads an analog override through moving blocks; this is unsupported"));
            }
        }
        for input in [
            crate::redpiler::analysis::ports::ConsumerInput::Main,
            crate::redpiler::analysis::ports::ConsumerInput::ComparatorSide,
        ] {
            extractor.wires.clear();
            extractor.terms.clear();
            let mut power = FALSE;
            let mut queue = VecDeque::new();
            for (root, face, channel) in
                crate::redpiler::analysis::ports::consumer_roots(block, pos)
            {
                if channel == input {
                    extractor.consumer_signal(block, input, root, face, &mut power, &mut queue)?;
                }
            }
            extractor.walk_wires(usize::MAX, power, queue)?;
            if !extractor.terms.iter().any(|term| term.guard > TRUE) {
                continue;
            }
            // Comparator overrides need their own occupancy-dependent read
            // protocol. Do not silently replace an inventory read with power.
            if input == crate::redpiler::analysis::ports::ConsumerInput::Main
                && matches!(block, Block::RedstoneComparator { .. })
            {
                let Block::RedstoneComparator { comparator } = block else {
                    unreachable!()
                };
                let rear = pos.offset(comparator.facing.block_face());
                if crate::redstone::comparator::has_override(world.get_block(rear))
                    || crate::redstone::comparator::get_far_input(world, pos, comparator.facing)
                        .is_some()
                {
                    return Err(format!("comparator at {pos:?} reads an analog override through moving blocks; this is unsupported"));
                }
            }
            consumer_wires.extend(extractor.wires.iter().copied());
            let mut output = OutputPort {
                consumer: pos,
                input,
                terms: std::mem::take(&mut extractor.terms),
                initial_strength: 0,
            };
            output.initial_strength = output.entry_strength(&extractor.arena, |source| {
                redstone::source_strength(world.get_block(source), world, source)
            });
            extractor
                .sources
                .extend(output.terms.iter().filter_map(|term| term.source));
            outputs.push(output);
        }
    }
    let mut sampling_wires = FxHashSet::default();
    for event in sampling.iter_mut() {
        let super::sampling::SamplingSource::Power {
            writer, pos, terms, ..
        } = &mut event.source
        else {
            continue;
        };
        extractor.terms.clear();
        extractor.wires.clear();
        extractor.walk_wires(usize::MAX, FALSE, VecDeque::from([(*pos, 0, TRUE)]))?;
        sampling_wires.extend(extractor.wires.iter().copied());
        *terms = std::mem::take(&mut extractor.terms);
        if terms
            .iter()
            .any(|term| term.source.is_some_and(|source| source != *writer))
            || super::sequential::dependencies(
                &extractor.arena,
                terms.iter().map(|term| term.guard),
            )
            .iter()
            .any(|variable| !matches!(variable, Variable::Signal { .. }))
        {
            return Err(format!("sampling dust at {pos:?} depends on moving blocks or another power source; expected one fixed source"));
        }
        extractor.sources.insert(*writer);
        extractor
            .sources
            .extend(terms.iter().filter_map(|term| term.source));
    }
    let mut handoff_wires = Vec::new();
    if handoff {
        let mut internals =
            crate::redpiler::analysis::families::reset_internals(&report.recognition);
        internals.extend(owned_handoff.unwrap().iter().copied());
        let mut positions: Vec<_> = wires
            .iter()
            .chain(&consumer_wires)
            .chain(&sampling_wires)
            .chain(&internals)
            .copied()
            .filter(|&pos| matches!(world.get_block(pos), Block::RedstoneWire { .. }))
            .collect();
        positions.sort_by_key(|pos| (pos.y, pos.z, pos.x));
        positions.dedup();
        for pos in positions {
            extractor.terms.clear();
            extractor.walk_wires(usize::MAX, FALSE, VecDeque::from([(pos, 0, TRUE)]))?;
            let mut terms = std::mem::take(&mut extractor.terms);
            for term in &mut terms {
                if let Some(source) = term.source.filter(|source| {
                    internals.contains(source) && !extractor.sources.contains(source)
                }) {
                    // Ideal execution does not run the reset protocol. Its
                    // internal sources retain their saved presentation only
                    // for handoff; this never specializes a live response.
                    let strength =
                        redstone::source_strength(world.get_block(source), world, source).min(15);
                    term.source = None;
                    term.attenuation = term.attenuation.saturating_add(15 - strength);
                }
            }
            terms.retain(|term| term.attenuation < 15 && term.guard != FALSE);
            extractor
                .sources
                .extend(terms.iter().filter_map(|term| term.source));
            handoff_wires.push((pos, terms));
        }
    }
    extractor.wires = wires;
    extractor.check()?;
    let mut sources: Vec<_> = extractor.sources.into_iter().collect();
    sources.sort_by_key(|pos| (pos.y, pos.z, pos.x));
    let mut responses: Vec<_> = resolved.into_iter().map(Option::unwrap).collect();
    let response_count = responses.len();
    responses.extend(
        outputs
            .iter()
            .flat_map(|output| output.terms.iter().map(|term| term.guard)),
    );
    responses.extend(
        handoff_wires
            .iter()
            .flat_map(|(_, terms)| terms.iter().map(|term| term.guard)),
    );
    responses.extend(
        sampling
            .iter()
            .flat_map(|event| match &event.source {
                super::sampling::SamplingSource::Power { terms, .. } => terms.as_slice(),
                super::sampling::SamplingSource::Generator(_) => &[],
            })
            .map(|term| term.guard),
    );
    let arena = extractor.arena.compact(&mut responses);
    let mut guards = responses[response_count..].iter().copied();
    for output in &mut outputs {
        for term in &mut output.terms {
            term.guard = guards.next().unwrap();
        }
    }
    for (_, terms) in &mut handoff_wires {
        for term in terms {
            term.guard = guards.next().unwrap();
        }
    }
    for event in sampling {
        if let super::sampling::SamplingSource::Power { terms, .. } = &mut event.source {
            for term in terms {
                term.guard = guards.next().unwrap();
            }
        }
    }
    responses.truncate(response_count);
    Ok(WaveLogic {
        arena,
        responses,
        response_order,
        sources,
        #[cfg(test)]
        response_sources,
        wires: extractor.wires,
        consumer_wires,
        outputs,
        handoff_wires,
        follows_payload,
        context: extractor.context,
    })
}

impl<W: World> Extractor<'_, W> {
    fn own_group(&self, actor: usize) -> usize {
        self.group_of.get(actor).copied().unwrap_or(usize::MAX)
    }
    fn check(&mut self) -> Result<(), String> {
        if self.monitor.cancelled() {
            return Err("instant extraction cancelled".into());
        }
        self.steps += 1;
        if self.steps
            > (if self.sequential {
                33_554_432
            } else {
                8_388_608
            }) * self.monitor.budget_multiplier()
            || self.arena.exhausted
            || self.signal_order.len()
                > (if self.sequential || self.ideal {
                    65_536
                } else {
                    64
                }) * self.monitor.budget_multiplier()
        {
            return Err(format!(
                "instant conditional geometry budget exceeded ({} steps, {} sources, {} decisions)",
                self.steps,
                self.signal_order.len(),
                self.arena.nodes.len()
            ));
        }
        Ok(())
    }

    fn read(&mut self, pos: BlockPos) -> Result<Block, String> {
        self.check()?;
        let (lo, hi) = self.report.bounds;
        if pos.x < lo.x
            || pos.y < lo.y
            || pos.z < lo.z
            || pos.x > hi.x
            || pos.y > hi.y
            || pos.z > hi.z
        {
            let width = crate::plot::PLOT_BLOCK_WIDTH;
            if self.sequential
                && lo.x.rem_euclid(width) == 0
                && lo.z.rem_euclid(width) == 0
                && hi.x - lo.x + 1 == width
                && hi.z - lo.z + 1 == width
                && lo.y == 0
                && hi.y + 1 == crate::plot::PLOT_BLOCK_HEIGHT
            {
                // Full plots are isolated electrical worlds, as in ordinary
                // graph search. A smaller selection still needs its context.
                return Ok(Block::Air);
            }
            return Err(format!(
                "instant context at {pos:?} is outside the selection"
            ));
        }
        self.context.insert(pos);
        Ok(self.world.get_block(pos))
    }

    fn actors_at(&self, pos: BlockPos, actor: usize) -> Vec<usize> {
        let group = self
            .far
            .get(&pos)
            .copied()
            .or_else(|| self.near.get(&pos).map(|&id| self.group_of[id]));
        if let Some(group) = group {
            if group == self.own_group(actor) {
                return Vec::new();
            }
            return self.report.payload_groups[group].members.clone();
        }
        self.bases
            .get(&pos)
            .copied()
            .filter(|&id| self.group_of[id] != self.own_group(actor))
            .into_iter()
            .collect()
    }

    fn assigned(&self, pos: BlockPos, actor: usize, actors: &[usize], bits: usize) -> Block {
        let fired = |id| {
            actors
                .iter()
                .position(|&a| a == id)
                .is_some_and(|bit| bits & (1 << bit) != 0)
        };
        if let Some(&group) = self.far.get(&pos) {
            if group != self.own_group(actor)
                && self.report.payload_groups[group]
                    .members
                    .iter()
                    .any(|&id| fired(id))
            {
                return Block::Air;
            }
            if self.ideal {
                return self.payloads[group];
            }
        }
        if let Some(&owner) = self.near.get(&pos) {
            let group = self.group_of[owner];
            if group != self.own_group(actor) && fired(owner) {
                if self.memory.contains(&owner) {
                    return self.payloads[group];
                }
                // Retraction carries the block in a moving-piston entity.
                // It provides neither conduction nor redstone power during
                // the response wave; settled near occupancy belongs to reset.
                return Block::Air;
            }
            if self.ideal {
                return Block::PistonHead {
                    head: self.report.pistons[owner].piston.extend(true).into(),
                };
            }
        }
        if let Some(&owner) = self.bases.get(&pos) {
            if self.group_of[owner] != self.own_group(actor) && fired(owner) {
                let mut piston = self.report.pistons[owner].piston;
                piston.extended = false;
                return Block::Piston { piston };
            }
            if self.ideal {
                return Block::Piston {
                    piston: self.report.pistons[owner].piston.extend(true),
                };
            }
        }
        self.world.get_block(pos)
    }

    fn assignment_guard(&mut self, actors: &[usize], bits: usize) -> Expr {
        let mut guard = TRUE;
        for (bit, &actor) in actors.iter().enumerate() {
            let variable = if self.memory.contains(&actor) {
                Variable::Memory(actor)
            } else {
                Variable::Actuator(actor)
            };
            let mut variable = self.arena.variable(variable);
            if bits & (1 << bit) == 0 {
                variable = self.arena.not(variable);
            }
            guard = self.arena.and(guard, variable);
        }
        guard
    }

    fn variants(&mut self, pos: BlockPos, actor: usize) -> Result<Vec<(Block, Expr)>, String> {
        self.read(pos)?;
        if self.output_mode {
            return Ok(self.output_variants(pos));
        }
        let actors = self.actors_at(pos, actor);
        if actors.len() > 8 {
            return Err(format!("too many payload owners at {pos:?}"));
        }
        let mut variants: Vec<(Block, Expr)> = Vec::new();
        for bits in 0..(1 << actors.len()) {
            let block = self.assigned(pos, actor, &actors, bits);
            let guard = self.assignment_guard(&actors, bits);
            if let Some((_, condition)) =
                variants.iter_mut().find(|(existing, _)| *existing == block)
            {
                *condition = self.arena.or(*condition, guard);
            } else {
                variants.push((block, guard));
            }
        }
        Ok(variants)
    }

    fn geometry(&mut self, actor: usize, part: GeometryPart) -> Expr {
        self.arena.variable(Variable::Geometry { actor, part })
    }

    fn output_variants(&mut self, pos: BlockPos) -> Vec<(Block, Expr)> {
        if let Some(&group) = self.far.get(&pos) {
            let owner = self.report.payload_groups[group].members[0];
            let present = self.geometry(owner, GeometryPart::FarPayload);
            let absent = self.arena.not(present);
            return vec![(self.payloads[group], present), (Block::Air, absent)];
        }
        if let Some(&owner) = self.near.get(&pos) {
            let near = self.geometry(owner, GeometryPart::NearPayload);
            let head = self.geometry(owner, GeometryPart::Head);
            let occupied = self.arena.or(near, head);
            let empty = self.arena.not(occupied);
            return vec![
                (self.payloads[self.group_of[owner]], near),
                (
                    if self.sequential || self.ideal {
                        Block::PistonHead {
                            head: self.report.pistons[owner].piston.extend(true).into(),
                        }
                    } else {
                        self.world.get_block(pos)
                    },
                    head,
                ),
                (Block::Air, empty),
            ];
        }
        if let Some(&owner) = self.bases.get(&pos) {
            let retracted = self.geometry(owner, GeometryPart::RetractedBase);
            let moving = self.geometry(owner, GeometryPart::MovingBase);
            let changing = self.arena.or(retracted, moving);
            let extended = self.arena.not(changing);
            let mut piston = self.report.pistons[owner].piston;
            piston.extended = false;
            return vec![
                (Block::Piston { piston }, retracted),
                (
                    Block::MovingPiston {
                        moving: piston.into(),
                    },
                    moving,
                ),
                (
                    if self.sequential || self.ideal {
                        Block::Piston {
                            piston: self.report.pistons[owner].piston.extend(true),
                        }
                    } else {
                        self.world.get_block(pos)
                    },
                    extended,
                ),
            ];
        }
        vec![(self.world.get_block(pos), TRUE)]
    }

    fn output_wire_shapes(
        &mut self,
        pos: BlockPos,
        wire: RedstoneWire,
        positions: &[BlockPos],
        raw: bool,
    ) -> Result<Vec<(Expr, RedstoneWire)>, String> {
        let mut assignments = vec![(FxHashMap::default(), TRUE)];
        for &position in positions {
            let variants = self.variants(position, usize::MAX)?;
            // Fixed neighbors are read by the shared shape calculator. Only
            // conditional cells need to be copied into each configuration.
            if variants.len() == 1 && variants[0].1 == TRUE {
                continue;
            }
            let mut next = Vec::new();
            for (blocks, guard) in assignments {
                for &(block, variant) in &variants {
                    let guard = self.arena.and(guard, variant);
                    if guard == FALSE {
                        continue;
                    }
                    let mut blocks = blocks.clone();
                    blocks.insert(position, block);
                    next.push((blocks, guard));
                    if next.len() > 4096 {
                        return Err(format!(
                            "output wire at {pos:?} needs too many occupancy configurations"
                        ));
                    }
                }
                self.check()?;
            }
            assignments = next;
        }
        let mut shapes: Vec<(Expr, RedstoneWire)> = Vec::new();
        for (blocks, guard) in assignments {
            let read = |position| {
                blocks
                    .get(&position)
                    .copied()
                    .unwrap_or_else(|| self.world.get_block(position))
            };
            let shape = if raw {
                redstone::wire::get_raw_sides_from(wire, pos, read)
            } else {
                redstone::wire::get_regulated_sides_from(wire, pos, read)
            };
            if let Some((known, _)) = shapes.iter_mut().find(|(_, existing)| *existing == shape) {
                *known = self.arena.or(*known, guard);
            } else {
                shapes.push((guard, shape));
            }
        }
        Ok(shapes)
    }

    fn wire_shapes(
        &mut self,
        pos: BlockPos,
        actor: usize,
        wire: RedstoneWire,
    ) -> Result<Vec<(Expr, RedstoneWire)>, String> {
        if let Some(shapes) = self.shapes.get(&(actor, pos)) {
            return Ok(shapes.clone());
        }
        let mut positions = vec![pos.offset(BlockFace::Top)];
        for side in BlockFace::values()
            .into_iter()
            .filter(|face| face.is_horizontal())
        {
            let neighbor = pos.offset(side);
            positions.extend([
                neighbor,
                neighbor.offset(BlockFace::Top),
                neighbor.offset(BlockFace::Bottom),
            ]);
        }
        if self.output_mode {
            let shapes = if self.sequential {
                let raw_shapes = self.output_wire_shapes(pos, wire, &positions, true)?;
                let mut shapes: Vec<(Expr, RedstoneWire)> = Vec::new();
                for (guard, raw) in raw_shapes {
                    let dot = RedstoneWire {
                        power: wire.power,
                        ..Default::default()
                    };
                    let connected = RedstoneWire {
                        north: mchprs_blocks::blocks::RedstoneWireSide::Side,
                        ..dot
                    };
                    let dot_shape = redstone::wire::regulate_sides(dot, raw);
                    let connected_shape = redstone::wire::regulate_sides(connected, raw);
                    let candidates = if dot_shape == connected_shape {
                        vec![(guard, dot_shape)]
                    } else {
                        let dot = self.arena.variable(Variable::WireDot(pos));
                        let not_dot = self.arena.not(dot);
                        vec![
                            (self.arena.and(guard, dot), dot_shape),
                            (self.arena.and(guard, not_dot), connected_shape),
                        ]
                    };
                    for (guard, shape) in candidates {
                        if let Some((known, _)) =
                            shapes.iter_mut().find(|(_, existing)| *existing == shape)
                        {
                            *known = self.arena.or(*known, guard);
                        } else {
                            shapes.push((guard, shape));
                        }
                    }
                }
                shapes
            } else {
                self.output_wire_shapes(pos, wire, &positions, false)?
            };
            self.shapes.insert((actor, pos), shapes.clone());
            return Ok(shapes);
        }
        let mut actors: Vec<_> = positions
            .iter()
            .flat_map(|&pos| self.actors_at(pos, actor))
            .collect();
        actors.sort_unstable();
        actors.dedup();
        if actors.len() > 8 {
            return Err(format!(
                "wire at {pos:?} needs too many occupancy configurations"
            ));
        }
        for position in positions {
            self.read(position)?;
        }
        let mut shapes: Vec<(Expr, RedstoneWire)> = Vec::new();
        for bits in 0..(1 << actors.len()) {
            let shape = redstone::wire::get_regulated_sides_from(wire, pos, |p| {
                self.assigned(p, actor, &actors, bits)
            });
            let guard = self.assignment_guard(&actors, bits);
            if let Some((condition, _)) = shapes.iter_mut().find(|(_, existing)| *existing == shape)
            {
                *condition = self.arena.or(*condition, guard);
            } else {
                shapes.push((guard, shape));
            }
        }
        self.shapes.insert((actor, pos), shapes.clone());
        Ok(shapes)
    }

    fn source(
        &mut self,
        actor: usize,
        pos: BlockPos,
        block: Block,
        distance: u8,
        guard: Expr,
        result: &mut Expr,
    ) {
        if guard == FALSE
            || distance >= 15
            || (!self.sequential
                && self.owned_reset.contains(&pos)
                && matches!(block, Block::Observer { .. }))
        {
            return;
        }
        if self.sequential && matches!(block, Block::RedstoneWire { .. }) {
            self.wires.insert(pos);
        }
        if self.output_mode {
            let (source, guard) = if self.sequential && matches!(block, Block::Observer { .. }) {
                let Some(&observer) = self.observers.get(&pos) else {
                    return;
                };
                let powered = self.arena.variable(Variable::Observer(observer));
                (None, self.arena.and(guard, powered))
            } else {
                ((block != Block::RedstoneBlock).then_some(pos), guard)
            };
            if let Some(term) = self
                .terms
                .iter_mut()
                .find(|term| term.source == source && term.attenuation == distance)
            {
                term.guard = self.arena.or(term.guard, guard);
            } else {
                self.terms.push(PowerTerm {
                    guard,
                    source,
                    attenuation: distance,
                });
            }
            return;
        }
        if self
            .far
            .get(&pos)
            .copied()
            .or_else(|| self.near.get(&pos).map(|&id| self.group_of[id]))
            == Some(self.own_group(actor))
        {
            return;
        }
        let value = if block == Block::RedstoneBlock {
            TRUE
        } else {
            self.sources.insert(pos);
            let next = self.signal_order.len();
            let order = *self.signal_order.entry(pos).or_insert(next);
            self.arena.variable(Variable::Signal {
                pos,
                threshold: distance,
                order,
            })
        };
        let value = self.arena.and(guard, value);
        *result = self.arena.or(*result, value);
    }

    fn wire_roots(
        &mut self,
        actor: usize,
        pos: BlockPos,
        wire: RedstoneWire,
        side: BlockFace,
        guard: Expr,
        roots: &mut VecDeque<(BlockPos, u8, Expr)>,
    ) -> Result<(), String> {
        for (shape_guard, shape) in self.wire_shapes(pos, actor, wire)? {
            let reaches = match side {
                BlockFace::Top => true,
                BlockFace::Bottom => false,
                _ => !redstone::wire::get_current_side(shape, side.unwrap_direction().opposite())
                    .is_none(),
            };
            if reaches {
                let condition = self.arena.and(guard, shape_guard);
                if self.wire_signals {
                    let mut ignored = FALSE;
                    self.source(
                        actor,
                        pos,
                        Block::RedstoneWire { wire },
                        0,
                        condition,
                        &mut ignored,
                    );
                } else {
                    roots.push_back((pos, 0, condition));
                }
            }
        }
        Ok(())
    }

    fn signal(
        &mut self,
        actor: usize,
        pos: BlockPos,
        side: BlockFace,
        result: &mut Expr,
        roots: &mut VecDeque<(BlockPos, u8, Expr)>,
    ) -> Result<(), String> {
        for (block, guard) in self.variants(pos, actor)? {
            if block.is_solid() {
                for face in BlockFace::values() {
                    let neighbor = pos.offset(face);
                    for (source, source_guard) in self.variants(neighbor, actor)? {
                        let guard = self.arena.and(guard, source_guard);
                        if let Block::RedstoneWire { wire } = source {
                            self.wire_roots(actor, neighbor, wire, face, guard, roots)?;
                        } else if power::emits_strong_power(
                            source, self.world, neighbor, face, false,
                        ) {
                            self.source(actor, neighbor, source, 0, guard, result);
                        }
                    }
                }
            } else if let Block::RedstoneWire { wire } = block {
                self.wire_roots(actor, pos, wire, side, guard, roots)?;
            } else if power::emits_weak_power(block, self.world, pos, side, false) {
                self.source(actor, pos, block, 0, guard, result);
            }
        }
        Ok(())
    }

    /// Diodes read the strength of adjacent dust irrespective of its side
    /// shape. Comparator side inputs accept dust, diodes and redstone blocks;
    /// a strongly powered ordinary conductor is not a side input.
    fn consumer_signal(
        &mut self,
        consumer: Block,
        input: crate::redpiler::analysis::ports::ConsumerInput,
        pos: BlockPos,
        side: BlockFace,
        result: &mut Expr,
        roots: &mut VecDeque<(BlockPos, u8, Expr)>,
    ) -> Result<(), String> {
        use crate::redpiler::analysis::ports::ConsumerInput;
        // The ordinary comparator reads a fixed rear inventory directly.
        // Its main channel must not become a conditional electrical port,
        // even if moving geometry or an observer powers that same support.
        if matches!(consumer, Block::RedstoneComparator { .. })
            && input == ConsumerInput::Main
            && !self.far.contains_key(&pos)
            && !self.near.contains_key(&pos)
            && !self.bases.contains_key(&pos)
            && redstone::comparator::has_override(self.read(pos)?)
        {
            return Ok(());
        }
        if input == ConsumerInput::ComparatorSide {
            for (block, guard) in self.variants(pos, usize::MAX)? {
                if matches!(block, Block::RedstoneWire { .. }) {
                    if self.wire_signals {
                        self.source(usize::MAX, pos, block, 0, guard, result);
                    } else {
                        roots.push_back((pos, 0, guard));
                    }
                } else if block == Block::RedstoneBlock
                    || (redstone::is_diode(block)
                        && power::emits_weak_power(block, self.world, pos, side, false))
                {
                    self.source(usize::MAX, pos, block, 0, guard, result);
                }
            }
        } else if redstone::is_diode(consumer)
            && matches!(self.read(pos)?, Block::RedstoneWire { .. })
        {
            if self.wire_signals {
                self.source(usize::MAX, pos, self.world.get_block(pos), 0, TRUE, result);
            } else {
                roots.push_back((pos, 0, TRUE));
            }
        } else {
            self.signal(usize::MAX, pos, side, result, roots)?;
        }
        Ok(())
    }

    fn piston_power(&mut self, actor: usize, p: &PistonDescriptor) -> Result<Expr, String> {
        let mut result = FALSE;
        let mut queue = VecDeque::new();
        for side in BlockFace::values() {
            if side != BlockFace::from(p.piston.facing) {
                self.signal(actor, p.pos.offset(side), side, &mut result, &mut queue)?;
            }
            self.signal(
                actor,
                p.pos.offset(BlockFace::Top).offset(side),
                side,
                &mut result,
                &mut queue,
            )?;
        }
        self.walk_wires(actor, result, queue)
    }

    fn terms_power(&mut self, terms: &[PowerTerm]) -> Expr {
        let mut power = FALSE;
        for term in terms {
            let source = if let Some(pos) = term.source {
                self.sources.insert(pos);
                let next = self.signal_order.len();
                let order = *self.signal_order.entry(pos).or_insert(next);
                self.arena.variable(Variable::Signal {
                    pos,
                    threshold: term.attenuation,
                    order,
                })
            } else {
                TRUE
            };
            let powered = self.arena.and(term.guard, source);
            power = self.arena.or(power, powered);
        }
        power
    }

    fn walk_wires(
        &mut self,
        actor: usize,
        mut result: Expr,
        mut queue: VecDeque<(BlockPos, u8, Expr)>,
    ) -> Result<Expr, String> {
        let mut visited: FxHashMap<BlockPos, Expr> = FxHashMap::default();
        while let Some((pos, distance, guard)) = queue.pop_front() {
            if distance >= 15 || guard == FALSE || result == TRUE {
                continue;
            }
            let known = visited.get(&pos).copied().unwrap_or(FALSE);
            let unknown = self.arena.not(known);
            let guard = self.arena.and(guard, unknown);
            if guard == FALSE {
                continue;
            }
            let covered = self.arena.or(known, guard);
            visited.insert(pos, covered);
            self.wires.insert(pos);
            let above = self.variants(pos.offset(BlockFace::Top), actor)?;
            for side in BlockFace::values() {
                let neighbor = pos.offset(side);
                for (block, condition) in self.variants(neighbor, actor)? {
                    let guard = self.arena.and(guard, condition);
                    if guard == FALSE {
                        continue;
                    }
                    if block.is_solid() {
                        for face in BlockFace::values() {
                            let source_pos = neighbor.offset(face);
                            for (source, source_guard) in self.variants(source_pos, actor)? {
                                if power::emits_strong_power(
                                    source, self.world, source_pos, face, false,
                                ) {
                                    let guard = self.arena.and(guard, source_guard);
                                    self.source(
                                        actor,
                                        source_pos,
                                        source,
                                        distance,
                                        guard,
                                        &mut result,
                                    );
                                }
                            }
                        }
                    } else if power::emits_weak_power(block, self.world, neighbor, side, false) {
                        self.source(actor, neighbor, block, distance, guard, &mut result);
                    }
                    if matches!(block, Block::RedstoneWire { .. }) {
                        if self.wire_signals {
                            self.source(actor, neighbor, block, distance + 1, guard, &mut result);
                        } else {
                            queue.push_back((neighbor, distance + 1, guard));
                        }
                    }
                    if side.is_horizontal() {
                        if !block.is_transparent()
                            && matches!(
                                self.read(neighbor.offset(BlockFace::Top))?,
                                Block::RedstoneWire { .. }
                            )
                        {
                            for &(block, above_guard) in &above {
                                if !block.is_solid() {
                                    let guard = self.arena.and(guard, above_guard);
                                    let wire_pos = neighbor.offset(BlockFace::Top);
                                    if self.wire_signals {
                                        self.source(
                                            actor,
                                            wire_pos,
                                            self.world.get_block(wire_pos),
                                            distance + 1,
                                            guard,
                                            &mut result,
                                        );
                                    } else {
                                        queue.push_back((wire_pos, distance + 1, guard));
                                    }
                                }
                            }
                        }
                        if !block.is_solid()
                            && matches!(
                                self.read(neighbor.offset(BlockFace::Bottom))?,
                                Block::RedstoneWire { .. }
                            )
                        {
                            let wire_pos = neighbor.offset(BlockFace::Bottom);
                            if self.wire_signals {
                                self.source(
                                    actor,
                                    wire_pos,
                                    self.world.get_block(wire_pos),
                                    distance + 1,
                                    guard,
                                    &mut result,
                                );
                            } else {
                                queue.push_back((wire_pos, distance + 1, guard));
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}
