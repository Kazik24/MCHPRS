//! Fixed electrical queries and the dust-free weighted graph they imply at equilibrium.
//! Live queries keep original wire state operands; transitive traces are witnesses,
//! never permission to publish a source through undelivered wiring or reorder events.
use super::*;
use crate::redpiler::{TaskMonitor, power_rules};
use mchprs_blocks::blocks::RedstoneWireSide;
use std::collections::BTreeMap;

/// Provenance is optional: exhausting this work budget never disables exact live queries.
const MAX_TRACE_WORK: usize = 32_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WeightedSignal {
    /// Existing state owner, read when the original electrical query executes.
    pub(crate) node: NodeId,
    /// Accumulated strength loss, strictly less than fifteen.
    pub(crate) attenuation: u8,
    /// Isolated dust can retain a dot; this tests stored sides without geometry lookup.
    pub(crate) require_cross: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FixedSignalQuery {
    pub(crate) target: PowerTarget,
    pub(crate) constant: u8,
    pub(crate) inputs: Box<[WeightedSignal]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FixedSignalPlans {
    /// Exact live queries, with conductor chains and directional tests compiled away.
    pub(crate) live: Vec<FixedSignalQuery>,
    /// Explicit fallbacks keep contact/state-dependent queries on general execution.
    pub(crate) fallback: Vec<PowerTarget>,
    /// Historical-tracer-style settled equations; wire operands have been expanded.
    pub(crate) traced: Vec<FixedSignalQuery>,
    /// Explanations distinguish inference failure from receiver-execution refusal.
    pub(crate) declines: BTreeMap<String, usize>,
}

#[derive(Default, Clone)]
struct Expression {
    constant: u8,
    inputs: BTreeMap<(NodeId, bool), u8>,
}

impl Expression {
    fn include(&mut self, other: &Self, attenuation: u8) {
        self.constant = self.constant.max(other.constant.saturating_sub(attenuation));
        for (&key, &loss) in &other.inputs {
            let loss = loss.saturating_add(attenuation);
            if loss < 15 {
                self.inputs.entry(key).and_modify(|old| *old = (*old).min(loss)).or_insert(loss);
            }
        }
    }

    fn query(self, target: PowerTarget) -> FixedSignalQuery {
        FixedSignalQuery {
            target,
            constant: self.constant,
            inputs: self
                .inputs
                .into_iter()
                .map(|((node, require_cross), attenuation)| WeightedSignal {
                    node,
                    attenuation,
                    require_cross,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

struct Analysis<'a> {
    graph: &'a CompileGraph,
    links: BTreeMap<PowerTarget, Vec<&'a PowerLink>>,
    shapes: Vec<Option<RedstoneWire>>,
    stable_shapes: Vec<bool>,
}

impl Analysis<'_> {
    fn block(&self, site: SiteId) -> Result<(Block, Option<ProfileId>, bool, bool), &'static str> {
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(id) => {
                let p = self.graph.profiles[id.index()];
                Ok((Block::from_id(p.raw_state), Some(id), p.solid, p.transparent))
            }
            SiteSource::Component(node) => {
                let spec = &self.graph.nodes[node.index()];
                let block = if let ComponentState::Wire(wire) = spec.state {
                    Block::RedstoneWire { wire }
                } else {
                    Block::from_id(spec.raw_state.ok_or("component without fixed class")?)
                };
                Ok((block, None, block.is_solid(), block.is_transparent()))
            }
            SiteSource::Contact(_) => Err("selectable contact"),
        }
    }

    fn guard(&self, id: GuardId, depth: usize) -> Result<bool, &'static str> {
        if depth > 64 {
            return Err("guard depth");
        }
        Ok(match &self.graph.guards[id.index()] {
            Guard::Always => true,
            Guard::State(_) => return Err("state-dependent guard"),
            // A power query supplies no capture, even for an immutable site.
            Guard::Site {
                view: OperandView::Captured, ..
            } => false,
            Guard::Site { site, test, .. } => {
                let (block, profile, solid, transparent) = self.block(*site)?;
                match test {
                    SiteTest::Solid => solid,
                    SiteTest::Transparent => transparent,
                    SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(id) => profile == Some(*id),
                }
            }
            Guard::Not(child) => !self.guard(*child, depth + 1)?,
            Guard::All(children) => {
                let mut result = true;
                for child in children {
                    if !self.guard(*child, depth + 1)? {
                        result = false;
                        break;
                    }
                }
                result
            }
            Guard::Any(children) => {
                let mut result = false;
                for child in children {
                    if self.guard(*child, depth + 1)? {
                        result = true;
                        break;
                    }
                }
                result
            }
        })
    }

    fn wire_emission(&self, node: NodeId, face: BlockFace) -> Result<Option<bool>, &'static str> {
        if face == BlockFace::Top {
            return Ok(Some(false));
        }
        if face == BlockFace::Bottom {
            return Ok(None);
        }
        let raw = self.shapes[node.index()].ok_or("wire direction has dynamic geometry")?;
        if crate::redstone::wire::is_dot(raw) {
            return Ok(Some(true));
        }
        let regulated = crate::redstone::wire::regulate_sides(RedstoneWire::default(), raw);
        Ok((crate::redstone::wire::get_current_side(regulated, face.unwrap_direction().opposite()) != RedstoneWireSide::None).then_some(false))
    }

    fn source(&self, site: SiteId, port: PowerPort, depth: usize) -> Result<Expression, &'static str> {
        if let PowerPort::Conducted { include_dust } = port {
            return self.query(PowerTarget::Conductor { site, include_dust }, depth + 1);
        }
        let mut require_cross = false;
        if let PowerPort::Weak(face) | PowerPort::Strong(face) = port {
            let (block, _, _, _) = self.block(site)?;
            let emits = if let SiteSource::Component(node) = self.graph.sites[site.index()].source
                && matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_))
            {
                match self.wire_emission(node, face)? {
                    Some(gate) => {
                        require_cross = gate;
                        true
                    }
                    None => false,
                }
            } else if matches!(port, PowerPort::Weak(_)) {
                power_rules::emits_weak(block, face)
            } else {
                power_rules::emits_strong(block, face)
            };
            if !emits {
                return Ok(Expression::default());
            }
        }
        match self.graph.sites[site.index()].source {
            SiteSource::Component(node) => Ok(Expression {
                constant: 0,
                inputs: BTreeMap::from([((node, require_cross), 0)]),
            }),
            SiteSource::Constant(_) => Ok(Expression {
                constant: self.graph.sites[site.index()].source_strength,
                ..Default::default()
            }),
            SiteSource::Contact(_) => Err("selectable signal source"),
        }
    }

    fn query(&self, target: PowerTarget, depth: usize) -> Result<Expression, &'static str> {
        if depth > 16 {
            return Err("conducted query depth or cycle");
        }
        let mut expression = Expression::default();
        for link in self.links.get(&target).into_iter().flatten() {
            if link.attenuation < 15 && self.guard(link.guard, 0)? {
                expression.include(&self.source(link.from, link.from_port, depth)?, link.attenuation);
            }
        }
        Ok(expression)
    }

    fn trace(&self, root: &Expression, monitor: &TaskMonitor, work: &mut usize) -> Result<Expression, &'static str> {
        let mut result = Expression {
            constant: root.constant,
            ..Default::default()
        };
        let mut queue = root
            .inputs
            .iter()
            .map(|(&(node, gate), &loss)| (node, gate, loss))
            .collect::<VecDeque<_>>();
        let mut visited = BTreeMap::<NodeId, u8>::new();
        while let Some((node, gate, loss)) = queue.pop_front() {
            if *work >= MAX_TRACE_WORK {
                return Err("provenance work bound");
            }
            *work += 1;
            if *work % 256 == 0 && monitor.cancelled() {
                return Err("cancelled");
            }
            if loss >= 15 {
                continue;
            }
            if gate {
                if !self.stable_shapes[node.index()] {
                    return Err("wire dot/cross not stable");
                }
                let ComponentState::Wire(wire) = self.graph.nodes[node.index()].state else {
                    unreachable!()
                };
                if crate::redstone::wire::is_dot(wire) {
                    continue;
                }
            }
            if !matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) {
                result
                    .inputs
                    .entry((node, false))
                    .and_modify(|old| *old = (*old).min(loss))
                    .or_insert(loss);
                continue;
            }
            if visited.get(&node).is_some_and(|old| *old <= loss) {
                continue;
            }
            visited.insert(node, loss);
            let next = self.query(PowerTarget::Input { node, port: InputPort::Main }, 0)?;
            result.constant = result.constant.max(next.constant.saturating_sub(loss));
            for ((source, gate), distance) in next.inputs {
                let distance = loss.saturating_add(distance);
                if distance < 15 {
                    queue.push_back((source, gate, distance));
                }
            }
        }
        Ok(result)
    }
}

pub(crate) fn infer_fixed_signals(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<FixedSignalPlans, String> {
    let mut links = BTreeMap::<_, Vec<_>>::new();
    for (index, link) in graph.power_links.iter().enumerate() {
        if index % 4096 == 0 && monitor.cancelled() {
            return Err("cancelled indexing fixed signals".into());
        }
        links.entry(link.to).or_default().push(link);
    }
    let mut analysis = Analysis {
        graph,
        links,
        shapes: vec![None; graph.nodes.len()],
        stable_shapes: vec![false; graph.nodes.len()],
    };
    for wire in &graph.wires {
        if monitor.cancelled() {
            return Err("cancelled inferring fixed wire signals".into());
        }
        let raw = WireQueryFacts::infer(graph, wire).raw_wire(0);
        analysis.shapes[wire.node.index()] = raw;
        if let (Some(raw), ComponentState::Wire(saved)) = (raw, &graph.nodes[wire.node.index()].state) {
            let raw = RedstoneWire { power: saved.power, ..raw };
            analysis.stable_shapes[wire.node.index()] = BlockFace::values()
                .into_iter()
                .all(|face| crate::redstone::wire::on_neighbor_changed_from(*saved, raw, face) == *saved);
        }
    }
    let mut plans = FixedSignalPlans::default();
    let mut trace_work = 0;
    for &target in analysis.links.keys() {
        if monitor.cancelled() {
            return Err("cancelled compiling fixed signal queries".into());
        }
        match analysis.query(target, 0) {
            Ok(expression) => {
                if let PowerTarget::Input { node, .. } = target
                    && !matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_))
                {
                    match analysis.trace(&expression, monitor, &mut trace_work) {
                        Ok(traced) => plans.traced.push(traced.query(target)),
                        Err(reason) => *plans.declines.entry(format!("trace: {reason}")).or_default() += 1,
                    }
                }
                plans.live.push(expression.query(target));
            }
            Err(reason) => {
                plans.fallback.push(target);
                *plans.declines.entry(format!("live: {reason}")).or_default() += 1;
            }
        }
    }
    Ok(plans)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FixedSignalProgram {
    ranges: Box<[(u8, Range<usize>)]>,
    inputs: Box<[WeightedSignal]>,
    nodes: Box<[[u32; 3]]>,
    sites: Box<[[u32; 2]]>,
}

impl FixedSignalProgram {
    pub(crate) fn lower(plans: &FixedSignalPlans, nodes: usize, sites: usize) -> Self {
        if plans.live.is_empty() && plans.fallback.is_empty() {
            return Self::default();
        }
        let mut program = Self {
            ranges: Box::new([]),
            inputs: Box::new([]),
            nodes: vec![[0; 3]; nodes].into_boxed_slice(),
            sites: vec![[0; 2]; sites].into_boxed_slice(),
        };
        let mut ranges = vec![(0, 0..0)];
        let mut inputs = Vec::new();
        for query in &plans.live {
            let start = inputs.len();
            inputs.extend_from_slice(&query.inputs);
            let index = ranges.len() as u32;
            ranges.push((query.constant, start..inputs.len()));
            *program.slot_mut(query.target) = index;
        }
        for target in &plans.fallback {
            *program.slot_mut(*target) = u32::MAX;
        }
        program.ranges = ranges.into_boxed_slice();
        program.inputs = inputs.into_boxed_slice();
        program
    }

    fn slot_mut(&mut self, target: PowerTarget) -> &mut u32 {
        match target {
            PowerTarget::Input { node, port } => &mut self.nodes[node.index()][port as usize],
            PowerTarget::Conductor { site, include_dust } => &mut self.sites[site.index()][usize::from(include_dust)],
        }
    }

    pub(crate) fn query(&self, target: PowerTarget) -> Option<(u8, &[WeightedSignal])> {
        let index = match target {
            PowerTarget::Input { node, port } => *self.nodes.get(node.index())?.get(port as usize)?,
            PowerTarget::Conductor { site, include_dust } => self.sites.get(site.index())?[usize::from(include_dust)],
        };
        let (constant, range) = self.ranges.get(index as usize)?;
        Some((*constant, &self.inputs[range.clone()]))
    }
}
