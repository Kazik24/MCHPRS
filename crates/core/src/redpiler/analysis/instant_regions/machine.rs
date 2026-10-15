//! Ahead-of-time partial evaluation of small, closed instant selections.
//! Every retained edge is an exact ordered Graph transition; missing edges do not speculate.
use super::*;
use crate::redpiler::backend::graph::GraphBackend;
use crate::redpiler::components::NodeType;
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::sync::Arc;

const MAX_STATES: usize = 2048;
const MAX_BYTES: usize = 16 * 1024 * 1024;

fn controls(graph: &CompileGraph) -> Option<Vec<NodeId>> {
    let [region] = graph.instant_regions.as_slice() else {
        return None;
    };
    if !region.stable_inputs
        || graph.nodes.len() > 128
        || graph.pistons.is_empty()
        || graph.pistons.len() > 16
        || region.actors.len() != graph.pistons.len()
        || region.resources.len() != graph.resources.len()
        || region.contacts.len() != graph.contacts.len()
        || region.wires.len() != graph.wires.len()
    {
        return None;
    }
    let mut controls = Vec::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        match node.kind {
            CircuitNodeKind::Electrical(NodeType::Lever) => controls.push(NodeId::new(index as u32)),
            CircuitNodeKind::Electrical(NodeType::Constant | NodeType::Repeater { .. })
            | CircuitNodeKind::BuiltinReset { .. }
            | CircuitNodeKind::Wire(_)
            | CircuitNodeKind::Piston(_)
            | CircuitNodeKind::Contact(_) => {}
            // No host effects, outside observers, analog feedback or independently changing controls.
            _ => return None,
        }
    }
    (1..=4).contains(&controls.len()).then_some(controls)
}

fn evaluator(graph: &CompileGraph) -> Result<GraphBackend, String> {
    let mut base = graph.clone();
    base.instant_machine = None;
    let (program, state) = base.lower().map_err(|e| e.to_string())?;
    GraphBackend::new(program, state, crate::redpiler::backend::graph::MACHINE_EPOCH).map_err(|e| e.to_string())
}

pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Option<Arc<InstantMachine>>, String> {
    let Some(controls) = controls(graph) else {
        return Ok(None);
    };
    let mut sandbox = evaluator(graph)?;
    let mut initial = MachineState::default();
    if !sandbox.capture_machine_state(&mut initial) {
        return Ok(None);
    }
    let mut machine = InstantMachine {
        controls,
        states: vec![initial.clone()],
        transitions: Vec::new(),
        index: FxHashMap::default(),
        kernel: empty_kernel(),
    };
    machine.index.insert(initial, 0);
    machine.transitions.push(vec![None; machine.controls.len() + 1]);
    // Spend the bounded state budget on held-input histories; unknown histories stay general.
    // Repetition detects a periodic endpoint; no guessed "reset after N ticks" is used.
    for mask in 0..(1usize << machine.controls.len()) {
        for reverse in [false, true] {
            check_cancelled(monitor)?;
            let mut source = 0;
            for offset in 0..machine.controls.len() {
                let control = if reverse { machine.controls.len() - 1 - offset } else { offset };
                if mask & (1 << control) != 0 {
                    let Some(next) = evaluate(&mut machine, &mut sandbox, source, control + 1) else {
                        break;
                    };
                    source = next;
                }
            }
            let endpoint = follow_ticks(&mut machine, &mut sandbox, source, monitor)?;
            for action in 1..=machine.controls.len() {
                if let Some(next) = evaluate(&mut machine, &mut sandbox, endpoint, action) {
                    follow_ticks(&mut machine, &mut sandbox, next, monitor)?;
                }
            }
        }
    }
    if machine.estimated_bytes() > MAX_BYTES {
        return Ok(None);
    }
    let Some(kernel) = compile_kernel(graph, &machine.controls, monitor)? else {
        return Ok(None);
    };
    machine.kernel = kernel;
    if machine.estimated_bytes() > MAX_BYTES {
        return Ok(None);
    }
    check_cancelled(monitor)?;
    Ok(Some(Arc::new(machine)))
}

fn evaluate(machine: &mut InstantMachine, sandbox: &mut GraphBackend, source: usize, action: usize) -> Option<usize> {
    if let Some(edge) = &machine.transitions[source][action] {
        return Some(edge.next as usize);
    }
    let (state, mut edge) = sandbox.evaluate_machine_step(&machine.states[source], &machine.controls, action).ok()?;
    let next = if let Some(&existing) = machine.index.get(&state) {
        existing
    } else {
        if machine.states.len() == MAX_STATES {
            return None;
        }
        let next = machine.states.len() as u32;
        machine.index.insert(state.clone(), next);
        machine.states.push(state);
        machine.transitions.push(vec![None; machine.controls.len() + 1]);
        next
    };
    edge.next = next;
    machine.transitions[source][action] = Some(edge);
    Some(next as usize)
}

fn follow_ticks(machine: &mut InstantMachine, sandbox: &mut GraphBackend, mut source: usize, monitor: &TaskMonitor) -> Result<usize, String> {
    let mut seen = std::collections::HashSet::new();
    while seen.insert(source) {
        check_cancelled(monitor)?;
        let Some(next) = evaluate(machine, sandbox, source, 0) else { break };
        source = next;
    }
    Ok(source)
}

const MAX_KERNEL_PHASES: usize = 128;
const MAX_KERNEL_PREP_TICKS: usize = 256;
const NO_KERNEL_VALUE: u64 = u64::MAX;

#[derive(Default)]
struct ExpressionBuilder {
    /// This hash-conses identical scalar leaves across every hot and cold output root.
    nodes: Vec<KernelExpr>,
    /// This maps each scalar leaf to its shared DAG identifier.
    constants: HashMap<u64, KernelExprId>,
    /// This maps each ordered control branch to its shared DAG identifier.
    selects: HashMap<(u8, KernelExprId, KernelExprId), KernelExprId>,
}

impl ExpressionBuilder {
    fn constant(&mut self, value: u64) -> KernelExprId {
        if let Some(&id) = self.constants.get(&value) {
            return id;
        }
        let id = KernelExprId(self.nodes.len() as u32);
        self.nodes.push(KernelExpr::Constant(value));
        self.constants.insert(value, id);
        id
    }

    fn values(&mut self, values: &[u64], controls: usize) -> KernelExprId {
        let masks: Vec<_> = (0..values.len()).map(|mask| mask as u8).collect();
        self.branch(values, &masks, controls, 0)
    }

    fn branch(&mut self, values: &[u64], masks: &[u8], controls: usize, control: usize) -> KernelExprId {
        if let Some((&first, rest)) = values.split_first()
            && rest.iter().all(|value| *value == first)
        {
            return self.constant(first);
        }
        if control == controls {
            return self.constant(values[0]);
        }
        let bit = 1 << control;
        let mut low_values = Vec::new();
        let mut low_masks = Vec::new();
        let mut high_values = Vec::new();
        let mut high_masks = Vec::new();
        for (&mask, &value) in masks.iter().zip(values) {
            if mask & bit == 0 {
                low_masks.push(mask);
                low_values.push(value);
            } else {
                high_masks.push(mask);
                high_values.push(value);
            }
        }
        let low = self.branch(&low_values, &low_masks, controls, control + 1);
        let high = self.branch(&high_values, &high_masks, controls, control + 1);
        if low == high {
            return low;
        }
        let key = (control as u8, low, high);
        if let Some(&id) = self.selects.get(&key) {
            return id;
        }
        let id = KernelExprId(self.nodes.len() as u32);
        self.nodes.push(KernelExpr::Select {
            control: control as u8,
            low,
            high,
        });
        self.selects.insert(key, id);
        id
    }
}

struct Trajectory {
    /// These exact states end at the first state already seen in the same trajectory.
    states: Vec<MachineState>,
    /// These exact ordered transitions connect adjacent states and close the cycle.
    edges: Vec<MachineTransition>,
    /// This is the first state in the repeated periodic suffix.
    prefix: usize,
    /// This is the exact repeated period in ticks.
    period: usize,
}

impl Trajectory {
    fn state_at(&self, tick: usize) -> &MachineState {
        if tick < self.states.len() {
            &self.states[tick]
        } else {
            &self.states[self.prefix + (tick - self.prefix) % self.period]
        }
    }

    fn edge_at(&self, tick: usize) -> &MachineTransition {
        if tick < self.edges.len() {
            &self.edges[tick]
        } else {
            &self.edges[self.prefix + (tick - self.prefix) % self.period]
        }
    }
}

fn empty_kernel() -> InstantKernel {
    InstantKernel {
        trigger: NodeId::new(0),
        data_controls: Box::new([]),
        expressions: Box::new([KernelExpr::Constant(0)]),
        entry: KernelStateExpr {
            components: Box::new([]),
            pistons: Box::new([]),
            resources: Box::new([]),
            future: KernelFutureExpr {
                tick_len: KernelExprId(0),
                ticks: Box::new([]),
                request_len: KernelExprId(0),
                requests: Box::new([]),
                deferred_len: KernelExprId(0),
                deferred: Box::new([]),
            },
        },
        phases: Box::new([]),
        prefix: 0,
        period: 0,
    }
}

fn compile_kernel(graph: &CompileGraph, controls: &[NodeId], monitor: &TaskMonitor) -> Result<Option<InstantKernel>, String> {
    let [region] = graph.instant_regions.as_slice() else {
        return Ok(None);
    };
    // Whole-selection ingress is retained when its receiver is in the admitted component family.
    if !region.stable_inputs || !region.observers.is_empty() || !region.wire_readers.is_empty() {
        return Ok(None);
    }
    let (program, state) = {
        let mut base = graph.clone();
        base.instant_machine = None;
        base.lower().map_err(|error| error.to_string())?
    };
    let initial_backend = GraphBackend::new(program, state, crate::redpiler::backend::graph::MACHINE_EPOCH).map_err(|error| error.to_string())?;
    let mut initial = MachineState::default();
    if !initial_backend.capture_machine_state(&mut initial) {
        return Ok(None);
    }
    let powered: Vec<_> = controls
        .iter()
        .copied()
        .filter(|node| initial.components[node.index()] & 16 != 0)
        .collect();
    if powered.is_empty() {
        return Ok(None);
    }
    let mut admitted = None;
    for trigger in powered {
        if let Some(kernel) = compile_kernel_candidate(graph, controls, trigger, monitor)? {
            if admitted.is_some() {
                return Ok(None);
            }
            admitted = Some(kernel);
        }
    }
    Ok(admitted)
}

fn compile_kernel_candidate(
    graph: &CompileGraph,
    controls: &[NodeId],
    trigger: NodeId,
    monitor: &TaskMonitor,
) -> Result<Option<InstantKernel>, String> {
    let data_controls: Vec<_> = controls.iter().copied().filter(|&node| node != trigger).collect();
    if data_controls.len() > 3 {
        return Ok(None);
    }
    let hot_nodes = kernel_boundary(graph);
    let variants = 1usize << data_controls.len();
    let mut entries = Vec::with_capacity(variants);
    let mut trajectories = Vec::with_capacity(variants);
    for mask in 0..variants {
        check_cancelled(monitor)?;
        let Ok((forward_entry, forward_backend)) = prepare_kernel_entry(graph, &data_controls, trigger, mask as u8, false) else {
            return Ok(None);
        };
        let Ok((reverse_entry, _)) = prepare_kernel_entry(graph, &data_controls, trigger, mask as u8, true) else {
            return Ok(None);
        };
        if forward_entry != reverse_entry {
            return Ok(None);
        }
        entries.push(forward_entry.clone());
        let trajectory = match compile_trajectory(forward_backend, &forward_entry, controls, monitor) {
            Ok(trajectory) => trajectory,
            Err(error) if monitor.cancelled() => return Err(error),
            Err(_) => return Ok(None),
        };
        trajectories.push(trajectory);
    }
    let prefix = trajectories.iter().map(|trajectory| trajectory.prefix).max().unwrap_or(0);
    let period = trajectories.iter().try_fold(1usize, |period, trajectory| {
        let gcd = gcd(period, trajectory.period);
        period.checked_div(gcd)?.checked_mul(trajectory.period)
    });
    let phase_count = period
        .filter(|&period| period > 0)
        .and_then(|period| prefix.checked_add(period))
        .filter(|&count| count <= MAX_KERNEL_PHASES);
    let Some(phase_count) = phase_count else {
        return Ok(None);
    };
    let period = period.expect("a bounded common period was just checked");
    let mut expressions = ExpressionBuilder::default();
    let entry = build_state_expr(&mut expressions, &entries, data_controls.len());
    let mut phases = Vec::with_capacity(phase_count);
    for tick in 0..phase_count {
        check_cancelled(monitor)?;
        let edge_rows: Vec<_> = trajectories.iter().map(|trajectory| trajectory.edge_at(tick)).collect();
        let before_rows: Vec<_> = trajectories.iter().map(|trajectory| trajectory.state_at(tick)).collect();
        let after_rows: Vec<_> = trajectories.iter().map(|trajectory| trajectory.state_at(tick + 1)).collect();
        if edge_rows.iter().any(|edge| edge.accepted != edge.effects.len() as u64) {
            return Ok(None);
        }
        phases.push(build_phase(
            &mut expressions,
            data_controls.len(),
            &edge_rows,
            &before_rows,
            &after_rows,
            &hot_nodes,
        ));
    }
    Ok(Some(InstantKernel {
        trigger,
        data_controls: data_controls.into_boxed_slice(),
        expressions: expressions.nodes.into_boxed_slice(),
        entry,
        phases: phases.into_boxed_slice(),
        prefix: prefix as u16,
        period: period as u16,
    }))
}

fn prepare_kernel_entry(
    graph: &CompileGraph,
    controls: &[NodeId],
    trigger: NodeId,
    mask: u8,
    reverse: bool,
) -> Result<(MachineState, GraphBackend), String> {
    let mut base = graph.clone();
    base.instant_machine = None;
    let (program, state) = base.lower().map_err(|error| error.to_string())?;
    let mut backend = GraphBackend::new(program, state, crate::redpiler::backend::graph::MACHINE_EPOCH).map_err(|error| error.to_string())?;
    let mut initial = MachineState::default();
    if !backend.capture_machine_state(&mut initial) {
        return Err("cannot capture canonical kernel start".into());
    }
    let order: Box<dyn Iterator<Item = usize>> = if reverse {
        Box::new((0..controls.len()).rev())
    } else {
        Box::new(0..controls.len())
    };
    for index in order {
        let node = controls[index];
        let desired = mask & (1 << index) != 0;
        let powered = initial.components[node.index()] & 16 != 0;
        if powered != desired {
            backend
                .use_block(graph.nodes[node.index()].position.ok_or("control has no graph position")?)
                .map_err(|error| error.to_string())?;
        }
    }
    let mut seen = rustc_hash::FxHashSet::default();
    let entry = loop {
        let mut current = MachineState::default();
        if !backend.capture_machine_state(&mut current) {
            return Err("canonical input preparation leaves the certified state bounds".into());
        }
        if quiescent_entry(&current) {
            break current;
        }
        if !seen.insert(current) || seen.len() > MAX_KERNEL_PREP_TICKS {
            return Err("canonical input preparation did not reach an exact quiescent entry".into());
        }
        backend.tick().map_err(|error| error.to_string())?;
    };
    if entry.components[trigger.index()] & 16 == 0 {
        return Err("graph-derived activation control is not initially powered".into());
    }
    backend
        .use_block(graph.nodes[trigger.index()].position.ok_or("trigger has no graph position")?)
        .map_err(|error| error.to_string())?;
    let mut activated = MachineState::default();
    if !backend.capture_machine_state(&mut activated) {
        return Err("activation leaves the certified state bounds".into());
    }
    Ok((activated, backend))
}

fn quiescent_entry(state: &MachineState) -> bool {
    state.ticks.is_empty() && state.requests.is_empty() && state.deferred.is_empty() && state.pistons.iter().all(|pose| pose >> 2 == 0)
}

fn compile_trajectory(mut backend: GraphBackend, entry: &MachineState, controls: &[NodeId], monitor: &TaskMonitor) -> Result<Trajectory, String> {
    let mut states = vec![entry.clone()];
    let mut edges = Vec::new();
    let mut seen = HashMap::from([(entry.clone(), 0usize)]);
    loop {
        check_cancelled(monitor)?;
        if states.len() > 2048 {
            return Err("canonical trajectory did not repeat within the proof bound".into());
        }
        let (next, edge) = backend
            .evaluate_machine_step(states.last().unwrap(), controls, 0)
            .map_err(|error| error.to_string())?;
        edges.push(edge);
        if let Some(&prefix) = seen.get(&next) {
            return Ok(Trajectory {
                period: states.len() - prefix,
                states,
                edges,
                prefix,
            });
        }
        seen.insert(next.clone(), states.len());
        states.push(next);
    }
}

fn kernel_boundary(graph: &CompileGraph) -> Vec<NodeId> {
    // Whole-inventory ownership and region analyses rule out independent readers.
    // Repeater outputs are the electrical boundary; their sampled wires stay cold
    // because the program also compiles every selected repeater transition.
    graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| matches!(node.kind, CircuitNodeKind::Electrical(_)).then_some(NodeId::new(index as u32)))
        .collect()
}

fn build_state_expr(builder: &mut ExpressionBuilder, states: &[MachineState], controls: usize) -> KernelStateExpr {
    let components = (0..states[0].components.len())
        .map(|index| {
            builder.values(
                &states.iter().map(|state| u64::from(state.components[index])).collect::<Vec<_>>(),
                controls,
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let pistons = (0..states[0].pistons.len())
        .map(|index| builder.values(&states.iter().map(|state| u64::from(state.pistons[index])).collect::<Vec<_>>(), controls))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let resources = (0..states[0].resources.len())
        .map(|index| {
            let contacts = states
                .iter()
                .map(|state| u64::from(states_id(state.resources[index].0)))
                .collect::<Vec<_>>();
            let enabled = states.iter().map(|state| u64::from(state.resources[index].1)).collect::<Vec<_>>();
            (builder.values(&contacts, controls), builder.values(&enabled, controls))
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let future = build_future_expr(builder, states, controls);
    KernelStateExpr {
        components,
        pistons,
        resources,
        future,
    }
}

fn states_id(id: ContactId) -> u32 {
    id.index() as u32
}

fn build_future_expr(builder: &mut ExpressionBuilder, states: &[MachineState], controls: usize) -> KernelFutureExpr {
    let tick_len = builder.values(&states.iter().map(|state| state.ticks.len() as u64).collect::<Vec<_>>(), controls);
    let tick_count = states.iter().map(|state| state.ticks.len()).max().unwrap_or(0);
    let ticks = (0..tick_count)
        .map(|index| KernelTickExpr {
            node: builder.values(
                &states
                    .iter()
                    .map(|state| state.ticks.get(index).map_or(NO_KERNEL_VALUE, |tick| tick.node.index() as u64))
                    .collect::<Vec<_>>(),
                controls,
            ),
            delay: builder.values(
                &states
                    .iter()
                    .map(|state| state.ticks.get(index).map_or(NO_KERNEL_VALUE, |tick| u64::from(tick.delay)))
                    .collect::<Vec<_>>(),
                controls,
            ),
            priority: builder.values(
                &states
                    .iter()
                    .map(|state| state.ticks.get(index).map_or(NO_KERNEL_VALUE, |tick| tick.priority as u64))
                    .collect::<Vec<_>>(),
                controls,
            ),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let request_len = builder.values(&states.iter().map(|state| state.requests.len() as u64).collect::<Vec<_>>(), controls);
    let request_count = states.iter().map(|state| state.requests.len()).max().unwrap_or(0);
    let requests = (0..request_count)
        .map(|index| KernelRequestExpr {
            piston: builder.values(
                &states
                    .iter()
                    .map(|state| state.requests.get(index).map_or(NO_KERNEL_VALUE, |request| request.0.index() as u64))
                    .collect::<Vec<_>>(),
                controls,
            ),
            action: builder.values(
                &states
                    .iter()
                    .map(|state| state.requests.get(index).map_or(NO_KERNEL_VALUE, |request| request.1 as u64))
                    .collect::<Vec<_>>(),
                controls,
            ),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let deferred_len = builder.values(&states.iter().map(|state| state.deferred.len() as u64).collect::<Vec<_>>(), controls);
    let deferred_count = states.iter().map(|state| state.deferred.len()).max().unwrap_or(0);
    let deferred = (0..deferred_count)
        .map(|index| KernelDeferredExpr {
            piston: builder.values(
                &states
                    .iter()
                    .map(|state| state.deferred.get(index).map_or(NO_KERNEL_VALUE, |event| event.piston.index() as u64))
                    .collect::<Vec<_>>(),
                controls,
            ),
            revision: builder.values(
                &states
                    .iter()
                    .map(|state| state.deferred.get(index).map_or(NO_KERNEL_VALUE, |event| event.revision))
                    .collect::<Vec<_>>(),
                controls,
            ),
            resource: builder.values(
                &states
                    .iter()
                    .map(|state| {
                        state
                            .deferred
                            .get(index)
                            .and_then(|event| event.resource)
                            .map_or(NO_KERNEL_VALUE, |resource| resource.0.index() as u64)
                    })
                    .collect::<Vec<_>>(),
                controls,
            ),
            resource_revision: builder.values(
                &states
                    .iter()
                    .map(|state| {
                        state
                            .deferred
                            .get(index)
                            .and_then(|event| event.resource)
                            .map_or(NO_KERNEL_VALUE, |resource| resource.1)
                    })
                    .collect::<Vec<_>>(),
                controls,
            ),
            contact: builder.values(
                &states
                    .iter()
                    .map(|state| {
                        state
                            .deferred
                            .get(index)
                            .and_then(|event| event.resource)
                            .map_or(NO_KERNEL_VALUE, |resource| resource.2.index() as u64)
                    })
                    .collect::<Vec<_>>(),
                controls,
            ),
            delay: builder.values(
                &states
                    .iter()
                    .map(|state| state.deferred.get(index).map_or(NO_KERNEL_VALUE, |event| event.delay))
                    .collect::<Vec<_>>(),
                controls,
            ),
            extending: builder.values(
                &states
                    .iter()
                    .map(|state| state.deferred.get(index).map_or(NO_KERNEL_VALUE, |event| u64::from(event.extending)))
                    .collect::<Vec<_>>(),
                controls,
            ),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    KernelFutureExpr {
        tick_len,
        ticks,
        request_len,
        requests,
        deferred_len,
        deferred,
    }
}

fn build_phase(
    builder: &mut ExpressionBuilder,
    controls: usize,
    edges: &[&MachineTransition],
    before: &[&MachineState],
    after: &[&MachineState],
    hot_nodes: &[NodeId],
) -> KernelPhase {
    let state = build_state_expr(builder, &after.iter().map(|state| (*state).clone()).collect::<Vec<_>>(), controls);
    let components = hot_nodes
        .iter()
        .filter_map(|&node| {
            let changed: Vec<_> = before
                .iter()
                .zip(after)
                .map(|(before, after)| u64::from(before.components[node.index()] != after.components[node.index()]))
                .collect();
            changed.iter().any(|&value| value != 0).then(|| KernelComponentDelta {
                node,
                changed: builder.values(&changed, controls),
                value: builder.values(
                    &after.iter().map(|state| u64::from(state.components[node.index()])).collect::<Vec<_>>(),
                    controls,
                ),
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let dirty_nodes: rustc_hash::FxHashSet<_> = edges.iter().flat_map(|edge| edge.dirty.iter().copied()).collect();
    let mut dirty_nodes: Vec<_> = dirty_nodes.into_iter().collect();
    dirty_nodes.sort_unstable();
    let dirty = dirty_nodes
        .into_iter()
        .map(|node| {
            let values = edges.iter().map(|edge| u64::from(edge.dirty.contains(&node))).collect::<Vec<_>>();
            (node, builder.values(&values, controls))
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let piston_revisions = (0..before[0].pistons.len())
        .filter_map(|index| {
            let values = edges
                .iter()
                .map(|edge| edge.pistons.iter().find(|entry| entry.0.index() == index).map_or(0, |entry| entry.2))
                .collect::<Vec<_>>();
            values
                .iter()
                .any(|&value| value != 0)
                .then(|| (PistonId::new(index as u32), builder.values(&values, controls)))
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let resource_revisions = (0..before[0].resources.len())
        .filter_map(|index| {
            let values = edges
                .iter()
                .map(|edge| edge.resources.iter().find(|entry| entry.0.index() == index).map_or(0, |entry| entry.3))
                .collect::<Vec<_>>();
            values
                .iter()
                .any(|&value| value != 0)
                .then(|| (ResourceId::new(index as u32), builder.values(&values, controls)))
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut effects = Vec::new();
    for (mask, edge) in edges.iter().enumerate() {
        assert_eq!(
            edge.accepted,
            edge.effects.len() as u64,
            "kernel phase compilation requires one captured piston presentation per accepted action"
        );
        let guard_values: Vec<_> = (0..edges.len()).map(|candidate| u64::from(candidate == mask)).collect();
        let guard = builder.values(&guard_values, controls);
        for &(piston, action, extended) in &edge.effects {
            effects.push(KernelEffect {
                enabled: guard,
                piston,
                action,
                extended,
            });
        }
    }
    let scheduled = builder.values(&edges.iter().map(|edge| edge.scheduled).collect::<Vec<_>>(), controls);
    let accepted = builder.values(&edges.iter().map(|edge| edge.accepted).collect::<Vec<_>>(), controls);
    let published = builder.values(&edges.iter().map(|edge| edge.published).collect::<Vec<_>>(), controls);
    KernelPhase {
        components,
        dirty,
        piston_revisions,
        resource_revisions,
        effects: effects.into_boxed_slice(),
        scheduled,
        accepted,
        published,
        state,
    }
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn kernel_state_roots_valid(state: &KernelStateExpr, graph: &CompileGraph, expression_count: usize) -> bool {
    let root_exists = |root: KernelExprId| (root.0 as usize) < expression_count;
    state.components.len() == graph.nodes.len()
        && state.pistons.len() == graph.pistons.len()
        && state.resources.len() == graph.resources.len()
        && state.future.ticks.len() <= 128
        && state.future.requests.len() <= 64
        && state.future.deferred.len() <= 128
        && state.components.iter().copied().all(root_exists)
        && state.pistons.iter().copied().all(root_exists)
        && state
            .resources
            .iter()
            .all(|&(contact, enabled)| root_exists(contact) && root_exists(enabled))
        && root_exists(state.future.tick_len)
        && state
            .future
            .ticks
            .iter()
            .all(|tick| root_exists(tick.node) && root_exists(tick.delay) && root_exists(tick.priority))
        && root_exists(state.future.request_len)
        && state
            .future
            .requests
            .iter()
            .all(|request| root_exists(request.piston) && root_exists(request.action))
        && root_exists(state.future.deferred_len)
        && state.future.deferred.iter().all(|event| {
            root_exists(event.piston)
                && root_exists(event.revision)
                && root_exists(event.resource)
                && root_exists(event.resource_revision)
                && root_exists(event.contact)
                && root_exists(event.delay)
                && root_exists(event.extending)
        })
}

fn kernel_shape_valid(graph: &CompileGraph, controls: &[NodeId], kernel: &InstantKernel) -> bool {
    let phase_count = usize::from(kernel.prefix).checked_add(usize::from(kernel.period));
    let expected_data: Vec<_> = controls.iter().copied().filter(|&control| control != kernel.trigger).collect();
    if !controls.contains(&kernel.trigger)
        || kernel.data_controls.as_ref() != expected_data
        || kernel.data_controls.len() > 3
        || kernel.period == 0
        || phase_count != Some(kernel.phases.len())
        || kernel.phases.len() > MAX_KERNEL_PHASES
        || kernel.expressions.is_empty()
        || !kernel_state_roots_valid(&kernel.entry, graph, kernel.expressions.len())
    {
        return false;
    }
    for (index, expression) in kernel.expressions.iter().enumerate() {
        if let KernelExpr::Select { control, low, high } = *expression
            && (usize::from(control) >= kernel.data_controls.len() || low.0 as usize >= index || high.0 as usize >= index)
        {
            return false;
        }
    }
    let root_exists = |root: KernelExprId| (root.0 as usize) < kernel.expressions.len();
    for phase in kernel.phases.iter() {
        if !kernel_state_roots_valid(&phase.state, graph, kernel.expressions.len())
            || !root_exists(phase.scheduled)
            || !root_exists(phase.accepted)
            || !root_exists(phase.published)
            || phase.components.iter().any(|delta| {
                delta.node.index() >= graph.nodes.len()
                    || !matches!(graph.nodes[delta.node.index()].kind, CircuitNodeKind::Electrical(_))
                    || !root_exists(delta.changed)
                    || !root_exists(delta.value)
            })
            || phase
                .dirty
                .iter()
                .any(|(node, changed)| node.index() >= graph.nodes.len() || !root_exists(*changed))
            || phase
                .piston_revisions
                .iter()
                .any(|(piston, delta)| piston.index() >= graph.pistons.len() || !root_exists(*delta))
            || phase
                .resource_revisions
                .iter()
                .any(|(resource, delta)| resource.index() >= graph.resources.len() || !root_exists(*delta))
            || phase
                .effects
                .iter()
                .any(|effect| effect.piston.index() >= graph.pistons.len() || !root_exists(effect.enabled))
            || !phase.components.windows(2).all(|pair| pair[0].node < pair[1].node)
            || !phase.dirty.windows(2).all(|pair| pair[0].0 < pair[1].0)
            || !phase.piston_revisions.windows(2).all(|pair| pair[0].0 < pair[1].0)
            || !phase.resource_revisions.windows(2).all(|pair| pair[0].0 < pair[1].0)
        {
            return false;
        }
    }
    true
}

/// Independent replay validates both target memory and every compiled write/obligation.
pub(crate) fn validate(graph: &CompileGraph, machine: &InstantMachine) -> Result<(), GraphBuildError> {
    let invalid = |state, action| GraphBuildError::InvalidInstantMachine { state, action };
    if controls(graph).as_ref() != Some(&machine.controls)
        || machine.states.is_empty()
        || machine.states.len() > MAX_STATES
        || machine.estimated_bytes() > MAX_BYTES
        || !kernel_shape_valid(graph, &machine.controls, &machine.kernel)
        || machine.kernel.estimated_bytes() > MAX_BYTES
        || machine.transitions.len() != machine.states.len()
        || machine.index.len() != machine.states.len()
    {
        return Err(invalid(0, 0));
    }
    let mut sandbox = evaluator(graph).map_err(|_| invalid(0, 0))?;
    for (source, state) in machine.states.iter().enumerate() {
        if machine.index.get(state) != Some(&(source as u32)) || machine.transitions[source].len() != machine.controls.len() + 1 {
            return Err(invalid(source, 0));
        }
        sandbox.restore_machine_state(state).map_err(|_| invalid(source, 0))?;
        let mut roundtrip = MachineState::default();
        if !sandbox.capture_machine_state(&mut roundtrip) || roundtrip != *state {
            return Err(invalid(source, 0));
        }
        for (action, edge) in machine.transitions[source].iter().enumerate() {
            let Some(edge) = edge else {
                continue;
            };
            let Some(expected) = machine.states.get(edge.next as usize) else {
                return Err(invalid(source, action));
            };
            let (actual, mut actual_edge) = sandbox
                .evaluate_machine_step(state, &machine.controls, action)
                .map_err(|_| invalid(source, action))?;
            actual_edge.next = edge.next;
            if &actual != expected || actual_edge != *edge {
                return Err(invalid(source, action));
            }
        }
    }
    let Some(rebuilt) = compile_kernel(graph, &machine.controls, &TaskMonitor::default()).map_err(|_| invalid(0, 0))? else {
        return Err(invalid(0, 0));
    };
    if rebuilt != machine.kernel {
        return Err(invalid(0, 0));
    }
    Ok(())
}
