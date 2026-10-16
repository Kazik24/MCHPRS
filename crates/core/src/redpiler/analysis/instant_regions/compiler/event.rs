//! Compile complete region entry closures into sparse state and ordered effect roots.
use super::execution::{Effect, Execution, Guarded, Publication, Request, ResourcePublication};
use super::*;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    SymbolicEventKernel, SymbolicRegionCapture, SymbolicRegionCaptureVariant, SymbolicRegionCapturedProperties, SymbolicRegionEffect,
    SymbolicRegionEntry, SymbolicRegionEventRoutine, SymbolicRegionWrite,
};
use mchprs_redpiler_opt::{Limits, optimize};
use rustc_hash::FxHashSet;

/// Build one regional event kernel, declining the entire region if an owned entry is unsupported.
pub(crate) fn compile(graph: &CompileGraph, region_index: usize, monitor: &TaskMonitor) -> std::result::Result<SymbolicEventKernel, String> {
    let region = graph.plans.instant.regions.get(region_index).ok_or("unknown instant region")?;
    if !region.stable_inputs {
        return Err("region inputs are not certified stable".into());
    }
    let mut entries = FxHashSet::default();
    for occurrence in &region.incoming {
        check_cancelled(monitor)?;
        let Some(route) = graph.routes.get(occurrence.route.index()) else {
            return Err("incoming route occurrence is missing".into());
        };
        let Some(delivery) = route.deliveries.get(occurrence.ordinal as usize) else {
            return Err("incoming route delivery is missing".into());
        };
        let receiver = delivery
            .receiver
            .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
        if let Some(receiver) = receiver {
            entries.insert(SymbolicRegionEntry::Callback {
                target: delivery.target,
                receiver,
                kind: delivery.kind,
                captured: delivery.view == OperandView::Captured,
            });
        }
    }
    for &actor in &region.actors {
        entries.insert(SymbolicRegionEntry::Sample(actor));
        for action in [
            mchprs_world::PistonAction::Extend,
            mchprs_world::PistonAction::Retract,
            mchprs_world::PistonAction::RetractWithoutPull,
        ] {
            entries.insert(SymbolicRegionEntry::Accept { actor, action });
        }
        for extending in [false, true] {
            entries.insert(SymbolicRegionEntry::Publish {
                actor,
                extending,
                resource: None,
            });
            let spec = graph.pistons[actor.index()];
            if let PistonLane::SingleResource { far } = spec.lane {
                let resource = region.resources.iter().copied().find(|&resource| {
                    let contacts = &graph.resources[resource.index()].contacts;
                    contacts.contains(&spec.near) && contacts.contains(&far)
                });
                if let Some(resource) = resource {
                    let contact = if extending { far } else { spec.near };
                    entries.insert(SymbolicRegionEntry::Publish {
                        actor,
                        extending,
                        resource: Some((resource, contact)),
                    });
                }
            }
        }
    }
    for &node in &region.resets {
        entries.insert(SymbolicRegionEntry::Tick(node));
    }

    let mut keys: Vec<_> = entries.into_iter().collect();
    keys.sort_by_key(entry_sort_key);
    let limits = Limits {
        max_work: (256usize * 1024 * 1024).saturating_mul(monitor.budget_multiplier()),
        max_nodes: Limits::default().max_nodes.saturating_mul(monitor.budget_multiplier()),
    };
    let mut programs = Vec::with_capacity(keys.len());
    for key in keys {
        check_cancelled(monitor)?;
        let routine = compile_entry(graph, region, key, limits, monitor).map_err(|reason| format!("entry {key:?}: {reason}"))?;
        programs.push(std::sync::Arc::new(routine));
    }
    Ok(SymbolicEventKernel {
        region: region_index,
        entries: programs.into_boxed_slice(),
    })
}

fn entry_sort_key(entry: &SymbolicRegionEntry) -> (u8, u32, u32, u32, u32) {
    match *entry {
        SymbolicRegionEntry::Callback {
            target,
            receiver,
            kind,
            captured,
        } => {
            let (receiver_kind, receiver_id) = match receiver {
                CallbackReceiver::Component(node) => (0, node.index() as u32),
                CallbackReceiver::PistonBase(actor) => (1, actor.index() as u32),
                CallbackReceiver::PistonHead(actor) => (2, actor.index() as u32),
                CallbackReceiver::Contact(contact) => (3, contact.index() as u32),
            };
            let (kind, direction) = match kind {
                DeliveryKind::Update { direction } => (0, direction.map_or(0, |face| face as u32 + 1)),
                DeliveryKind::Shape { direction } => (1, direction as u32 + 1),
            };
            (
                0,
                target.index() as u32,
                receiver_id,
                receiver_kind,
                kind << 16 | direction << 1 | u32::from(captured),
            )
        }
        SymbolicRegionEntry::Sample(actor) => (1, actor.index() as u32, 0, 0, 0),
        SymbolicRegionEntry::Accept { actor, action } => (2, actor.index() as u32, action as u32, 0, 0),
        SymbolicRegionEntry::Tick(node) => (3, node.index() as u32, 0, 0, 0),
        SymbolicRegionEntry::Publish { actor, extending, resource } => (
            4,
            actor.index() as u32,
            resource.map_or(u32::MAX, |(resource, _)| resource.index() as u32),
            u32::from(extending),
            resource.map_or(0, |(_, contact)| contact.index() as u32),
        ),
    }
}

fn compile_entry(
    graph: &CompileGraph,
    region: &crate::redpiler::program_graph::InstantRegion,
    key: SymbolicRegionEntry,
    limits: Limits,
    monitor: &TaskMonitor,
) -> std::result::Result<SymbolicRegionEventRoutine, String> {
    let mut compiler = Compiler::new_region_monitored(graph, region, limits, monitor).map_err(|error| format!("region input binding: {error:?}"))?;
    let mut execution = Execution::for_region(&mut compiler, region).map_err(|error| format!("pending work binding: {error:?}"))?;
    match key {
        SymbolicRegionEntry::Callback {
            target,
            receiver,
            kind,
            captured,
        } => {
            let captured = captured
                .then(|| compiler.capture_site(target))
                .transpose()
                .map_err(|error| format!("capture binding: {error:?}"))?;
            execution
                .deliver(
                    &mut compiler,
                    Delivery {
                        occurrence: None,
                        when: Formula::TRUE,
                        target,
                        receiver,
                        kind,
                        captured,
                    },
                )
                .map_err(|error| compile_error("callback closure", error, &compiler, limits))?;
        }
        SymbolicRegionEntry::Sample(actor) => execution
            .sample(&mut compiler, actor, Formula::TRUE)
            .map_err(|error| compile_error("actor sample", error, &compiler, limits))?,
        SymbolicRegionEntry::Accept { actor, action } => execution
            .accept(
                &mut compiler,
                Guarded {
                    when: Formula::TRUE,
                    value: Request { actor, action },
                },
            )
            .map_err(|error| compile_error("action acceptance", error, &compiler, limits))?,
        SymbolicRegionEntry::Tick(node) => execution
            .tick_component(&mut compiler, node, Formula::TRUE)
            .map_err(|error| compile_error("reset tick", error, &compiler, limits))?,
        SymbolicRegionEntry::Publish { actor, extending, resource } => {
            let actor_valid = compiler
                .region_input(RegionRead::DeferredActorValid(actor))
                .map_err(|error| format!("deferred actor witness: {error:?}"))?;
            let resource = resource
                .map(|(resource, contact)| {
                    compiler
                        .region_input(RegionRead::DeferredResourceValid(resource))
                        .map(|when| ResourcePublication {
                            when,
                            resource,
                            stamp: 0,
                            contact,
                        })
                        .map_err(|error| format!("deferred resource witness: {error:?}"))
                })
                .transpose()?;
            execution
                .publish(
                    &mut compiler,
                    Publication {
                        when: Formula::TRUE,
                        actor_valid,
                        actor,
                        stamp: 0,
                        resource,
                        due_tick: 0,
                        extending,
                    },
                )
                .map_err(|error| compile_error("deferred publication", error, &compiler, limits))?;
        }
    }
    if monitor.cancelled() {
        return Err("cancelled while tracing a regional entry".into());
    }

    let mut roots = Vec::new();
    let final_state = compiler.changed_region_state();
    let writes = final_state
        .iter()
        .copied()
        .into_iter()
        .map(|(slot, value)| SymbolicRegionWrite {
            slot,
            root: push_root(&mut roots, value),
        })
        .collect::<Vec<_>>();
    let mut effects = Vec::with_capacity(compiler.effects.len());
    let logged_effects = compiler.effects.clone();
    for (_, effect) in &logged_effects {
        effects.push(lower_effect(&mut compiler, effect, &final_state, &mut roots)?);
    }
    let circuit = compiler
        .logic
        .circuit_live_inputs(&roots)
        .map_err(|error| format!("regional roots: {error:?}"))?;
    let plan = optimize(&circuit, limits, || monitor.cancelled()).map_err(|error| {
        format!(
            "regional plan: {error:?}; decisions={}, inputs={}, logic work={}, semantic work={}",
            compiler.logic.node_count(),
            compiler.region_reads.len(),
            compiler.logic.work(),
            limits.max_work - compiler.remaining_work()
        )
    })?;
    let reads = plan
        .inputs()
        .iter()
        .map(|&input| compiler.region_reads.get(input).copied().ok_or("regional plan retained an unknown input"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut actor_owners = FxHashSet::default();
    let mut resource_owners = FxHashSet::default();
    for write in &writes {
        match write.slot {
            RegionStateSlot::ActorExtended(actor)
            | RegionStateSlot::ActorHead(actor)
            | RegionStateSlot::ActorAvailability { actor, .. }
            | RegionStateSlot::ActorRevision { actor, .. } => {
                actor_owners.insert(actor);
            }
            RegionStateSlot::ResourceAt { resource, .. }
            | RegionStateSlot::ResourceEnabled(resource)
            | RegionStateSlot::ResourceRevision { resource, .. } => {
                resource_owners.insert(resource);
            }
            _ => {}
        }
    }
    for effect in &effects {
        match effect {
            SymbolicRegionEffect::Request { actor, .. }
            | SymbolicRegionEffect::Accepted { actor, .. }
            | SymbolicRegionEffect::Publication { actor, .. } => {
                actor_owners.insert(*actor);
            }
            SymbolicRegionEffect::Callback { receiver, .. } => match receiver {
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => {
                    actor_owners.insert(*actor);
                }
                _ => {}
            },
            SymbolicRegionEffect::Schedule { .. } => {}
        }
        if let SymbolicRegionEffect::Publication {
            resource: Some((_, resource, ..)),
            ..
        } = effect
        {
            resource_owners.insert(*resource);
        }
    }
    let mut actor_owners: Vec<_> = actor_owners.into_iter().collect();
    actor_owners.sort_unstable_by_key(|actor| actor.index());
    let mut resource_owners: Vec<_> = resource_owners.into_iter().collect();
    resource_owners.sort_unstable_by_key(|resource| resource.index());
    Ok(SymbolicRegionEventRoutine {
        key,
        reads: reads.into_boxed_slice(),
        plan,
        actor_owners: actor_owners.into_boxed_slice(),
        resource_owners: resource_owners.into_boxed_slice(),
        writes: writes.into_boxed_slice(),
        effects: effects.into_boxed_slice(),
    })
}

fn lower_effect(
    compiler: &mut Compiler<'_>,
    effect: &Effect,
    final_state: &[(RegionStateSlot, Formula)],
    roots: &mut Vec<Formula>,
) -> std::result::Result<SymbolicRegionEffect, String> {
    Ok(match effect {
        Effect::Schedule { when, node, delay, priority } => SymbolicRegionEffect::Schedule {
            when: push_root(roots, *when),
            node: *node,
            delay: *delay,
            priority: *priority,
        },
        Effect::Request { when, request } => SymbolicRegionEffect::Request {
            when: push_root(roots, *when),
            actor: request.actor,
            action: request.action,
        },
        Effect::Publication(publication) => {
            let when = push_root(roots, publication.when);
            let actor_valid = push_root(roots, publication.actor_valid);
            let resource = publication
                .resource
                .as_ref()
                .map(|payload| (push_root(roots, payload.when), payload.resource, payload.stamp as u32, payload.contact));
            SymbolicRegionEffect::Publication {
                when,
                actor_valid,
                actor: publication.actor,
                actor_stamp: publication.stamp as u32,
                resource,
                due_tick: publication.due_tick,
                extending: publication.extending,
            }
        }
        Effect::Accepted(accepted) => SymbolicRegionEffect::Accepted {
            when: push_root(roots, accepted.when),
            actor: accepted.request.actor,
            action: accepted.request.action,
            previous_extended: push_root(roots, accepted.previous_extended),
        },
        Effect::Callback { delivery, snapshot } => {
            let when = push_root(roots, delivery.when);
            let snapshot = lower_overlay(compiler, snapshot, final_state, roots);
            let captured = delivery.captured.as_ref().map(|view| lower_capture(compiler, view, roots)).transpose()?;
            SymbolicRegionEffect::Callback {
                when,
                occurrence: delivery.occurrence,
                target: delivery.target,
                receiver: delivery.receiver,
                kind: delivery.kind,
                snapshot,
                captured,
            }
        }
    })
}

fn lower_overlay(
    compiler: &Compiler<'_>,
    snapshot: &[(RegionStateSlot, Formula)],
    final_state: &[(RegionStateSlot, Formula)],
    roots: &mut Vec<Formula>,
) -> Box<[SymbolicRegionWrite]> {
    let snapshot_values: rustc_hash::FxHashMap<_, _> = snapshot.iter().copied().collect();
    let final_values: rustc_hash::FxHashMap<_, _> = final_state.iter().copied().collect();
    let slots: FxHashSet<_> = snapshot_values.keys().chain(final_values.keys()).copied().collect();
    let mut overlay = Vec::new();
    for slot in slots {
        let entry = compiler.entry_region_value(slot);
        let event_value = snapshot_values.get(&slot).copied().unwrap_or(entry);
        let final_value = final_values.get(&slot).copied().unwrap_or(entry);
        if event_value != final_value {
            overlay.push(SymbolicRegionWrite {
                slot,
                root: push_root(roots, event_value),
            });
        }
    }
    overlay.into_boxed_slice()
}

fn lower_capture(compiler: &mut Compiler<'_>, view: &SiteView, roots: &mut Vec<Formula>) -> std::result::Result<SymbolicRegionCapture, String> {
    let mut variants = Vec::with_capacity(view.variants.len());
    for variant in &view.variants {
        let properties = match variant.receiver {
            Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_)) => SymbolicRegionCapturedProperties::Piston {
                extended: push_root(
                    roots,
                    variant.captured_actor_extended.ok_or("captured piston has no frozen extension bit")?,
                ),
            },
            Some(CallbackReceiver::Component(node)) if matches!(compiler.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                let component = variant.component.as_ref().ok_or("captured wire has no compiled component")?;
                let shape = component.shape.ok_or("captured wire has no compiled shape")?;
                let signal = component.signal.bits().map(|bit| push_root(roots, bit));
                let mut sides = [(0, 0); 4];
                for (index, side) in shape.sides.into_iter().enumerate() {
                    sides[index] = (push_root(roots, side.up), push_root(roots, side.flat));
                }
                SymbolicRegionCapturedProperties::Wire { signal, sides }
            }
            Some(CallbackReceiver::Component(node))
                if matches!(compiler.graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CopperBulb)) =>
            {
                let component = variant.component.as_ref().ok_or("captured bulb has no compiled state")?;
                SymbolicRegionCapturedProperties::CopperBulb {
                    powered: push_root(roots, component.powered),
                    lit: push_root(
                        roots,
                        compiler
                            .logic
                            .powered(component.signal)
                            .map_err(|error| format!("captured bulb: {error:?}"))?,
                    ),
                }
            }
            Some(CallbackReceiver::Component(_)) => {
                let component = variant.component.as_ref().ok_or("captured component has no compiled state")?;
                SymbolicRegionCapturedProperties::Electrical {
                    powered: push_root(roots, component.powered),
                    locked: push_root(roots, component.locked),
                }
            }
            _ => match variant.block {
                Block::RedstoneWire { wire } => {
                    let signal = (0..4)
                        .map(|bit| push_root(roots, boolean_formula(wire.power & (1 << bit) != 0)))
                        .collect::<Vec<_>>()
                        .try_into()
                        .expect("wire strength has four bits");
                    let sides =
                        [wire.north, wire.south, wire.east, wire.west].map(|side| (side == RedstoneWireSide::Up, side == RedstoneWireSide::Side));
                    SymbolicRegionCapturedProperties::Wire {
                        signal,
                        sides: sides.map(|(up, flat)| (push_root(roots, boolean_formula(up)), push_root(roots, boolean_formula(flat)))),
                    }
                }
                _ => SymbolicRegionCapturedProperties::Passive,
            },
        };
        variants.push(SymbolicRegionCaptureVariant {
            when: push_root(roots, variant.when),
            profile: variant.profile,
            block: variant.block,
            solid: variant.solid,
            transparent: variant.transparent,
            receiver: variant.receiver,
            properties,
        });
    }
    Ok(SymbolicRegionCapture {
        variants: variants.into_boxed_slice(),
    })
}

fn push_root(roots: &mut Vec<Formula>, value: Formula) -> usize {
    let index = roots.len();
    roots.push(value);
    index
}

fn boolean_formula(value: bool) -> Formula {
    if value { Formula::TRUE } else { Formula::FALSE }
}

fn compile_error(stage: &str, error: CompileError, compiler: &Compiler<'_>, limits: Limits) -> String {
    format!(
        "{stage} at {}: {error:?}; decisions={}, inputs={}, logic work={}, semantic work={}",
        compiler.trace_stage,
        compiler.logic.node_count(),
        compiler.region_reads.len(),
        compiler.logic.work(),
        limits.max_work - compiler.remaining_work()
    )
}

fn check_cancelled(monitor: &TaskMonitor) -> std::result::Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while compiling regional events".into())
    } else {
        Ok(())
    }
}
