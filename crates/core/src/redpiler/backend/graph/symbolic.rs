//! Execute compiled event expressions against the existing state and ordered work queues.
//! Resource transfers and callback delivery stay shared primitives, never a second simulator.
use super::*;
use crate::redpiler::program_graph::{
    ACTOR_ROOTS, ActorEvent, ActorRead, MAX_ACTOR_INPUTS, MAX_ACTOR_OPS, ResetEvent, ResetRead, SymbolicActorRoutine, SymbolicResetProgram,
};

impl GraphBackend {
    pub(super) fn symbolic_actor_roots(&mut self, plan: &SymbolicActorRoutine, event: ActorEvent) -> [bool; ACTOR_ROOTS] {
        let mut inputs = [false; MAX_ACTOR_INPUTS];
        let actor = self.state.pistons[plan.actor.index()];
        let routine = &plan.routines[match event {
            ActorEvent::Sample => 0,
            ActorEvent::Accept => 1,
        }];
        for &slot in routine.used_reads.iter() {
            inputs[slot] = match plan.reads[slot] {
                ActorRead::Power(link) => self.guard_matches(link.guard, None) && self.site_power(link.from, link.from_port) > link.attenuation,
                ActorRead::Extended => actor.extended,
                ActorRead::BasePending => actor.availability.base_pending(),
                ActorRead::Ready => actor.availability == PistonAvailability::Ready,
                ActorRead::LaneAvailable => self.piston_lane_available(plan.actor),
                ActorRead::RetractionAvailable => self.piston_retraction_available(plan.actor, false),
                ActorRead::EarlyRetractionAvailable => self.piston_retraction_available(plan.actor, true),
                ActorRead::EarlyResource => self.early_resource_retraction(plan.actor),
            };
        }
        let mut scratch = [false; MAX_ACTOR_OPS];
        let mut roots = [false; ACTOR_ROOTS];
        routine
            .plan
            .evaluate(&inputs[..plan.reads.len()], &mut scratch, &mut roots)
            .expect("validated symbolic event plan");
        self.counts.symbolic_region_steps += 1;
        roots
    }

    pub(super) fn symbolic_reset_roots(
        &mut self,
        plan: &SymbolicResetProgram,
        event: ResetEvent,
        reset_view: Option<(bool, bool)>,
    ) -> [bool; ACTOR_ROOTS] {
        let mut inputs = [false; MAX_ACTOR_INPUTS];
        let (powered, qualifies) = reset_view.unwrap_or_else(|| (self.node_state(plan.node).powered, false));
        let routine = &plan.routines[match event {
            ResetEvent::Update => 0,
            ResetEvent::Tick => 1,
        }];
        for &slot in routine.used_reads.iter() {
            inputs[slot] = match plan.reads[slot] {
                ResetRead::Powered => powered,
                ResetRead::Pending => self.is_pending(plan.node),
                ResetRead::Qualifies => qualifies,
            };
        }
        let mut scratch = [false; MAX_ACTOR_OPS];
        let mut roots = [false; ACTOR_ROOTS];
        routine
            .plan
            .evaluate(&inputs[..plan.reads.len()], &mut scratch, &mut roots)
            .expect("validated symbolic reset event plan");
        self.counts.symbolic_region_steps += 1;
        roots
    }

    pub(super) fn symbolic_reset_update(
        &mut self,
        node: NodeId,
        facing: BlockFace,
        direction: Option<BlockFace>,
        captured: Option<&ObservedSite>,
    ) -> bool {
        let Some(plan) = self.program.symbolic_reset(node).cloned() else {
            return false;
        };
        let powered = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, .. } => Some(powered),
                _ => None,
            })
            .unwrap_or(self.node_state(node).powered);
        debug_assert_eq!(facing, plan.facing);
        let qualifies = direction.map_or(powered, |direction| direction == plan.facing);
        let roots = self.symbolic_reset_roots(&plan, ResetEvent::Update, Some((powered, qualifies)));
        if roots[0] {
            self.schedule(node, 1, TickPriority::Normal);
        }
        true
    }

    pub(super) fn symbolic_reset_tick(&mut self, node: NodeId, notify: bool) -> Option<Result<(), GraphExecutionError>> {
        let plan = self.program.symbolic_reset(node)?.clone();
        let roots = self.symbolic_reset_roots(&plan, ResetEvent::Tick, None);
        // A clock's follow-up is enqueued before its rising-edge output callbacks.
        if roots[0] {
            self.schedule(node, 1, TickPriority::Normal);
        }
        Some(self.set_node(node, roots[1], u8::from(roots[1]) * 15, None, notify))
    }
}
