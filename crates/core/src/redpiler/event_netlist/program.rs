//! Component execution data. Physical sites, wires and callback routes are not runtime inputs.
use crate::redpiler::program_graph::{ContactId, InputPort, NodeId, PistonId, ProfileId, ResourceId, SiteTest};
use std::collections::HashSet;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Input,
    Wire,
    Lamp,
    Observer,
    Piston,
    Repeater { delay: u8 },
    Torch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ComponentState {
    pub(crate) output: u8,
    pub(crate) powered: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Component {
    pub(crate) kind: ComponentKind,
    pub(crate) initial: ComponentState,
}

#[derive(Clone, Debug)]
pub(crate) struct ElectricalLink {
    pub(crate) source: NodeId,
    pub(crate) target: NodeId,
    pub(crate) port: InputPort,
    pub(crate) attenuation: u8,
    pub(crate) condition: Option<EventCondition>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EventProfile {
    pub(crate) id: ProfileId,
    pub(crate) solid: bool,
    pub(crate) transparent: bool,
    pub(crate) wire: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct EventContactVariant {
    pub(crate) profile: EventProfile,
    pub(crate) condition: EventCondition,
}

/// Runtime guard AST retains the reference predicate's negation and conjunctions.
#[derive(Clone, Debug)]
pub(crate) enum EventCondition {
    ResourceAt {
        resource: ResourceId,
        contact: ContactId,
    },
    ResourceEnabled(ResourceId),
    PistonExtended {
        piston: PistonId,
        extended: bool,
    },
    PistonHeadPresent {
        piston: PistonId,
        present: bool,
    },
    PistonReady(PistonId),
    ContactTest {
        contact: ContactId,
        test: SiteTest,
        variants: Box<[EventContactVariant]>,
    },
    Not(Box<EventCondition>),
    All(Box<[EventCondition]>),
    Any(Box<[EventCondition]>),
}

#[derive(Clone, Debug)]
pub(crate) struct EventResource {
    pub(crate) contacts: Box<[ContactId]>,
    pub(crate) initial_at: ContactId,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EventPiston {
    pub(crate) input: NodeId,
    pub(crate) resource: Option<ResourceId>,
    pub(crate) near: ContactId,
    pub(crate) far: ContactId,
    pub(crate) sticky: bool,
    pub(crate) initial_extended: bool,
    pub(crate) initial_head_present: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResourceEventPhase {
    Departure,
    Arrival,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ResourceInstruction {
    Notify(NodeId),
    NotifyObserver { target: NodeId, shape: bool },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceOperation {
    pub(crate) resource: ResourceId,
    pub(crate) contact: ContactId,
    pub(crate) phase: ResourceEventPhase,
    pub(crate) instruction: ResourceInstruction,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CompiledResourceInstruction {
    Contribution(usize),
    RecomputeWire(NodeId),
    NotifyIfChanged(NodeId),
    Notify(NodeId),
    NotifyObserver { target: NodeId, shape: bool },
}

/// A contribution change and a receiver update are separate observable operations.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Operation {
    Contribution(usize),
    RecomputeWire(NodeId),
    Notify(NodeId),
    /// Observer watchers receive shape notifications, not ordinary updates.
    NotifyObserver {
        target: NodeId,
        shape: bool,
    },
}

pub(crate) struct EventProgram {
    pub(super) components: Box<[Component]>,
    pub(super) links: Box<[ElectricalLink]>,
    pub(super) source_ranges: Box<[Range<usize>]>,
    pub(super) operations: Box<[Operation]>,
    pub(super) input_ranges: Box<[Range<usize>]>,
    pub(super) input_links: Box<[usize]>,
    pub(super) resources: Box<[EventResource]>,
    pub(super) pistons: Box<[EventPiston]>,
    pub(super) resource_ranges: Box<[Range<usize>]>,
    pub(super) resource_operations: Box<[(ContactId, CompiledResourceInstruction)]>,
    pub(super) piston_ranges: Box<[Range<usize>]>,
    pub(super) piston_links: Box<[usize]>,
    pub(super) wire_watchers: Box<[Box<[NodeId]>]>,
}

impl EventProgram {
    /// Preserve instruction order within each source while lowering builder records into arenas.
    pub(crate) fn new(components: Vec<Component>, links: Vec<ElectricalLink>, operations: Vec<(NodeId, Operation)>) -> Result<Self, &'static str> {
        Self::new_with_movement(components, links, operations, Vec::new(), Vec::new(), Vec::new())
    }

    pub(crate) fn new_with_movement(
        components: Vec<Component>,
        links: Vec<ElectricalLink>,
        operations: Vec<(NodeId, Operation)>,
        resources: Vec<EventResource>,
        pistons: Vec<EventPiston>,
        resource_operations: Vec<ResourceOperation>,
    ) -> Result<Self, &'static str> {
        Self::new_with_wire_watchers(components, links, operations, resources, pistons, resource_operations, Vec::new())
    }

    pub(crate) fn new_with_wire_watchers(
        components: Vec<Component>,
        links: Vec<ElectricalLink>,
        operations: Vec<(NodeId, Operation)>,
        resources: Vec<EventResource>,
        pistons: Vec<EventPiston>,
        resource_operations: Vec<ResourceOperation>,
        wire_watchers: Vec<(NodeId, NodeId)>,
    ) -> Result<Self, &'static str> {
        let count = components.len();
        if u32::try_from(count).is_err() || u32::try_from(resources.len()).is_err() || u32::try_from(pistons.len()).is_err() {
            return Err("too many event components");
        }
        for component in &components {
            if component.initial.output > 15 {
                return Err("initial component strength exceeds fifteen");
            }
            match component.kind {
                ComponentKind::Input if component.initial.powered != (component.initial.output > 0) => {
                    return Err("input power and strength disagree");
                }
                ComponentKind::Observer if component.initial.powered != (component.initial.output > 0) => {
                    return Err("observer power and strength disagree");
                }
                ComponentKind::Observer if component.initial.powered => {
                    return Err("initially powered observer requires pending work");
                }
                ComponentKind::Wire if component.initial.powered != (component.initial.output > 0) => {
                    return Err("wire power and strength disagree");
                }
                ComponentKind::Lamp if component.initial.output != 0 => {
                    return Err("lamps do not emit electrical power");
                }
                ComponentKind::Piston if component.initial.output != 0 || component.initial.powered => {
                    return Err("pistons have no initial output state");
                }
                ComponentKind::Repeater { delay } if !(1..=4).contains(&delay) => return Err("invalid repeater delay"),
                ComponentKind::Repeater { .. } if component.initial.powered != (component.initial.output > 0) => {
                    return Err("repeater power and strength disagree");
                }
                ComponentKind::Torch if component.initial.output != if component.initial.powered { 15 } else { 0 } => {
                    return Err("torch power and strength disagree");
                }
                _ => {}
            }
        }
        for (index, resource) in resources.iter().enumerate() {
            if resource.contacts.is_empty() || !resource.contacts.contains(&resource.initial_at) {
                return Err("invalid initial resource contact");
            }
            if resources[..index].iter().any(|previous| previous.initial_at == resource.initial_at) {
                return Err("multiple resources occupy one initial contact");
            }
            for (i, contact) in resource.contacts.iter().enumerate() {
                if resource.contacts[..i].contains(contact) {
                    return Err("duplicate resource contact");
                }
            }
        }
        for (index, piston) in pistons.iter().enumerate() {
            if piston.input.index() >= count
                || components[piston.input.index()].kind != ComponentKind::Piston
                || pistons[..index].iter().any(|previous| previous.input == piston.input)
                || piston.near == piston.far
                || piston.resource.is_some_and(|resource| {
                    resources
                        .get(resource.index())
                        .is_none_or(|resource| !resource.contacts.contains(&piston.near) || !resource.contacts.contains(&piston.far))
                })
            {
                return Err("invalid event piston");
            }
        }
        if components
            .iter()
            .enumerate()
            .any(|(index, component)| component.kind == ComponentKind::Piston && !pistons.iter().any(|piston| piston.input.index() == index))
        {
            return Err("piston component has no movement specification");
        }
        let mut inputs = vec![Vec::new(); count];
        for (id, link) in links.iter().enumerate() {
            if link.source.index() >= count || link.target.index() >= count || link.attenuation > 15 {
                return Err("invalid electrical link");
            }
            if link
                .condition
                .as_ref()
                .is_some_and(|condition| !valid_condition(condition, &resources, &pistons))
            {
                return Err("invalid conditional electrical link");
            }
            if !matches!(
                components[link.source.index()].kind,
                ComponentKind::Input | ComponentKind::Wire | ComponentKind::Observer | ComponentKind::Repeater { .. } | ComponentKind::Torch
            ) || !matches!(
                components[link.target.index()].kind,
                ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. } | ComponentKind::Wire | ComponentKind::Torch
            ) {
                return Err("unsupported electrical source or receiver");
            }
            inputs[link.target.index()].push(id);
        }
        let mut by_source = vec![Vec::new(); count];
        let mut bound = vec![false; links.len()];
        for (source, operation) in operations {
            if source.index() >= count
                || !matches!(
                    components[source.index()].kind,
                    ComponentKind::Input | ComponentKind::Wire | ComponentKind::Observer | ComponentKind::Repeater { .. } | ComponentKind::Torch
                )
            {
                return Err("unsupported operation source");
            }
            match operation {
                Operation::Contribution(id) => {
                    let Some(link) = links.get(id) else {
                        return Err("operation references an invalid link");
                    };
                    if link.source != source || bound[id] {
                        return Err("link has a wrong or repeated source operation");
                    }
                    bound[id] = true;
                }
                Operation::RecomputeWire(target) => {
                    if target.index() >= count || components[target.index()].kind != ComponentKind::Wire {
                        return Err("invalid wire recomputation target");
                    }
                }
                Operation::Notify(target) => {
                    if target.index() >= count
                        || !matches!(
                            components[target.index()].kind,
                            ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. } | ComponentKind::Torch
                        )
                    {
                        return Err("unsupported notification receiver");
                    }
                }
                Operation::NotifyObserver { target, .. } => {
                    if target.index() >= count || components[target.index()].kind != ComponentKind::Observer {
                        return Err("invalid observer notification receiver");
                    }
                }
            }
            by_source[source.index()].push(operation);
        }
        if bound.iter().any(|bound| !bound) {
            return Err("electrical link has no source operation");
        }
        let (source_ranges, operations) = flatten(by_source);
        let (input_ranges, input_links) = flatten(inputs);

        let mut resource_buckets = vec![Vec::new(); resources.len() * 2];
        let mut resource_notifications = HashSet::new();
        let mut notifications = Vec::new();
        for (link_index, link) in links.iter().enumerate() {
            if let Some(condition) = &link.condition {
                let mut guarded_resources = Vec::new();
                condition_resources(condition, &mut guarded_resources);
                guarded_resources.sort_by_key(|resource| resource.index());
                guarded_resources.dedup();
                for resource in guarded_resources {
                    for &contact in resources[resource.index()].contacts.iter() {
                        let departure = resource.index() * 2;
                        let arrival = departure + 1;
                        for phase in [departure, arrival] {
                            resource_buckets[phase].push((contact, CompiledResourceInstruction::Contribution(link_index)));
                            if components[link.target.index()].kind == ComponentKind::Wire {
                                if resource_notifications.insert((phase, contact, link.target)) {
                                    notifications.push((phase, contact, link.target));
                                }
                            } else if link.port != InputPort::Quasi && resource_notifications.insert((phase, contact, link.target)) {
                                notifications.push((phase, contact, link.target));
                            }
                        }
                    }
                }
            }
        }
        for (phase, contact, target) in notifications {
            let instruction = if components[target.index()].kind == ComponentKind::Wire {
                CompiledResourceInstruction::RecomputeWire(target)
            } else {
                CompiledResourceInstruction::NotifyIfChanged(target)
            };
            resource_buckets[phase].push((contact, instruction));
        }
        for operation in resource_operations {
            let Some(resource) = resources.get(operation.resource.index()) else {
                return Err("resource operation references an invalid resource");
            };
            if !resource.contacts.contains(&operation.contact) {
                return Err("resource operation references an invalid contact");
            }
            let instruction = match operation.instruction {
                ResourceInstruction::Notify(target) => {
                    if target.index() >= count
                        || !matches!(
                            components[target.index()].kind,
                            ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. } | ComponentKind::Torch
                        )
                    {
                        return Err("invalid resource notification receiver");
                    }
                    CompiledResourceInstruction::Notify(target)
                }
                ResourceInstruction::NotifyObserver { target, shape } => {
                    if target.index() >= count || components[target.index()].kind != ComponentKind::Observer {
                        return Err("invalid resource observer receiver");
                    }
                    CompiledResourceInstruction::NotifyObserver { target, shape }
                }
            };
            let phase = operation.resource.index() * 2 + if operation.phase == ResourceEventPhase::Arrival { 1 } else { 0 };
            resource_buckets[phase].push((operation.contact, instruction));
        }
        let (resource_ranges, resource_operations) = flatten(resource_buckets);
        let mut piston_buckets = vec![Vec::new(); pistons.len()];
        for (link_index, link) in links.iter().enumerate() {
            if let Some(condition) = &link.condition {
                let mut guarded_pistons = Vec::new();
                condition_pistons(condition, &mut guarded_pistons);
                guarded_pistons.sort_by_key(|piston| piston.index());
                guarded_pistons.dedup();
                for piston in guarded_pistons {
                    piston_buckets[piston.index()].push(link_index);
                }
            }
        }
        let (piston_ranges, piston_links) = flatten(piston_buckets);
        let mut watcher_buckets = vec![Vec::new(); count];
        for (wire, piston) in wire_watchers {
            if wire.index() >= count
                || components[wire.index()].kind != ComponentKind::Wire
                || piston.index() >= count
                || components[piston.index()].kind != ComponentKind::Piston
            {
                return Err("invalid watched wire receiver");
            }
            if !watcher_buckets[wire.index()].contains(&piston) {
                watcher_buckets[wire.index()].push(piston);
            }
        }
        let wire_watchers = watcher_buckets
            .into_iter()
            .map(Vec::into_boxed_slice)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Self {
            components: components.into(),
            links: links.into(),
            source_ranges,
            operations,
            input_ranges,
            input_links,
            resources: resources.into(),
            pistons: pistons.into(),
            resource_ranges,
            resource_operations,
            piston_ranges,
            piston_links,
            wire_watchers,
        })
    }
}

fn flatten<T>(buckets: Vec<Vec<T>>) -> (Box<[Range<usize>]>, Box<[T]>) {
    let mut ranges = Vec::with_capacity(buckets.len());
    let mut values = Vec::with_capacity(buckets.iter().map(Vec::len).sum());
    for bucket in buckets {
        let start = values.len();
        values.extend(bucket);
        ranges.push(start..values.len());
    }
    (ranges.into(), values.into())
}

fn valid_condition(condition: &EventCondition, resources: &[EventResource], pistons: &[EventPiston]) -> bool {
    match condition {
        EventCondition::ResourceAt { resource, contact } => resources
            .get(resource.index())
            .is_some_and(|resource| resource.contacts.contains(contact)),
        EventCondition::ResourceEnabled(resource) => resource.index() < resources.len(),
        EventCondition::PistonExtended { piston, .. } | EventCondition::PistonHeadPresent { piston, .. } | EventCondition::PistonReady(piston) => {
            piston.index() < pistons.len()
        }
        EventCondition::ContactTest { variants, .. } => variants.iter().all(|variant| valid_condition(&variant.condition, resources, pistons)),
        EventCondition::Not(child) => valid_condition(child, resources, pistons),
        EventCondition::All(children) | EventCondition::Any(children) => children.iter().all(|child| valid_condition(child, resources, pistons)),
    }
}

fn condition_resources(condition: &EventCondition, resources: &mut Vec<ResourceId>) {
    match condition {
        EventCondition::ResourceAt { resource, .. } | EventCondition::ResourceEnabled(resource) => resources.push(*resource),
        EventCondition::ContactTest { variants, .. } => {
            for variant in variants.iter() {
                condition_resources(&variant.condition, resources);
            }
        }
        EventCondition::Not(child) => condition_resources(child, resources),
        EventCondition::All(children) | EventCondition::Any(children) => {
            for child in children.iter() {
                condition_resources(child, resources);
            }
        }
        EventCondition::PistonExtended { .. } | EventCondition::PistonHeadPresent { .. } | EventCondition::PistonReady(_) => {}
    }
}

fn condition_pistons(condition: &EventCondition, pistons: &mut Vec<PistonId>) {
    match condition {
        EventCondition::PistonExtended { piston, .. } | EventCondition::PistonHeadPresent { piston, .. } | EventCondition::PistonReady(piston) => {
            pistons.push(*piston)
        }
        EventCondition::ContactTest { variants, .. } => {
            for variant in variants.iter() {
                condition_pistons(&variant.condition, pistons);
            }
        }
        EventCondition::Not(child) => condition_pistons(child, pistons),
        EventCondition::All(children) | EventCondition::Any(children) => {
            for child in children.iter() {
                condition_pistons(child, pistons);
            }
        }
        EventCondition::ResourceAt { .. } | EventCondition::ResourceEnabled(_) => {}
    }
}
