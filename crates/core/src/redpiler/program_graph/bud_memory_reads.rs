//! Private upper storage and lower instant read ports, with an exact live QC predicate.
use super::*;
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BudMemoryRead {
    /// This ordinary piston owns all writes, pose and pending actions.
    pub(crate) storage: PistonId,
    /// This instant actor samples the stored bit through its Quasi input.
    pub(crate) reader: PistonId,
    /// This conserved redstone payload carries the bit and its publication revision.
    pub(crate) resource: ResourceId,
    /// The bit is readable only while the payload is enabled at this Far contact.
    pub(crate) source: ContactId,
    /// This sole original Quasi contribution is the read's complete electrical witness.
    pub(crate) link: PowerLink,
}

pub(crate) fn infer_bud_memory_reads(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<BudMemoryRead>, String> {
    if monitor.cancelled() {
        return Err("cancelled before BUD memory read discovery".into());
    }
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return Ok(Vec::new());
    };
    let admitted: FxHashSet<_> = catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
    let mut readers: Vec<_> = admitted.iter().copied().collect();
    readers.sort_unstable();
    let power = crate::redpiler::passes::utils::instant_mechanisms::index_power(graph);
    let mut payloads: FxHashMap<ContactId, Vec<ResourceId>> = FxHashMap::default();
    for (index, resource) in graph.resources.iter().enumerate() {
        for &contact in &resource.contacts {
            payloads.entry(contact).or_default().push(ResourceId::new(index as u32));
        }
    }
    let mut storage_at: FxHashMap<ContactId, Vec<PistonId>> = FxHashMap::default();
    for (index, piston) in graph.pistons.iter().enumerate() {
        if let PistonLane::SingleResource { far } = piston.lane {
            storage_at.entry(far).or_default().push(PistonId::new(index as u32));
        }
    }
    let mut result = Vec::new();
    for reader in readers {
        if monitor.cancelled() {
            return Err("cancelled while checking BUD memory read".into());
        }
        let spec = graph.pistons[reader.index()];
        let Some(links) = power.get(&PowerTarget::Input {
            node: spec.node,
            port: InputPort::Quasi,
        }) else {
            continue;
        };
        let [link] = links.as_slice() else { continue };
        let link = *link;
        if link.attenuation != 0 || !matches!(link.from_port, PowerPort::Weak(_) | PowerPort::Strong(_)) {
            continue;
        }
        let SiteSource::Contact(source) = graph.sites[link.from.index()].source else {
            continue;
        };
        let Some([resource]) = payloads.get(&source).map(Vec::as_slice) else {
            continue;
        };
        let resource = *resource;
        let Some([storage]) = storage_at.get(&source).map(Vec::as_slice) else {
            continue;
        };
        let storage = *storage;
        let writer = graph.pistons[storage.index()];
        if admitted.contains(&storage)
            || !writer.sticky
            || writer.facing != BlockFace::Bottom
            || graph.contacts[source.index()].position != spec.position.offset(BlockFace::Top).offset(BlockFace::Top)
            || graph.resources[resource.index()].contacts.len() != 2
            || !graph.resources[resource.index()].contacts.contains(&writer.near)
            || Block::from_id(graph.profiles[graph.resources[resource.index()].material.index()].raw_state) != Block::RedstoneBlock
        {
            continue;
        }
        let Some(first) = graph.contacts[source.index()].variants.first() else {
            continue;
        };
        if first.when != link.guard
            || first.receiver.is_some()
            || Block::from_id(graph.profiles[first.profile.index()].raw_state) != Block::RedstoneBlock
        {
            continue;
        }
        let mut tests = FxHashSet::default();
        if !presence_guard(graph, link.guard, 0, &mut tests)
            || tests
                != [StateTest::ResourceAt { resource, contact: source }, StateTest::ResourceEnabled(resource)]
                    .into_iter()
                    .collect()
        {
            continue;
        }
        result.push(BudMemoryRead {
            storage,
            reader,
            resource,
            source,
            link,
        });
    }
    Ok(result)
}

fn presence_guard(graph: &CompileGraph, id: GuardId, depth: usize, tests: &mut FxHashSet<StateTest>) -> bool {
    if depth > 64 {
        return false;
    }
    match &graph.guards[id.index()] {
        Guard::Always => true,
        Guard::State(test @ (StateTest::ResourceAt { .. } | StateTest::ResourceEnabled(_))) => {
            tests.insert(*test);
            true
        }
        Guard::All(children) => children.iter().all(|child| presence_guard(graph, *child, depth + 1, tests)),
        _ => false,
    }
}
