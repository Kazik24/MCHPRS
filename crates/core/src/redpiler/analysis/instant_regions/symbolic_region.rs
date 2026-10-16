//! Compile event routines once per owned actor/resource/reset region.
//! State-changing effects reuse Graph's ordered primitives; no input valuations are enumerated.
use super::*;
use mchprs_redpiler_opt::{Circuit, Limits, Op};

/// Compile each eligible region as one boundary and shared actor/reset routine group.
pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SymbolicRegionCompilation, String> {
    let mut compilation = SymbolicRegionCompilation::default();
    for (region_index, region) in graph.plans.instant.regions.iter().enumerate() {
        check_cancelled(monitor)?;
        if region.actors.is_empty() {
            continue;
        }
        if !region.stable_inputs {
            compilation.declines.push(region_decline(region_index, region, "region inputs are not certified stable"));
            continue;
        }
        let event_kernel = match crate::redpiler::analysis::instant_regions::compiler::event::compile(graph, region_index, monitor) {
            Ok(kernel) => {
                let plan_bytes: Vec<_> = kernel.entries.iter().map(|entry| entry.plan.estimated_heap_bytes()).collect();
                eprintln!(
                    "REGIONAL_COMPILE region={region_index} entries={} ops={} roots={} plan_bytes={plan_bytes:?}",
                    kernel.entries.len(),
                    kernel.entries.iter().map(|entry| entry.plan.ops().len()).sum::<usize>(),
                    kernel.entries.iter().map(|entry| entry.plan.roots().len()).sum::<usize>(),
                );
                Some(std::sync::Arc::new(kernel))
            }
            Err(reason) => {
                if monitor.cancelled() {
                    return Err(reason);
                }
                tracing::debug!(region_index, %reason, "regional event kernel declined");
                eprintln!("REGIONAL_DECLINE region={region_index} actors={} resets={} reason={reason}", region.actors.len(), region.resets.len());
                compilation.declines.push(region_decline(region_index, region, reason));
                continue;
            }
        };
        let mut actors = Vec::with_capacity(region.actors.len());
        let mut decline = None;
        for &actor in &region.actors {
            let Some(program) = compile_actor(graph, actor, monitor)? else {
                decline = Some("actor_event_plan_unsupported");
                break;
            };
            actors.push(std::sync::Arc::new(program));
        }
        if let Some(reason) = decline {
            compilation.declines.push(region_decline(region_index, region, reason));
            continue;
        }
        let mut resets = Vec::with_capacity(region.resets.len());
        for &node in &region.resets {
            let Some(program) = compile_reset(graph, node, monitor)? else {
                decline = Some("reset_event_plan_unsupported");
                break;
            };
            resets.push(std::sync::Arc::new(program));
        }
        if let Some(reason) = decline {
            compilation.declines.push(region_decline(region_index, region, reason));
        } else {
            compilation.programs.push(SymbolicRegion {
                region: region_index,
                boundary: KernelBoundary::from_region(graph, region),
                actors: actors.into_boxed_slice(),
                resets: resets.into_boxed_slice(),
                event_kernel,
            });
        }
    }
    Ok(compilation)
}

fn region_decline(region_index: usize, region: &InstantRegion, reason: impl Into<String>) -> SymbolicRegionDecline {
    SymbolicRegionDecline {
        region: region_index,
        actor_count: region.actors.len(),
        reset_count: region.resets.len(),
        reason: reason.into(),
    }
}

fn compile_actor(graph: &CompileGraph, actor: PistonId, monitor: &TaskMonitor) -> Result<Option<SymbolicActorRoutine>, String> {
    let mut builder = Builder::<ActorRead>::default();
    let target = graph.pistons[actor.index()].node;
    let mut powered = builder.op(Op::Const(false));
    for link in &graph.power_links {
        check_cancelled(monitor)?;
        if !matches!(link.to, PowerTarget::Input { node, port: InputPort::Main | InputPort::Quasi } if node == target) {
            continue;
        }
        // A power source is read at event time, including internal reset feedback.
        let source = builder.read(ActorRead::Power(*link));
        powered = builder.op(Op::Or(powered, source));
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
    if builder.overflowed {
        return Ok(None);
    }
    let routines = [
        compile_routine(&builder, vec![request_extend, request_retract, request_early], monitor)?,
        compile_routine(&builder, vec![accept_extend, accept_retract, accept_early], monitor)?,
    ];
    if routines.iter().any(Option::is_none) {
        return Ok(None);
    }
    check_cancelled(monitor)?;
    Ok(Some(SymbolicActorRoutine {
        actor,
        reads: builder.reads.into_boxed_slice(),
        routines: routines.map(Option::unwrap),
    }))
}

fn compile_reset(graph: &CompileGraph, node: NodeId, monitor: &TaskMonitor) -> Result<Option<SymbolicResetProgram>, String> {
    let facing = match graph.nodes[node.index()].kind {
        CircuitNodeKind::Observer { facing, .. } | CircuitNodeKind::BuiltinReset { facing, .. } => facing,
        _ => return Ok(None),
    };
    let mut builder = Builder::<ResetRead>::default();
    let powered = builder.read(ResetRead::Powered);
    let pending = builder.read(ResetRead::Pending);
    let qualifies = builder.read(ResetRead::Qualifies);
    let not_pending = builder.op(Op::Not(pending));
    let arm = builder.op(Op::And(qualifies, not_pending));
    let next = builder.op(Op::Not(powered));
    if builder.overflowed {
        return Ok(None);
    }
    let routines = [
        compile_routine(&builder, vec![arm], monitor)?,
        compile_routine(&builder, vec![next, next], monitor)?,
    ];
    if routines.iter().any(Option::is_none) {
        return Ok(None);
    }
    check_cancelled(monitor)?;
    Ok(Some(SymbolicResetProgram {
        node,
        facing,
        reads: builder.reads.into_boxed_slice(),
        routines: routines.map(Option::unwrap),
    }))
}

fn compile_routine<R: Copy + PartialEq>(builder: &Builder<R>, roots: Vec<usize>, monitor: &TaskMonitor) -> Result<Option<EventRoutine>, String> {
    let inputs = (0..builder.reads.len()).collect();
    let circuit = Circuit::new(inputs, builder.ops.clone(), roots);
    let plan = mchprs_redpiler_opt::optimize(&circuit, Limits::default(), || monitor.cancelled()).map_err(|error| error.to_string())?;
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
    Ok(Some(EventRoutine {
        used_reads: used_reads.into_boxed_slice(),
        plan,
    }))
}

struct Builder<R> {
    /// These typed bindings are interned so repeated reads share a Boolean input.
    reads: Vec<R>,
    /// These operations are appended after their operands, preserving topological order.
    ops: Vec<Op>,
    /// Exhausted construction budgets decline before building or executing a malformed plan.
    overflowed: bool,
}

impl<R> Default for Builder<R> {
    fn default() -> Self {
        Self {
            reads: Vec::new(),
            ops: Vec::new(),
            overflowed: false,
        }
    }
}

impl<R: Copy + PartialEq> Builder<R> {
    fn read(&mut self, read: R) -> usize {
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
