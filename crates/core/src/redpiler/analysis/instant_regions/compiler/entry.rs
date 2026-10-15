//! A parameterized, stationary entry relation; internal dust strengths are derived, not inputs.
//! Admission must check the complete projected state and empty work, as well as these clauses.
use super::*;
use crate::redpiler::components::NodeType;
use mchprs_blocks::BlockPos;
mod shapes;
pub(crate) use crate::redpiler::program_graph::WireShapeMemory as ShapeMemory;

pub(crate) use crate::redpiler::program_graph::InstantInput as Input;

pub(crate) struct Entry {
    /// These typed reads supply symbolic variables in their declared decision order.
    pub(crate) inputs: Vec<Input>,
    /// These local codebooks preserve legal shape histories without independent side-bit combinations.
    pub(crate) shapes: Vec<ShapeMemory>,
    /// These independent admission clauses exclude invalid occupancy, shapes and non-quiescent dust.
    pub(crate) conditions: Vec<Formula>,
}

impl Entry {
    pub(crate) fn read_inputs(&self, graph: &CompileGraph, state: &CircuitState) -> Option<Vec<bool>> {
        self.inputs.iter().map(|input| input.read_draft(graph, state, &self.shapes)).collect()
    }
    pub(crate) fn derive(compiler: &mut Compiler<'_>, activation: NodeId) -> Result<Self> {
        let graph = compiler.graph;
        if !graph
            .nodes
            .get(activation.index())
            .is_some_and(|node| node.kind == CircuitNodeKind::Electrical(NodeType::Lever))
        {
            return Err(CompileError::UnsupportedNode(activation));
        }
        let shapes = shapes::infer(compiler)?;
        let inputs = Self::ordered_inputs(graph, &shapes, activation)?;
        let mut values = FxHashMap::default();
        let mut resource_bits: FxHashMap<ResourceId, Vec<(u8, Formula)>> = FxHashMap::default();
        for (ordinal, &input) in inputs.iter().enumerate() {
            let value = compiler.logic.input(ordinal)?;
            values.insert(input, value);
            match input {
                Input::Powered(node) => {
                    compiler.state.components[node.index()].powered = value;
                    if !matches!(
                        graph.nodes[node.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::Comparator { .. } | NodeType::Hopper | NodeType::Lamp)
                    ) {
                        compiler.state.components[node.index()].signal =
                            compiler.logic.signal_select(value, Signal::literal(15)?, Signal::literal(0)?)?;
                    }
                }
                Input::Locked(node) => compiler.state.components[node.index()].locked = value,
                Input::SignalBit { node, bit } => {
                    let mut bits = compiler.state.components[node.index()].signal.bits();
                    bits[bit as usize] = value;
                    compiler.state.components[node.index()].signal = Signal::from_bits(bits);
                }
                Input::Extended(actor) => {
                    let state = &mut compiler.state.actors[actor.index()];
                    state.extended = value;
                    state.head = value;
                    state.availability = [Formula::TRUE, Formula::FALSE, Formula::FALSE, Formula::FALSE, Formula::FALSE];
                }
                Input::ResourceBit { resource, bit } => resource_bits.entry(resource).or_default().push((bit, value)),
                Input::ShapeBit { .. } => {}
            }
        }
        compiler.state.components[activation.index()].powered = Formula::TRUE;
        compiler.state.components[activation.index()].signal = Signal::literal(15)?;
        let mut conditions = Vec::new();
        for (index, state) in compiler.state.resources.iter_mut().enumerate() {
            let resource = ResourceId::new(index as u32);
            let mut allowed = Formula::FALSE;
            for (ordinal, (_, at)) in state.contacts.iter_mut().enumerate() {
                let mut selected = Formula::TRUE;
                for &(bit, value) in resource_bits.get(&resource).into_iter().flatten() {
                    let matches = if ordinal & (1 << bit) != 0 { value } else { compiler.logic.not(value)? };
                    selected = compiler.logic.and(selected, matches)?;
                }
                *at = selected;
                allowed = compiler.logic.or(allowed, selected)?;
            }
            state.enabled = Formula::TRUE;
            if allowed != Formula::TRUE {
                conditions.push(allowed);
            }
        }
        for (index, node) in graph.nodes.iter().enumerate() {
            if matches!(node.kind, CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }) {
                compiler.state.components[index].powered = Formula::FALSE;
                compiler.state.components[index].signal = Signal::literal(0)?;
            }
        }
        compiler.state_written();
        Self::occupancy(compiler, &mut conditions)?;
        for (index, memory) in shapes.iter().enumerate() {
            let mut shape = WireShape::literal(RedstoneWire::default());
            let mut allowed = Formula::FALSE;
            for (ordinal, sides) in memory.values.iter().enumerate() {
                let mut selected = Formula::TRUE;
                for bit in 0..ordinal_bits(memory.values.len()) {
                    let value = values[&Input::ShapeBit { memory: index as u32, bit }];
                    let matches = if ordinal & (1 << bit) != 0 { value } else { compiler.logic.not(value)? };
                    selected = compiler.logic.and(selected, matches)?;
                }
                allowed = compiler.logic.or(allowed, selected)?;
                for (target, &side) in shape.sides.iter_mut().zip(sides) {
                    if side == RedstoneWireSide::Up {
                        target.up = compiler.logic.or(target.up, selected)?;
                    }
                    if side == RedstoneWireSide::Side {
                        target.flat = compiler.logic.or(target.flat, selected)?;
                    }
                }
            }
            if allowed != Formula::TRUE {
                conditions.push(allowed);
            }
            compiler.state.components[memory.node.index()].shape = Some(shape);
            compiler.state.components[memory.node.index()].signal = Signal::literal(0)?;
        }
        compiler.state_written();
        Self::derive_steady_electrical(compiler, &mut conditions)?;
        Ok(Self { inputs, shapes, conditions })
    }

    fn settle_wires(compiler: &mut Compiler<'_>) -> Result<()> {
        let graph = compiler.graph;
        // Every dust hop attenuates by one, so fifteen synchronous rounds cover every nonzero source path.
        for _ in 0..15 {
            let next = graph
                .wires
                .iter()
                .map(|wire| compiler.seed_power(wire.node))
                .collect::<Result<Vec<_>>>()?;
            for (wire, power) in graph.wires.iter().zip(next) {
                compiler.state.components[wire.node.index()].signal = power;
            }
            compiler.state_written();
        }
        Ok(())
    }

    fn derive_steady_electrical(compiler: &mut Compiler<'_>, conditions: &mut Vec<Formula>) -> Result<()> {
        use super::execution::Execution;
        let graph = compiler.graph;
        let nodes: Vec<_> = graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(
                        NodeType::Repeater { .. } | NodeType::Torch | NodeType::Comparator { .. } | NodeType::Lamp | NodeType::Hopper
                    )
                )
                .then_some(NodeId::new(index as u32))
            })
            .collect();
        // QC-only actors retain their explicit BUD memory; only main-driven actors use this entry family.
        let actors: Vec<_> = graph
            .pistons
            .iter()
            .enumerate()
            .filter_map(|(index, actor)| {
                graph
                    .power_links
                    .iter()
                    .any(|link| {
                        link.to
                            == PowerTarget::Input {
                                node: actor.node,
                                port: InputPort::Main,
                            }
                    })
                    .then_some(PistonId::new(index as u32))
            })
            .collect();
        let ready = Execution::empty(graph);
        // An acyclic chain settles in its component count; feedback keeps symbolic memory and explicit equations.
        for _ in 0..=nodes.len() + actors.len() {
            Self::settle_wires(compiler)?;
            let next = nodes
                .iter()
                .map(|&node| Execution::steady_component(compiler, node).map(|value| value.expect("ordinary electrical entry node")))
                .collect::<Result<Vec<_>>>()?;
            let poses = actors
                .iter()
                .map(|&actor| ready.steady_actor_pose(compiler, actor))
                .collect::<Result<Vec<_>>>()?;
            let changed = nodes
                .iter()
                .zip(&next)
                .any(|(node, value)| compiler.state.components[node.index()] != *value)
                || actors
                    .iter()
                    .zip(&poses)
                    .any(|(actor, &pose)| compiler.state.actors[actor.index()].extended != pose);
            for (&node, value) in nodes.iter().zip(next) {
                compiler.state.components[node.index()] = value;
            }
            for (&actor, pose) in actors.iter().zip(poses) {
                compiler.state.actors[actor.index()].extended = pose;
                compiler.state.actors[actor.index()].head = pose;
            }
            let resources_changed = Self::resolve_resources(compiler, conditions)?;
            if !changed && !resources_changed {
                break;
            }
            compiler.state_written();
        }
        Self::settle_wires(compiler)?;
        for &actor in &actors {
            let pose = ready.steady_actor_pose(compiler, actor)?;
            let differs = compiler.logic.xor(compiler.state.actors[actor.index()].extended, pose)?;
            let stable = compiler.logic.not(differs)?;
            if stable != Formula::TRUE {
                conditions.push(stable);
            }
        }
        for &node in &nodes {
            let next = Execution::steady_component(compiler, node)?.unwrap();
            let current = &compiler.state.components[node.index()];
            let same_signal = compiler.logic.signal_equal(current.signal, next.signal)?;
            let power_differs = compiler.logic.xor(current.powered, next.powered)?;
            let same_power = compiler.logic.not(power_differs)?;
            let lock_differs = compiler.logic.xor(current.locked, next.locked)?;
            let same_lock = compiler.logic.not(lock_differs)?;
            for same in [same_signal, same_power, same_lock] {
                if same != Formula::TRUE {
                    conditions.push(same);
                }
            }
        }
        for wire in &graph.wires {
            let power = compiler.seed_power(wire.node)?;
            let stable = compiler.logic.signal_equal(power, compiler.state.components[wire.node.index()].signal)?;
            if stable != Formula::TRUE {
                conditions.push(stable);
            }
        }
        Ok(())
    }

    fn resolve_resources(compiler: &mut Compiler<'_>, conditions: &mut Vec<Formula>) -> Result<bool> {
        let graph = compiler.graph;
        let mut changed = false;
        for resource in &mut compiler.state.resources {
            let possible: Vec<_> = resource
                .contacts
                .iter()
                .filter_map(|&(contact, _)| {
                    let blocked = graph
                        .pistons
                        .iter()
                        .zip(&compiler.state.actors)
                        .any(|(actor, state)| actor.base == contact || (actor.near == contact && state.head == Formula::TRUE));
                    (!blocked).then_some(contact)
                })
                .collect();
            let [only] = possible.as_slice() else { continue };
            if resource.at(*only) == Formula::TRUE {
                continue;
            }
            // A conserved payload with one unblocked contact has no independent position memory.
            conditions.push(resource.at(*only));
            for (contact, at) in &mut resource.contacts {
                *at = if contact == only { Formula::TRUE } else { Formula::FALSE };
            }
            changed = true;
        }
        Ok(changed)
    }

    fn ordered_inputs(graph: &CompileGraph, shapes: &[ShapeMemory], activation: NodeId) -> Result<Vec<Input>> {
        let mut inputs = Vec::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            let id = NodeId::new(index as u32);
            if id == activation {
                continue;
            }
            let position = node.position;
            let mut push = |input| inputs.push((position.expect("electrical nodes have positions"), input));
            match node.kind {
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::PressurePlate | NodeType::Torch | NodeType::Lamp | NodeType::Hopper) => {
                    push(Input::Powered(id))
                }
                CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => {
                    push(Input::Powered(id));
                    push(Input::Locked(id));
                }
                CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => {
                    push(Input::Powered(id));
                    for bit in 0..4 {
                        push(Input::SignalBit { node: id, bit });
                    }
                }
                CircuitNodeKind::Wire(_) => {}
                CircuitNodeKind::Observer { .. }
                | CircuitNodeKind::BuiltinReset { .. }
                | CircuitNodeKind::Piston(_)
                | CircuitNodeKind::Contact(_)
                | CircuitNodeKind::Electrical(NodeType::Constant) => {}
                _ => return Err(CompileError::UnsupportedNode(id)),
            }
        }
        for (index, memory) in shapes.iter().enumerate() {
            let position = graph.nodes[memory.node.index()].position.expect("wire positions were validated");
            for bit in 0..ordinal_bits(memory.values.len()) {
                inputs.push((position, Input::ShapeBit { memory: index as u32, bit }));
            }
        }
        for (index, actor) in graph.pistons.iter().enumerate() {
            inputs.push((actor.position, Input::Extended(PistonId::new(index as u32))));
        }
        for (index, resource) in graph.resources.iter().enumerate() {
            let count = resource.contacts.len();
            let bits = usize::BITS - count.saturating_sub(1).leading_zeros();
            let position = graph.contacts[resource.contacts[0].index()].position;
            for bit in 0..bits {
                inputs.push((
                    position,
                    Input::ResourceBit {
                        resource: ResourceId::new(index as u32),
                        bit: bit as u8,
                    },
                ));
            }
        }
        if let Some(&(first, _)) = inputs.first() {
            let (low, high) = inputs
                .iter()
                .fold((first, first), |(low, high), (position, _)| (low.min(*position), high.max(*position)));
            let spread = high - low;
            let axis = [(spread.x, 0), (spread.y, 1), (spread.z, 2)].into_iter().max().unwrap().1;
            let key = |position: BlockPos| match axis {
                0 => (position.x, position.y, position.z),
                1 => (position.y, position.z, position.x),
                _ => (position.z, position.x, position.y),
            };
            // Neighboring state and data variables stay adjacent; ordering changes cost, never semantics.
            inputs.sort_by_key(|(position, _)| key(*position));
        }
        Ok(inputs.into_iter().map(|(_, input)| input).collect())
    }

    fn occupancy(compiler: &mut Compiler<'_>, conditions: &mut Vec<Formula>) -> Result<()> {
        let graph = compiler.graph;
        for (index, actor) in graph.pistons.iter().enumerate() {
            let head = compiler.state.actors[index].head;
            for resource in &compiler.state.resources {
                let head_overlap = compiler.logic.and(head, resource.at(actor.near))?;
                let overlap = compiler.logic.or(head_overlap, resource.at(actor.base))?;
                let clear = compiler.logic.not(overlap)?;
                if clear != Formula::TRUE {
                    conditions.push(clear);
                }
            }
        }
        for (index, resource) in compiler.state.resources.iter().enumerate() {
            for other in &compiler.state.resources[..index] {
                for &(contact, at) in &resource.contacts {
                    let overlap = compiler.logic.and(at, other.at(contact))?;
                    let clear = compiler.logic.not(overlap)?;
                    if clear != Formula::TRUE {
                        conditions.push(clear);
                    }
                }
            }
        }
        Ok(())
    }
}

fn ordinal_bits(count: usize) -> u8 {
    (usize::BITS - count.saturating_sub(1).leading_zeros()) as u8
}
