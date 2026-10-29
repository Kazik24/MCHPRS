//! Execution of compiled contribution/notification instructions; no physical propagation.
use super::program::{CompiledResourceInstruction, ComponentKind, ComponentState, EventCondition, EventProgram, Operation, ResourceEventPhase};
use crate::redpiler::program_graph::{ContactId, InputPort, NodeId, PistonId, ResourceId};
use crate::redpiler::TickScheduler;
use mchprs_world::PistonAction;
use mchprs_world::TickPriority;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExecutionCounts {
    pub(crate) source_events: u64,
    pub(crate) contribution_updates: u64,
    pub(crate) notifications: u64,
    pub(crate) scheduled_ticks: u64,
    pub(crate) piston_actions: u64,
    pub(crate) rejected_piston_actions: u64,
    pub(crate) resource_movements: u64,
    pub(crate) motion_completions: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventResourceState {
    pub(crate) at: ContactId,
    pub(crate) enabled: bool,
    pub(crate) revision: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EventPistonState {
    pub(crate) extended: bool,
    pub(crate) head_present: bool,
    pub(crate) pending_motion: bool,
    pub(crate) revision: u64,
}

#[derive(Clone, Copy, Debug)]
struct PistonRequest {
    piston: PistonId,
    action: PistonAction,
    source: Option<NodeId>,
}

#[derive(Clone, Copy, Debug)]
struct MotionState {
    piston: PistonId,
    resource: Option<ResourceId>,
    destination: Option<ContactId>,
    resource_revision: u64,
    piston_revision: u64,
    due_tick: u64,
}

pub(crate) struct EventState {
    program: Arc<EventProgram>,
    components: Box<[ComponentState]>,
    contributions: Box<[u8]>,
    link_active: Box<[bool]>,
    resources: Box<[EventResourceState]>,
    pistons: Box<[EventPistonState]>,
    piston_requests: VecDeque<PistonRequest>,
    motions: VecDeque<MotionState>,
    pending: Box<[bool]>,
    scheduler: TickScheduler<NodeId>,
    logical_tick: u64,
    counts: ExecutionCounts,
}

impl EventState {
    pub(crate) fn new(program: Arc<EventProgram>, logical_tick: u64) -> Self {
        let components: Box<[_]> = program.components.iter().map(|component| component.initial).collect();
        let resources: Box<[_]> = program
            .resources
            .iter()
            .map(|resource| EventResourceState {
                at: resource.initial_at,
                enabled: true,
                revision: 0,
            })
            .collect();
        let pistons: Box<[_]> = program
            .pistons
            .iter()
            .map(|spec| EventPistonState {
                extended: spec.initial_extended,
                head_present: spec.initial_head_present,
                ..Default::default()
            })
            .collect();
        let link_active: Box<[_]> = program
            .links
            .iter()
            .map(|link| {
                link.condition
                    .as_ref()
                    .is_none_or(|condition| condition_matches(&program, &resources, &pistons, condition))
            })
            .collect();
        let contributions = program
            .links
            .iter()
            .zip(link_active.iter())
            .map(|(link, &active)| {
                if active {
                    components[link.source.index()].output.saturating_sub(link.attenuation)
                } else {
                    0
                }
            })
            .collect();
        let pending = vec![false; components.len()].into();
        // Loaded receiver state is retained. Computing contributions does not deliver an update.
        Self {
            program,
            components,
            contributions,
            link_active,
            resources,
            pistons,
            piston_requests: VecDeque::new(),
            motions: VecDeque::new(),
            pending,
            scheduler: TickScheduler::default(),
            logical_tick,
            counts: ExecutionCounts::default(),
        }
    }

    pub(crate) fn component(&self, node: NodeId) -> Option<ComponentState> {
        self.components.get(node.index()).copied()
    }

    pub(crate) fn counts(&self) -> ExecutionCounts {
        self.counts
    }

    pub(crate) fn logical_tick(&self) -> u64 {
        self.logical_tick
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        self.pending.iter().any(|pending| *pending) || !self.piston_requests.is_empty() || !self.motions.is_empty()
    }

    pub(crate) fn resource(&self, resource: ResourceId) -> Option<EventResourceState> {
        self.resources.get(resource.index()).copied()
    }

    pub(crate) fn piston(&self, piston: PistonId) -> Option<EventPistonState> {
        self.pistons.get(piston.index()).copied()
    }

    fn condition_matches(&self, condition: &EventCondition) -> bool {
        condition_matches(&self.program, &self.resources, &self.pistons, condition)
    }

    pub(crate) fn input_strength(&self, node: NodeId) -> Option<u8> {
        let range = self.program.input_ranges.get(node.index())?.clone();
        Some(
            self.program.input_links[range]
                .iter()
                .map(|&link| self.contributions[link])
                .max()
                .unwrap_or(0),
        )
    }

    #[cfg(test)]
    pub(crate) fn link_state_for_test(&self, link_index: usize) -> Option<(u8, bool, u8)> {
        let link = self.program.links.get(link_index)?;
        Some((
            self.components[link.source.index()].output,
            self.link_active[link_index],
            self.contributions[link_index],
        ))
    }

    #[cfg(test)]
    pub(crate) fn scheduled_delay_for_test(&self, node: NodeId) -> Option<usize> {
        self.scheduler
            .iter()
            .find_map(|(scheduled, delay, _)| (*scheduled == node).then_some(delay))
    }

    pub(crate) fn set_input(&mut self, node: NodeId, strength: u8) -> Result<(), &'static str> {
        if strength > 15
            || !self
                .program
                .components
                .get(node.index())
                .is_some_and(|node| node.kind == ComponentKind::Input)
        {
            return Err("invalid event input");
        }
        if self.components[node.index()].output != strength {
            self.components[node.index()] = ComponentState {
                output: strength,
                powered: strength > 0,
            };
            self.run_source(node);
        }
        Ok(())
    }

    /// A represented notification can occur without changing source strength.
    pub(crate) fn notify_source(&mut self, node: NodeId) -> Result<(), &'static str> {
        if !self
            .program
            .components
            .get(node.index())
            .is_some_and(|node| node.kind == ComponentKind::Input)
        {
            return Err("invalid event notification source");
        }
        self.run_source(node);
        Ok(())
    }

    pub(crate) fn queue_piston_action(&mut self, piston: PistonId, action: PistonAction, source: Option<NodeId>) -> Result<bool, &'static str> {
        if piston.index() >= self.program.pistons.len() || source.is_some_and(|source| source.index() >= self.components.len()) {
            return Err("invalid piston request");
        }
        if self
            .piston_requests
            .iter()
            .any(|request| request.piston == piston && request.action == action)
        {
            return Ok(false);
        }
        self.piston_requests.push_back(PistonRequest { piston, action, source });
        Ok(true)
    }

    fn run_source(&mut self, source: NodeId) {
        let mut sources = VecDeque::from([source]);
        while let Some(source) = sources.pop_front() {
            self.counts.source_events += 1;
            for index in self.program.source_ranges[source.index()].clone() {
                match self.program.operations[index] {
                    Operation::Contribution(id) => {
                        self.refresh_link(id);
                    }
                    Operation::RecomputeWire(wire) => {
                        if self.refresh_wire(wire) {
                            sources.push_back(wire);
                        }
                    }
                    Operation::Notify(receiver) => {
                        self.counts.notifications += 1;
                        self.update_receiver(receiver);
                    }
                    Operation::NotifyObserver { target, shape } => {
                        self.counts.notifications += 1;
                        if shape {
                            self.update_observer(target);
                        }
                    }
                }
            }
            if self.program.components[source.index()].kind == ComponentKind::Wire {
                let receivers = self.program.wire_watchers[source.index()].to_vec();
                for receiver in receivers {
                    self.counts.notifications += 1;
                    self.update_receiver(receiver);
                }
            }
        }
    }

    fn refresh_wire(&mut self, wire: NodeId) -> bool {
        let output = self.input_strength(wire).expect("compiled wire input range");
        if self.components[wire.index()].output == output {
            return false;
        }
        self.components[wire.index()] = ComponentState { output, powered: output > 0 };
        true
    }

    fn update_receiver(&mut self, node: NodeId) {
        match self.program.components[node.index()].kind {
            ComponentKind::Lamp => {
                if self.input_strength(node).unwrap() > 0 {
                    self.components[node.index()].powered = true;
                } else if self.components[node.index()].powered && !self.pending[node.index()] {
                    self.scheduler.schedule_half_tick(node, 4, TickPriority::Normal);
                    self.pending[node.index()] = true;
                }
            }
            ComponentKind::Piston => {
                let Some(index) = self.program.pistons.iter().position(|spec| spec.input == node) else {
                    unreachable!("validated piston receiver")
                };
                let action = if self.input_strength(node).unwrap() > 0 && !self.pistons[index].extended {
                    Some(PistonAction::Extend)
                } else if self.input_strength(node).unwrap() == 0 && self.pistons[index].extended {
                    Some(PistonAction::Retract)
                } else {
                    None
                };
                if let Some(action) = action {
                    let _ = self.queue_piston_action(PistonId::new(index as u32), action, None);
                }
            }
            ComponentKind::Repeater { delay } => {
                let powered = self.input_strength(node).unwrap() > 0;
                if powered != self.components[node.index()].powered && !self.pending[node.index()] {
                    self.scheduler.schedule_half_tick(node, usize::from(delay) * 2, TickPriority::Normal);
                    self.pending[node.index()] = true;
                }
            }
            ComponentKind::Input | ComponentKind::Observer | ComponentKind::Wire => unreachable!("validated notification target"),
        }
    }

    fn update_observer(&mut self, node: NodeId) {
        if !self.components[node.index()].powered && !self.pending[node.index()] {
            self.scheduler.schedule_half_tick(node, 2, TickPriority::Normal);
            self.pending[node.index()] = true;
        }
    }

    fn refresh_link(&mut self, id: usize) -> bool {
        let link = &self.program.links[id];
        let active = link.condition.as_ref().is_none_or(|condition| self.condition_matches(condition));
        let contribution = if active {
            self.components[link.source.index()].output.saturating_sub(link.attenuation)
        } else {
            0
        };
        let changed = self.link_active[id] != active || self.contributions[id] != contribution;
        self.link_active[id] = active;
        self.contributions[id] = contribution;
        self.counts.contribution_updates += 1;
        changed
    }

    pub(crate) fn tick(&mut self) -> Result<(), &'static str> {
        self.logical_tick = self.logical_tick.checked_add(1).ok_or("event clock overflow")?;
        self.run_due_ticks();
        self.scheduler.end_last_tick_move_next();
        self.run_due_ticks();
        self.run_piston_actions()?;
        self.complete_motions();
        Ok(())
    }

    fn run_due_ticks(&mut self) {
        loop {
            let Some(node) = self.scheduler.this_tick().pop_first() else { break };
            self.pending[node.index()] = false;
            self.counts.scheduled_ticks += 1;
            match self.program.components[node.index()].kind {
                ComponentKind::Lamp => {
                    if self.input_strength(node).unwrap() == 0 {
                        self.components[node.index()].powered = false;
                    }
                }
                ComponentKind::Observer => {
                    let state = &mut self.components[node.index()];
                    state.powered = !state.powered;
                    state.output = if state.powered { 15 } else { 0 };
                    if state.powered {
                        self.scheduler.schedule_half_tick(node, 2, TickPriority::Normal);
                        self.pending[node.index()] = true;
                    }
                    self.run_source(node);
                }
                ComponentKind::Repeater { .. } => {
                    let powered = self.input_strength(node).unwrap() > 0;
                    if powered != self.components[node.index()].powered {
                        self.components[node.index()] = ComponentState {
                            output: if powered { 15 } else { 0 },
                            powered,
                        };
                        self.run_source(node);
                    }
                }
                ComponentKind::Input | ComponentKind::Wire | ComponentKind::Piston => unreachable!("inputs and pistons cannot have scheduled work"),
            }
        }
    }

    fn run_piston_actions(&mut self) -> Result<(), &'static str> {
        if !self.piston_requests.is_empty() && self.logical_tick.checked_add(2).is_none() {
            return Err("event clock overflow");
        }
        while let Some(request) = self.piston_requests.pop_front() {
            let _source = request.source;
            let accepted = self.apply_piston_action(request.piston, request.action)?;
            if !accepted {
                self.counts.rejected_piston_actions += 1;
            }
        }
        Ok(())
    }

    fn apply_piston_action(&mut self, id: PistonId, action: PistonAction) -> Result<bool, &'static str> {
        let spec = *self.program.pistons.get(id.index()).ok_or("piston is unavailable")?;
        let state = self.pistons[id.index()];
        let powered = self.input_strength(spec.input).ok_or("piston input is unavailable")? > 0;
        let due_tick = self.logical_tick.checked_add(2).ok_or("event clock overflow")?;
        if state.pending_motion
            || (action == PistonAction::Extend && (!powered || state.extended))
            || (action != PistonAction::Extend && (powered || !state.extended))
        {
            return Ok(false);
        }

        let destination = match (action, spec.resource) {
            (PistonAction::Extend, Some(resource)) => {
                let current = self.resources[resource.index()];
                (current.enabled && current.at == spec.near).then_some(spec.far)
            }
            (PistonAction::Retract, Some(resource)) if spec.sticky => {
                let current = self.resources[resource.index()];
                (current.enabled && current.at == spec.far && self.resource_destination_free(resource, spec.near)).then_some(spec.near)
            }
            _ => None,
        };
        if destination.is_some_and(|contact| {
            let resource = spec.resource.expect("destination implies resource");
            !self.resource_destination_free(resource, contact)
        }) {
            return Ok(false);
        }

        let mut resource_revision = 0;
        if let (Some(resource), Some(contact)) = (spec.resource, destination) {
            let old = self.resources[resource.index()].at;
            let state = &mut self.resources[resource.index()];
            state.at = contact;
            state.enabled = false;
            state.revision = state.revision.wrapping_add(1);
            resource_revision = state.revision;
            self.counts.resource_movements += 1;
            self.run_resource_event(resource, old, ResourceEventPhase::Departure);
        }

        let piston_state = &mut self.pistons[id.index()];
        piston_state.extended = action == PistonAction::Extend;
        piston_state.head_present = piston_state.extended;
        piston_state.pending_motion = true;
        piston_state.revision = piston_state.revision.wrapping_add(1);
        let piston_revision = piston_state.revision;
        self.run_piston_event(id);
        self.motions.push_back(MotionState {
            piston: id,
            resource: spec.resource.filter(|_| destination.is_some()),
            destination,
            resource_revision,
            piston_revision,
            due_tick,
        });
        self.counts.piston_actions += 1;
        Ok(true)
    }

    fn resource_destination_free(&self, resource: ResourceId, contact: ContactId) -> bool {
        self.resources
            .iter()
            .enumerate()
            .all(|(index, other)| index == resource.index() || other.at != contact)
    }

    fn complete_motions(&mut self) {
        let mut index = 0;
        while index < self.motions.len() {
            if self.motions[index].due_tick > self.logical_tick {
                index += 1;
                continue;
            }
            let motion = self.motions.remove(index).unwrap();
            if self.pistons[motion.piston.index()].revision != motion.piston_revision {
                continue;
            }
            self.pistons[motion.piston.index()].pending_motion = false;
            if let (Some(resource), Some(contact)) = (motion.resource, motion.destination) {
                let current = self.resources[resource.index()];
                if current.revision == motion.resource_revision && current.at == contact && !current.enabled {
                    let state = &mut self.resources[resource.index()];
                    state.enabled = true;
                    state.revision = state.revision.wrapping_add(1);
                    self.run_resource_event(resource, contact, ResourceEventPhase::Arrival);
                }
            }
            self.run_piston_event(motion.piston);
            self.counts.motion_completions += 1;
        }
    }

    fn run_resource_event(&mut self, resource: ResourceId, contact: ContactId, phase: ResourceEventPhase) {
        let range_index = resource.index() * 2 + if phase == ResourceEventPhase::Arrival { 1 } else { 0 };
        let range = self.program.resource_ranges[range_index].clone();
        let mut changed_targets = HashSet::new();
        for index in range {
            let (operation_contact, instruction) = self.program.resource_operations[index];
            if operation_contact != contact {
                continue;
            }
            match instruction {
                CompiledResourceInstruction::Contribution(link) => {
                    let target = self.program.links[link].target;
                    if self.refresh_link(link) {
                        if self.program.links[link].port != InputPort::Quasi {
                            changed_targets.insert(target);
                        }
                    }
                }
                CompiledResourceInstruction::RecomputeWire(wire) => {
                    if self.refresh_wire(wire) {
                        self.run_source(wire);
                    }
                }
                CompiledResourceInstruction::NotifyIfChanged(target) => {
                    if changed_targets.remove(&target) {
                        self.counts.notifications += 1;
                        self.update_receiver(target);
                    }
                }
                CompiledResourceInstruction::Notify(target) => {
                    self.counts.notifications += 1;
                    self.update_receiver(target);
                }
                CompiledResourceInstruction::NotifyObserver { target, shape } => {
                    self.counts.notifications += 1;
                    if shape {
                        self.update_observer(target);
                    }
                }
            }
        }
    }

    fn run_piston_event(&mut self, piston: PistonId) {
        let range = self.program.piston_ranges[piston.index()].clone();
        let links = self.program.piston_links[range].to_vec();
        let mut changed_targets = Vec::new();
        let mut seen = HashSet::new();
        for link in links {
            if self.refresh_link(link) {
                let target = self.program.links[link].target;
                if seen.insert(target) {
                    changed_targets.push(target);
                }
            }
        }
        for target in changed_targets {
            if self.program.components[target.index()].kind == ComponentKind::Wire {
                if self.refresh_wire(target) {
                    self.run_source(target);
                }
            } else {
                self.counts.notifications += 1;
                self.update_receiver(target);
            }
        }
    }
}

fn condition_matches(program: &EventProgram, resources: &[EventResourceState], pistons: &[EventPistonState], condition: &EventCondition) -> bool {
    match condition {
        EventCondition::ResourceAt { resource, contact } => resources[resource.index()].at == *contact,
        EventCondition::ResourceEnabled(resource) => resources[resource.index()].enabled,
        EventCondition::PistonExtended { piston, extended } => pistons[piston.index()].extended == *extended,
        EventCondition::PistonHeadPresent { piston, present } => pistons[piston.index()].head_present == *present,
        EventCondition::PistonReady(piston) => !pistons[piston.index()].pending_motion,
        EventCondition::ContactTest { test, variants, .. } => {
            let Some(profile) = variants
                .iter()
                .find(|variant| condition_matches(program, resources, pistons, &variant.condition))
                .map(|variant| variant.profile)
            else {
                return false;
            };
            match test {
                crate::redpiler::program_graph::SiteTest::Solid => profile.solid,
                crate::redpiler::program_graph::SiteTest::Transparent => profile.transparent,
                crate::redpiler::program_graph::SiteTest::IsWire => profile.wire,
                crate::redpiler::program_graph::SiteTest::Profile(id) => profile.id == *id,
            }
        }
        EventCondition::Not(child) => !condition_matches(program, resources, pistons, child),
        EventCondition::All(children) => children.iter().all(|child| condition_matches(program, resources, pistons, child)),
        EventCondition::Any(children) => children.iter().any(|child| condition_matches(program, resources, pistons, child)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::event_netlist::program::{
        Component, ElectricalLink, EventCondition, EventContactVariant, EventPiston, EventProfile, EventResource, ResourceEventPhase,
        ResourceInstruction, ResourceOperation,
    };

    const A: NodeId = NodeId::new(0);
    const B: NodeId = NodeId::new(1);
    const OBSERVER: NodeId = NodeId::new(2);
    const LAMP: NodeId = NodeId::new(2);
    const OBSERVER_LAMP: NodeId = NodeId::new(3);
    const PISTON_A: NodeId = NodeId::new(3);
    const PISTON_B: NodeId = NodeId::new(4);
    const CONTACT_A: ContactId = ContactId::new(0);
    const CONTACT_B: ContactId = ContactId::new(1);
    const CONTACT_C: ContactId = ContactId::new(2);
    const RESOURCE_A: ResourceId = ResourceId::new(0);
    const RESOURCE_B: ResourceId = ResourceId::new(1);

    fn input(output: u8) -> Component {
        Component {
            kind: ComponentKind::Input,
            initial: ComponentState { output, powered: output > 0 },
        }
    }

    fn lamp(powered: bool) -> Component {
        Component {
            kind: ComponentKind::Lamp,
            initial: ComponentState { output: 0, powered },
        }
    }

    fn wire(output: u8) -> Component {
        Component {
            kind: ComponentKind::Wire,
            initial: ComponentState { output, powered: output > 0 },
        }
    }

    fn observer(powered: bool) -> Component {
        Component {
            kind: ComponentKind::Observer,
            initial: ComponentState {
                output: if powered { 15 } else { 0 },
                powered,
            },
        }
    }

    fn piston_component() -> Component {
        Component {
            kind: ComponentKind::Piston,
            initial: ComponentState::default(),
        }
    }

    #[test]
    fn piston_quasi_power_is_live_but_does_not_deliver_an_update() {
        let piston = NodeId::new(2);
        let piston_id = PistonId::new(0);
        let program = EventProgram::new_with_movement(
            vec![input(15), input(15), piston_component()],
            vec![
                ElectricalLink {
                    source: A,
                    target: piston,
                    port: InputPort::Main,
                    attenuation: 0,
                    condition: None,
                },
                ElectricalLink {
                    source: B,
                    target: piston,
                    port: InputPort::Quasi,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![
                (A, Operation::Contribution(0)),
                (A, Operation::Notify(piston)),
                (B, Operation::Contribution(1)),
            ],
            vec![],
            vec![EventPiston {
                input: piston,
                resource: None,
                near: CONTACT_A,
                far: CONTACT_B,
                sticky: false,
                initial_extended: true,
                initial_head_present: true,
            }],
            vec![],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);

        state.set_input(A, 0).unwrap();
        assert_eq!(state.input_strength(piston), Some(15));
        assert!(state.piston(piston_id).unwrap().extended);

        let notifications = state.counts().notifications;
        state.set_input(B, 0).unwrap();
        assert_eq!(state.counts().notifications, notifications);
        assert_eq!(state.input_strength(piston), Some(0));
        assert!(state.piston(piston_id).unwrap().extended);

        state.notify_source(A).unwrap();
        assert_eq!(state.piston_requests.len(), 1);
        state.set_input(B, 15).unwrap();
        state.tick().unwrap();
        assert!(
            state.piston(piston_id).unwrap().extended,
            "stale retract must be rejected against live quasi power"
        );
        assert_eq!(state.counts().rejected_piston_actions, 1);

        state.set_input(B, 0).unwrap();
        state.notify_source(A).unwrap();
        state.tick().unwrap();
        assert!(!state.piston(piston_id).unwrap().extended);
        assert_eq!(state.counts().piston_actions, 1);
    }

    #[test]
    fn watched_wire_change_notifies_piston_only_when_strength_changes() {
        const WIRE: NodeId = NodeId::new(2);
        const PISTON: NodeId = NodeId::new(3);
        const QUASI: NodeId = NodeId::new(4);
        let program = EventProgram::new_with_wire_watchers(
            vec![input(0), input(12), wire(12), piston_component(), input(0)],
            vec![
                ElectricalLink {
                    source: A,
                    target: WIRE,
                    port: InputPort::Main,
                    attenuation: 0,
                    condition: None,
                },
                ElectricalLink {
                    source: B,
                    target: WIRE,
                    port: InputPort::Main,
                    attenuation: 0,
                    condition: None,
                },
                ElectricalLink {
                    source: QUASI,
                    target: PISTON,
                    port: InputPort::Quasi,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![
                (A, Operation::Contribution(0)),
                (A, Operation::RecomputeWire(WIRE)),
                (B, Operation::Contribution(1)),
                (B, Operation::RecomputeWire(WIRE)),
                (QUASI, Operation::Contribution(2)),
            ],
            vec![],
            vec![EventPiston {
                input: PISTON,
                resource: None,
                near: CONTACT_A,
                far: CONTACT_B,
                sticky: false,
                initial_extended: true,
                initial_head_present: true,
            }],
            vec![],
            vec![(WIRE, PISTON)],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);

        state.set_input(A, 8).unwrap();
        assert_eq!(state.component(WIRE).unwrap().output, 12, "the stronger source masks this change");
        assert_eq!(state.counts().notifications, 0);

        state.set_input(B, 0).unwrap();
        assert_eq!(state.component(WIRE).unwrap().output, 8);
        assert_eq!(state.piston_requests.len(), 1, "a changed watched wire delivers a piston update");

        state.set_input(QUASI, 15).unwrap();
        state.tick().unwrap();
        assert!(
            state.piston(PistonId::new(0)).unwrap().extended,
            "the queued request samples live quasi power"
        );
        assert_eq!(state.counts().rejected_piston_actions, 1);
    }

    #[test]
    fn compiled_contributions_preserve_masking_and_lamp_delay() {
        let program = EventProgram::new(
            vec![input(15), input(15), lamp(false)],
            vec![
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: LAMP,
                    attenuation: 2,
                    condition: None,
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: B,
                    target: LAMP,
                    attenuation: 4,
                    condition: None,
                },
            ],
            vec![
                (A, Operation::Contribution(0)),
                (A, Operation::Notify(LAMP)),
                (B, Operation::Contribution(1)),
                (B, Operation::Notify(LAMP)),
            ],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 20);
        assert_eq!(state.input_strength(LAMP), Some(13));
        assert!(!state.component(LAMP).unwrap().powered, "loaded state is not normalized");
        state.notify_source(A).unwrap();
        assert!(state.component(LAMP).unwrap().powered);
        state.set_input(A, 0).unwrap();
        assert_eq!(state.input_strength(LAMP), Some(11));
        assert!(state.component(LAMP).unwrap().powered);
        state.set_input(B, 0).unwrap();
        for _ in 0..3 {
            state.tick().unwrap();
            assert!(state.component(LAMP).unwrap().powered);
        }
        state.notify_source(B).unwrap();
        state.tick().unwrap();
        assert!(!state.component(LAMP).unwrap().powered);
        assert!(!state.has_pending_work());
        assert_eq!(state.logical_tick(), 24);
        assert_eq!(state.counts().scheduled_ticks, 1, "repeated notifications preserve the deadline");
    }

    #[test]
    fn unrelated_input_does_not_normalize_an_unnotified_receiver() {
        let program = EventProgram::new(vec![input(0), input(0), lamp(true)], vec![], vec![(A, Operation::Notify(LAMP))]).unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        state.set_input(B, 15).unwrap();
        for _ in 0..8 {
            state.tick().unwrap();
        }
        assert!(state.component(LAMP).unwrap().powered);
        assert!(!state.has_pending_work());
        state.notify_source(A).unwrap();
        for _ in 0..4 {
            state.tick().unwrap();
        }
        assert!(!state.component(LAMP).unwrap().powered);
    }

    #[test]
    fn source_instruction_order_controls_when_receiver_reads_contributions() {
        let program = EventProgram::new(
            vec![input(0), input(0), lamp(false)],
            vec![ElectricalLink {
                port: InputPort::Main,
                source: A,
                target: LAMP,
                attenuation: 0,
                condition: None,
            }],
            vec![(A, Operation::Notify(LAMP)), (A, Operation::Contribution(0))],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        state.set_input(A, 15).unwrap();
        assert_eq!(state.input_strength(LAMP), Some(15));
        assert!(!state.component(LAMP).unwrap().powered);
        state.notify_source(A).unwrap();
        assert!(state.component(LAMP).unwrap().powered);
        let before = state.counts();
        assert!(state.set_input(A, 16).is_err());
        assert_eq!(state.component(A).unwrap().output, 15);
        assert_eq!(state.counts(), before);
    }

    #[test]
    fn malformed_link_binding_is_rejected_before_execution() {
        let links = vec![ElectricalLink {
            port: InputPort::Main,
            source: A,
            target: LAMP,
            attenuation: 0,
            condition: None,
        }];
        assert!(EventProgram::new(vec![input(0), input(0), lamp(false)], links.clone(), vec![]).is_err());
        assert!(EventProgram::new(vec![input(0), input(0), lamp(false)], links, vec![(B, Operation::Contribution(0))]).is_err());
        assert!(EventProgram::new(vec![input(0)], vec![], vec![(A, Operation::Notify(LAMP))]).is_err());
    }

    fn observer_program() -> EventProgram {
        EventProgram::new(
            vec![input(0), input(0), observer(false), lamp(false)],
            vec![ElectricalLink {
                port: InputPort::Main,
                source: OBSERVER,
                target: OBSERVER_LAMP,
                attenuation: 0,
                condition: None,
            }],
            vec![
                (
                    A,
                    Operation::NotifyObserver {
                        target: OBSERVER,
                        shape: true,
                    },
                ),
                (
                    B,
                    Operation::NotifyObserver {
                        target: OBSERVER,
                        shape: false,
                    },
                ),
                (OBSERVER, Operation::Contribution(0)),
                (OBSERVER, Operation::Notify(OBSERVER_LAMP)),
            ],
        )
        .unwrap()
    }

    #[test]
    fn unchanged_source_notification_triggers_compiled_observer_pulse_and_lamp_timeline() {
        let mut state = EventState::new(Arc::new(observer_program()), 0);
        state.notify_source(A).unwrap();
        assert!(!state.component(OBSERVER).unwrap().powered);
        assert!(!state.component(OBSERVER_LAMP).unwrap().powered);

        state.tick().unwrap();
        assert!(!state.component(OBSERVER).unwrap().powered);
        state.tick().unwrap();
        assert!(state.component(OBSERVER).unwrap().powered);
        assert!(state.component(OBSERVER_LAMP).unwrap().powered);
        assert_eq!(state.component(OBSERVER).unwrap().output, 15);

        state.tick().unwrap();
        assert!(state.component(OBSERVER).unwrap().powered);
        assert!(state.component(OBSERVER_LAMP).unwrap().powered);
        state.tick().unwrap();
        assert!(!state.component(OBSERVER).unwrap().powered);
        assert!(state.component(OBSERVER_LAMP).unwrap().powered, "lamp uses its four-tick turn-off delay");
        assert_eq!(state.component(OBSERVER).unwrap().output, 0);
        for _ in 0..3 {
            state.tick().unwrap();
            assert!(state.component(OBSERVER_LAMP).unwrap().powered);
        }
        state.tick().unwrap();
        assert!(!state.component(OBSERVER_LAMP).unwrap().powered);
        assert_eq!(state.counts().scheduled_ticks, 3);
    }

    #[test]
    fn duplicate_observer_notifications_do_not_duplicate_or_postpone_pending_pulse() {
        let mut state = EventState::new(Arc::new(observer_program()), 0);
        // An update-only event does not reach an observer watcher.
        state.notify_source(B).unwrap();
        assert_eq!(state.scheduler.iter().count(), 0, "update-only notification does not trigger an observer");
        state.notify_source(A).unwrap();
        state.notify_source(A).unwrap();
        assert_eq!(state.scheduler.iter().count(), 1);
        state.tick().unwrap();
        state.notify_source(A).unwrap();
        assert!(!state.component(OBSERVER).unwrap().powered);
        state.tick().unwrap();
        assert!(state.component(OBSERVER).unwrap().powered, "first notification deadline is retained");
        state.notify_source(A).unwrap();
        state.tick().unwrap();
        state.notify_source(A).unwrap();
        assert!(state.component(OBSERVER).unwrap().powered);
        state.tick().unwrap();
        assert!(!state.component(OBSERVER).unwrap().powered, "on-pulse off deadline is retained");
        assert_eq!(state.counts().scheduled_ticks, 2);
    }

    #[test]
    fn shared_resource_departure_and_arrival_drive_observer_and_conditional_receiver() {
        const CONDITIONAL_LAMP: NodeId = NodeId::new(4);
        const PULSE_LAMP: NodeId = NodeId::new(5);
        let program = EventProgram::new_with_movement(
            vec![input(0), input(0), observer(false), piston_component(), lamp(false), lamp(false)],
            vec![
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: PISTON_A,
                    attenuation: 0,
                    condition: None,
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: CONDITIONAL_LAMP,
                    attenuation: 0,
                    condition: Some(EventCondition::All(
                        vec![
                            EventCondition::ResourceAt {
                                resource: RESOURCE_A,
                                contact: CONTACT_A,
                            },
                            EventCondition::ResourceEnabled(RESOURCE_A),
                        ]
                        .into_boxed_slice(),
                    )),
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: CONDITIONAL_LAMP,
                    attenuation: 0,
                    condition: Some(EventCondition::All(
                        vec![
                            EventCondition::ResourceAt {
                                resource: RESOURCE_A,
                                contact: CONTACT_B,
                            },
                            EventCondition::ResourceEnabled(RESOURCE_A),
                        ]
                        .into_boxed_slice(),
                    )),
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: OBSERVER,
                    target: PULSE_LAMP,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![
                (A, Operation::Contribution(0)),
                (A, Operation::Notify(PISTON_A)),
                (A, Operation::Contribution(1)),
                (A, Operation::Notify(CONDITIONAL_LAMP)),
                (A, Operation::Contribution(2)),
                (A, Operation::Notify(CONDITIONAL_LAMP)),
                (OBSERVER, Operation::Contribution(3)),
                (OBSERVER, Operation::Notify(PULSE_LAMP)),
            ],
            vec![EventResource {
                contacts: vec![CONTACT_A, CONTACT_B].into(),
                initial_at: CONTACT_A,
            }],
            vec![EventPiston {
                input: PISTON_A,
                resource: Some(RESOURCE_A),
                near: CONTACT_A,
                far: CONTACT_B,
                sticky: true,
                initial_extended: false,
                initial_head_present: false,
            }],
            vec![
                ResourceOperation {
                    resource: RESOURCE_A,
                    contact: CONTACT_A,
                    phase: ResourceEventPhase::Departure,
                    instruction: ResourceInstruction::NotifyObserver {
                        target: OBSERVER,
                        shape: true,
                    },
                },
                ResourceOperation {
                    resource: RESOURCE_A,
                    contact: CONTACT_B,
                    phase: ResourceEventPhase::Arrival,
                    instruction: ResourceInstruction::NotifyObserver {
                        target: OBSERVER,
                        shape: true,
                    },
                },
            ],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        state.set_input(A, 15).unwrap();
        assert!(state.component(CONDITIONAL_LAMP).unwrap().powered);
        assert_eq!(state.input_strength(CONDITIONAL_LAMP), Some(15));

        state.tick().unwrap();
        assert_eq!(state.counts().piston_actions, 1);
        assert_eq!(state.counts().resource_movements, 1);
        assert_eq!(
            state.resource(RESOURCE_A).unwrap(),
            EventResourceState {
                at: CONTACT_B,
                enabled: false,
                revision: 1,
            }
        );
        assert_eq!(
            state.input_strength(CONDITIONAL_LAMP),
            Some(0),
            "departure withdraws old conditional source: contributions={:?}, updates={}",
            state.contributions,
            state.counts.contribution_updates
        );
        assert!(!state.component(OBSERVER).unwrap().powered);

        state.tick().unwrap();
        assert!(!state.resource(RESOURCE_A).unwrap().enabled);
        assert!(!state.component(OBSERVER).unwrap().powered);

        // Graph publishes the extended piston/resource arrival two logical ticks after action.
        state.tick().unwrap();
        assert!(state.resource(RESOURCE_A).unwrap().enabled);
        assert_eq!(
            state.input_strength(CONDITIONAL_LAMP),
            Some(15),
            "arrival enables the new conditional source"
        );
        assert!(state.component(OBSERVER).unwrap().powered);
        assert!(state.component(PULSE_LAMP).unwrap().powered);

        state.tick().unwrap();
        assert!(state.component(OBSERVER).unwrap().powered);
        state.tick().unwrap();
        assert!(!state.component(OBSERVER).unwrap().powered);
        assert!(state.component(PULSE_LAMP).unwrap().powered);
        for _ in 0..3 {
            state.tick().unwrap();
            assert!(state.component(PULSE_LAMP).unwrap().powered);
        }
        state.tick().unwrap();
        assert!(!state.component(PULSE_LAMP).unwrap().powered);
        assert_eq!(state.counts().notifications, 9);
        assert_eq!(state.counts().motion_completions, 1);
    }

    #[test]
    fn resource_updates_notify_only_when_a_compiled_link_switches() {
        const DRIVER: NodeId = NodeId::new(1);
        const PISTON: NodeId = NodeId::new(2);
        const TARGET: NodeId = NodeId::new(3);
        let at_a = EventCondition::All(
            vec![
                EventCondition::ResourceAt {
                    resource: RESOURCE_A,
                    contact: CONTACT_A,
                },
                EventCondition::ResourceEnabled(RESOURCE_A),
            ]
            .into_boxed_slice(),
        );
        let at_b = EventCondition::All(
            vec![
                EventCondition::ResourceAt {
                    resource: RESOURCE_A,
                    contact: CONTACT_B,
                },
                EventCondition::ResourceEnabled(RESOURCE_A),
            ]
            .into_boxed_slice(),
        );
        let program = EventProgram::new_with_movement(
            vec![input(0), input(0), piston_component(), lamp(false)],
            vec![
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: TARGET,
                    attenuation: 0,
                    condition: Some(at_a),
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: TARGET,
                    attenuation: 0,
                    condition: Some(at_b),
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: DRIVER,
                    target: PISTON,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![
                (A, Operation::Contribution(0)),
                (A, Operation::Contribution(1)),
                (DRIVER, Operation::Contribution(2)),
                (DRIVER, Operation::Notify(PISTON)),
            ],
            vec![EventResource {
                contacts: vec![CONTACT_A, CONTACT_B, CONTACT_C].into(),
                initial_at: CONTACT_A,
            }],
            vec![EventPiston {
                input: PISTON,
                resource: Some(RESOURCE_A),
                near: CONTACT_A,
                far: CONTACT_B,
                sticky: true,
                initial_extended: false,
                initial_head_present: false,
            }],
            Vec::new(),
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        assert_eq!(state.link_state_for_test(0), Some((0, true, 0)));
        state.set_input(DRIVER, 15).unwrap();
        assert_eq!(state.counts().notifications, 1, "only the explicit piston update has run");

        state.tick().unwrap();
        assert_eq!(state.counts().notifications, 2, "the zero strength link switched at departure");
        assert_eq!(state.link_state_for_test(0), Some((0, false, 0)));
        assert_eq!(state.link_state_for_test(1), Some((0, false, 0)));
        state.tick().unwrap();
        assert_eq!(state.counts().notifications, 2, "an unchanged guard during motion does not notify");
        state.tick().unwrap();
        assert_eq!(state.counts().notifications, 3, "the destination link switched on arrival");
        assert_eq!(state.link_state_for_test(1), Some((0, true, 0)));
    }

    #[test]
    fn piston_guarded_links_refresh_on_action_and_motion_completion() {
        const DRIVER: NodeId = NodeId::new(1);
        const PISTON: NodeId = NodeId::new(2);
        const EXTENDED_LAMP: NodeId = NodeId::new(3);
        const READY_LAMP: NodeId = NodeId::new(4);
        let program = EventProgram::new_with_movement(
            vec![input(15), input(0), piston_component(), lamp(false), lamp(true)],
            vec![
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: EXTENDED_LAMP,
                    attenuation: 0,
                    condition: Some(EventCondition::PistonExtended {
                        piston: PistonId::new(0),
                        extended: true,
                    }),
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: READY_LAMP,
                    attenuation: 0,
                    condition: Some(EventCondition::PistonReady(PistonId::new(0))),
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: DRIVER,
                    target: PISTON,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![
                (A, Operation::Contribution(0)),
                (A, Operation::Notify(EXTENDED_LAMP)),
                (A, Operation::Contribution(1)),
                (A, Operation::Notify(READY_LAMP)),
                (DRIVER, Operation::Contribution(2)),
                (DRIVER, Operation::Notify(PISTON)),
            ],
            Vec::new(),
            vec![EventPiston {
                input: PISTON,
                resource: None,
                near: CONTACT_A,
                far: CONTACT_B,
                sticky: false,
                initial_extended: false,
                initial_head_present: false,
            }],
            Vec::new(),
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        assert_eq!(state.input_strength(EXTENDED_LAMP), Some(0));
        assert_eq!(state.input_strength(READY_LAMP), Some(15));

        state.set_input(DRIVER, 15).unwrap();
        state.tick().unwrap();
        assert!(state.piston(PistonId::new(0)).unwrap().extended);
        assert_eq!(state.input_strength(EXTENDED_LAMP), Some(15));
        assert_eq!(state.input_strength(READY_LAMP), Some(0));
        assert!(state.component(EXTENDED_LAMP).unwrap().powered);

        state.tick().unwrap();
        state.tick().unwrap();
        assert!(!state.piston(PistonId::new(0)).unwrap().pending_motion);
        assert_eq!(state.input_strength(READY_LAMP), Some(15));
        state.tick().unwrap();
        state.tick().unwrap();
        assert!(
            state.component(READY_LAMP).unwrap().powered,
            "arrival refreshed the ready-guarded source before its pending off tick"
        );
    }

    #[test]
    fn negated_contact_guard_preserves_position_and_enabled_predicates() {
        use crate::redpiler::program_graph::ProfileId;
        use crate::redpiler::program_graph::SiteTest;

        let occupied = EventCondition::All(
            vec![
                EventCondition::ResourceAt {
                    resource: RESOURCE_A,
                    contact: CONTACT_A,
                },
                EventCondition::ResourceEnabled(RESOURCE_A),
            ]
            .into_boxed_slice(),
        );
        let condition = EventCondition::Not(Box::new(EventCondition::ContactTest {
            contact: CONTACT_A,
            test: SiteTest::Solid,
            variants: vec![
                EventContactVariant {
                    profile: EventProfile {
                        id: ProfileId::new(0),
                        solid: true,
                        transparent: false,
                        wire: false,
                    },
                    condition: occupied.clone(),
                },
                EventContactVariant {
                    profile: EventProfile {
                        id: ProfileId::new(1),
                        solid: false,
                        transparent: true,
                        wire: false,
                    },
                    condition: EventCondition::Not(Box::new(occupied)),
                },
            ]
            .into_boxed_slice(),
        }));
        let program = EventProgram::new_with_movement(
            vec![input(0)],
            vec![],
            vec![],
            vec![EventResource {
                contacts: vec![CONTACT_A, CONTACT_B].into(),
                initial_at: CONTACT_A,
            }],
            vec![],
            vec![],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        let original = |resource: EventResourceState| !(resource.enabled && resource.at == CONTACT_A);

        let at_enabled = state.resource(RESOURCE_A).unwrap();
        assert_eq!(state.condition_matches(&condition), original(at_enabled));
        assert!(!state.condition_matches(&condition));

        state.resources[0].at = CONTACT_B;
        let elsewhere_enabled = state.resource(RESOURCE_A).unwrap();
        assert_eq!(state.condition_matches(&condition), original(elsewhere_enabled));
        assert!(state.condition_matches(&condition));

        state.resources[0].at = CONTACT_A;
        state.resources[0].enabled = false;
        let in_motion = state.resource(RESOURCE_A).unwrap();
        assert_eq!(state.condition_matches(&condition), original(in_motion));
        assert!(state.condition_matches(&condition));
    }

    fn shared_resource_program() -> EventProgram {
        EventProgram::new_with_movement(
            vec![input(0), input(0), input(0), piston_component(), piston_component()],
            vec![
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: PISTON_A,
                    attenuation: 0,
                    condition: None,
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: B,
                    target: PISTON_B,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![(A, Operation::Contribution(0)), (B, Operation::Contribution(1))],
            vec![EventResource {
                contacts: vec![CONTACT_A, CONTACT_B, CONTACT_C].into(),
                initial_at: CONTACT_A,
            }],
            vec![
                EventPiston {
                    input: PISTON_A,
                    resource: Some(RESOURCE_A),
                    near: CONTACT_A,
                    far: CONTACT_B,
                    sticky: true,
                    initial_extended: false,
                    initial_head_present: false,
                },
                EventPiston {
                    input: PISTON_B,
                    resource: Some(RESOURCE_A),
                    near: CONTACT_A,
                    far: CONTACT_C,
                    sticky: true,
                    initial_extended: false,
                    initial_head_present: false,
                },
            ],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn two_pistons_share_one_resource_and_fifo_order_selects_the_mover() {
        for (first, second, destination, other) in [
            (PistonId::new(0), PistonId::new(1), CONTACT_B, PistonId::new(1)),
            (PistonId::new(1), PistonId::new(0), CONTACT_C, PistonId::new(0)),
        ] {
            let mut state = EventState::new(Arc::new(shared_resource_program()), 0);
            state.set_input(A, 15).unwrap();
            state.set_input(B, 15).unwrap();
            assert!(state.queue_piston_action(first, PistonAction::Extend, None).unwrap());
            assert!(state.queue_piston_action(second, PistonAction::Extend, None).unwrap());
            state.tick().unwrap();
            assert_eq!(state.resource(RESOURCE_A).unwrap().at, destination);
            assert!(!state.resource(RESOURCE_A).unwrap().enabled);
            assert!(state.piston(first).unwrap().extended);
            assert!(state.piston(other).unwrap().extended, "the second piston extends empty");
            assert_eq!(state.counts().piston_actions, 2);
            assert_eq!(
                state.counts().resource_movements,
                1,
                "one identity cannot be moved twice in one FIFO drain"
            );
            state.tick().unwrap();
            state.tick().unwrap();
            assert!(state.resource(RESOURCE_A).unwrap().enabled);
            assert_eq!(state.resource(RESOURCE_A).unwrap().at, destination);
            assert_eq!(state.counts().motion_completions, 2);
        }
    }

    #[test]
    fn stale_and_blocked_requests_leave_resources_available_for_a_valid_action() {
        let program = EventProgram::new_with_movement(
            vec![input(0), input(0), input(0), piston_component(), piston_component()],
            vec![
                ElectricalLink {
                    port: InputPort::Main,
                    source: A,
                    target: PISTON_A,
                    attenuation: 0,
                    condition: None,
                },
                ElectricalLink {
                    port: InputPort::Main,
                    source: B,
                    target: PISTON_B,
                    attenuation: 0,
                    condition: None,
                },
            ],
            vec![(A, Operation::Contribution(0)), (B, Operation::Contribution(1))],
            vec![
                EventResource {
                    contacts: vec![CONTACT_A, CONTACT_B].into(),
                    initial_at: CONTACT_A,
                },
                EventResource {
                    contacts: vec![CONTACT_B, CONTACT_C].into(),
                    initial_at: CONTACT_B,
                },
            ],
            vec![
                EventPiston {
                    input: PISTON_A,
                    resource: Some(RESOURCE_A),
                    near: CONTACT_A,
                    far: CONTACT_B,
                    sticky: true,
                    initial_extended: false,
                    initial_head_present: false,
                },
                EventPiston {
                    input: PISTON_B,
                    resource: Some(RESOURCE_B),
                    near: CONTACT_B,
                    far: CONTACT_C,
                    sticky: true,
                    initial_extended: false,
                    initial_head_present: false,
                },
            ],
            vec![],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        state.set_input(A, 15).unwrap();
        state.set_input(B, 15).unwrap();

        state.queue_piston_action(PistonId::new(0), PistonAction::Extend, Some(A)).unwrap();
        state.set_input(A, 0).unwrap();
        state.tick().unwrap();
        assert_eq!(state.resource(RESOURCE_A).unwrap().at, CONTACT_A);
        assert!(state.resource(RESOURCE_A).unwrap().enabled);
        assert_eq!(state.resource(RESOURCE_A).unwrap().revision, 0);

        state.set_input(A, 15).unwrap();
        state.queue_piston_action(PistonId::new(0), PistonAction::Extend, Some(A)).unwrap();
        state.queue_piston_action(PistonId::new(1), PistonAction::Extend, Some(B)).unwrap();
        state.tick().unwrap();
        assert_eq!(
            state.resource(RESOURCE_A).unwrap().at,
            CONTACT_A,
            "blocked request leaves its resource untouched"
        );
        assert_eq!(state.resource(RESOURCE_A).unwrap().revision, 0);
        assert_eq!(state.resource(RESOURCE_B).unwrap().at, CONTACT_C, "second actor clears the destination");

        state.queue_piston_action(PistonId::new(0), PistonAction::Extend, Some(A)).unwrap();
        state.tick().unwrap();
        assert_eq!(state.resource(RESOURCE_A).unwrap().at, CONTACT_B);
        assert!(!state.resource(RESOURCE_A).unwrap().enabled);
        assert_eq!(state.counts().rejected_piston_actions, 2);
        assert_eq!(state.counts().piston_actions, 2);
        assert_eq!(state.counts().resource_movements, 2);
        state.tick().unwrap();
        assert!(state.resource(RESOURCE_B).unwrap().enabled);
        assert!(!state.resource(RESOURCE_A).unwrap().enabled);
        state.tick().unwrap();
        assert!(state.resource(RESOURCE_A).unwrap().enabled);
    }

    #[test]
    fn empty_piston_action_succeeds_without_moving_a_resource() {
        let program = EventProgram::new_with_movement(
            vec![input(0), input(0), input(0), piston_component()],
            vec![ElectricalLink {
                port: InputPort::Main,
                source: A,
                target: PISTON_A,
                attenuation: 0,
                condition: None,
            }],
            vec![(A, Operation::Contribution(0))],
            vec![],
            vec![EventPiston {
                input: PISTON_A,
                resource: None,
                near: CONTACT_A,
                far: CONTACT_B,
                sticky: true,
                initial_extended: false,
                initial_head_present: false,
            }],
            vec![],
        )
        .unwrap();
        let mut state = EventState::new(Arc::new(program), 0);
        state.set_input(A, 15).unwrap();
        state.queue_piston_action(PistonId::new(0), PistonAction::Extend, None).unwrap();
        state.tick().unwrap();
        assert!(state.piston(PistonId::new(0)).unwrap().extended);
        assert!(state.piston(PistonId::new(0)).unwrap().pending_motion);
        assert_eq!(state.counts().piston_actions, 1);
        assert_eq!(state.counts().resource_movements, 0);
        state.tick().unwrap();
        state.tick().unwrap();
        assert!(!state.piston(PistonId::new(0)).unwrap().pending_motion);
        assert_eq!(state.counts().motion_completions, 1);
    }

    #[test]
    fn invalid_shared_resource_ownership_is_rejected_at_compile_time() {
        let shared_initial_contact = EventResource {
            contacts: vec![CONTACT_A, CONTACT_B].into(),
            initial_at: CONTACT_A,
        };
        assert!(EventProgram::new_with_movement(
            vec![input(0)],
            vec![],
            vec![],
            vec![shared_initial_contact.clone(), shared_initial_contact],
            vec![],
            vec![],
        )
        .is_err());

        assert!(EventProgram::new_with_movement(
            vec![piston_component(), piston_component()],
            vec![],
            vec![],
            vec![],
            vec![
                EventPiston {
                    input: NodeId::new(0),
                    resource: None,
                    near: CONTACT_A,
                    far: CONTACT_B,
                    sticky: true,
                    initial_extended: false,
                    initial_head_present: false,
                },
                EventPiston {
                    input: NodeId::new(0),
                    resource: None,
                    near: CONTACT_A,
                    far: CONTACT_C,
                    sticky: true,
                    initial_extended: false,
                    initial_head_present: false,
                },
            ],
            vec![],
        )
        .is_err());
    }

    #[test]
    fn invalid_observer_program_and_initial_state_are_rejected() {
        assert!(EventProgram::new(
            vec![input(0), observer(false)],
            vec![],
            vec![(A, Operation::NotifyObserver { target: A, shape: true })]
        )
        .is_err());
        assert!(EventProgram::new(vec![input(0), observer(true)], vec![], vec![]).is_err());
    }
}
