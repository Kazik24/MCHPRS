//! Settled logical equations, independent of physical callback and reset execution.
use super::*;
use crate::redpiler::components::NodeType;
use rustc_hash::FxHashSet;
#[cfg(test)]
mod controller;
#[cfg(test)]
mod counter;
#[cfg(test)]
pub(crate) use controller::controller_support;
#[cfg(test)]
#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
#[cfg(test)]
mod divider;
#[cfg(test)]
mod fpu;
mod settled;
pub(crate) mod timing;
#[cfg(test)]
pub(crate) use settled::diagnostic_all_phase_raw_query;
#[cfg(test)]
pub(crate) use settled::diagnostic_query;
pub(crate) use settled::{compile_device, compile_public_boundary, compile_settled};
#[cfg(test)]
mod adders11;
#[cfg(test)]
mod settled_tests;
#[cfg(test)]
mod test_samples;

struct Equations<'a> {
    graph: &'a CompileGraph,
    power: PowerIndex,
    wires: FxHashSet<NodeId>,
    resets: FxHashSet<NodeId>,
    cells: FxHashMap<ResourceId, (ContactId, Vec<PistonId>)>,
    /// Near contacts held disabled by an admitted delayed token-transfer restoration.
    disabled_destinations: FxHashSet<(ResourceId, ContactId)>,
    actors: FxHashSet<PistonId>,
    registers: FxHashMap<ResourceId, BudMemoryRead>,
    inline_repeaters: bool,
    stateful_repeaters: FxHashSet<NodeId>,
    raw: LayerBuilder<'a>,
    reduced: LayerBuilder<'a>,
    expanded: FxHashMap<LayerExprId, LayerExprId>,
    sources: FxHashMap<LayerInputSource, LayerExprId>,
    pending: FxHashSet<LayerInputSource>,
    pending_repeaters: FxHashSet<NodeId>,
    retain_classic: bool,
}

impl<'a> Equations<'a> {
    fn new(graph: &'a CompileGraph, monitor: &'a TaskMonitor) -> Result<Self, ConstructionError> {
        let catalog = graph.plans.instant.boundaries.as_ref().ok_or("missing cell boundaries")?;
        let disabled_destinations = disabled_destinations(graph, catalog);
        Ok(Self {
            graph,
            power: index_power(graph),
            wires: graph
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(id, node)| matches!(node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(id as u32)))
                .collect(),
            resets: catalog
                .cells
                .iter()
                .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer()))
                .collect(),
            cells: catalog
                .cells
                .iter()
                .flat_map(|cell| {
                    cell.resources
                        .iter()
                        .map(|resource| (resource.resource, (resource.source, resource.owners.to_vec())))
                })
                .collect(),
            disabled_destinations,
            actors: catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect(),
            registers: graph.plans.bud_memory_reads.iter().map(|read| (read.resource, *read)).collect(),
            inline_repeaters: false,
            stateful_repeaters: graph
                .power_links
                .iter()
                .filter_map(|link| match link.to {
                    PowerTarget::Input { node, port: InputPort::Side }
                        if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) =>
                    {
                        Some(node)
                    }
                    _ => None,
                })
                .collect(),
            raw: LayerBuilder::new(monitor)?,
            reduced: LayerBuilder::new(monitor)?,
            expanded: FxHashMap::default(),
            sources: FxHashMap::default(),
            pending: FxHashSet::default(),
            pending_repeaters: FxHashSet::default(),
            retain_classic: false,
        })
    }

    fn input_power(&mut self, node: NodeId, port: InputPort) -> Result<LayerExprId, ConstructionError> {
        self.query(PowerTarget::Input { node, port }, 0)
    }

    fn query(&mut self, target: PowerTarget, threshold: u8) -> Result<LayerExprId, ConstructionError> {
        let state = FxHashMap::default();
        let mut tracer = signals::SignalTracer::new(self.graph, &self.power, &self.wires, &state, LayerInputRole::RequestSample)
            .with_disabled_destinations(&self.disabled_destinations);
        let raw = tracer.query(target, threshold, &mut self.raw)?;
        self.expand(raw)
    }

    fn powered(&mut self, actor: PistonId) -> Result<LayerExprId, ConstructionError> {
        let node = self.graph.pistons[actor.index()].node;
        let main = self.input_power(node, InputPort::Main)?;
        let quasi = self.input_power(node, InputPort::Quasi)?;
        self.reduced.any([main, quasi])
    }

    fn expand(&mut self, id: LayerExprId) -> Result<LayerExprId, ConstructionError> {
        self.reduced.visit()?;
        if let Some(&value) = self.expanded.get(&id) {
            return Ok(value);
        }
        let value = match self.raw.expression(id)?.clone() {
            LayerExpr::Constant(value) => self.reduced.constant(value)?,
            LayerExpr::Input(input) => self.source(self.raw.inputs[input.0 as usize].source)?,
            LayerExpr::Not(child) => {
                let child = self.expand(child)?;
                self.reduced.not(child)?
            }
            LayerExpr::All(children) | LayerExpr::Any(children) => {
                let values = children.iter().map(|&child| self.expand(child)).collect::<Result<Vec<_>, _>>()?;
                self.reduced.combine(values, matches!(self.raw.expression(id)?, LayerExpr::All(_)))?
            }
        };
        self.expanded.insert(id, value);
        Ok(value)
    }

    fn leaf(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        let input = self.reduced.input(source, LayerInputRole::RequestSample)?;
        self.reduced.input_expr(input)
    }

    fn source(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        if let Some(&value) = self.sources.get(&source) {
            return Ok(value);
        }
        let repeater = self.inline_repeaters.then(|| self.repeater_source_node(source)).flatten();
        if self.pending.contains(&source) || repeater.is_some_and(|node| self.pending_repeaters.contains(&node)) {
            if self.inline_repeaters {
                self.stateful_repeaters.extend(self.pending_repeaters.iter().copied());
                self.stateful_repeaters.extend(repeater);
            }
            return Err(format!("logical feedback requires a register: {source:?}").into());
        }
        if self.pending.len() >= 256 {
            return Err("logical equation exceeds its source nesting bound".into());
        }
        self.pending.insert(source);
        if let Some(node) = repeater {
            self.pending_repeaters.insert(node);
        }
        let result = self.source_value(source);
        self.pending.remove(&source);
        if let Some(node) = repeater {
            self.pending_repeaters.remove(&node);
        }
        let value = match result {
            Ok(value) => value,
            Err(ConstructionError::Invalid(message))
                if self.inline_repeaters
                    && message.starts_with("logical feedback requires a register:")
                    && self
                        .repeater_source_node(source)
                        .is_some_and(|node| self.stateful_repeaters.contains(&node)) =>
            {
                self.binary_source_leaf(source)?
            }
            Err(error) => return Err(error),
        };
        let value = if self.inline_repeaters
            && self
                .repeater_source_node(source)
                .is_some_and(|node| self.stateful_repeaters.contains(&node))
        {
            self.binary_source_leaf(source)?
        } else {
            value
        };
        self.sources.insert(source, value);
        Ok(value)
    }

    fn repeater_source_node(&self, source: LayerInputSource) -> Option<NodeId> {
        let LayerInputSource::SourceAbove { site, .. } = source else {
            return None;
        };
        let SiteSource::Component(node) = self.graph.sites[site.index()].source else {
            return None;
        };
        matches!(
            self.graph.nodes[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Repeater { .. })
        )
        .then_some(node)
    }

    fn binary_source_leaf(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        let LayerInputSource::SourceAbove { site, .. } = source else {
            return Err("logical repeater state is not a source input".into());
        };
        self.leaf(LayerInputSource::SourceAbove {
            site,
            port: PowerPort::Output,
            threshold: 0,
        })
    }

    fn source_value(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        let value = match source {
            LayerInputSource::State(StateTest::PistonAvailability { piston, is }) => {
                if self.actors.contains(&piston) || self.registers.values().any(|read| read.storage == piston) {
                    self.reduced.constant(is == PistonAvailability::Ready)?
                } else {
                    self.leaf(source)?
                }
            }
            LayerInputSource::State(StateTest::ResourceEnabled(resource)) => {
                if !self.cells.contains_key(&resource) && !self.registers.contains_key(&resource) {
                    return if self.retain_classic {
                        self.leaf(source)
                    } else {
                        Err(format!("unclassified logical resource {resource:?}").into())
                    };
                }
                self.reduced.constant(true)?
            }
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
                if let Some(read) = self.registers.get(&resource).copied() {
                    let value = self.leaf(LayerInputSource::State(StateTest::ResourceAt {
                        resource,
                        contact: read.source,
                    }))?;
                    let spec = self.graph.pistons[read.storage.index()];
                    let PistonLane::SingleResource { far } = spec.lane else {
                        return Err("logical register has unsupported storage geometry".into());
                    };
                    if contact == read.source {
                        value
                    } else if contact == spec.near || contact == far {
                        self.reduced.not(value)?
                    } else {
                        return Err("logical register read is outside its storage lane".into());
                    }
                } else if let Some((origin, actors)) = self.cells.get(&resource).cloned() {
                    if origin == contact {
                        let values = actors.into_iter().map(|actor| self.powered(actor)).collect::<Result<Vec<_>, _>>()?;
                        self.reduced.all(values)?
                    } else {
                        if self.retain_classic {
                            return Err("logical cell destination requires an explicit transfer equation".into());
                        }
                        self.reduced.constant(false)?
                    }
                } else {
                    return if self.retain_classic {
                        self.leaf(source)
                    } else {
                        Err(format!("unclassified logical resource {resource:?}").into())
                    };
                }
            }
            LayerInputSource::State(StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor)) => {
                if let Some(read) = self.registers.values().find(|read| read.storage == actor).copied() {
                    let value = self.leaf(LayerInputSource::State(StateTest::ResourceAt {
                        resource: read.resource,
                        contact: read.source,
                    }))?;
                    if matches!(self.graph.pistons[actor.index()].lane, PistonLane::SingleResource { far } if far == read.source) {
                        value
                    } else {
                        self.reduced.not(value)?
                    }
                } else if self.actors.contains(&actor) {
                    self.reduced.constant(true)?
                } else {
                    return if self.retain_classic {
                        self.leaf(source)
                    } else {
                        Err(format!("unclassified logical actor {actor:?}").into())
                    };
                }
            }
            LayerInputSource::SourceAbove { site, port: _, threshold } => {
                let SiteSource::Component(node) = self.graph.sites[site.index()].source else {
                    return Err("logical input is not a component".into());
                };
                let kind = &self.graph.nodes[node.index()].kind;
                let binary_source = matches!(
                    kind,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                        | CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Torch | NodeType::Observer { .. })
                        | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                );
                let can_sample_binary_source = binary_source
                    && (self.retain_classic
                        || matches!(
                            kind,
                            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                        ));
                let retained_state_source = matches!(
                    kind,
                    CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                        | CircuitNodeKind::Electrical(
                            NodeType::Repeater { .. } | NodeType::Torch | NodeType::Comparator { .. } | NodeType::Observer { .. }
                        )
                );
                // These equations describe the settled computation, after private
                // reset pulses. Observable reset effects remain in retained_effects.
                if threshold >= 15 || self.resets.contains(&node) {
                    self.reduced.constant(false)?
                } else if self.inline_repeaters && matches!(kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) {
                    if self.stateful_repeaters.contains(&node) {
                        self.binary_source_leaf(source)?
                    } else {
                        self.input_power(node, InputPort::Main)?
                    }
                } else if can_sample_binary_source {
                    // Direction was checked by SignalTracer before it emitted this
                    // boundary. These source classes emit only 0 or 15, so all
                    // positive thresholds and directional views share one bit.
                    self.leaf(LayerInputSource::SourceAbove {
                        site,
                        port: PowerPort::Output,
                        threshold: 0,
                    })?
                } else if self.retain_classic && retained_state_source {
                    self.leaf(source)?
                } else {
                    match self.graph.nodes[node.index()].kind {
                        CircuitNodeKind::Electrical(NodeType::Constant) => {
                            let ComponentState::Electrical(ref state) = self.graph.nodes[node.index()].state else {
                                return Err("constant source has no electrical state".into());
                            };
                            self.reduced.constant(state.output_strength > threshold)?
                        }
                        CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => self.leaf(source)?,
                        CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp) => self.input_power(node, InputPort::Main)?,
                        CircuitNodeKind::Electrical(NodeType::Torch) => {
                            let power = self.input_power(node, InputPort::Main)?;
                            self.reduced.not(power)?
                        }
                        ref kind => return Err(format!("unsupported logical source {node:?}: {kind:?}").into()),
                    }
                }
            }
            other => return Err(format!("unsupported logical input {other:?}").into()),
        };
        Ok(value)
    }
}

/// A contract proves that an accepted pull disables the payload at Near until
/// its matching delayed restoration. Only these exact positive state pairs
/// can be folded by the electrical guard tracer at a settled logical cut.
fn disabled_destinations(graph: &CompileGraph, catalog: &BoundaryCatalog) -> FxHashSet<(ResourceId, ContactId)> {
    let mut result = FxHashSet::default();
    for cell in &catalog.cells {
        for resource in &cell.resources {
            for &actor in &resource.owners {
                let Some(use_) = cell.uses.iter().find(|use_| use_.actor == actor) else {
                    continue;
                };
                let Some(transfer) = use_.retraction.transfer else {
                    continue;
                };
                let Some(wake) = use_.retraction.wake.resource else {
                    continue;
                };
                let reset_witness = match &use_.reset {
                    CellReset::Observer { wake, .. } => !wake.on_base_changed.is_empty() && !wake.on_base_restored.is_empty(),
                    CellReset::Payload { .. } | CellReset::InputRestoration { .. } => true,
                };
                let actor_spec = graph.pistons[actor.index()];
                if reset_witness
                    && transfer.resource == resource.resource
                    && transfer.source == resource.source
                    && transfer.destination == actor_spec.near
                    && transfer.destination != transfer.source
                    && transfer.when_at_source == resource.present
                    && resource.owners.contains(&actor)
                    && wake.resource == transfer.resource
                    && wake.contact == transfer.destination
                    && use_.retraction.wake.ticks_after_acceptance > 0
                    && actor_spec.routes.resource_restored_near == Some(wake.route)
                {
                    result.insert((transfer.resource, transfer.destination));
                }
            }
        }
    }
    result
}

#[cfg(test)]
fn equation_inputs(builder: &LayerBuilder<'_>, root: LayerExprId) -> FxHashSet<LayerInputSource> {
    let mut inputs = FxHashSet::default();
    let mut visited = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        match &builder.expressions[id.0 as usize] {
            LayerExpr::Input(input) => {
                inputs.insert(builder.inputs[input.0 as usize].source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
            LayerExpr::Constant(_) => {}
        }
    }
    inputs
}
