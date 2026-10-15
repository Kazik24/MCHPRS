//! Certifies immutable properties used by repeated wire-neighbor queries.
//!
//! The same rule compiles the maximum-of-wire-strengths calculation into direct
//! input slots and only the diagonal terms needing dynamic geometry. Seed and
//! captured Turbo expressions stay distinct, including their different vertical
//! and ascent rules. Contacts without any possible wire no longer cause strength
//! or geometry reads in that expression. The original Turbo walk still owns all
//! intermediate writes, captures, duplicate callbacks and scheduling. No electrical
//! fixed point replaces event ordering. Inference is O(24 + contact variants) per
//! wire, and validation independently re-derives the complete plan.
//!
//! Each wire receives masks inferred by the shared graph rule over its 24
//! canonical sites. Constant profiles and fixed component properties may be
//! known; contacts remain dynamic. Only proven zero queries may omit a power
//! read, and component strength, saved state, callbacks, and captured operands
//! remain general. Validation rechecks each certificate against that same rule.
//! A distinct neighborhood with no possible wire also certifies exact callback
//! replay without Turbo's discovery, heading, maps or queues. All contacts may
//! still change among non-wire variants; capture and admission precede delivery.
//! Tests below cover static/dynamic boundaries, state independence, corruption,
//! invalidation, cancellation, disabled execution, and idempotence.
//!
//! This pass runs at O2 before hop compilation, preserving dust paths and Turbo offsets. Later
//! passes must preserve these facts explicitly or let the runner invalidate
//! them. Planning scans 24 sites per wire and uses one optional fact record per
//! wire temporarily; publication is atomic after the final cancellation check.

use super::{GraphPass, PassContext};
use crate::redpiler::PreparationTableStatistics;
use crate::redpiler::program_graph::{CompileGraph, WireQueryFacts};

/// Attaches checked static query facts without changing wire state or routing.
pub(crate) struct SpecializeWireQueries;

impl GraphPass for SpecializeWireQueries {
    fn name(&self) -> &'static str {
        "specialize-wire-queries"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let mut plans = Vec::with_capacity(graph.wires.len());
        for wire in &graph.wires {
            if context.monitor.cancelled() {
                return Err("cancelled while specializing wire queries".into());
            }
            let facts = WireQueryFacts::infer(graph, wire);
            plans.push((facts.known != 0 || facts.isolated).then_some(facts));
        }
        if context.monitor.cancelled() {
            return Err("cancelled before publishing wire query facts".into());
        }
        let isolated = plans.iter().flatten().filter(|facts| facts.isolated).count();
        let transfers = plans.iter().flatten().filter(|facts| facts.transfer.is_some()).count();
        for (wire, facts) in graph.wires.iter_mut().zip(plans) {
            wire.query_facts = facts;
        }
        context.monitor.record_preparation_tables(
            [("certified isolated wires", isolated), ("compiled wire transfers", transfers)]
                .into_iter()
                .map(|(table, entries)| PreparationTableStatistics {
                    stage: "wire query specialization",
                    table: table.into(),
                    entries,
                    capacity: entries,
                    estimated_bytes: 0,
                })
                .collect(),
        );
        Ok(())
    }

    fn preserves_dust_paths(&self) -> bool {
        true
    }

    fn preserves_turbo_callback_indices(&self) -> bool {
        true
    }

    fn preserves_wire_query_facts(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::components::{NodeState, NodeType};
    use crate::redpiler::passes::{PassEntry, PassPipeline, PassPipelineError, run_graph_pipeline};
    use crate::redpiler::program_graph::{
        CircuitNodeDraft, CircuitNodeKind, ComponentState, ContactId, ContactSpec, ContactVariant, EntryState, Guard, GuardId, NodeId, Profile,
        ProfileElectrical, ProfileId, SiteId, SiteSource, SiteSpec, WirePlan,
    };
    use crate::redpiler::{CompilerOptions, TaskMonitor};
    use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneWire};
    use mchprs_blocks::{BlockDirection, BlockPos};

    const ALL: u32 = (1 << 24) - 1;

    fn graph() -> CompileGraph {
        let wire = RedstoneWire::default();
        let lever = Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        };
        let nodes = vec![
            CircuitNodeDraft {
                position: Some(BlockPos::new(0, 20, 0)),
                raw_state: Some(Block::RedstoneWire { wire }.get_id()),
                kind: CircuitNodeKind::Wire(0),
                state: ComponentState::Wire(wire),
                output_route: None,
            },
            CircuitNodeDraft {
                position: Some(BlockPos::new(1, 20, 0)),
                raw_state: Some(lever.get_id()),
                kind: CircuitNodeKind::Electrical(NodeType::Lever),
                state: ComponentState::Electrical(NodeState::default()),
                output_route: None,
            },
            CircuitNodeDraft {
                position: Some(BlockPos::new(2, 20, 0)),
                raw_state: None,
                kind: CircuitNodeKind::Contact(ContactId::new(0)),
                state: ComponentState::Specialized,
                output_route: None,
            },
        ];
        let profiles = [Block::Air, Block::Stone {}]
            .into_iter()
            .map(|block| Profile {
                raw_state: block.get_id(),
                registry_type: block.registry_id(),
                solid: block.is_solid(),
                transparent: block.is_transparent(),
                cube: block.is_cube(),
                electrical: ProfileElectrical::None,
            })
            .collect();
        CompileGraph {
            nodes,
            links: vec![],
            power_links: vec![],
            comparator_inputs: vec![],
            profiles,
            guards: vec![Guard::Always],
            contacts: vec![ContactSpec {
                node: NodeId::new(2),
                position: BlockPos::new(2, 20, 0),
                variants: vec![ContactVariant {
                    when: GuardId::new(0),
                    profile: ProfileId::new(0),
                    receiver: None,
                }]
                .into_boxed_slice(),
            }],
            routes: vec![],
            turbo_callback_indices: vec![],
            wires: vec![WirePlan {
                node: NodeId::new(0),
                canonical_neighbors: std::array::from_fn(|slot| SiteId::new(slot as u32)),
                shape_route: None,
                turbo_route: None,
                query_facts: None,
                instant_reset: None,
                hop_root: None,
            }],
            dust_paths: vec![],
            wire_relations: vec![],
            sites: (0..24)
                .map(|slot| SiteSpec {
                    position: BlockPos::new(slot, 20, 1),
                    source: match slot {
                        1 => SiteSource::Constant(ProfileId::new(1)),
                        2 => SiteSource::Component(NodeId::new(1)),
                        3 => SiteSource::Component(NodeId::new(0)),
                        4 => SiteSource::Contact(ContactId::new(0)),
                        _ => SiteSource::Constant(ProfileId::new(0)),
                    },
                    source_strength: 0,
                })
                .collect(),
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
            instant_regions: Vec::new(),
            instant_proxies: Vec::new(),
            instant_machine: None,
        }
    }

    fn entry() -> EntryState {
        EntryState {
            logical_tick: 18,
            pending_ticks: vec![],
        }
    }

    fn run(graph: &mut CompileGraph, enabled: bool, monitor: &TaskMonitor) -> Result<(), PassPipelineError> {
        run_graph_pipeline(
            graph,
            &CompilerOptions::default(),
            &entry(),
            &PassPipeline::new(vec![PassEntry {
                pass: Box::new(SpecializeWireQueries),
                enabled,
            }]),
            monitor,
        )
    }

    #[test]
    fn certifies_static_properties_but_leaves_contacts_and_component_power_general() {
        let mut graph = graph();
        let before = graph.clone();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        let facts = graph.wires[0].query_facts.unwrap();
        assert_eq!(facts.known, ALL & !(1 << 4));
        assert_eq!(facts.solid, 1 << 1);
        assert_eq!(facts.wire, 1 << 3);
        assert_eq!(facts.zero_no_dust, ALL & !((1 << 1) | (1 << 2) | (1 << 4)));
        let lever = Block::from_id(graph.nodes[1].raw_state.unwrap());
        assert_eq!(facts.transparent & (1 << 2), u32::from(lever.is_transparent()) << 2);
        assert_eq!(facts.transparent & 1, u32::from(graph.profiles[0].transparent));
        assert_eq!(facts.transparent & ((1 << 1) | (1 << 4)), 0);
        let mut expected = before;
        expected.wires[0].query_facts = Some(facts);
        assert_eq!(graph, expected);
    }

    #[test]
    fn facts_do_not_depend_on_saved_component_or_wire_strengths() {
        let mut graph = graph();
        let expected = WireQueryFacts::infer(&graph, &graph.wires[0]);
        for strength in 0..=15 {
            let ComponentState::Electrical(state) = &mut graph.nodes[1].state else {
                unreachable!()
            };
            state.output_strength = strength;
            state.powered = strength != 0;
            state.pending_tick = strength % 2 == 0;
            let ComponentState::Wire(wire) = &mut graph.nodes[0].state else {
                unreachable!()
            };
            wire.power = strength;
            graph.sites[0].source_strength = strength;
            assert_eq!(WireQueryFacts::infer(&graph, &graph.wires[0]), expected);
            graph.validate().unwrap();
        }
    }

    #[test]
    fn dynamic_geometry_can_certify_a_nonwire_only_neighborhood() {
        let mut graph = graph();
        for site in &mut graph.sites {
            site.source = SiteSource::Contact(ContactId::new(0));
        }
        let mut expected = graph.clone();
        expected.wires[0].query_facts = Some(WireQueryFacts {
            isolated: true,
            transfer: WireQueryFacts::infer(&graph, &graph.wires[0]).transfer,
            ..Default::default()
        });
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, expected);
        assert_eq!(graph.wires[0].query_facts.unwrap().known, 0);
    }

    #[test]
    fn isolated_neighborhoods_allow_changing_nonwire_contacts_but_reject_possible_wires() {
        let mut graph = graph();
        for site in &mut graph.sites {
            site.source = SiteSource::Constant(ProfileId::new(0));
        }
        graph.sites[4].source = SiteSource::Contact(ContactId::new(0));
        assert!(WireQueryFacts::infer(&graph, &graph.wires[0]).isolated);

        graph.contacts[0].variants[0].receiver = Some(crate::redpiler::program_graph::CallbackReceiver::Component(NodeId::new(0)));
        assert!(
            !WireQueryFacts::infer(&graph, &graph.wires[0]).isolated,
            "typed wire receiver remains possible"
        );
        assert_ne!(WireQueryFacts::infer(&graph, &graph.wires[0]).transfer.unwrap().seed.direct & (1 << 4), 0);
        graph.contacts[0].variants[0].receiver = None;
        let block = Block::RedstoneWire { wire: Default::default() };
        graph.profiles.push(Profile {
            raw_state: block.get_id(),
            registry_type: block.registry_id(),
            solid: block.is_solid(),
            transparent: block.is_transparent(),
            cube: block.is_cube(),
            electrical: ProfileElectrical::None,
        });
        graph.contacts[0].variants[0].profile = ProfileId::new(2);
        assert!(!WireQueryFacts::infer(&graph, &graph.wires[0]).isolated, "wire profile remains possible");
        assert_ne!(WireQueryFacts::infer(&graph, &graph.wires[0]).transfer.unwrap().seed.direct & (1 << 4), 0);
        graph.contacts[0].variants[0].profile = ProfileId::new(0);
        graph.wires[0].canonical_neighbors[1] = graph.wires[0].canonical_neighbors[0];
        assert!(
            !WireQueryFacts::infer(&graph, &graph.wires[0]).isolated,
            "duplicate sites share visit state"
        );
    }

    #[test]
    fn validator_rejects_invalid_contact_profiles_before_checking_facts() {
        let mut graph = graph();
        graph.sites[3].source = SiteSource::Constant(ProfileId::new(0));
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        graph.contacts[0].variants[0].profile = ProfileId::new(99);
        assert!(matches!(
            graph.validate(),
            Err(crate::redpiler::program_graph::GraphBuildError::InvalidReference {
                table: "profiles",
                index: 99,
                ..
            })
        ));
    }

    #[test]
    fn validator_rejects_corrupted_or_stale_facts() {
        let mut graph = graph();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        for corrupt in 0..7 {
            let mut bad = graph.clone();
            let facts = bad.wires[0].query_facts.as_mut().unwrap();
            match corrupt {
                0 => facts.known |= 1 << 24,
                1 => facts.known |= 1 << 4,
                2 => facts.solid ^= 1,
                3 => facts.transparent ^= 1,
                4 => facts.wire ^= 1,
                5 => facts.zero_no_dust |= 1 << 2,
                _ => facts.isolated = true,
            }
            assert!(bad.validate().is_err(), "corruption {corrupt} was accepted");
        }
        graph.sites[0].source = SiteSource::Contact(ContactId::new(0));
        assert!(graph.validate().is_err());
    }

    #[test]
    fn builder_is_idempotent_disabled_is_untouched_and_cancellation_is_atomic() {
        let mut graph = graph();
        let before = graph.clone();
        run(&mut graph, false, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, before);
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(matches!(run(&mut graph, true, &monitor), Err(PassPipelineError::Cancelled)));
        assert_eq!(graph, before);
        let options = CompilerOptions::default();
        let entry = entry();
        assert!(
            SpecializeWireQueries
                .run(
                    &mut graph,
                    &PassContext {
                        monitor: &monitor,
                        options: &options,
                        entry: &entry
                    }
                )
                .is_err()
        );
        assert_eq!(graph, before);
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        let once = graph.clone();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, once);
        assert!(SpecializeWireQueries.preserves_dust_paths());
        assert!(SpecializeWireQueries.preserves_turbo_callback_indices());
        assert!(SpecializeWireQueries.preserves_wire_query_facts());
    }

    #[test]
    fn later_graph_mutation_invalidates_facts_and_builder_can_recompute_them() {
        struct ChangeProfile;
        impl GraphPass for ChangeProfile {
            fn name(&self) -> &'static str {
                "change-static-profile"
            }
            fn validate_input(&self, _: &CompileGraph) -> Result<(), String> {
                Ok(())
            }
            fn run(&self, graph: &mut CompileGraph, _: &PassContext<'_>) -> Result<(), String> {
                graph.sites[0].source = SiteSource::Contact(ContactId::new(0));
                Ok(())
            }
        }
        let mut graph = graph();
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert!(graph.wires[0].query_facts.is_some());
        run_graph_pipeline(
            &mut graph,
            &CompilerOptions::default(),
            &entry(),
            &PassPipeline::new(vec![PassEntry {
                pass: Box::new(ChangeProfile),
                enabled: true,
            }]),
            &TaskMonitor::default(),
        )
        .unwrap();
        assert_eq!(graph.wires[0].query_facts, None);
        run(&mut graph, true, &TaskMonitor::default()).unwrap();
        assert_eq!(graph.wires[0].query_facts.unwrap().known, ALL & !(1 | (1 << 4)));
    }
}
