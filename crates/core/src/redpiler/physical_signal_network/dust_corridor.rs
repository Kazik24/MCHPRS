//! Conservative source-to-dust lowering for fixed, electrically isolated dust networks.
use super::{WireSample, site_block};
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{CircuitNodeKind, CircuitProgram, CircuitState, PowerPort, PowerTarget, SiteId, SiteSource};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Source {
    pub(super) site: SiteId,
    pub(super) port: PowerPort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ResourceObserver {
    pub(super) resource: crate::redpiler::program_graph::ResourceId,
    pub(super) contact: crate::redpiler::program_graph::ContactId,
    pub(super) observer: crate::redpiler::program_graph::NodeId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::backend::graph::{
        piston_probe::tests::{BOUNDS, fixture},
        wire_probe::physical_program,
    };
    use crate::redpiler::physical_signal_network::{EntryWork, PhysicalSignalNetwork};
    use crate::world::World;
    use mchprs_blocks::{BlockPos, blocks::Block};

    #[test]
    fn compiles_checked_dust_fixture() {
        let mut world = fixture();
        world.set_block(BlockPos::new(8, 20, 5), Block::RedstoneLamp { lit: false });
        let positions: Vec<_> = (1..=12)
            .flat_map(|x| (1..=3).map(move |z| BlockPos::new(x, 20, z)))
            .chain(std::iter::once(BlockPos::new(4, 20, 4)))
            .collect();
        for &pos in &positions {
            world.set_block(BlockPos::new(pos.x, pos.y - 1, pos.z), Block::Stone {});
            world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
        }
        for pos in positions {
            let wire = crate::redstone::wire::get_state_for_placement(&world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
        let corridor = Corridor::compile(&program, &state, &network.wire_samples).unwrap();
        assert_eq!(corridor.plan.wires.len(), 37);
        assert!(!corridor.plan.sources.is_empty());
        assert!(corridor.plan.outgoing.len() >= 37);
    }

    #[test]
    fn observer_corridor_is_declined_without_mutation() {
        use crate::redpiler::backend::graph::wire_probe::tests::{BOUNDS, fixture};
        let mut world = fixture();
        let observer = BlockPos::new(2, 8, 6);
        world.set_block(observer, Block::from_name("observer").unwrap());
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
        let before = network.state().clone();
        assert_eq!(
            network.enable_dust_corridor().unwrap_err(),
            "dust corridor observer notifications are unsupported"
        );
        assert_eq!(network.state(), &before);
        assert_eq!(world.get_block(observer), Block::from_name("observer").unwrap());
    }
}

pub(super) struct Plan {
    pub(super) sources: Vec<Source>,
    pub(super) source_ranges: Vec<std::ops::Range<usize>>,
    pub(super) outgoing: Vec<(usize, u8)>,
    pub(super) receiver_ranges: Vec<std::ops::Range<usize>>,
    pub(super) incoming: Vec<(usize, u8)>,
    pub(super) wires: Vec<usize>,
    pub(super) resource_observers: Box<[ResourceObserver]>,
}

#[derive(Clone)]
pub(super) struct Corridor {
    pub(super) plan: std::sync::Arc<Plan>,
    pub(super) values: Vec<u8>,
    pub(super) affected: Vec<bool>,
    pub(super) affected_nodes: Vec<usize>,
    pub(super) operations: u64,
}

impl Corridor {
    pub(super) fn compile(program: &CircuitProgram, state: &CircuitState, samples: &[(usize, Vec<WireSample>)]) -> Result<Self, &'static str> {
        let mut resource_observers = Vec::new();
        for (observer_index, node) in program.nodes().iter().enumerate() {
            let CircuitNodeKind::Observer { watched, .. } = node.kind else {
                continue;
            };
            let SiteSource::Contact(contact) = program.sites()[watched.index()].source else {
                return Err("observer watches a component site; compiled piston watchers are not yet represented");
            };
            let resources = program.resources_at_contact(contact);
            if resources.is_empty()
                || resources
                    .iter()
                    .any(|resource| !program.resources()[resource.index()].contacts.contains(&contact))
            {
                return Err("observer watches a contact that is not owned by a movable resource");
            }
            for &resource in resources {
                resource_observers.push(ResourceObserver {
                    resource,
                    contact,
                    observer: crate::redpiler::program_graph::NodeId::new(observer_index as u32),
                });
            }
        }
        if samples.is_empty() {
            return Err("no dust corridor");
        }
        let wires: Vec<_> = samples.iter().map(|(node, _)| *node).collect();
        // The entry snapshot may omit a solid neighbor from `samples`; validate
        // all movable neighbors so later contact changes cannot introduce a source.
        for wire in program.wires() {
            for (face, slot) in [
                (mchprs_blocks::BlockFace::West, 0),
                (mchprs_blocks::BlockFace::East, 1),
                (mchprs_blocks::BlockFace::Bottom, 2),
                (mchprs_blocks::BlockFace::Top, 3),
                (mchprs_blocks::BlockFace::North, 4),
                (mchprs_blocks::BlockFace::South, 5),
            ] {
                let site_id = wire.canonical_neighbors[slot];
                if let SiteSource::Contact(contact) = program.sites()[site_id.index()].source {
                    let inert = program.contacts()[contact.index()].variants.iter().all(|variant| {
                        program
                            .profile_block(variant.profile)
                            .is_some_and(|block| !crate::redpiler::power_rules::emits_weak(block, face))
                    }) && program
                        .power_links_to(PowerTarget::Conductor {
                            site: site_id,
                            include_dust: false,
                        })
                        .is_empty();
                    if !inert {
                        return Err("dust depends on a movable contact");
                    }
                }
            }
        }
        let mut edges = vec![Vec::new(); wires.len()];
        let mut sources = Vec::new();
        let mut roots = Vec::new();
        for (target, (_, neighbors)) in samples.iter().enumerate() {
            for sample in neighbors {
                let site = &program.sites()[sample.site.index()];
                if sample.dust {
                    let SiteSource::Component(node) = site.source else {
                        return Err("dust neighbor is not a fixed component");
                    };
                    let Some(source) = wires.iter().position(|&wire| wire == node.index()) else {
                        return Err("dust neighbor is outside the corridor");
                    };
                    edges[source].push(target);
                    continue;
                }
                let block = site_block(program, state, sample.site)?;
                let port = if block.is_solid() {
                    if !program
                        .power_links_to(PowerTarget::Conductor {
                            site: sample.site,
                            include_dust: false,
                        })
                        .is_empty()
                    {
                        return Err("dust depends on a conductor");
                    }
                    continue;
                } else {
                    PowerPort::Weak(sample.face)
                };
                match site.source {
                    SiteSource::Contact(contact) => {
                        // A contact may change between air and a piston base without ever
                        // contributing weak or conducted power to this wire.
                        let inert = program.contacts()[contact.index()].variants.iter().all(|variant| {
                            program
                                .profile_block(variant.profile)
                                .is_some_and(|block| !crate::redpiler::power_rules::emits_weak(block, sample.face))
                        }) && program
                            .power_links_to(PowerTarget::Conductor {
                                site: sample.site,
                                include_dust: false,
                            })
                            .is_empty();
                        if inert {
                            continue;
                        }
                        return Err("dust depends on a movable contact");
                    }
                    SiteSource::Constant(_) if !crate::redpiler::power_rules::emits_weak(block, sample.face) => continue,
                    SiteSource::Component(node) => {
                        if !matches!(
                            program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Torch | NodeType::Repeater { .. } | NodeType::Constant)
                        ) {
                            return Err("dust has an unsupported component source");
                        }
                        if !crate::redpiler::power_rules::emits_weak(block, sample.face) {
                            continue;
                        }
                    }
                    _ => {}
                }
                let source = Source { site: sample.site, port };
                let index = match sources.iter().position(|candidate| *candidate == source) {
                    Some(index) => index,
                    None => {
                        sources.push(source);
                        roots.push(Vec::new());
                        sources.len() - 1
                    }
                };
                roots[index].push(target);
            }
        }
        let mut links: Vec<Vec<(usize, u8)>> = Vec::new();
        for source_roots in roots {
            let mut distances = vec![u8::MAX; wires.len()];
            let mut queue = std::collections::VecDeque::new();
            for root in source_roots {
                distances[root] = 0;
                queue.push_back(root);
            }
            while let Some(node) = queue.pop_front() {
                let distance = distances[node].saturating_add(1);
                if distance >= 15 {
                    continue;
                }
                for &next in &edges[node] {
                    if distance < distances[next] {
                        distances[next] = distance;
                        queue.push_back(next);
                    }
                }
            }
            links.push(
                distances
                    .into_iter()
                    .enumerate()
                    .filter_map(|(wire, distance)| (distance < 15).then_some((wires[wire], distance)))
                    .collect(),
            );
        }
        let mut outgoing = Vec::new();
        let mut source_ranges = Vec::with_capacity(links.len());
        let mut incoming_buckets = vec![Vec::new(); program.nodes().len()];
        for (source, entries) in links.into_iter().enumerate() {
            let start = outgoing.len();
            for (node, attenuation) in entries {
                outgoing.push((node, attenuation));
                incoming_buckets[node].push((source, attenuation));
            }
            source_ranges.push(start..outgoing.len());
        }
        let mut incoming = Vec::new();
        let mut receiver_ranges = Vec::with_capacity(incoming_buckets.len());
        for bucket in incoming_buckets {
            let start = incoming.len();
            incoming.extend(bucket);
            receiver_ranges.push(start..incoming.len());
        }
        Ok(Self {
            values: vec![u8::MAX; sources.len()],
            affected: vec![false; program.nodes().len()],
            affected_nodes: Vec::new(),
            plan: std::sync::Arc::new(Plan {
                sources,
                source_ranges,
                outgoing,
                receiver_ranges,
                incoming,
                wires,
                resource_observers: resource_observers.into(),
            }),
            operations: 0,
        })
    }
}
