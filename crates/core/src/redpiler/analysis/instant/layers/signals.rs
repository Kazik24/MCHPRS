//! Settled corridor equations, deliberately separate from permission to deliver a sample.
//!
//! max(a,b) > k becomes (a > k) OR (b > k); saturating subtraction adds to k.
//! Every dust edge loses a level, so cycles terminate at threshold 15. Contact guards
//! stay symbolic. These equations prove values at a settled cut, never Turbo arrival order.
use super::*;
use crate::redpiler::power_rules;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::Block;
use rustc_hash::{FxHashMap, FxHashSet};

/// Completed corridor equations that can be reused by another tracer over the
/// same immutable graph, query context, and `LayerBuilder` arena.
///
/// Pending recursion markers are intentionally excluded: an interrupted or
/// failed query must never make a later tracer believe that a query completed.
#[derive(Default)]
pub(super) struct SignalMemo {
    sources: FxHashMap<(SiteId, PowerPort, u8), LayerExprId>,
    targets: FxHashMap<(PowerTarget, u8), LayerExprId>,
    guards: FxHashMap<GuardId, LayerExprId>,
}

pub(super) struct SignalTracer<'a> {
    /// These immutable directional contributions already encode conducted and dust channels.
    graph: &'a CompileGraph,
    /// This indexes incoming links once for the containing compilation.
    power: &'a PowerIndex,
    /// These wires must be substituted, never exposed as independent boundary inputs.
    internal: &'a FxHashSet<NodeId>,
    /// These logical prefix values replace owned state reads after preceding cell writes.
    state: &'a FxHashMap<StateTest, LayerExprId>,
    /// These transferred payloads are disabled at Near during the accepted retraction
    /// until their delayed restoration; only positive All-guard conjunctions may use it.
    disabled_destinations: Option<&'a FxHashSet<(ResourceId, ContactId)>>,
    /// Settled logical extraction may bind saved-dot mode as a live input; legacy callers decline.
    stable_wire_modes: bool,
    /// This fixes the semantic sampling cut of any remaining outside read.
    role: LayerInputRole,
    /// These source/threshold roots share repeated electrical subqueries at this prefix.
    sources: FxHashMap<(SiteId, PowerPort, u8), LayerExprId>,
    /// These completed target roots share converging paths without path enumeration.
    targets: FxHashMap<(PowerTarget, u8), LayerExprId>,
    /// These pending queries detect unsupported zero-loss conductor cycles.
    visiting: FxHashSet<(PowerTarget, u8)>,
    /// These predicates share repeated contact alternatives and geometry guards.
    guards: FxHashMap<GuardId, LayerExprId>,
    /// These guard/site expansions bound recursion and reject self-dependent contact tests.
    pending_guards: FxHashSet<GuardId>,
}

impl<'a> SignalTracer<'a> {
    pub(super) fn new(
        graph: &'a CompileGraph,
        power: &'a PowerIndex,
        internal: &'a FxHashSet<NodeId>,
        state: &'a FxHashMap<StateTest, LayerExprId>,
        role: LayerInputRole,
    ) -> Self {
        Self {
            graph,
            power,
            internal,
            state,
            role,
            disabled_destinations: None,
            stable_wire_modes: false,
            sources: FxHashMap::default(),
            targets: FxHashMap::default(),
            visiting: FxHashSet::default(),
            guards: FxHashMap::default(),
            pending_guards: FxHashSet::default(),
        }
    }

    /// Apply a source-derived settled-cell fact without changing other tracer callers.
    pub(super) fn with_disabled_destinations(mut self, destinations: &'a FxHashSet<(ResourceId, ContactId)>) -> Self {
        self.disabled_destinations = Some(destinations);
        self
    }

    /// Bind an initially saved dot's live mode for settled logical-query extraction.
    pub(super) fn with_stable_wire_modes(mut self) -> Self {
        self.stable_wire_modes = true;
        self
    }

    /// Reuse completed equations in the same builder arena and exact query context.
    /// The caller must not reuse the memo across graphs, input/state maps, modes,
    /// roles, or builders because cached roots are arena-local.
    pub(super) fn with_memo(mut self, memo: SignalMemo) -> Self {
        self.sources = memo.sources;
        self.targets = memo.targets;
        self.guards = memo.guards;
        self
    }

    /// Return completed equations only; all in-progress recursion markers are dropped.
    pub(super) fn into_memo(self) -> SignalMemo {
        SignalMemo {
            sources: self.sources,
            targets: self.targets,
            guards: self.guards,
        }
    }

    /// Exact wire nodes encountered among this tracer's input and conductor targets.
    pub(super) fn queried_wire_nodes(&self) -> FxHashSet<NodeId> {
        self.targets
            .keys()
            .filter_map(|(target, _)| {
                let node = match target {
                    PowerTarget::Input { node, .. } => *node,
                    PowerTarget::Conductor { site, .. } => {
                        let SiteSource::Component(node) = self.graph.sites[site.index()].source else {
                            return None;
                        };
                        node
                    }
                };
                matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)).then_some(node)
            })
            .collect()
    }

    pub(super) fn contribution(&mut self, link: PowerLink, threshold: u8, builder: &mut LayerBuilder<'_>) -> Result<LayerExprId, ConstructionError> {
        builder.visit()?;
        let threshold = threshold.saturating_add(link.attenuation);
        if threshold >= 15 {
            return builder.constant(false);
        }
        let guard = self.guard(link.guard, builder)?;
        if matches!(builder.expression(guard)?, LayerExpr::Constant(false)) {
            return Ok(guard);
        }
        let source = self.source(link.from, link.from_port, threshold, builder)?;
        builder.all([guard, source])
    }

    pub(super) fn query(&mut self, target: PowerTarget, threshold: u8, builder: &mut LayerBuilder<'_>) -> Result<LayerExprId, ConstructionError> {
        builder.visit()?;
        if threshold >= 15 {
            return builder.constant(false);
        }
        let key = (target, threshold);
        if let Some(&root) = self.targets.get(&key) {
            return Ok(root);
        }
        if self.visiting.len() >= 128 {
            return Err("instant corridor exceeds its query nesting bound".into());
        }
        if !self.visiting.insert(key) {
            return Err("instant corridor has a zero-loss conductor cycle".into());
        }
        let links = self.power.get(&target).map(Vec::as_slice).unwrap_or(&[]);
        let mut terms = Vec::with_capacity(links.len());
        for &link in links {
            terms.push(self.contribution(link, threshold, builder)?);
        }
        let root = builder.any(terms)?;
        self.visiting.remove(&key);
        self.targets.insert(key, root);
        Ok(root)
    }

    fn source(&mut self, site: SiteId, port: PowerPort, threshold: u8, builder: &mut LayerBuilder<'_>) -> Result<LayerExprId, ConstructionError> {
        builder.visit()?;
        let key = (site, port, threshold);
        if let Some(&root) = self.sources.get(&key) {
            return Ok(root);
        }
        let root = if let PowerPort::Conducted { include_dust } = port {
            self.query(PowerTarget::Conductor { site, include_dust }, threshold, builder)?
        } else {
            match self.graph.sites[site.index()].source {
                SiteSource::Component(node) if self.internal.contains(&node) => {
                    let emits = self.wire_emits(node, port, builder)?;
                    let value = self.query(PowerTarget::Input { node, port: InputPort::Main }, threshold, builder)?;
                    builder.all([emits, value])?
                }
                SiteSource::Component(node) => {
                    let spec = &self.graph.nodes[node.index()];
                    if !matches!(spec.kind, CircuitNodeKind::Wire(_)) {
                        let block = Block::from_id(spec.raw_state.ok_or("outside source has no fixed block class")?);
                        if !emits(block, port) {
                            return builder.constant(false);
                        }
                    }
                    let input = builder.input(LayerInputSource::SourceAbove { site, port, threshold }, self.role)?;
                    builder.input_expr(input)?
                }
                SiteSource::Constant(profile) => {
                    let block = Block::from_id(self.graph.profiles[profile.index()].raw_state);
                    builder.constant(emits(block, port) && self.graph.sites[site.index()].source_strength > threshold)?
                }
                SiteSource::Contact(contact) => {
                    let mut alternatives = Vec::new();
                    let mut unmatched = builder.constant(true)?;
                    for variant in &self.graph.contacts[contact.index()].variants {
                        builder.visit()?;
                        let condition = self.guard(variant.when, builder)?;
                        let selected = builder.all([unmatched, condition])?;
                        let block = Block::from_id(self.graph.profiles[variant.profile.index()].raw_state);
                        let electrical = self.graph.profiles[variant.profile.index()].electrical;
                        if electrical == ProfileElectrical::Component {
                            return Err("corridor source is an electrically active contact component".into());
                        }
                        // Movable electrical components are rejected by geometry; inert cubes emit no power.
                        if electrical == ProfileElectrical::RedstoneSource && emits(block, port) {
                            alternatives.push(selected);
                        }
                        let not_condition = builder.not(condition)?;
                        unmatched = builder.all([unmatched, not_condition])?;
                    }
                    builder.any(alternatives)?
                }
            }
        };
        self.sources.insert(key, root);
        Ok(root)
    }

    pub(super) fn guard(&mut self, guard: GuardId, builder: &mut LayerBuilder<'_>) -> Result<LayerExprId, ConstructionError> {
        builder.visit()?;
        if let Some(&root) = self.guards.get(&guard) {
            return Ok(root);
        }
        if self.pending_guards.len() >= 128 || !self.pending_guards.insert(guard) {
            return Err("instant corridor has a recursive or too deeply nested geometry guard".into());
        }
        let root = match &self.graph.guards[guard.index()] {
            Guard::Always => builder.constant(true)?,
            Guard::State(test) => {
                if let Some(&root) = self.state.get(test) {
                    root
                } else {
                    let input = builder.input(LayerInputSource::State(*test), self.role)?;
                    builder.input_expr(input)?
                }
            }
            // Electrical sampling has no captured callback view, exactly as in Graph.
            Guard::Site {
                view: OperandView::Captured, ..
            } => builder.constant(false)?,
            Guard::Site { site, test, .. } => self.site_test(*site, *test, builder)?,
            Guard::Not(child) => {
                let value = self.guard(*child, builder)?;
                builder.not(value)?
            }
            Guard::All(children) if self.has_disabled_transfer_conjunction(children, builder)? => builder.constant(false)?,
            Guard::All(children) | Guard::Any(children) => {
                let mut values = Vec::with_capacity(children.len());
                for &child in children {
                    values.push(self.guard(child, builder)?);
                }
                if matches!(&self.graph.guards[guard.index()], Guard::All(_)) {
                    builder.all(values)?
                } else {
                    builder.any(values)?
                }
            }
        };
        self.pending_guards.remove(&guard);
        self.guards.insert(guard, root);
        Ok(root)
    }

    fn has_disabled_transfer_conjunction(&self, children: &[GuardId], builder: &mut LayerBuilder<'_>) -> Result<bool, ConstructionError> {
        let Some(disabled) = self.disabled_destinations else {
            return Ok(false);
        };
        if disabled.is_empty() {
            return Ok(false);
        }
        let mut pending = children.to_vec();
        let mut visited = FxHashSet::default();
        let mut at_destination = FxHashSet::default();
        let mut enabled = FxHashSet::default();
        while let Some(guard) = pending.pop() {
            builder.visit()?;
            if !visited.insert(guard) {
                continue;
            }
            match self.graph.guards.get(guard.index()).ok_or("guard references a missing row")? {
                Guard::State(StateTest::ResourceAt { resource, contact }) if disabled.contains(&(*resource, *contact)) => {
                    at_destination.insert(*resource);
                }
                Guard::State(StateTest::ResourceEnabled(resource)) => {
                    enabled.insert(*resource);
                }
                Guard::All(nested) => pending.extend(nested.iter().copied()),
                // Do not infer facts through disjunction or negation.
                Guard::Always | Guard::State(_) | Guard::Site { .. } | Guard::Not(_) | Guard::Any(_) => {}
            }
        }
        Ok(at_destination.iter().any(|resource| enabled.contains(resource)))
    }

    pub(super) fn site_test(&mut self, site: SiteId, test: SiteTest, builder: &mut LayerBuilder<'_>) -> Result<LayerExprId, ConstructionError> {
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(profile) => builder.constant(profile_matches(self.graph.profiles[profile.index()], Some(profile), test)),
            SiteSource::Component(node) => {
                let node = &self.graph.nodes[node.index()];
                let block = match node.state {
                    ComponentState::Wire(wire) => Block::RedstoneWire { wire },
                    _ => Block::from_id(node.raw_state.ok_or("component has no fixed block class")?),
                };
                let value = match test {
                    SiteTest::Solid => block.is_solid(),
                    SiteTest::Transparent => block.is_transparent(),
                    SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(_) => false,
                };
                builder.constant(value)
            }
            SiteSource::Contact(contact) => {
                let mut terms = Vec::new();
                let mut unmatched = builder.constant(true)?;
                for variant in &self.graph.contacts[contact.index()].variants {
                    builder.visit()?;
                    let condition = self.guard(variant.when, builder)?;
                    let selected = builder.all([unmatched, condition])?;
                    if profile_matches(self.graph.profiles[variant.profile.index()], Some(variant.profile), test) {
                        terms.push(selected);
                    }
                    let not_condition = builder.not(condition)?;
                    unmatched = builder.all([unmatched, not_condition])?;
                }
                builder.any(terms)
            }
        }
    }

    fn wire_emits(&mut self, node: NodeId, port: PowerPort, builder: &mut LayerBuilder<'_>) -> Result<LayerExprId, ConstructionError> {
        let face = match port {
            PowerPort::Output | PowerPort::Stored => return builder.constant(true),
            PowerPort::Weak(face) | PowerPort::Strong(face) => face,
            PowerPort::Conducted { .. } => unreachable!(),
        };
        if face == BlockFace::Top {
            return builder.constant(true);
        }
        if face == BlockFace::Bottom {
            return builder.constant(false);
        }
        let CircuitNodeKind::Wire(index) = self.graph.nodes[node.index()].kind else {
            return Err("corridor owns a non-wire component".into());
        };
        let saved_dot = matches!(self.graph.nodes[node.index()].state, ComponentState::Wire(saved) if crate::redstone::wire::is_dot(saved));
        if saved_dot && !self.stable_wire_modes {
            return Err("directional corridor output depends on dot/cross state".into());
        }
        // Native neighbor updates preserve cross mode; player edits invalidate the compiled graph.
        let dot_mode = if saved_dot {
            let input = builder.input(LayerInputSource::WireDot { node }, self.role)?;
            Some(builder.input_expr(input)?)
        } else {
            None
        };
        let neighbors = self.graph.wires[index].canonical_neighbors;
        let above_solid = self.site_test(neighbors[3], SiteTest::Solid, builder)?;
        let headroom = builder.not(above_solid)?;
        let mut connected = Vec::with_capacity(4);
        for (direction, neighbor, above, below) in [
            (BlockDirection::North, 4, 20, 17),
            (BlockDirection::South, 5, 21, 18),
            (BlockDirection::East, 1, 13, 12),
            (BlockDirection::West, 0, 8, 7),
        ] {
            let direct = self.block_test(
                neighbors[neighbor],
                |block| crate::redstone::wire::can_connect_to_uncached(block, direction),
                builder,
            )?;
            let up_wire = self.site_test(neighbors[above], SiteTest::IsWire, builder)?;
            let up = builder.all([headroom, up_wire])?;
            let solid = self.site_test(neighbors[neighbor], SiteTest::Solid, builder)?;
            let open = builder.not(solid)?;
            let down_wire = self.site_test(neighbors[below], SiteTest::IsWire, builder)?;
            let down = builder.all([open, down_wire])?;
            connected.push(builder.any([direct, up, down])?);
        }
        // Non-dot dust remains non-dot: regulation adds the missing opposite side or a cross.
        let (side, perpendicular) = match face.unwrap_direction().opposite() {
            BlockDirection::North => (0, [2, 3]),
            BlockDirection::South => (1, [2, 3]),
            BlockDirection::East => (2, [0, 1]),
            BlockDirection::West => (3, [0, 1]),
        };
        let first = builder.not(connected[perpendicular[0]])?;
        let second = builder.not(connected[perpendicular[1]])?;
        let complete_axis = builder.all([first, second])?;
        if let Some(dot_mode) = dot_mode {
            let not_dot = builder.not(dot_mode)?;
            let cross_axis_emits = builder.all([not_dot, complete_axis])?;
            builder.any([connected[side], cross_axis_emits])
        } else {
            builder.any([connected[side], complete_axis])
        }
    }

    fn block_test(
        &mut self,
        site: SiteId,
        predicate: impl Fn(Block) -> bool,
        builder: &mut LayerBuilder<'_>,
    ) -> Result<LayerExprId, ConstructionError> {
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(profile) => builder.constant(predicate(Block::from_id(self.graph.profiles[profile.index()].raw_state))),
            SiteSource::Component(node) => {
                let spec = &self.graph.nodes[node.index()];
                let block = match spec.state {
                    ComponentState::Wire(wire) => Block::RedstoneWire { wire },
                    _ => Block::from_id(spec.raw_state.ok_or("component has no fixed block class")?),
                };
                builder.constant(predicate(block))
            }
            SiteSource::Contact(contact) => {
                let mut unmatched = builder.constant(true)?;
                let mut alternatives = Vec::new();
                for variant in &self.graph.contacts[contact.index()].variants {
                    let condition = self.guard(variant.when, builder)?;
                    let selected = builder.all([unmatched, condition])?;
                    if predicate(Block::from_id(self.graph.profiles[variant.profile.index()].raw_state)) {
                        alternatives.push(selected);
                    }
                    let not_condition = builder.not(condition)?;
                    unmatched = builder.all([unmatched, not_condition])?;
                }
                builder.any(alternatives)
            }
        }
    }
}

fn emits(block: Block, port: PowerPort) -> bool {
    match port {
        PowerPort::Output | PowerPort::Stored => true,
        PowerPort::Weak(face) => power_rules::emits_weak(block, face),
        PowerPort::Strong(face) => power_rules::emits_strong(block, face),
        PowerPort::Conducted { .. } => unreachable!(),
    }
}

pub(super) fn profile_matches(profile: Profile, id: Option<ProfileId>, test: SiteTest) -> bool {
    match test {
        SiteTest::Solid => profile.solid,
        SiteTest::Transparent => profile.transparent,
        SiteTest::IsWire => matches!(Block::from_id(profile.raw_state), Block::RedstoneWire { .. }),
        SiteTest::Profile(expected) => id == Some(expected),
    }
}

#[cfg(test)]
mod tests;
