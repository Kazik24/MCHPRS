//! Concrete source fragments for compiler declines; this never changes admission.
use super::*;

pub(super) fn examples(graph: &CompileGraph, catalog: &BoundaryCatalog, plan: &LayeredInstantPlan) -> Value {
    let mut groups = BTreeMap::<String, Vec<&LayerDecline>>::new();
    for decline in &plan.declines {
        if !decline.cells.is_empty() {
            groups.entry(format!("{:?}", decline.reason)).or_default().push(decline);
        }
    }
    let examples: BTreeMap<_, _> = groups
        .into_iter()
        .map(|(reason, mut declines)| {
            declines.sort_by_key(|decline| (decline.cells.len(), decline.corridors.len(), decline.cells[0]));
            let smallest = details(graph, catalog, declines[0]);
            let largest = details(graph, catalog, declines[declines.len() - 1]);
            (reason, json!({"smallest":smallest,"largest":largest}))
        })
        .collect();
    json!(examples)
}

fn details(graph: &CompileGraph, catalog: &BoundaryCatalog, decline: &LayerDecline) -> Value {
    let actors: Vec<_> = decline
        .cells
        .iter()
        .take(4)
        .flat_map(|&cell| catalog.cells[cell as usize].uses.iter())
        .collect();
    let nodes: BTreeSet<_> = actors.iter().map(|use_| graph.pistons[use_.actor.index()].node).collect();
    let input_links: Vec<_> = graph
        .power_links
        .iter()
        .filter(|link| matches!(link.to, PowerTarget::Input { node, .. } if nodes.contains(&node)))
        .take(64)
        .map(|link| power_details(graph, *link))
        .collect();
    let owners: BTreeSet<_> = decline
        .cells
        .iter()
        .map(|&id| (false, id))
        .chain(decline.corridors.iter().map(|&id| (true, id)))
        .collect();
    let mut boundary_counts = BTreeMap::<String, usize>::new();
    let mut boundary_samples = BTreeMap::<String, Value>::new();
    for port in &catalog.ports {
        let owner = match port.owner {
            BoundaryOwner::Cell(id) => (false, id),
            BoundaryOwner::Corridor(id) => (true, id),
        };
        if !owners.contains(&owner) {
            continue;
        }
        let description = format!("{:?}", port.kind);
        let family = description.split_whitespace().next().unwrap();
        let key = format!("{:?}: {family}", port.direction);
        *boundary_counts.entry(key.clone()).or_default() += 1;
        boundary_samples.entry(key).or_insert_with(|| match port.kind {
            BoundaryPortKind::Power { link } => power_details(graph, graph.power_links[link as usize]),
            BoundaryPortKind::Callback { occurrence } => callback_details(graph, occurrence),
            _ => json!(description),
        });
    }
    json!({
        "cells":decline.cells.len(),"corridors":decline.corridors.len(),"witness":format!("{:?}",decline.witness),
        "actors":actors.iter().map(|use_|{
            let spec=graph.pistons[use_.actor.index()];
            json!({"id":use_.actor.index(),"world_position":spec.position,"schematic_position":spec.position-cpus::CPUS[2].origin,
                "facing":format!("{:?}",spec.facing),"reset":format!("{:?}",use_.reset),
                "resource":use_.retraction.transfer.map(|transfer|{
                    let resource=&graph.resources[transfer.resource.index()];
                    json!({"id":transfer.resource.index(),"material":Block::from_id(graph.profiles[resource.material.index()].raw_state).get_name(),
                        "source":graph.contacts[transfer.source.index()].position,"destination":graph.contacts[transfer.destination.index()].position,
                        "permitted_contacts":resource.contacts.iter().map(|contact|graph.contacts[contact.index()].position).collect::<Vec<_>>()})
                })})
        }).collect::<Vec<_>>(),
        "input_links":input_links,"boundary_counts":boundary_counts,"boundary_samples":boundary_samples,
        "outside_activation_routes":outside_activation_routes(graph,catalog,decline),
        "wire_groups":decline.corridors.iter().take(2).map(|&id|{
            let wires=&catalog.corridors[id as usize].wires;
            let nodes: BTreeSet<_> = wires.iter().copied().collect();
            let inputs: Vec<_> = graph.power_links.iter().filter(|link| matches!(link.to,PowerTarget::Input { node,.. } if nodes.contains(&node))).take(32).map(|link|power_details(graph,*link)).collect();
            json!({"id":id,"wire_count":wires.len(),"first_positions":wires.iter().take(16).map(|node|graph.nodes[node.index()].position).collect::<Vec<_>>(),"inputs":inputs})
        }).collect::<Vec<_>>(),
        "callback_witness":match decline.witness { LayerWitness::Callback(event)=>callback_details(graph,event), _=>Value::Null },
    })
}

fn power_details(graph: &CompileGraph, link: PowerLink) -> Value {
    let site = graph.sites[link.from.index()];
    let kind = match site.source {
        SiteSource::Component(node) => format!("{:?}", graph.nodes[node.index()].kind),
        SiteSource::Contact(_) => "selectable contact".into(),
        SiteSource::Constant(profile) => Block::from_id(graph.profiles[profile.index()].raw_state).get_name().to_owned(),
    };
    let target = match link.to {
        PowerTarget::Input { node, .. } => graph.nodes[node.index()].position,
        PowerTarget::Conductor { site, .. } => Some(graph.sites[site.index()].position),
    };
    json!({"from":site.position,"source":format!("{:?}",site.source),"port":format!("{:?}",link.from_port),
        "source_kind":kind,"target_position":target,
        "to":format!("{:?}",link.to),"attenuation":link.attenuation,"guard":format!("{:?}",graph.guards[link.guard.index()])})
}

fn outside_activation_routes(graph: &CompileGraph, catalog: &BoundaryCatalog, decline: &LayerDecline) -> Value {
    let uses: Vec<_> = decline.cells.iter().flat_map(|&cell| catalog.cells[cell as usize].uses.iter()).collect();
    let mut owned: BTreeSet<_> = uses.iter().map(|use_| graph.pistons[use_.actor.index()].node).collect();
    owned.extend(uses.iter().filter_map(|use_| use_.reset.observer()));
    owned.extend(
        decline
            .corridors
            .iter()
            .flat_map(|&id| catalog.corridors[id as usize].wires.iter().copied()),
    );
    let routes: BTreeSet<_> = uses
        .iter()
        .flat_map(|use_| use_.activations.iter().map(|event| event.route))
        .filter(|route| !owned.contains(&graph.routes[route.index()].source))
        .collect();
    json!({"count":routes.len(),"examples":routes.iter().take(12).map(|&route|{
        let source=graph.routes[route.index()].source;
        let phases = if let CircuitNodeKind::Piston(actor)=graph.nodes[source.index()].kind {
            let routes=graph.pistons[actor.index()].routes;
            [("base_changed",Some(routes.base_changed)),("far_changed",routes.far_changed),("near_changed",Some(routes.near_changed)),
             ("base_restored",Some(routes.base_restored)),("head_restored",Some(routes.head_restored)),
             ("resource_restored_near",routes.resource_restored_near),("resource_restored_far",routes.resource_restored_far)]
                .into_iter().filter_map(|(name,id)|(id==Some(route)).then_some(name)).collect::<Vec<_>>()
        } else {vec![]};
        json!({"route":route.index(),"source_kind":format!("{:?}",graph.nodes[source.index()].kind),"from":graph.nodes[source.index()].position,"phases":phases})
    }).collect::<Vec<_>>()})
}

fn callback_details(graph: &CompileGraph, event: RouteOccurrence) -> Value {
    let route = &graph.routes[event.route.index()];
    let delivery = route.deliveries[event.ordinal as usize];
    json!({"route":event.route.index(),"ordinal":event.ordinal,"from":graph.nodes[route.source.index()].position,
        "source_kind":format!("{:?}",graph.nodes[route.source.index()].kind),"target":graph.sites[delivery.target.index()].position,
        "receiver":format!("{:?}",delivery.receiver),"kind":format!("{:?}",delivery.kind),"view":format!("{:?}",delivery.view),
        "guard":format!("{:?}",graph.guards[delivery.guard.index()])})
}
