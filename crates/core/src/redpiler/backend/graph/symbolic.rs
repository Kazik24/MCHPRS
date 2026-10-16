//! Execute compiled event expressions against the existing state and ordered work queues.
//! Resource transfers and callback delivery stay shared primitives, never a second simulator.
use super::pistons::DeferredResourceEvent;
use super::*;
use crate::redpiler::program_graph::{
    ACTOR_ROOTS, ActorEvent, ActorRead, MAX_ACTOR_INPUTS, MAX_ACTOR_OPS, RegionRead, RegionStateSlot, ResetEvent, ResetRead, SymbolicActorRoutine,
    SymbolicRegionEffect, SymbolicRegionEntry, SymbolicRegionEventRoutine, SymbolicRegionWrite, SymbolicResetProgram,
};

fn wire_side_mut(wire: &mut RedstoneWire, side: u8) -> Option<&mut RedstoneWireSide> {
    match side {
        0 => Some(&mut wire.north),
        1 => Some(&mut wire.south),
        2 => Some(&mut wire.east),
        3 => Some(&mut wire.west),
        _ => None,
    }
}

fn wire_side_code(side: RedstoneWireSide) -> u8 {
    match side {
        RedstoneWireSide::None => 0,
        RedstoneWireSide::Side => 1,
        RedstoneWireSide::Up => 2,
    }
}

/// Reusable storage sized to the largest event routine evaluated at this nesting depth.
#[derive(Default)]
pub(super) struct SymbolicRegionFrame {
    /// These are event-time values for the routine's compact Plan input interface.
    inputs: Vec<bool>,
    /// This executes the immutable plan without allocating temporary Boolean storage.
    scratch: Vec<bool>,
    /// These retain ordered state and effect roots until the event stream is replayed.
    roots: Vec<bool>,
    /// These preserve entry revisions for sparse final writes and event-time witnesses.
    actor_revisions: Vec<u64>,
    /// These preserve entry revisions for shared resource witnesses.
    resource_revisions: Vec<u64>,
    /// These count selected actor revision writes when materializing an entry prefix.
    actor_revision_counts: Vec<u64>,
    /// These count selected resource revision writes when materializing an entry prefix.
    resource_revision_counts: Vec<u64>,
    /// These mark actors whose revision was changed by the current sparse write set.
    actor_revision_touched: Vec<bool>,
    /// These mark resources whose revision was changed by the current sparse write set.
    resource_revision_touched: Vec<bool>,
    /// These save physical owned values while an outside callback sees an earlier snapshot.
    overlay_restore: Vec<(RegionStateSlot, bool)>,
    /// These save actor revisions while an outside callback sees its captured prefix.
    actor_revision_restore: Vec<(PistonId, u64)>,
    /// These save payload revisions while an outside callback sees its captured prefix.
    resource_revision_restore: Vec<(ResourceId, u64)>,
}

impl GraphBackend {
    /// Evaluate one regional entry against live Graph state and reuse its scratch afterward.
    fn evaluate_symbolic_region(
        &mut self,
        routine: &SymbolicRegionEventRoutine,
        captured: Option<&[Option<ObservedSite>]>,
        deferred: Option<DeferredPistonEvent>,
    ) -> SymbolicRegionFrame {
        let mut frame = self.symbolic_region_frames.pop().unwrap_or_default();
        frame.inputs.resize(routine.reads.len(), false);
        for (slot, read) in routine.reads.iter().copied().enumerate() {
            frame.inputs[slot] = self.symbolic_region_read(read, captured, deferred);
        }
        frame.scratch.resize(routine.plan.scratch_len(), false);
        frame.roots.resize(routine.plan.roots().len(), false);
        routine
            .plan
            .evaluate(&frame.inputs, &mut frame.scratch, &mut frame.roots)
            .expect("validated regional event plan");
        self.counts.symbolic_region_steps += 1;
        frame
    }

    /// Run one compiled boundary entry and replay only its sparse ordered outputs.
    pub(super) fn run_symbolic_region(
        &mut self,
        key: SymbolicRegionEntry,
        captured: Option<&[Option<ObservedSite>]>,
        deferred: Option<DeferredPistonEvent>,
    ) -> Result<bool, GraphExecutionError> {
        let Some(routine) = self.program.symbolic_event_routine(key).cloned() else {
            return Ok(false);
        };
        let mut frame = self.evaluate_symbolic_region(&routine, captured, deferred);
        frame.actor_revisions.clear();
        frame
            .actor_revisions
            .extend(routine.actor_owners.iter().map(|actor| self.state.pistons[actor.index()].revision));
        frame.resource_revisions.clear();
        frame.resource_revisions.extend(
            routine
                .resource_owners
                .iter()
                .map(|resource| self.state.resources[resource.index()].revision),
        );

        let result = (|| {
            self.apply_symbolic_region_writes(&mut frame, &routine, true);
            match key {
                SymbolicRegionEntry::Sample(_) => self.counts.piston_samples += 1,
                SymbolicRegionEntry::Callback {
                    receiver: CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_),
                    ..
                } => self.counts.piston_samples += 1,
                _ => {}
            }
            for effect in routine.effects.iter() {
                match *effect {
                    SymbolicRegionEffect::Schedule { when, node, delay, priority } if frame.roots[when] => {
                        self.schedule_half_tick(node, delay, priority);
                    }
                    SymbolicRegionEffect::Request { when, actor, action } if frame.roots[when] => {
                        self.enqueue_piston(PistonRequest { piston: actor, action });
                    }
                    SymbolicRegionEffect::Publication {
                        when,
                        actor_valid: _,
                        actor,
                        actor_stamp,
                        resource,
                        due_tick,
                        extending,
                    } if frame.roots[when] => {
                        let actor_revision = Self::symbolic_actor_revision_at(&frame, &routine, actor, actor_stamp);
                        let resource = resource.and_then(|(when, resource, stamp, contact)| {
                            frame.roots[when].then(|| DeferredResourceEvent {
                                resource,
                                revision: Self::symbolic_resource_revision_at(&frame, &routine, resource, stamp),
                                contact,
                            })
                        });
                        self.future.deferred_pistons.push_back(DeferredPistonEvent {
                            piston: actor,
                            revision: actor_revision,
                            resource,
                            due_tick: self.future.clock.logical_tick + due_tick,
                            extending,
                        });
                    }
                    SymbolicRegionEffect::Accepted {
                        when,
                        actor,
                        action,
                        previous_extended,
                    } if frame.roots[when] => {
                        self.accepted(PistonRequest { piston: actor, action }, frame.roots[previous_extended]);
                    }
                    SymbolicRegionEffect::Callback { .. } => {}
                    _ => {}
                }
            }
            Ok(())
        })();
        self.symbolic_region_frames.push(frame);
        result?;
        Ok(true)
    }

    fn apply_symbolic_region_writes(&mut self, frame: &mut SymbolicRegionFrame, routine: &SymbolicRegionEventRoutine, dirty: bool) {
        frame.actor_revision_counts.resize(routine.actor_owners.len(), 0);
        frame.actor_revision_counts.fill(0);
        frame.actor_revision_touched.resize(routine.actor_owners.len(), false);
        frame.actor_revision_touched.fill(false);
        frame.resource_revision_counts.resize(routine.resource_owners.len(), 0);
        frame.resource_revision_counts.fill(0);
        frame.resource_revision_touched.resize(routine.resource_owners.len(), false);
        frame.resource_revision_touched.fill(false);
        for write in &routine.writes {
            let value = frame.roots[write.root];
            match write.slot {
                RegionStateSlot::ActorRevision { actor, .. } => {
                    let index = routine.actor_owners.binary_search(&actor).expect("revision owner is part of the routine");
                    frame.actor_revision_touched[index] = true;
                    frame.actor_revision_counts[index] += u64::from(value);
                }
                RegionStateSlot::ResourceRevision { resource, .. } => {
                    let index = routine
                        .resource_owners
                        .binary_search(&resource)
                        .expect("revision owner is part of the routine");
                    frame.resource_revision_touched[index] = true;
                    frame.resource_revision_counts[index] += u64::from(value);
                }
                slot => {
                    self.set_symbolic_region_slot(slot, value, dirty);
                }
            }
        }
        for (index, &actor) in routine.actor_owners.iter().enumerate() {
            if frame.actor_revision_touched[index] {
                self.state.pistons[actor.index()].revision = frame.actor_revisions[index].wrapping_add(frame.actor_revision_counts[index]);
            }
        }
        for (index, &resource) in routine.resource_owners.iter().enumerate() {
            if frame.resource_revision_touched[index] {
                self.state.resources[resource.index()].revision = frame.resource_revisions[index].wrapping_add(frame.resource_revision_counts[index]);
            }
        }
    }

    fn symbolic_region_slot(&self, slot: RegionStateSlot) -> bool {
        match slot {
            RegionStateSlot::ComponentSignal { node, bit } => match &self.state.components[node.index()] {
                ComponentState::Electrical(state) => state.output_strength & (1 << bit) != 0,
                ComponentState::Wire(wire) => wire.power & (1 << bit) != 0,
                ComponentState::Specialized => false,
            },
            RegionStateSlot::ComponentPowered(node) => self.node_state(node).powered,
            RegionStateSlot::ComponentLocked(node) => self.node_state(node).repeater_locked,
            RegionStateSlot::WireUp { node, side } => self.wire_side(node, side).0,
            RegionStateSlot::WireFlat { node, side } => self.wire_side(node, side).1,
            RegionStateSlot::ActorExtended(actor) => self.state.pistons[actor.index()].extended,
            RegionStateSlot::ActorHead(actor) => self.state.pistons[actor.index()].head_present,
            RegionStateSlot::ActorAvailability { actor, state } => self.state.pistons[actor.index()].availability as u8 == state,
            RegionStateSlot::ResourceAt { resource, contact } => self.state.resources[resource.index()].at == contact,
            RegionStateSlot::ResourceEnabled(resource) => self.state.resources[resource.index()].enabled,
            RegionStateSlot::ActorRevision { .. } | RegionStateSlot::ResourceRevision { .. } => false,
        }
    }

    fn set_symbolic_region_slot(&mut self, slot: RegionStateSlot, value: bool, dirty: bool) {
        let changed = match slot {
            RegionStateSlot::ComponentSignal { node, bit } => match &mut self.state.components[node.index()] {
                ComponentState::Electrical(state) => {
                    let old = state.output_strength;
                    state.output_strength = if value { old | (1 << bit) } else { old & !(1 << bit) };
                    state.output_strength != old
                }
                ComponentState::Wire(wire) => {
                    let old = wire.power;
                    wire.power = if value { old | (1 << bit) } else { old & !(1 << bit) };
                    wire.power != old
                }
                ComponentState::Specialized => false,
            },
            RegionStateSlot::ComponentPowered(node) => {
                let ComponentState::Electrical(state) = &mut self.state.components[node.index()] else {
                    unreachable!("powered state requested for a specialized graph node")
                };
                let changed = state.powered != value;
                state.powered = value;
                changed
            }
            RegionStateSlot::ComponentLocked(node) => {
                let ComponentState::Electrical(state) = &mut self.state.components[node.index()] else {
                    unreachable!("lock state requested for a specialized graph node")
                };
                let changed = state.repeater_locked != value;
                state.repeater_locked = value;
                changed
            }
            RegionStateSlot::WireUp { node, side } | RegionStateSlot::WireFlat { node, side } => {
                let ComponentState::Wire(wire) = &mut self.state.components[node.index()] else {
                    return;
                };
                let Some(side_value) = wire_side_mut(wire, side) else {
                    return;
                };
                let old = *side_value;
                let is_up_slot = matches!(slot, RegionStateSlot::WireUp { .. });
                let up = if is_up_slot { value } else { old == RedstoneWireSide::Up };
                let flat = if is_up_slot { old == RedstoneWireSide::Side } else { value };
                *side_value = if up {
                    RedstoneWireSide::Up
                } else if flat {
                    RedstoneWireSide::Side
                } else {
                    RedstoneWireSide::None
                };
                *side_value != old
            }
            RegionStateSlot::ActorExtended(actor) => {
                let state = &mut self.state.pistons[actor.index()].extended;
                let changed = *state != value;
                *state = value;
                changed
            }
            RegionStateSlot::ActorHead(actor) => {
                let state = &mut self.state.pistons[actor.index()].head_present;
                let changed = *state != value;
                *state = value;
                changed
            }
            RegionStateSlot::ActorAvailability { actor, state } if value => {
                let availability = match state {
                    0 => PistonAvailability::Ready,
                    1 => PistonAvailability::HeadPending,
                    2 => PistonAvailability::PublishingHeadRemoval,
                    3 => PistonAvailability::BasePendingHeadPresent,
                    4 => PistonAvailability::BasePending,
                    _ => return,
                };
                let current = &mut self.state.pistons[actor.index()].availability;
                let changed = *current != availability;
                *current = availability;
                changed
            }
            RegionStateSlot::ActorAvailability { .. } => false,
            RegionStateSlot::ResourceAt { resource, contact } if value => {
                let current = &mut self.state.resources[resource.index()].at;
                let changed = *current != contact;
                *current = contact;
                changed
            }
            RegionStateSlot::ResourceAt { .. } => false,
            RegionStateSlot::ResourceEnabled(resource) => {
                let current = &mut self.state.resources[resource.index()].enabled;
                let changed = *current != value;
                *current = value;
                changed
            }
            RegionStateSlot::ActorRevision { .. } | RegionStateSlot::ResourceRevision { .. } => false,
        };
        if dirty && changed {
            match slot {
                RegionStateSlot::ComponentSignal { node, .. }
                | RegionStateSlot::ComponentPowered(node)
                | RegionStateSlot::ComponentLocked(node)
                | RegionStateSlot::WireUp { node, .. }
                | RegionStateSlot::WireFlat { node, .. } => self.dirty[node.index()] = true,
                _ => {}
            }
        }
    }

    fn symbolic_actor_revision_at(frame: &SymbolicRegionFrame, routine: &SymbolicRegionEventRoutine, actor: PistonId, stamp: u32) -> u64 {
        let increments = routine
            .writes
            .iter()
            .filter(|write| matches!(write.slot, RegionStateSlot::ActorRevision { actor: owner, write: index } if owner == actor && index < stamp))
            .filter(|write| frame.roots[write.root])
            .count() as u64;
        let index = routine
            .actor_owners
            .binary_search(&actor)
            .expect("publication actor is part of the routine");
        frame.actor_revisions[index].wrapping_add(increments)
    }

    fn symbolic_resource_revision_at(frame: &SymbolicRegionFrame, routine: &SymbolicRegionEventRoutine, resource: ResourceId, stamp: u32) -> u64 {
        let increments = routine
            .writes
            .iter()
            .filter(|write| matches!(write.slot, RegionStateSlot::ResourceRevision { resource: owner, write: index } if owner == resource && index < stamp))
            .filter(|write| frame.roots[write.root])
            .count() as u64;
        let index = routine
            .resource_owners
            .binary_search(&resource)
            .expect("publication resource is part of the routine");
        frame.resource_revisions[index].wrapping_add(increments)
    }

    fn symbolic_region_read(&self, read: RegionRead, captured: Option<&[Option<ObservedSite>]>, deferred: Option<DeferredPistonEvent>) -> bool {
        match read {
            RegionRead::ComponentSignal { node, bit } => {
                let strength = match &self.state.components[node.index()] {
                    ComponentState::Electrical(state) => state.output_strength,
                    ComponentState::Wire(wire) => wire.power,
                    ComponentState::Specialized => 0,
                };
                strength & (1 << bit) != 0
            }
            RegionRead::ComponentPowered(node) => self.node_state(node).powered,
            RegionRead::ComponentLocked(node) => self.node_state(node).repeater_locked,
            RegionRead::WireSideBit { node, side, bit } => self.wire_side_bit(node, side, bit),
            RegionRead::ActorExtended(actor) => self.state.pistons[actor.index()].extended,
            RegionRead::ActorHead(actor) => self.state.pistons[actor.index()].head_present,
            RegionRead::ActorAvailability { actor, state } => self.state.pistons[actor.index()].availability as u8 == state,
            RegionRead::ResourceAt { resource, contact } => self.state.resources[resource.index()].at == contact,
            RegionRead::ResourceEnabled(resource) => self.state.resources[resource.index()].enabled,
            RegionRead::Pending(node) => self.is_pending(node),
            RegionRead::CapturePresent(site) => self.captured_region_site(captured, site).is_some(),
            RegionRead::CaptureAlternative { site, profile, receiver } => self
                .captured_region_site(captured, site)
                .is_some_and(|value| value.profile == Some(profile) && value.receiver == receiver),
            RegionRead::CapturePowered(site) => self
                .captured_region_site(captured, site)
                .and_then(|value| match &value.properties {
                    ObservedProperties::Electrical { powered, .. } | ObservedProperties::CopperBulb { powered, .. } => Some(*powered),
                    _ => None,
                })
                .unwrap_or(false),
            RegionRead::CaptureLocked(site) => self
                .captured_region_site(captured, site)
                .and_then(|value| match &value.properties {
                    ObservedProperties::Electrical { locked, .. } => Some(*locked),
                    _ => None,
                })
                .unwrap_or(false),
            RegionRead::CaptureLit(site) => self
                .captured_region_site(captured, site)
                .and_then(|value| match &value.properties {
                    ObservedProperties::CopperBulb { lit, .. } => Some(*lit),
                    _ => None,
                })
                .unwrap_or(false),
            RegionRead::CaptureWireSideBit { site, side, bit } => self.captured_wire_side_bit(captured, site, side, bit),
            RegionRead::CaptureWireSignalBit { site, bit } => self
                .captured_region_site(captured, site)
                .and_then(|value| match &value.properties {
                    ObservedProperties::Wire(wire) => Some(wire.power & (1 << bit) != 0),
                    _ => None,
                })
                .unwrap_or_else(|| match self.program.sites()[site.index()].source {
                    SiteSource::Component(node) => match &self.state.components[node.index()] {
                        ComponentState::Wire(wire) => wire.power & (1 << bit) != 0,
                        _ => false,
                    },
                    _ => false,
                }),
            RegionRead::CaptureActorExtended(site) => self
                .captured_region_site(captured, site)
                .and_then(|value| match &value.properties {
                    ObservedProperties::Piston { extended } => Some(*extended),
                    _ => None,
                })
                .unwrap_or(false),
            RegionRead::DeferredActorValid(actor) => {
                deferred.is_some_and(|event| event.piston == actor && self.state.pistons[actor.index()].revision == event.revision)
            }
            RegionRead::DeferredResourceValid(resource) => deferred.is_some_and(|event| {
                event
                    .resource
                    .is_some_and(|pending| pending.resource == resource && self.state.resources[resource.index()].revision == pending.revision)
            }),
            RegionRead::PendingExtension {
                resource,
                contact,
                facing,
                early_only,
            } => self.pending_resource_extension(resource, contact, facing).is_some_and(|event| {
                !early_only
                    || event.due_tick > self.future.clock.logical_tick
                    || matches!(self.future.clock.phase, GraphPhase::ScheduledTicks | GraphPhase::PistonActions)
            }),
        }
    }

    fn captured_region_site<'a>(&self, captured: Option<&'a [Option<ObservedSite>]>, site: SiteId) -> Option<&'a ObservedSite> {
        captured.and_then(|values| values.get(site.index())).and_then(Option::as_ref)
    }

    fn wire_side(&self, node: NodeId, side: u8) -> (bool, bool) {
        let ComponentState::Wire(wire) = &self.state.components[node.index()] else {
            return (false, false);
        };
        wire_sides(*wire)
            .get(side as usize)
            .map_or((false, false), |side| (*side == RedstoneWireSide::Up, *side == RedstoneWireSide::Side))
    }

    fn wire_side_bit(&self, node: NodeId, side: u8, bit: u8) -> bool {
        let ComponentState::Wire(wire) = &self.state.components[node.index()] else {
            return false;
        };
        wire_sides(*wire)
            .get(side as usize)
            .is_some_and(|value| wire_side_code(*value) & (1 << bit) != 0)
    }

    fn captured_wire_side_bit(&self, captured: Option<&[Option<ObservedSite>]>, site: SiteId, side: u8, bit: u8) -> bool {
        self.captured_region_site(captured, site)
            .and_then(|value| match &value.properties {
                ObservedProperties::Wire(wire) => wire_sides(*wire).get(side as usize).copied(),
                _ => None,
            })
            .is_some_and(|value| wire_side_code(value) & (1 << bit) != 0)
    }

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
