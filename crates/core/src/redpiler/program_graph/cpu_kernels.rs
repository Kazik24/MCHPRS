//! Flat pure predicates for FIFO-local kernels; mutable values stay in CircuitState.
use super::*;
use crate::redpiler::{TaskMonitor, power_rules};

const MAX_READS: usize = 64;
const MAX_CLAUSES: usize = 64;
const MAX_WORK: usize = 16_384;
const MAX_GUARD_GLOBAL_WORK: usize = 8_000_000;
const MAX_GUARD_PLAN_BYTES: usize = 64 * 1024 * 1024;
const NO_GUARD_PREDICATE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CpuRead {
    /// Sample the original piston/resource state at this query's execution cut.
    State(StateTest),
    /// Read an existing strength owner, including active classic wire overlays.
    Strength { node: NodeId, threshold: u8 },
    /// A fixed wire owner with live strength, shape and dot/cross regulation.
    WireEmission { node: NodeId, face: BlockFace, threshold: u8 },
    /// Unsupported live geometry keeps its original pure guard operation.
    Guard(GuardId),
    /// Unsupported emission keeps the original pure source query, not a cached value.
    Source { site: SiteId, port: PowerPort, threshold: u8 },
    /// Budget/ownership fallback samples an original Main or Quasi input.
    Input { node: NodeId, port: InputPort },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CpuPredicateKind {
    #[default]
    Masks,
    Literal,
    All,
    /// One conjunction with positive and negative literals, evaluated in short-circuit order.
    AllLiterals,
    Any,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CpuPredicate {
    /// First contiguous read; read order is optimized only within a pure sampling cut.
    pub(crate) read_start: u32,
    /// First contiguous pair of clause masks in the two parallel arrays.
    pub(crate) clause_start: u32,
    /// At most 64 read results are held in one machine word.
    pub(crate) read_len: u16,
    /// Zero clauses means false; one empty clause means true.
    pub(crate) clause_len: u16,
    /// Derived from the masks once, keeping the common scalar forms out of bit packing.
    pub(crate) kind: CpuPredicateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CpuKernelEntry {
    /// Original actor, which still owns every action, state and future obligation.
    pub(crate) actor: PistonId,
    /// Main input is sampled first, preserving the original short circuit.
    pub(crate) main: CpuPredicate,
    /// Quasi input is sampled only when Main is false.
    pub(crate) quasi: CpuPredicate,
    /// Resource acceptance after the original ready/extended/unpowered checks.
    pub(crate) valid: CpuPredicate,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CpuKernelPlans {
    /// Sorted hot descriptors, with one dense actor binding installed at lowering.
    pub(crate) entries: Vec<CpuKernelEntry>,
    /// Contiguous typed reads; there are no per-entry operand allocations.
    pub(crate) reads: Vec<CpuRead>,
    /// Parallel clause masks allow four independent u64 comparisons per AVX2 operation.
    pub(crate) require_true: Vec<u64>,
    /// A clause also requires these sampled bits to be clear.
    pub(crate) require_false: Vec<u64>,
}

/// Live Main/Quasi piston power predicates. These never contain action or acceptance rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CpuPowerQueryEntry {
    pub(crate) actor: PistonId,
    pub(crate) main: CpuPredicate,
    pub(crate) quasi: CpuPredicate,
}

/// Independently compiled power queries for the O6 exact-query path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CpuPowerQueryPlans {
    pub(crate) entries: Vec<CpuPowerQueryEntry>,
    /// Shared mask evaluator; its event-entry list remains empty in this plan.
    pub(crate) packed: CpuKernelPlans,
}

impl CpuPowerQueryPlans {
    /// Compile bounded live predicates for every piston. Any unsupported root becomes an exact
    /// original-input read; exceeding the global budget leaves remaining actors on the fallback.
    pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        if monitor.cancelled() {
            return Err("cancelled before piston power-query compilation".into());
        }
        let mut incoming: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for &link in &graph.power_links {
            if monitor.cancelled() {
                return Err("cancelled while indexing piston power-query inputs".into());
            }
            incoming.entry(link.to).or_default().push(link);
        }
        let mut result = Self::default();
        let mut total_work = 0usize;
        let max_bytes = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8;
        let bud_readers: rustc_hash::FxHashSet<_> = graph.plans.bud_memory_reads.iter().map(|read| read.reader).collect();
        let classic_main_receivers: rustc_hash::FxHashSet<_> = graph
            .plans
            .classic_nets
            .iter()
            .flat_map(|net| net.inputs.iter().map(|input| input.receiver))
            .collect();

        for (index, piston) in graph.pistons.iter().enumerate() {
            if monitor.cancelled() {
                return Err("cancelled while compiling piston power queries".into());
            }
            if total_work >= 8_000_000 {
                break;
            }
            let actor = PistonId::new(index as u32);
            let node = piston.node;
            let has_classic_main = classic_main_receivers.contains(&node);
            let packed_checkpoint = (result.packed.reads.len(), result.packed.require_true.len());
            let result_checkpoint = result.entries.len();
            let make = |work_limit| Builder {
                graph,
                incoming: &incoming,
                monitor,
                reads: Vec::new(),
                ids: FxHashMap::default(),
                guards: FxHashMap::default(),
                work: 0,
                work_limit,
                captured_site_reads: false,
            };
            let mut roots = [CpuPredicate::default(); 2];
            for (slot, port) in [InputPort::Main, InputPort::Quasi].into_iter().enumerate() {
                let force_input = (port == InputPort::Main && has_classic_main)
                    || graph.plans.fixed_classic.program.is_some()
                    || (port == InputPort::Quasi && bud_readers.contains(&actor));
                let remaining = 8_000_000usize.saturating_sub(total_work);
                let mut builder = make(MAX_WORK.min(remaining));
                let candidate = if force_input {
                    Err(())
                } else {
                    builder.query(PowerTarget::Input { node, port }, 0, 0)
                };
                total_work = total_work.saturating_add(builder.work);
                let (reads, clauses) = match candidate {
                    Ok(clauses) => (builder.reads, clauses),
                    Err(()) => {
                        let remaining = 8_000_000usize.saturating_sub(total_work);
                        if remaining == 0 {
                            result.packed.reads.truncate(packed_checkpoint.0);
                            result.packed.require_true.truncate(packed_checkpoint.1);
                            result.packed.require_false.truncate(packed_checkpoint.1);
                            result.entries.truncate(result_checkpoint);
                            return Ok(result);
                        }
                        let mut fallback = make(MAX_WORK.min(remaining));
                        let clauses = fallback
                            .atom(CpuRead::Input { node, port }, false)
                            .map_err(|_| "failed to build exact piston-input fallback")?;
                        total_work = total_work.saturating_add(fallback.work);
                        (fallback.reads, clauses)
                    }
                };
                roots[slot] = result.packed.append(reads, clauses)?;
                let bytes = result.packed.reads.len() * std::mem::size_of::<CpuRead>()
                    + result.packed.require_true.len() * 16
                    + (result.entries.len() + 1) * std::mem::size_of::<CpuPowerQueryEntry>();
                if bytes > max_bytes || total_work > 8_000_000 {
                    result.packed.reads.truncate(packed_checkpoint.0);
                    result.packed.require_true.truncate(packed_checkpoint.1);
                    result.packed.require_false.truncate(packed_checkpoint.1);
                    result.entries.truncate(result_checkpoint);
                    return Ok(result);
                }
            }
            result.entries.push(CpuPowerQueryEntry {
                actor,
                main: roots[0],
                quasi: roots[1],
            });
        }
        Ok(result)
    }

    pub(crate) fn entry(&self, actor: PistonId) -> Option<CpuPowerQueryEntry> {
        self.entries.get(actor.index()).copied().filter(|entry| entry.actor == actor)
    }

    pub(crate) fn input_fallback_entries(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                [entry.main, entry.quasi].iter().any(|predicate| {
                    let start = predicate.read_start as usize;
                    self.packed.reads[start..start + predicate.read_len as usize]
                        .iter()
                        .any(|read| matches!(read, CpuRead::Input { .. }))
                })
            })
            .count()
    }

    pub(crate) fn evaluate(&self, predicate: CpuPredicate, read: impl FnMut(CpuRead) -> bool, matches: ClauseMatcher) -> bool {
        self.packed.evaluate(predicate, read, matches)
    }
}

type Clause = (u64, u64);
type Dnf = Vec<Clause>;

struct Builder<'a> {
    graph: &'a CompileGraph,
    incoming: &'a FxHashMap<PowerTarget, Vec<PowerLink>>,
    monitor: &'a TaskMonitor,
    reads: Vec<CpuRead>,
    ids: FxHashMap<CpuRead, usize>,
    guards: FxHashMap<(GuardId, bool), Dnf>,
    work: usize,
    work_limit: usize,
    /// Event-kernel predicates have no capture context; standalone guards do.
    captured_site_reads: bool,
}

impl Builder<'_> {
    fn budget(&mut self, work: usize) -> Result<(), ()> {
        let next = self.work.saturating_add(work);
        if next > self.work_limit || self.monitor.cancelled() {
            Err(())
        } else {
            self.work = next;
            Ok(())
        }
    }

    fn atom(&mut self, read: CpuRead, negated: bool) -> Result<Dnf, ()> {
        let id = if let Some(&id) = self.ids.get(&read) {
            id
        } else {
            if self.reads.len() == MAX_READS {
                return Err(());
            }
            let id = self.reads.len();
            self.reads.push(read);
            self.ids.insert(read, id);
            id
        };
        Ok(vec![if negated { (0, 1u64 << id) } else { (1u64 << id, 0) }])
    }

    fn combine(&mut self, left: Dnf, right: Dnf, all: bool) -> Result<Dnf, ()> {
        self.budget(if all { left.len() * right.len() } else { left.len() + right.len() })?;
        let mut result = if all {
            left.iter()
                .flat_map(|&(lt, lf)| {
                    right.iter().filter_map(move |&(rt, rf)| {
                        let clause = (lt | rt, lf | rf);
                        (clause.0 & clause.1 == 0).then_some(clause)
                    })
                })
                .collect::<Vec<_>>()
        } else {
            left.into_iter().chain(right).collect::<Vec<_>>()
        };
        result.sort_unstable();
        result.dedup();
        if result.contains(&(0, 0)) {
            return Ok(vec![(0, 0)]);
        }
        if result.len() > MAX_CLAUSES {
            return Err(());
        }
        // Small predicates benefit from removing clauses already implied by a weaker one.
        if result.len() <= 16 {
            let source = result.clone();
            result.retain(|&(t, f)| !source.iter().any(|&(a, b)| (a, b) != (t, f) && a & t == a && b & f == b));
        }
        Ok(result)
    }

    fn guard(&mut self, id: GuardId, negated: bool, depth: usize) -> Result<Dnf, ()> {
        self.budget(1)?;
        if depth > 64 {
            return Err(());
        }
        if let Some(value) = self.guards.get(&(id, negated)) {
            return Ok(value.clone());
        }
        let result = match &self.graph.guards[id.index()] {
            Guard::Always => constant(!negated),
            Guard::State(test) => self.atom(CpuRead::State(*test), negated)?,
            Guard::Not(child) => self.guard(*child, !negated, depth + 1)?,
            Guard::All(children) | Guard::Any(children) => {
                let all = matches!(&self.graph.guards[id.index()], Guard::All(_)) ^ negated;
                let mut value = constant(all);
                for &child in children {
                    let next = self.guard(child, negated, depth + 1)?;
                    value = self.combine(value, next, all)?;
                }
                value
            }
            Guard::Site {
                view: OperandView::Captured, ..
            } if self.captured_site_reads => self.atom(CpuRead::Guard(id), negated)?,
            Guard::Site {
                view: OperandView::Captured, ..
            } => constant(negated),
            Guard::Site { site, test, .. } => self.site_test(*site, *test, negated, id, depth + 1)?,
        };
        self.guards.insert((id, negated), result.clone());
        Ok(result)
    }

    fn site_test(&mut self, site: SiteId, test: SiteTest, negated: bool, original: GuardId, depth: usize) -> Result<Dnf, ()> {
        let test_profile = |id: ProfileId| {
            let profile = self.graph.profiles[id.index()];
            match test {
                SiteTest::Solid => profile.solid,
                SiteTest::Transparent => profile.transparent,
                SiteTest::IsWire => matches!(Block::from_id(profile.raw_state), Block::RedstoneWire { .. }),
                SiteTest::Profile(expected) => id == expected,
            }
        };
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(profile) => Ok(constant(test_profile(profile) ^ negated)),
            SiteSource::Contact(contact) => {
                let variants = &self.graph.contacts[contact.index()].variants;
                if variants
                    .iter()
                    .any(|variant| matches!(variant.receiver, Some(CallbackReceiver::Component(_))))
                {
                    return self.atom(CpuRead::Guard(original), negated);
                }
                let selected: Vec<_> = variants.iter().map(|variant| (variant.when, test_profile(variant.profile))).collect();
                let mut result = Vec::new();
                let mut prior_false = constant(true);
                for (when, matches) in selected {
                    if matches ^ negated {
                        let guard = self.guard(when, false, depth)?;
                        let value = self.combine(prior_false.clone(), guard, true)?;
                        result = self.combine(result, value, false)?;
                    }
                    let not_guard = self.guard(when, true, depth)?;
                    prior_false = self.combine(prior_false, not_guard, true)?;
                }
                // No selected profile makes the underlying site test false.
                if negated {
                    result = self.combine(result, prior_false, false)?;
                }
                Ok(result)
            }
            SiteSource::Component(_) => self.atom(CpuRead::Guard(original), negated),
        }
    }

    fn query(&mut self, target: PowerTarget, threshold: u8, depth: usize) -> Result<Dnf, ()> {
        self.budget(1)?;
        if depth > 16 || threshold >= 15 {
            return if threshold >= 15 { Ok(Vec::new()) } else { Err(()) };
        }
        let mut result = Vec::new();
        for link in self.incoming.get(&target).into_iter().flatten() {
            let loss = threshold.saturating_add(link.attenuation);
            if loss >= 15 {
                continue;
            }
            let guard = self.guard(link.guard, false, 0)?;
            if guard.is_empty() {
                continue;
            }
            let source = self.source(link.from, link.from_port, loss, depth + 1)?;
            let value = self.combine(guard, source, true)?;
            result = self.combine(result, value, false)?;
        }
        Ok(result)
    }

    fn source(&mut self, site: SiteId, port: PowerPort, threshold: u8, depth: usize) -> Result<Dnf, ()> {
        self.budget(1)?;
        if threshold >= 15 {
            return Ok(Vec::new());
        }
        if let PowerPort::Conducted { include_dust } = port {
            return self.query(PowerTarget::Conductor { site, include_dust }, threshold, depth);
        }
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(profile) => {
                let block = Block::from_id(self.graph.profiles[profile.index()].raw_state);
                let emits = match port {
                    PowerPort::Weak(face) => power_rules::emits_weak(block, face),
                    PowerPort::Strong(face) => power_rules::emits_strong(block, face),
                    _ => true,
                };
                Ok(constant(emits && self.graph.sites[site.index()].source_strength > threshold))
            }
            SiteSource::Component(node) => {
                let spec = &self.graph.nodes[node.index()];
                let emits = match port {
                    PowerPort::Output | PowerPort::Stored => Some(true),
                    PowerPort::Weak(face) | PowerPort::Strong(face) if matches!(spec.kind, CircuitNodeKind::Wire(_)) => {
                        if let CircuitNodeKind::Wire(index) = spec.kind {
                            match face {
                                BlockFace::Top => Some(true),
                                BlockFace::Bottom => Some(false),
                                _ => self.graph.wires[index]
                                    .execution
                                    .shape
                                    .and_then(|shape| shape.emits(face.unwrap_direction().opposite())),
                            }
                        } else {
                            unreachable!()
                        }
                    }
                    PowerPort::Weak(face) | PowerPort::Strong(face) if matches!(spec.state, ComponentState::Electrical(_)) => {
                        spec.raw_state.map(Block::from_id).map(|block| {
                            if matches!(port, PowerPort::Weak(_)) {
                                power_rules::emits_weak(block, face)
                            } else {
                                power_rules::emits_strong(block, face)
                            }
                        })
                    }
                    _ => None,
                };
                match emits {
                    Some(false) => Ok(Vec::new()),
                    Some(true) => self.atom(CpuRead::Strength { node, threshold }, false),
                    None => {
                        if let CircuitNodeKind::Wire(_) = spec.kind
                            && let PowerPort::Weak(face) | PowerPort::Strong(face) = port
                        {
                            self.atom(CpuRead::WireEmission { node, face, threshold }, false)
                        } else {
                            self.atom(CpuRead::Source { site, port, threshold }, false)
                        }
                    }
                }
            }
            SiteSource::Contact(contact) => {
                let variants = &self.graph.contacts[contact.index()].variants;
                if variants
                    .iter()
                    .any(|variant| matches!(variant.receiver, Some(CallbackReceiver::Component(_))))
                {
                    return self.atom(CpuRead::Source { site, port, threshold }, false);
                }
                let mut result = Vec::new();
                let mut prior_false = constant(true);
                for variant in variants {
                    let profile = self.graph.profiles[variant.profile.index()];
                    let block = Block::from_id(profile.raw_state);
                    let emits = match port {
                        PowerPort::Weak(face) => power_rules::emits_weak(block, face),
                        PowerPort::Strong(face) => power_rules::emits_strong(block, face),
                        _ => true,
                    };
                    if profile.electrical == ProfileElectrical::RedstoneSource && emits {
                        let guard = self.guard(variant.when, false, 0)?;
                        let value = self.combine(prior_false.clone(), guard, true)?;
                        result = self.combine(result, value, false)?;
                    }
                    let not_guard = self.guard(variant.when, true, 0)?;
                    prior_false = self.combine(prior_false, not_guard, true)?;
                }
                Ok(result)
            }
        }
    }

    fn pin(&mut self, pins: &[CellPin], id: CellPinId, negated: bool, depth: usize) -> Result<Dnf, ()> {
        self.budget(1)?;
        if depth > 64 {
            return Err(());
        }
        match &pins[id.index()] {
            CellPin::Read(CellRead::State(test)) => self.atom(CpuRead::State(*test), negated),
            // Contribution reads are pure but negating a large disjunction need not expand it.
            CellPin::Read(CellRead::Contribution(_)) | CellPin::Read(CellRead::Activation(_)) => Err(()),
            CellPin::Gate {
                kind: CellGateKind::Buffer,
                inputs,
            } => self.pin(pins, inputs[0], negated, depth + 1),
            CellPin::Gate {
                kind: CellGateKind::Invert,
                inputs,
            } => self.pin(pins, inputs[0], !negated, depth + 1),
            CellPin::Gate { kind, inputs } => {
                let all = (*kind == CellGateKind::All) ^ negated;
                let mut result = constant(all);
                for &input in inputs {
                    let value = self.pin(pins, input, negated, depth + 1)?;
                    result = self.combine(result, value, all)?;
                }
                Ok(result)
            }
        }
    }
}

fn constant(value: bool) -> Dnf {
    if value { vec![(0, 0)] } else { Vec::new() }
}

impl CpuKernelPlans {
    pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        if monitor.cancelled() {
            return Err("cancelled before CPU kernel packing".into());
        }
        if graph.plans.instant_events.is_empty() {
            return Ok(Self::default());
        }
        let mut incoming: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for &link in &graph.power_links {
            if monitor.cancelled() {
                return Err("cancelled while indexing CPU kernel inputs".into());
            }
            incoming.entry(link.to).or_default().push(link);
        }
        let mut result = Self::default();
        result.require_true.push(0);
        result.require_false.push(0);
        'kernels: for kernel in &graph.plans.instant_events {
            let KernelProgram::Events(entries) = &kernel.program else {
                return Err("CPU packing requires event kernels".into());
            };
            for entry in entries {
                if monitor.cancelled() {
                    return Err("cancelled while packing CPU kernels".into());
                }
                let make = || Builder {
                    graph,
                    incoming: &incoming,
                    monitor,
                    reads: Vec::new(),
                    ids: FxHashMap::default(),
                    guards: FxHashMap::default(),
                    work: 0,
                    work_limit: MAX_WORK,
                    captured_site_reads: false,
                };
                let mut valid = make();
                let Ok(valid_clauses) = valid.pin(&entry.pins, entry.valid, false, 0) else {
                    continue;
                };
                let node = graph.pistons[entry.actor.index()].node;
                let checkpoint = (result.reads.len(), result.require_true.len());
                let mut compile_input = |port| {
                    let mut builder = make();
                    let clauses = if graph.plans.classic_nets.is_empty() {
                        builder.query(PowerTarget::Input { node, port }, 0, 0)
                    } else {
                        Err(())
                    };
                    let clauses = if let Ok(clauses) = clauses {
                        clauses
                    } else {
                        builder = make();
                        builder.atom(CpuRead::Input { node, port }, false).unwrap()
                    };
                    result.append(builder.reads, clauses)
                };
                let main = compile_input(InputPort::Main)?;
                let quasi = compile_input(InputPort::Quasi)?;
                let valid = result.append(valid.reads, valid_clauses)?;
                let bytes = result.reads.len() * std::mem::size_of::<CpuRead>()
                    + result.require_true.len() * 16
                    + (result.entries.len() + 1) * std::mem::size_of::<CpuKernelEntry>();
                if bytes > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8 {
                    result.reads.truncate(checkpoint.0);
                    result.require_true.truncate(checkpoint.1);
                    result.require_false.truncate(checkpoint.1);
                    break 'kernels;
                }
                result.entries.push(CpuKernelEntry {
                    actor: entry.actor,
                    main,
                    quasi,
                    valid,
                });
            }
        }
        result.entries.sort_unstable_by_key(|entry| entry.actor);
        if monitor.cancelled() {
            return Err("cancelled before publishing CPU kernels".into());
        }
        Ok(result)
    }

    fn append(&mut self, reads: Vec<CpuRead>, clauses: Dnf) -> Result<CpuPredicate, String> {
        let used = clauses.iter().fold(0, |bits, &(t, f)| bits | t | f);
        if used == 0 {
            if clauses.is_empty() {
                return Ok(CpuPredicate::default());
            }
            if self.require_true.is_empty() {
                self.require_true.push(0);
                self.require_false.push(0);
            }
            return Ok(CpuPredicate {
                clause_len: 1,
                ..Default::default()
            });
        }
        let mut order: Vec<_> = (0..reads.len()).filter(|&id| used & (1u64 << id) != 0).collect();
        order.sort_by_key(|&id| read_order(reads[id]));
        let predicate = CpuPredicate {
            read_start: u32::try_from(self.reads.len()).map_err(|_| "CPU read table overflow")?,
            clause_start: u32::try_from(self.require_true.len()).map_err(|_| "CPU clause table overflow")?,
            read_len: order.len() as u16,
            clause_len: clauses.len() as u16,
            kind: if order.len() == 1 && clauses.len() == 1 {
                CpuPredicateKind::Literal
            } else if clauses.len() == 1 && clauses[0].1 == 0 {
                CpuPredicateKind::All
            } else if clauses.len() == 1 {
                CpuPredicateKind::AllLiterals
            } else if clauses.len() == order.len() && clauses.iter().all(|&(yes, no)| no == 0 && yes.count_ones() == 1) {
                CpuPredicateKind::Any
            } else {
                CpuPredicateKind::Masks
            },
        };
        self.reads.extend(order.iter().map(|&id| reads[id]));
        let remap = |bits: u64| {
            order
                .iter()
                .enumerate()
                .fold(0, |value, (next, &old)| value | (((bits >> old) & 1) << next))
        };
        for (t, f) in clauses {
            self.require_true.push(remap(t));
            self.require_false.push(remap(f));
        }
        Ok(predicate)
    }

    pub(crate) fn evaluate(&self, predicate: CpuPredicate, mut read: impl FnMut(CpuRead) -> bool, matches: ClauseMatcher) -> bool {
        evaluate_cpu_predicate(predicate, &self.reads, &self.require_true, &self.require_false, &mut read, matches)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GuardPredicateStats {
    pub(crate) compiled: usize,
    pub(crate) fallback: usize,
    pub(crate) reads: usize,
    pub(crate) clauses: usize,
    pub(crate) work: usize,
    pub(crate) bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct GuardPredicatePlans {
    /// Dense predicate index per original GuardId; the sentinel retains recursive evaluation.
    pub(crate) by_guard: Vec<u32>,
    /// Contiguous descriptors, separate from the root map to keep per-guard storage compact.
    pub(crate) predicates: Vec<CpuPredicate>,
    pub(crate) reads: Vec<CpuRead>,
    pub(crate) require_true: Vec<u64>,
    pub(crate) require_false: Vec<u64>,
    pub(crate) stats: GuardPredicateStats,
}

impl GuardPredicatePlans {
    pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        if monitor.cancelled() {
            return Err("cancelled before guard predicate compilation".into());
        }
        let incoming: FxHashMap<PowerTarget, Vec<PowerLink>> = FxHashMap::default();
        let map_bytes = graph.guards.len().saturating_mul(std::mem::size_of::<u32>());
        let mut complex_guards = 0usize;
        for (index, guard) in graph.guards.iter().enumerate() {
            if index & 0x3ff == 0 && monitor.cancelled() {
                return Err("cancelled while counting guard roots".into());
            }
            if !matches!(guard, Guard::Always | Guard::State(_)) {
                complex_guards += 1;
            }
        }
        if map_bytes > MAX_GUARD_PLAN_BYTES {
            let mut result = Self::default();
            result.stats.fallback = complex_guards;
            return Ok(result);
        }
        let mut result = Self {
            by_guard: vec![NO_GUARD_PREDICATE; graph.guards.len()],
            ..Default::default()
        };
        let mut packed = CpuKernelPlans::default();
        let mut total_work = 0usize;
        let mut total_bytes = map_bytes;
        for (index, guard) in graph.guards.iter().enumerate() {
            if monitor.cancelled() {
                return Err("cancelled while compiling guard predicates".into());
            }
            if matches!(guard, Guard::Always | Guard::State(_)) {
                continue;
            }
            if total_work >= MAX_GUARD_GLOBAL_WORK || total_bytes >= MAX_GUARD_PLAN_BYTES {
                break;
            }
            let id = GuardId::try_from_index(index).ok_or("guard id overflow")?;
            let mut builder = Builder {
                graph,
                incoming: &incoming,
                monitor,
                reads: Vec::new(),
                ids: FxHashMap::default(),
                guards: FxHashMap::default(),
                work: 0,
                work_limit: MAX_WORK.min(MAX_GUARD_GLOBAL_WORK.saturating_sub(total_work)),
                captured_site_reads: true,
            };
            let clauses = builder.guard(id, false, 0);
            total_work = total_work.saturating_add(builder.work);
            if total_work > MAX_GUARD_GLOBAL_WORK {
                break;
            }
            let Ok(clauses) = clauses else {
                if monitor.cancelled() {
                    return Err("cancelled while compiling guard predicates".into());
                }
                result.stats.fallback += 1;
                continue;
            };
            // Preserve the cheapest direct path when a candidate merely calls itself recursively.
            if builder.reads.as_slice() == [CpuRead::Guard(id)] && clauses.as_slice() == [(1, 0)] {
                result.stats.fallback += 1;
                continue;
            }
            let used = clauses.iter().fold(0u64, |bits, &(yes, no)| bits | yes | no);
            let added_reads = used.count_ones() as usize;
            let added_bytes = added_reads * std::mem::size_of::<CpuRead>() + clauses.len() * 2 * std::mem::size_of::<u64>();
            let append_work = added_reads.saturating_add(clauses.len());
            if total_work.saturating_add(append_work) > MAX_GUARD_GLOBAL_WORK {
                break;
            }
            let descriptor_bytes = std::mem::size_of::<CpuPredicate>();
            if total_bytes.saturating_add(added_bytes).saturating_add(descriptor_bytes) > MAX_GUARD_PLAN_BYTES {
                break;
            }
            packed
                .reads
                .try_reserve(added_reads)
                .map_err(|_| "guard predicate read table allocation failed")?;
            packed
                .require_true
                .try_reserve(clauses.len())
                .map_err(|_| "guard predicate clause table allocation failed")?;
            packed
                .require_false
                .try_reserve(clauses.len())
                .map_err(|_| "guard predicate clause table allocation failed")?;
            result
                .predicates
                .try_reserve(1)
                .map_err(|_| "guard predicate descriptor allocation failed")?;
            let reserved_bytes = map_bytes
                .saturating_add(result.predicates.capacity() * std::mem::size_of::<CpuPredicate>())
                .saturating_add(packed.reads.capacity() * std::mem::size_of::<CpuRead>())
                .saturating_add(packed.require_true.capacity() * std::mem::size_of::<u64>())
                .saturating_add(packed.require_false.capacity() * std::mem::size_of::<u64>());
            if reserved_bytes > MAX_GUARD_PLAN_BYTES {
                result.predicates.shrink_to_fit();
                packed.reads.shrink_to_fit();
                packed.require_true.shrink_to_fit();
                packed.require_false.shrink_to_fit();
                break;
            }
            let predicate = packed.append(builder.reads, clauses)?;
            total_bytes = total_bytes.saturating_add(added_bytes).saturating_add(descriptor_bytes);
            total_work = total_work.saturating_add(append_work);
            let predicate_index = u32::try_from(result.predicates.len()).map_err(|_| "guard predicate index overflow")?;
            result.predicates.push(predicate);
            result.by_guard[index] = predicate_index;
            result.stats.compiled += 1;
        }
        if monitor.cancelled() {
            return Err("cancelled before publishing guard predicates".into());
        }
        result.stats.fallback = graph
            .guards
            .iter()
            .enumerate()
            .filter(|(index, guard)| !matches!(guard, Guard::Always | Guard::State(_)) && result.by_guard[*index] == NO_GUARD_PREDICATE)
            .count();
        result.stats.reads = packed.reads.len();
        result.stats.clauses = packed.require_true.len();
        result.stats.work = total_work;
        result.stats.bytes = map_bytes
            .saturating_add(result.predicates.capacity() * std::mem::size_of::<CpuPredicate>())
            .saturating_add(packed.reads.capacity() * std::mem::size_of::<CpuRead>())
            .saturating_add(packed.require_true.capacity() * std::mem::size_of::<u64>())
            .saturating_add(packed.require_false.capacity() * std::mem::size_of::<u64>());
        result.reads = packed.reads;
        result.require_true = packed.require_true;
        result.require_false = packed.require_false;
        if result.stats.compiled == 0 {
            result.by_guard.clear();
            result.by_guard.shrink_to_fit();
            result.stats.bytes = 0;
        }
        Ok(result)
    }

    pub(crate) fn predicate(&self, guard: GuardId) -> Option<CpuPredicate> {
        let index = *self.by_guard.get(guard.index())?;
        (index != NO_GUARD_PREDICATE)
            .then(|| self.predicates.get(index as usize).copied())
            .flatten()
    }

    pub(crate) fn evaluate(&self, predicate: CpuPredicate, mut read: impl FnMut(CpuRead) -> bool, matches: ClauseMatcher) -> bool {
        evaluate_cpu_predicate(predicate, &self.reads, &self.require_true, &self.require_false, &mut read, matches)
    }
}

fn evaluate_cpu_predicate(
    predicate: CpuPredicate,
    all_reads: &[CpuRead],
    require_true: &[u64],
    require_false: &[u64],
    read: &mut impl FnMut(CpuRead) -> bool,
    matches: ClauseMatcher,
) -> bool {
    if predicate.read_len == 0 {
        return predicate.clause_len != 0;
    }
    let start = predicate.read_start as usize;
    let reads = &all_reads[start..start + predicate.read_len as usize];
    match predicate.kind {
        CpuPredicateKind::Literal => return read(reads[0]) == (require_true[predicate.clause_start as usize] != 0),
        CpuPredicateKind::All => return reads.iter().copied().all(read),
        CpuPredicateKind::AllLiterals => {
            let required = require_true[predicate.clause_start as usize];
            return reads
                .iter()
                .enumerate()
                .all(|(index, &input)| read(input) == (required & (1u64 << index) != 0));
        }
        CpuPredicateKind::Any => return reads.iter().copied().any(read),
        CpuPredicateKind::Masks => {}
    }
    let values = reads
        .iter()
        .enumerate()
        .fold(0u64, |value, (bit, &input)| value | (u64::from(read(input)) << bit));
    let start = predicate.clause_start as usize;
    let end = start + predicate.clause_len as usize;
    matches(values, &require_true[start..end], &require_false[start..end])
}

fn read_order(read: CpuRead) -> (u8, usize) {
    match read {
        CpuRead::State(StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource)) => (0, resource.index()),
        CpuRead::State(
            StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. },
        ) => (1, actor.index()),
        CpuRead::Strength { node, .. } | CpuRead::WireEmission { node, .. } => (2, node.index()),
        CpuRead::Guard(guard) => (3, guard.index()),
        CpuRead::Source { site, .. } => (4, site.index()),
        CpuRead::Input { node, .. } => (5, node.index()),
    }
}

pub(crate) type ClauseMatcher = fn(u64, &[u64], &[u64]) -> bool;

pub(crate) fn scalar_clauses(values: u64, required: &[u64], forbidden: &[u64]) -> bool {
    required.iter().zip(forbidden).any(|(&yes, &no)| values & yes == yes && values & no == 0)
}

pub(crate) fn clause_matcher() -> ClauseMatcher {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        return avx2_clauses;
    }
    scalar_clauses
}

#[cfg(target_arch = "x86_64")]
fn avx2_clauses(values: u64, required: &[u64], forbidden: &[u64]) -> bool {
    assert_eq!(required.len(), forbidden.len());
    if required.len() < 4 {
        return scalar_clauses(values, required, forbidden);
    }
    // This private wrapper is selected only after the process's AVX2/OS support check.
    unsafe { match_avx2(values, required, forbidden) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn match_avx2(values: u64, required: &[u64], forbidden: &[u64]) -> bool {
    use std::arch::x86_64::*;
    let values = _mm256_set1_epi64x(values as i64);
    let zero = _mm256_setzero_si256();
    let full = required.len() / 4 * 4;
    for index in (0..full).step_by(4) {
        // The two equal-length slices contain all four lanes; unaligned loads are allowed.
        let yes = unsafe { _mm256_loadu_si256(required.as_ptr().add(index).cast()) };
        let no = unsafe { _mm256_loadu_si256(forbidden.as_ptr().add(index).cast()) };
        let set = _mm256_cmpeq_epi64(_mm256_and_si256(values, yes), yes);
        let clear = _mm256_cmpeq_epi64(_mm256_and_si256(values, no), zero);
        if _mm256_movemask_epi8(_mm256_and_si256(set, clear)) != 0 {
            return true;
        }
    }
    let value = _mm256_extract_epi64::<0>(values) as u64;
    scalar_clauses(value, &required[full..], &forbidden[full..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard_graph() -> (
        CompileGraph,
        ResourceId,
        ContactId,
        SiteId,
        ProfileId,
        ProfileId,
        GuardId,
        GuardId,
        GuardId,
    ) {
        let (world, bounds) = crate::redpiler::passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut graph =
            crate::redpiler::passes::rich::prepare(&world, bounds, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::default())
                .unwrap()
                .graph;
        let resource = ResourceId::new(0);
        let contact = graph.resources[0].contacts[0];
        let site = SiteId::try_from_index(graph.sites.iter().position(|site| site.source == SiteSource::Contact(contact)).unwrap()).unwrap();
        let profile_a = ProfileId::new(0);
        let profile_b = ProfileId::new(1);
        let enabled = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::State(StateTest::ResourceEnabled(resource)));
        let at = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::State(StateTest::ResourceAt { resource, contact }));
        let always = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Always);
        graph.contacts[contact.index()].variants = vec![
            ContactVariant {
                when: enabled,
                profile: profile_a,
                receiver: None,
            },
            ContactVariant {
                when: at,
                profile: profile_b,
                receiver: None,
            },
            ContactVariant {
                when: always,
                profile: profile_a,
                receiver: None,
            },
        ]
        .into_boxed_slice();
        (graph, resource, contact, site, profile_a, profile_b, enabled, at, always)
    }

    #[test]
    fn guard_predicates_match_compound_state_and_live_contact_first_match_truth_tables() {
        let (mut graph, _resource, _contact, site, _profile_a, profile_b, enabled, at, _always) = guard_graph();
        let site_guard = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Site {
            site,
            view: OperandView::Live,
            test: SiteTest::Profile(profile_b),
        });
        let extended = GuardId::try_from_index(graph.guards.len()).unwrap();
        let piston = PistonId::new(0);
        graph.guards.push(Guard::State(StateTest::PistonExtended(piston)));
        let all = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::All(vec![site_guard, extended].into_boxed_slice()));
        let not_enabled = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Not(enabled));
        let any = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Any(vec![site_guard, not_enabled].into_boxed_slice()));
        let negated = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Not(site_guard));

        let plans = GuardPredicatePlans::compile(&graph, &TaskMonitor::default()).unwrap();
        for root in [site_guard, all, any, negated] {
            assert!(plans.predicate(root).is_some(), "compound guard {root:?} should be packed");
        }
        for enabled_value in [false, true] {
            for at_value in [false, true] {
                for extended_value in [false, true] {
                    let site_value = !enabled_value && at_value;
                    let read = |input| match input {
                        CpuRead::State(StateTest::ResourceEnabled(_)) => enabled_value,
                        CpuRead::State(StateTest::ResourceAt { .. }) => at_value,
                        CpuRead::State(StateTest::PistonExtended(_)) => extended_value,
                        CpuRead::Guard(_) => panic!("live contact should expand into first-match state tests"),
                        _ => unreachable!("guard compiler emitted an unrelated read: {input:?}"),
                    };
                    for (root, expected) in [
                        (site_guard, site_value),
                        (all, site_value && extended_value),
                        (any, site_value || !enabled_value),
                        (negated, !site_value),
                    ] {
                        assert_eq!(plans.evaluate(plans.predicate(root).unwrap(), read, clause_matcher()), expected);
                    }
                }
            }
        }
        assert!(plans.stats.compiled >= 4);
        assert!(plans.stats.bytes <= MAX_GUARD_PLAN_BYTES);
        let _ = at;
    }

    #[test]
    fn guard_predicates_keep_captured_reads_and_skip_identity_fallbacks() {
        let (mut graph, _resource, _contact, site, _profile_a, _profile_b, enabled, _at, _always) = guard_graph();
        let captured = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Site {
            site,
            view: OperandView::Captured,
            test: SiteTest::Solid,
        });
        let combined = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::All(vec![captured, enabled].into_boxed_slice()));
        let disabled = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::Not(enabled));
        let mixed = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::All(vec![disabled, captured].into_boxed_slice()));
        let plans = GuardPredicatePlans::compile(&graph, &TaskMonitor::default()).unwrap();
        assert!(plans.predicate(captured).is_none(), "identity guard fallback should remain recursive");
        let predicate = plans.predicate(combined).expect("compound captured guard uses a packed context read");
        assert!(
            plans.reads[predicate.read_start as usize..(predicate.read_start + predicate.read_len as u32) as usize]
                .contains(&CpuRead::Guard(captured))
        );
        for captured_value in [false, true] {
            for enabled_value in [false, true] {
                let actual = plans.evaluate(
                    predicate,
                    |input| match input {
                        CpuRead::Guard(guard) if guard == captured => captured_value,
                        CpuRead::State(StateTest::ResourceEnabled(_)) => enabled_value,
                        other => unreachable!("unexpected captured guard read {other:?}"),
                    },
                    clause_matcher(),
                );
                assert_eq!(actual, captured_value && enabled_value);
            }
        }
        let mixed_predicate = plans.predicate(mixed).unwrap();
        assert_eq!(mixed_predicate.kind, CpuPredicateKind::AllLiterals);
        for captured_value in [false, true] {
            for enabled_value in [false, true] {
                let actual = plans.evaluate(
                    mixed_predicate,
                    |input| match input {
                        CpuRead::Guard(guard) if guard == captured => captured_value,
                        CpuRead::State(StateTest::ResourceEnabled(_)) => enabled_value,
                        other => unreachable!("unexpected mixed guard read {other:?}"),
                    },
                    clause_matcher(),
                );
                assert_eq!(actual, captured_value && !enabled_value);
            }
        }
        let mut sampled = Vec::new();
        assert!(!plans.evaluate(
            mixed_predicate,
            |input| {
                sampled.push(input);
                match input {
                    CpuRead::State(StateTest::ResourceEnabled(_)) => true,
                    CpuRead::Guard(_) => panic!("negative state literal must short-circuit the captured read"),
                    other => unreachable!("unexpected mixed guard read {other:?}"),
                }
            },
            clause_matcher(),
        ));
        assert_eq!(sampled.len(), 1);
    }

    #[test]
    fn guard_predicate_plan_rederives_rejects_forgery_and_honors_cancellation() {
        let (mut graph, _resource, _contact, site, _profile_a, _profile_b, enabled, _at, _always) = guard_graph();
        let compound = GuardId::try_from_index(graph.guards.len()).unwrap();
        graph.guards.push(Guard::All(vec![enabled, GuardId::new(0)].into_boxed_slice()));
        graph.plans.guard_predicates = GuardPredicatePlans::compile(&graph, &TaskMonitor::default()).unwrap();
        graph.validate_execution_plans_with_monitor(&TaskMonitor::default()).unwrap();

        let mut forged = graph.clone();
        forged.plans.guard_predicates.stats.work ^= 1;
        assert!(forged.validate_execution_plans_with_monitor(&TaskMonitor::default()).is_err());

        let mut stale = graph.clone();
        stale.guards[compound.index()] = Guard::Site {
            site,
            view: OperandView::Live,
            test: SiteTest::Transparent,
        };
        assert!(stale.validate_execution_plans_with_monitor(&TaskMonitor::default()).is_err());

        let cancelled = TaskMonitor::default();
        cancelled.cancel();
        assert!(GuardPredicatePlans::compile(&graph, &cancelled).is_err());
    }

    #[test]
    fn cpu_wire_emission_only_specializes_fixed_wire_owners() {
        let (world, bounds) = crate::redpiler::passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut graph =
            crate::redpiler::passes::rich::prepare(&world, bounds, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::default())
                .unwrap()
                .graph;
        let node = graph.wires[0].node;
        graph.wires[0].execution.shape = None;
        let site = SiteId::new(graph.sites.iter().position(|s| s.source == SiteSource::Component(node)).unwrap() as u32);
        let incoming = FxHashMap::default();
        for face in BlockFace::values() {
            for port in [PowerPort::Weak(face), PowerPort::Strong(face)] {
                for threshold in 0..15 {
                    let mut builder = Builder {
                        graph: &graph,
                        incoming: &incoming,
                        monitor: &monitor,
                        reads: vec![],
                        ids: FxHashMap::default(),
                        guards: FxHashMap::default(),
                        work: 0,
                        work_limit: MAX_WORK,
                        captured_site_reads: false,
                    };
                    let clauses = builder.source(site, port, threshold, 0).unwrap();
                    match face {
                        BlockFace::Top => assert_eq!(builder.reads, vec![CpuRead::Strength { node, threshold }]),
                        BlockFace::Bottom => assert!(clauses.is_empty()),
                        _ => assert_eq!(builder.reads, vec![CpuRead::WireEmission { node, face, threshold }]),
                    }
                }
            }
        }
        let contact = graph.resources[0].contacts[0];
        graph.sites[site.index()].source = SiteSource::Contact(contact);
        graph.contacts[contact.index()].variants[0].receiver = Some(CallbackReceiver::Component(node));
        let mut builder = Builder {
            graph: &graph,
            incoming: &incoming,
            monitor: &monitor,
            reads: vec![],
            ids: FxHashMap::default(),
            guards: FxHashMap::default(),
            work: 0,
            work_limit: MAX_WORK,
            captured_site_reads: false,
        };
        let port = PowerPort::Strong(BlockFace::North);
        builder.source(site, port, 3, 0).unwrap();
        assert_eq!(builder.reads, vec![CpuRead::Source { site, port, threshold: 3 }]);
    }

    #[test]
    fn cpu_scalar_predicates_match_masks_and_short_circuit() {
        for count in (1..=8).chain([64]) {
            let reads = (0..count)
                .map(|bit| CpuRead::Strength {
                    node: NodeId::new(bit),
                    threshold: 0,
                })
                .collect::<Vec<_>>();
            let all = u64::MAX >> (64 - count);
            for clauses in [vec![(all, 0)], (0..count).map(|bit| (1 << bit, 0)).collect(), vec![(0, all)]] {
                let mut plans = CpuKernelPlans::default();
                let predicate = plans.append(reads.clone(), clauses).unwrap();
                let general = CpuPredicate {
                    kind: CpuPredicateKind::Masks,
                    ..predicate
                };
                let words = if count == 64 {
                    vec![0, 1, u64::MAX >> 1, u64::MAX]
                } else {
                    (0..=all).collect()
                };
                for bits in words {
                    let read = |input| {
                        let CpuRead::Strength { node, .. } = input else { unreachable!() };
                        bits & (1 << node.index()) != 0
                    };
                    assert_eq!(
                        plans.evaluate(predicate, read, clause_matcher()),
                        plans.evaluate(general, read, clause_matcher())
                    );
                }
                if matches!(predicate.kind, CpuPredicateKind::All | CpuPredicateKind::Any | CpuPredicateKind::Literal) {
                    let mut calls = 0;
                    let value = predicate.kind == CpuPredicateKind::Any;
                    plans.evaluate(
                        predicate,
                        |_| {
                            calls += 1;
                            value
                        },
                        |_, _, _| panic!("scalar predicate called mask matcher"),
                    );
                    assert_eq!(calls, 1, "scalar predicate must stop at its first decisive read");
                }
            }
        }
        let mut plans = CpuKernelPlans::default();
        for clauses in [vec![], vec![(0, 0)]] {
            let expected = !clauses.is_empty();
            let predicate = plans.append(vec![], clauses).unwrap();
            assert_eq!(
                plans.evaluate(predicate, |_| panic!("constant read"), |_, _, _| panic!("constant matcher")),
                expected
            );
        }
    }

    #[test]
    fn cpu_kernel_contact_priority_and_missing_profile_are_exact() {
        let (world, bounds) = crate::redpiler::passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut graph =
            crate::redpiler::passes::rich::prepare(&world, bounds, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::default())
                .unwrap()
                .graph;
        let resource = ResourceId::new(0);
        let material = graph.resources[0].material;
        let air = ProfileId::new(graph.profiles.iter().position(|p| Block::from_id(p.raw_state) == Block::Air {}).unwrap() as u32);
        let contact = graph.resources[0].contacts[0];
        let site = SiteId::new(graph.sites.iter().position(|s| s.source == SiteSource::Contact(contact)).unwrap() as u32);
        let enabled = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::State(StateTest::ResourceEnabled(resource)));
        let always = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Always);
        graph.contacts[contact.index()].variants = vec![
            ContactVariant {
                when: enabled,
                profile: air,
                receiver: None,
            },
            ContactVariant {
                when: always,
                profile: material,
                receiver: None,
            },
        ]
        .into_boxed_slice();
        let incoming = FxHashMap::default();
        for threshold in 0..=15 {
            let mut builder = Builder {
                graph: &graph,
                incoming: &incoming,
                monitor: &monitor,
                reads: vec![],
                ids: FxHashMap::default(),
                guards: FxHashMap::default(),
                work: 0,
                work_limit: MAX_WORK,
                captured_site_reads: false,
            };
            let clauses = builder.source(site, PowerPort::Stored, threshold, 0).unwrap();
            let mut plan = CpuKernelPlans::default();
            let predicate = plan.append(builder.reads, clauses).unwrap();
            for value in [false, true] {
                assert_eq!(plan.evaluate(predicate, |_| value, clause_matcher()), !value && threshold < 15);
            }
        }
        graph.contacts[contact.index()].variants = vec![ContactVariant {
            when: enabled,
            profile: material,
            receiver: None,
        }]
        .into_boxed_slice();
        let test = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Site {
            site,
            view: OperandView::Live,
            test: SiteTest::Profile(material),
        });
        let mut builder = Builder {
            graph: &graph,
            incoming: &incoming,
            monitor: &monitor,
            reads: vec![],
            ids: FxHashMap::default(),
            guards: FxHashMap::default(),
            work: 0,
            work_limit: MAX_WORK,
            captured_site_reads: false,
        };
        let clauses = builder.guard(test, true, 0).unwrap();
        let mut plan = CpuKernelPlans::default();
        let predicate = plan.append(builder.reads, clauses).unwrap();
        for value in [false, true] {
            assert_eq!(plan.evaluate(predicate, |_| value, clause_matcher()), !value);
        }
        monitor.cancel();
        assert!(CpuKernelPlans::compile(&graph, &monitor).is_err());
    }

    #[test]
    fn cpu_kernel_vector_matches_scalar_for_tails_and_high_bits() {
        let selected = clause_matcher();
        let mut seed = 0x91af_8891_4317_abedu64;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for count in [0, 1, 2, 3, 4, 5, 7, 8, 9, 31, 63, 64] {
            for sample in 0..128 {
                let value = random();
                let mut yes = Vec::new();
                let mut no = Vec::new();
                for i in 0..count {
                    let required = random();
                    let forbidden = random() & !required;
                    yes.push(if sample % 2 == 0 && i % 3 == 0 { required & value } else { required });
                    no.push(if sample % 2 == 0 && i % 3 == 0 { forbidden & !value } else { forbidden });
                }
                assert_eq!(selected(value, &yes, &no), scalar_clauses(value, &yes, &no));
            }
        }
        assert!(selected(1 << 63, &[1, 2, 4, 1 << 63, 8], &[0; 5]));
        assert!(!selected(0, &[1, 2, 4, 1 << 63, 8], &[0; 5]));
        assert!(!selected(1 << 63, &[0; 5], &[1 << 63; 5]));
        println!(
            "CPU clause matcher: {}",
            if selected as usize == scalar_clauses as *const () as usize {
                "scalar"
            } else {
                "AVX2"
            }
        );
    }

    #[test]
    fn cpu_kernel_dnf_remapping_preserves_truth_table() {
        let mut plans = CpuKernelPlans::default();
        let reads = vec![
            CpuRead::State(StateTest::ResourceEnabled(ResourceId::new(2))),
            CpuRead::State(StateTest::ResourceEnabled(ResourceId::new(0))),
            CpuRead::State(StateTest::ResourceEnabled(ResourceId::new(1))),
        ];
        let predicate = plans.append(reads.clone(), vec![(1, 2), (4, 1)]).unwrap();
        for bits in 0u64..8 {
            let expected = scalar_clauses(bits, &[1, 4], &[2, 1]);
            assert_eq!(
                plans.evaluate(
                    predicate,
                    |input| bits & (1 << reads.iter().position(|r| *r == input).unwrap()) != 0,
                    clause_matcher()
                ),
                expected
            );
        }
    }
}
