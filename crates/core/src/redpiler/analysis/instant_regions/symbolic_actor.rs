//! Symbolic event routines for a single owned actor and its reset clock.
//! State-changing effects reuse Graph's ordered primitives; no input valuations are enumerated.
use super::*;
use mchprs_redpiler_opt::{Circuit, Limits, Op};

/// Compile event semantics, not sampled histories. Unsupported families keep general execution.
pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<SymbolicActor>, String> {
    let mut compiled = Vec::new();
    for region in &graph.plans.instant.regions {
        check_cancelled(monitor)?;
        let ([actor], [reset]) = (region.actors.as_slice(), region.resets.as_slice()) else {
            continue;
        };
        if !matches!(
            graph.nodes[reset.index()].kind,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }
        ) {
            continue;
        }
        if let Some(program) = compile_actor(graph, region, *actor, *reset, monitor)? {
            compiled.push(program);
        }
    }
    Ok(compiled)
}

fn compile_actor(
    graph: &CompileGraph,
    region: &InstantRegion,
    actor: PistonId,
    reset: NodeId,
    monitor: &TaskMonitor,
) -> Result<Option<SymbolicActor>, String> {
    let mut builder = Builder::default();
    let target = graph.pistons[actor.index()].node;
    let mut powered = builder.op(Op::Const(false));
    for link in &graph.power_links {
        check_cancelled(monitor)?;
        if !matches!(link.to, PowerTarget::Input { node, port: InputPort::Main | InputPort::Quasi } if node == target) {
            continue;
        }
        // A power source is read at event time, including internal reset feedback.
        let source = builder.read(ActorRead::Power(*link));
        let contribution = match &graph.guards[link.guard.index()] {
            Guard::Always => source,
            _ => {
                let guard = builder.read(ActorRead::Guard(link.guard));
                builder.op(Op::And(guard, source))
            }
        };
        powered = builder.op(Op::Or(powered, contribution));
        if builder.overflowed {
            return Ok(None);
        }
    }
    let extended = builder.read(ActorRead::Extended);
    let base_pending = builder.read(ActorRead::BasePending);
    let ready = builder.read(ActorRead::Ready);
    let lane = builder.read(ActorRead::LaneAvailable);
    let retract_available = builder.read(ActorRead::RetractionAvailable);
    let early_available = builder.read(ActorRead::EarlyRetractionAvailable);
    let early = builder.read(ActorRead::EarlyResource);
    let not_powered = builder.op(Op::Not(powered));
    let retracted = builder.op(Op::Not(extended));
    let not_pending = builder.op(Op::Not(base_pending));
    let extend = builder.op(Op::And(powered, retracted));
    let retract = builder.op(Op::And(not_powered, extended));
    let retract = builder.op(Op::And(retract, not_pending));
    let not_early = builder.op(Op::Not(early));
    let extend_lane = builder.op(Op::And(extend, lane));
    let request_extend = builder.op(Op::And(extend_lane, not_pending));
    let request_retract = builder.op(Op::And(retract, not_early));
    let request_early = builder.op(Op::And(retract, early));
    let accept_extend = builder.op(Op::And(extend_lane, ready));
    let accept_retract = builder.op(Op::And(retract, retract_available));
    let accept_early = builder.op(Op::And(retract, early_available));
    let reset_powered = builder.read(ActorRead::ResetPowered);
    let reset_pending = builder.read(ActorRead::ResetPending);
    let qualifies = builder.read(ActorRead::ResetQualifies);
    let reset_not_pending = builder.op(Op::Not(reset_pending));
    let arm_reset = builder.op(Op::And(qualifies, reset_not_pending));
    let next_reset = builder.op(Op::Not(reset_powered));
    if builder.overflowed {
        return Ok(None);
    }
    let mut routines = Vec::new();
    for roots in [
        vec![request_extend, request_retract, request_early],
        vec![accept_extend, accept_retract, accept_early],
        vec![arm_reset],
        vec![next_reset, next_reset],
    ] {
        let inputs = (0..builder.reads.len()).collect();
        let circuit = Circuit::new(inputs, builder.ops.clone(), roots);
        let plan = mchprs_redpiler_opt::optimize(&circuit, Limits::default(), || monitor.cancelled()).map_err(|e| e.to_string())?;
        if plan.scratch_len() > MAX_ACTOR_OPS {
            return Ok(None);
        }
        let mut used_reads: Vec<_> = plan
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Input(input) => Some(*input),
                _ => None,
            })
            .collect();
        used_reads.sort_unstable();
        used_reads.dedup();
        routines.push(ActorRoutine {
            used_reads: used_reads.into_boxed_slice(),
            plan,
        });
    }
    check_cancelled(monitor)?;
    Ok(Some(SymbolicActor {
        actor,
        reset,
        boundary: KernelBoundary::from_region(region),
        reads: builder.reads.into_boxed_slice(),
        routines: routines.try_into().expect("four event routines were constructed"),
    }))
}

#[derive(Default)]
struct Builder {
    /// These typed bindings are interned so repeated reads share a Boolean input.
    reads: Vec<ActorRead>,
    /// These operations are appended after their operands, preserving topological order.
    ops: Vec<Op>,
    /// Exhausted construction budgets decline before building or executing a malformed plan.
    overflowed: bool,
}

impl Builder {
    fn read(&mut self, read: ActorRead) -> usize {
        if self.reads.len() >= MAX_ACTOR_INPUTS && !self.reads.contains(&read) {
            self.overflowed = true;
            return 0;
        }
        let index = self.reads.iter().position(|existing| *existing == read).unwrap_or_else(|| {
            let index = self.reads.len();
            self.reads.push(read);
            index
        });
        self.op(Op::Input(index))
    }

    fn op(&mut self, op: Op) -> usize {
        if self.ops.len() >= MAX_ACTOR_OPS {
            self.overflowed = true;
            return 0;
        }
        let index = self.ops.len();
        self.ops.push(op);
        index
    }
}
