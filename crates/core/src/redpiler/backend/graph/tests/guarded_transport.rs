//! Conditional wire geometry and ordered receiver coverage for dense transport.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::program_graph::{ResourceState, ValueGeometrySelector};
use mchprs_blocks::blocks::{Lever, RedstoneComparator, RedstonePiston, RedstoneRepeater};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing};
use mchprs_world::TickPriority;

#[derive(Clone, Copy)]
enum Receiver {
    Piston,
    Torch,
    Comparator,
    Repeater,
}

fn guarded_stair(receiver: Receiver, moving_geometry: bool, singleton: bool) -> CompileGraph {
    guarded_stair_with_lock_source(receiver, moving_geometry, singleton, false)
}

fn guarded_stair_with_lock_source(receiver: Receiver, moving_geometry: bool, singleton: bool, lock_source: bool) -> CompileGraph {
    let mut world = empty_world();
    let source = BlockPos::new(4, 20, 6);
    let wires = if singleton {
        vec![BlockPos::new(5, 20, 6)]
    } else {
        vec![
            BlockPos::new(5, 20, 6),
            BlockPos::new(6, 21, 6),
            BlockPos::new(7, 21, 6),
            BlockPos::new(8, 20, 6),
            BlockPos::new(9, 20, 6),
        ]
    };
    let mut lever = Lever::default();
    lever.powered = true;
    world.set_block(source, Block::Lever { lever });
    for &wire in &wires {
        world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(wire, Block::RedstoneWire { wire: Default::default() });
    }
    if !singleton {
        // These supports enable the ascending and descending diagonal transfers.
        world.set_block(BlockPos::new(6, 20, 6), Block::Stone {});
        world.set_block(BlockPos::new(7, 20, 6), Block::Stone {});
    }

    // A recognized instant actor is required to form a transport corridor.
    let receiver_pos = wires.last().copied().unwrap().offset(BlockFace::East);
    world.set_block(receiver_pos.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        receiver_pos,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        receiver_pos.offset(BlockFace::East),
        Block::PistonHead {
            head: mchprs_blocks::blocks::RedstonePistonHead {
                facing: BlockFacing::East,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(receiver_pos.offset(BlockFace::East).offset(BlockFace::East), Block::RedstoneBlock {});
    world.set_block(
        receiver_pos.offset(BlockFace::Top),
        Block::Observer {
            observer: mchprs_blocks::blocks::RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(receiver_pos.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});

    // Delayed receivers share a side of the corridor; the instant piston cell
    // remains present in every case to give the corridor a certified boundary.
    let delayed_receiver_pos = wires[wires.len() / 2].offset(BlockFace::South);
    if !matches!(receiver, Receiver::Piston) {
        world.set_block(delayed_receiver_pos.offset(BlockFace::Bottom), Block::Stone {});
        let device = match receiver {
            Receiver::Piston => unreachable!(),
            Receiver::Torch => Block::RedstoneTorch { lit: false },
            Receiver::Comparator => Block::RedstoneComparator {
                comparator: RedstoneComparator {
                    facing: BlockDirection::North,
                    ..Default::default()
                },
            },
            Receiver::Repeater => Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: BlockDirection::North,
                    ..Default::default()
                },
            },
        };
        world.set_block(delayed_receiver_pos, device);
    }
    if lock_source && matches!(receiver, Receiver::Repeater) {
        world.set_block(
            delayed_receiver_pos.offset(BlockFace::East),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: BlockDirection::East,
                    powered: true,
                    ..Default::default()
                },
            },
        );
    }
    if moving_geometry && !singleton {
        // An unextended sticky piston can move this stone out of the horizontal
        // selector for a descending diagonal without removing a wire support.
        let selector = wires[1].offset(BlockFace::West);
        let base = selector.offset(BlockFace::North);
        world.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::South,
                    sticky: true,
                    extended: false,
                },
            },
        );
        world.set_block(selector, Block::Stone {});
    }

    let monitor = TaskMonitor::default();
    let options = CompilerOptions::parse("-O5").unwrap();
    let bounds = (BlockPos::new(1, 1, 1), BlockPos::new(16, 34, 16));
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let mut pipeline = passes::default_graph_pipeline(&options);
    if singleton {
        // Isolate transport ownership from the existing faster single-wire specialization.
        pipeline
            .entries
            .retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if builder.name() == "experimental-compile-instant-wire-nodes"));
    }
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    // A singleton may also have an exact signal-channel specialization; this
    // fixture isolates dense transport so the selected execution path is clear.
    prepared.graph.plans.signal_channels = Default::default();
    prepared.graph
}

fn reference_graph(graph: &CompileGraph) -> CompileGraph {
    let mut reference = graph.clone();
    reference.clear_execution_plans();
    reference
}

fn no_transport_panic(graph: &CompileGraph, context: &str) -> ! {
    panic!("{context}; compiler rejection report: {}", graph.instant_transport_rejection_report());
}

fn same_after_seed(graph: &CompileGraph, root: NodeId, repeater_case: Option<(bool, bool, bool)>) {
    let reference_graph = reference_graph(graph);
    let (candidate_program, initial) = graph.clone().lower().unwrap();
    let (reference_program, reference_initial) = reference_graph.clone().lower().unwrap();
    let mut candidate = GraphBackend::new(candidate_program, initial, 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, reference_initial, 0).unwrap();
    candidate.set_test_recording(true);
    reference.set_test_recording(true);

    for backend in [&mut candidate, &mut reference] {
        let wire_nodes = backend.program.wires().iter().map(|wire| wire.node).collect::<Vec<_>>();
        for (index, node) in wire_nodes.into_iter().enumerate() {
            let mut state = backend.wire_state(node);
            state.power = [0, 4, 15, 7, 1][index % 5];
            backend.set_wire(node, state);
        }
        if let Some((powered, locked, pending)) = repeater_case {
            let repeater = backend
                .program
                .nodes()
                .iter()
                .position(|node| {
                    node.position == Some(BlockPos::new(7, 21, 7)) && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
                })
                .map(|index| NodeId::new(index as u32))
                .unwrap();
            let ComponentState::Electrical(state) = &mut backend.state.components[repeater.index()] else {
                unreachable!()
            };
            state.powered = powered;
            state.output_strength = u8::from(powered) * 15;
            state.repeater_locked = locked;
            if pending {
                backend.schedule(repeater, 1, TickPriority::Normal);
            }
        }
        let seed = backend.seed_power_general(&backend.wire_plan(root).clone()).power;
        let mut root_state = backend.wire_state(root);
        root_state.power = if seed == 15 { 0 } else { 15 };
        backend.set_wire(root, root_state);
        backend.evaluate_seed(root, None).unwrap();
        if repeater_case.is_none_or(|case| case == (false, false, false)) {
            if let Some((index, node)) = backend
                .program
                .nodes()
                .iter()
                .enumerate()
                .find(|(_, node)| node.position == Some(BlockPos::new(7, 21, 7)))
            {
                if matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Comparator { .. })) {
                    assert!(
                        backend.future.pending_nodes[index] > 0,
                        "the changed wire must schedule its delayed receiver"
                    );
                } else if matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) {
                    let receiver = NodeId::new(index as u32);
                    let expected_pending = !backend.repeater_locked(receiver);
                    assert_eq!(
                        backend.future.pending_nodes[index] > 0,
                        expected_pending,
                        "an unlocked unpowered repeater schedules; a locked one does not"
                    );
                }
            }
        }
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    let expected_fast_path = graph
        .plans
        .instant_transport
        .nets
        .iter()
        .any(|net| net.wires.iter().any(|wire| wire.node == root));
    assert_eq!(candidate.execution_counts().instant_transport_invocations > 0, expected_fast_path);
}

fn same_after_seed_with_resource(graph: &CompileGraph, root: NodeId, occupied_selector: bool) {
    let ordinary = reference_graph(graph);
    let (candidate_program, initial) = graph.clone().lower().unwrap();
    let (reference_program, reference_initial) = ordinary.lower().unwrap();
    let mut candidate = GraphBackend::new(candidate_program, initial, 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, reference_initial, 0).unwrap();
    let selector = BlockPos::new(5, 21, 6);
    let site = graph.sites.iter().position(|site| site.position == selector).expect("selector site");
    let contact = match graph.sites[site].source {
        SiteSource::Contact(contact) => contact,
        other => panic!("expected dynamic selector contact, found {other:?}"),
    };
    for backend in [&mut candidate, &mut reference] {
        let resource = backend
            .program
            .resources()
            .iter()
            .position(|resource| resource.contacts.contains(&contact))
            .expect("selector resource");
        let other = backend.program.resources()[resource]
            .contacts
            .iter()
            .copied()
            .find(|&candidate| candidate != contact)
            .expect("resource has an alternate contact");
        backend.state.resources[resource].at = if occupied_selector { contact } else { other };
        backend.state.resources[resource].enabled = true;
        backend.evaluate_seed(root, None).unwrap();
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    assert!(candidate.execution_counts().instant_transport_invocations > 0);
}

#[test]
fn guarded_transport_retains_conditional_ascent_descent_and_selector_captures() {
    let graph = guarded_stair(Receiver::Piston, true, false);
    assert!(graph.plans.instant_transport.guarded);
    let transport = graph
        .plans
        .instant_transport
        .nets
        .iter()
        .find(|net| net.wires.iter().any(|wire| wire.conditional_inputs != 0))
        .unwrap_or_else(|| no_transport_panic(&graph, "the stair should compile as a guarded corridor"));
    let mut ascent = false;
    let mut descent = false;
    for wire in &transport.wires {
        let mask = wire.conditional_inputs;
        ascent |= mask & ((1 << 8) | (1 << 13) | (1 << 20) | (1 << 21)) != 0;
        descent |= mask & ((1 << 7) | (1 << 12) | (1 << 17) | (1 << 18)) != 0;
        for slot in 0..24 {
            if mask & (1 << slot) == 0 {
                continue;
            }
            assert_ne!(wire.neighbors[slot], u32::MAX, "conditional source capture slot {slot}");
            let geometry_slots: &[usize] = match slot {
                7 => &[0],
                8 => &[0, 3],
                12 => &[1],
                13 => &[1, 3],
                17 => &[4],
                20 => &[4, 3],
                18 => &[5],
                21 => &[5, 3],
                _ => panic!("unexpected conditional slot {slot}"),
            };
            for &geometry_slot in geometry_slots {
                assert_ne!(wire.neighbors[geometry_slot], u32::MAX, "geometry capture slot {geometry_slot}");
            }
        }
    }
    assert!(ascent && descent, "expected both conditional directions in {transport:?}");

    let ordinary = reference_graph(&graph);
    let (program, _) = ordinary.lower().unwrap();
    let root = program.position_index()[&BlockPos::new(6, 21, 6)];
    same_after_seed(&graph, root, None);
}

#[test]
fn guarded_transport_replays_torch_comparator_and_repeater_boundaries_in_order() {
    for receiver in [Receiver::Torch, Receiver::Comparator, Receiver::Repeater] {
        let graph = guarded_stair(receiver, false, false);
        let net = graph
            .plans
            .instant_transport
            .nets
            .iter()
            .find(|net| net.ordered_receivers)
            .unwrap_or_else(|| no_transport_panic(&graph, "the selected delayed receiver must mark ordered dispatch"));
        if matches!(receiver, Receiver::Repeater) {
            assert!(!net.canonical_repeaters.is_empty());
        }
        if matches!(receiver, Receiver::Repeater) {
            for lock_source in [false, true] {
                let graph = guarded_stair_with_lock_source(receiver, false, false, lock_source);
                let ordinary = reference_graph(&graph);
                let (program, _) = ordinary.lower().unwrap();
                let root = program.position_index()[&BlockPos::new(8, 20, 6)];
                let (check_program, check_state) = graph.clone().lower().unwrap();
                let check_backend = GraphBackend::new(check_program, check_state, 0).unwrap();
                let repeater = check_backend
                    .program
                    .nodes()
                    .iter()
                    .position(|node| {
                        node.position == Some(BlockPos::new(7, 21, 7)) && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
                    })
                    .map(|index| NodeId::new(index as u32))
                    .unwrap();
                assert_eq!(check_backend.repeater_locked(repeater), lock_source);
                for powered in [false, true] {
                    for locked in [false, true] {
                        for pending in [false, true] {
                            same_after_seed(&graph, root, Some((powered, locked, pending)));
                        }
                    }
                }
                same_after_seed(&graph, root, None);
            }
        } else {
            let ordinary = reference_graph(&graph);
            let (program, _) = ordinary.lower().unwrap();
            let root = program.position_index()[&BlockPos::new(8, 20, 6)];
            same_after_seed(&graph, root, None);
        }
    }
}

#[test]
fn guarded_transport_rejects_noncanonical_imported_repeater_before_effects() {
    let graph = guarded_stair(Receiver::Repeater, false, false);
    let transport = graph
        .plans
        .instant_transport
        .nets
        .iter()
        .position(|net| !net.canonical_repeaters.is_empty())
        .unwrap_or_else(|| no_transport_panic(&graph, "repeater receiver corridor"));
    let ordinary = reference_graph(&graph);
    let (candidate_program, mut candidate_state) = graph.clone().lower().unwrap();
    let (reference_program, _) = ordinary.lower().unwrap();
    let repeater = graph.plans.instant_transport.nets[transport].canonical_repeaters[0];
    let ComponentState::Electrical(state) = &mut candidate_state.components[repeater.index()] else {
        unreachable!()
    };
    state.powered = true;
    state.output_strength = 7;
    let initial = candidate_state.clone();
    let mut candidate = GraphBackend::new(candidate_program, candidate_state, 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, initial, 0).unwrap();
    assert!(!candidate.transport_entry_admitted(transport as u32));
    let root = graph.plans.instant_transport.nets[transport].wires[0].node;
    candidate.evaluate_seed(root, None).unwrap();
    reference.evaluate_seed(root, None).unwrap();
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.execution_counts().instant_transport_invocations, 0);
    assert!(candidate.execution_counts().wire_walks > 0);
}

#[test]
fn guarded_transport_retains_live_contact_geometry_and_admits_singletons() {
    let graph = guarded_stair(Receiver::Piston, true, false);
    let net = graph
        .plans
        .instant_transport
        .nets
        .iter()
        .find(|net| net.wires.iter().any(|wire| wire.conditional_inputs != 0))
        .unwrap_or_else(|| no_transport_panic(&graph, "contact stair corridor"));
    let contacts = net
        .sites
        .iter()
        .filter(|site| matches!(graph.sites[site.index()].source, SiteSource::Contact(_)))
        .copied()
        .collect::<Vec<_>>();
    assert!(!contacts.is_empty(), "the dynamic selector should be a retained contact site");
    let selector_site = graph.sites.iter().position(|site| site.position == BlockPos::new(5, 21, 6)).unwrap();
    assert!(contacts.contains(&SiteId::new(selector_site as u32)));
    let ordinary = reference_graph(&graph);
    let (program, _) = ordinary.lower().unwrap();
    let root = program.position_index()[&BlockPos::new(5, 20, 6)];
    same_after_seed_with_resource(&graph, root, true);
    same_after_seed_with_resource(&graph, root, false);

    let singleton = guarded_stair(Receiver::Piston, false, true);
    let singleton_net = singleton
        .plans
        .instant_transport
        .nets
        .iter()
        .find(|net| net.wires.len() == 1)
        .unwrap_or_else(|| no_transport_panic(&singleton, "guarded transport admits singleton corridors"));
    assert!(!singleton_net.ordered_receivers);
    assert!(singleton.plans.stable_instant_logic.nets.iter().all(Option::is_none));
    let ordinary = reference_graph(&singleton);
    let (program, _) = ordinary.lower().unwrap();
    same_after_seed(&singleton, program.position_index()[&BlockPos::new(5, 20, 6)], None);
}

#[test]
fn value_region_cache_rechecks_changed_conditional_selector_between_walks() {
    let mut graph = guarded_stair(Receiver::Piston, true, false);
    graph.plans.instant_transition_programs = Default::default();
    let (net_index, net) = graph
        .plans
        .instant_transport
        .nets
        .iter()
        .enumerate()
        .find(|(index, net)| {
            net.wires.iter().any(|wire| wire.conditional_inputs != 0)
                && graph.plans.instant_value_regions.nets[*index].as_ref().is_some_and(|values| {
                    values
                        .regions
                        .iter()
                        .any(|region| region.conditional_arcs.start != region.conditional_arcs.end)
                })
        })
        .map(|(index, net)| (index, net.clone()))
        .unwrap_or_else(|| no_transport_panic(&graph, "conditional value-region cache fixture"));
    let values = graph.plans.instant_value_regions.nets[net_index].as_ref().unwrap();
    let region = values
        .regions
        .iter()
        .find(|region| region.conditional_arcs.start != region.conditional_arcs.end)
        .unwrap();
    let root = values.region_wires[region.wires.start as usize];
    let mut selector_ids = values.conditional_arcs[region.conditional_arcs.start as usize..region.conditional_arcs.end as usize]
        .iter()
        .map(|arc| arc.selector)
        .collect::<Vec<_>>();
    selector_ids.sort_unstable();
    selector_ids.dedup();
    let selectors = selector_ids.iter().map(|&id| values.selectors[id as usize]).collect::<Vec<_>>();

    let selector_position = BlockPos::new(5, 21, 6);
    let selector_site_index = graph.sites.iter().position(|site| site.position == selector_position).unwrap();
    let SiteSource::Contact(selector_contact) = graph.sites[selector_site_index].source else {
        panic!("expected moving selector contact");
    };

    let mut ordinary = graph.clone();
    ordinary.plans.instant_value_regions = Default::default();
    let (program, mut initial) = graph.clone().lower().unwrap();
    let (reference_program, mut reference_initial) = ordinary.lower().unwrap();
    let source = program.position_index()[&BlockPos::new(4, 20, 6)];
    for state in [&mut initial, &mut reference_initial] {
        let ComponentState::Electrical(state) = &mut state.components[source.index()] else {
            unreachable!()
        };
        state.powered = false;
        state.output_strength = 0;
    }
    let mut candidate = GraphBackend::new(program, initial, 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, reference_initial, 0).unwrap();
    candidate.set_test_recording(true);
    reference.set_test_recording(true);

    let resource = candidate
        .program
        .resources()
        .iter()
        .position(|resource| resource.contacts.contains(&selector_contact))
        .expect("selector resource");
    let contacts = candidate.program.resources()[resource].contacts.to_vec();
    let original_resource = candidate.state.resources[resource];
    let mut poses = None;
    for &first in &contacts {
        let first_state = candidate.state.resources[resource];
        candidate.state.resources[resource] = ResourceState {
            at: first,
            enabled: true,
            revision: first_state.revision.wrapping_add(1),
        };
        let first_selectors = conditional_selector_values(&candidate, &selectors);
        if let Some(&second) = contacts.iter().find(|&&second| {
            if second == first {
                return false;
            }
            let previous = candidate.state.resources[resource];
            candidate.state.resources[resource] = ResourceState {
                at: second,
                enabled: true,
                revision: previous.revision.wrapping_add(1),
            };
            let second_selectors = conditional_selector_values(&candidate, &selectors);
            candidate.state.resources[resource] = previous;
            first_selectors != second_selectors
        }) {
            poses = Some((first, second));
            break;
        }
    }
    let (first_pose, second_pose) = poses.expect("selector contact has distinct solid/transparent poses");
    candidate.state.resources[resource] = original_resource;
    for backend in [&mut candidate, &mut reference] {
        backend.state.resources[resource] = ResourceState {
            at: first_pose,
            enabled: true,
            revision: backend.state.resources[resource].revision.wrapping_add(1),
        };
    }
    assert!(net.wires.iter().all(|wire| candidate.wire_state(wire.node).power == 0));

    let run_both = |candidate: &mut GraphBackend, reference: &mut GraphBackend| {
        candidate.run_instant_transport(net_index as u32, root, 0).unwrap();
        reference.run_instant_transport(net_index as u32, root, 0).unwrap();
        candidate.assert_same_execution_state(reference);
        assert_eq!(candidate.observed_operations, reference.observed_operations);
    };
    run_both(&mut candidate, &mut reference);
    let solves_after_first = candidate.execution_counts().instant_value_region_solves;
    run_both(&mut candidate, &mut reference);
    assert_eq!(candidate.execution_counts().instant_value_region_solves, solves_after_first);
    assert!(
        candidate.execution_counts().instant_value_region_cache_hits > 0,
        "unchanged inputs should reuse the inert certificate"
    );

    let old_selectors = conditional_selector_values(&candidate, &selectors);
    for backend in [&mut candidate, &mut reference] {
        let previous = backend.state.resources[resource];
        backend.state.resources[resource] = ResourceState {
            at: second_pose,
            enabled: true,
            revision: previous.revision.wrapping_add(1),
        };
    }
    let new_selectors = conditional_selector_values(&candidate, &selectors);
    assert_ne!(old_selectors, new_selectors, "the chosen resource pose must toggle a compiled selector");
    let solves_before_selector_change = candidate.execution_counts().instant_value_region_solves;
    run_both(&mut candidate, &mut reference);
    assert!(
        candidate.execution_counts().instant_value_region_solves > solves_before_selector_change,
        "a changed conditional selector must trigger a fresh solve even when every resulting wire remains zero"
    );
    assert!(net.wires.iter().all(|wire| candidate.wire_state(wire.node).power == 0));
}

#[test]
fn transition_replay_rechecks_a_used_conditional_geometry_selector() {
    let graph = guarded_stair(Receiver::Piston, true, false);
    let (net_index, net) = graph
        .plans
        .instant_transport
        .nets
        .iter()
        .enumerate()
        .find(|(index, net)| {
            net.wires.iter().any(|wire| wire.conditional_inputs != 0) && graph.plans.instant_transition_programs.nets[*index].is_some()
        })
        .map(|(index, net)| (index, net.clone()))
        .unwrap_or_else(|| no_transport_panic(&graph, "conditional transition-program fixture"));
    let values = graph.plans.instant_value_regions.nets[net_index]
        .as_ref()
        .unwrap_or_else(|| no_transport_panic(&graph, "conditional value-region selector fixture"));
    assert!(graph.plans.signal_channels.channels.get(net_index).is_none_or(Option::is_none));
    assert!(graph.plans.stable_instant_logic.nets.iter().all(Option::is_none));

    let selectors = values.selectors.to_vec();
    let selector_position = BlockPos::new(5, 21, 6);
    let selector_site_index = graph.sites.iter().position(|site| site.position == selector_position).unwrap();
    let SiteSource::Contact(selector_contact) = graph.sites[selector_site_index].source else {
        panic!("expected moving selector contact");
    };
    let ordinary = {
        let mut reference = graph.clone();
        reference.plans.instant_transition_programs = Default::default();
        reference
    };
    let (candidate_program, initial) = graph.clone().lower().unwrap();
    let (reference_program, _) = ordinary.lower().unwrap();
    let resource = candidate_program
        .resources()
        .iter()
        .position(|resource| resource.contacts.contains(&selector_contact))
        .expect("selector resource");
    let contacts = candidate_program.resources()[resource].contacts.to_vec();
    let source = candidate_program.position_index()[&BlockPos::new(4, 20, 6)];
    let transition = candidate_program.instant_transition_net(net_index as u32).unwrap();
    let mut poses = None;
    for &first in &contacts {
        let mut candidate_state = initial.clone();
        candidate_state.resources[resource] = ResourceState {
            at: first,
            enabled: true,
            revision: candidate_state.resources[resource].revision,
        };
        if let ComponentState::Electrical(state) = &mut candidate_state.components[source.index()] {
            state.powered = false;
            state.output_strength = 0;
        } else {
            unreachable!();
        }
        for wire in &net.wires {
            if let ComponentState::Wire(state) = &mut candidate_state.components[wire.node.index()] {
                state.power = 0;
            } else {
                unreachable!();
            }
        }
        let mut probe = GraphBackend::new(candidate_program.clone(), candidate_state, 0).unwrap();
        let first_values = conditional_selector_values(&probe, &selectors);
        let Some(&second) = contacts.iter().find(|&&second| {
            if second == first {
                return false;
            }
            let previous = probe.state.resources[resource];
            probe.state.resources[resource] = ResourceState {
                at: second,
                enabled: true,
                revision: previous.revision,
            };
            let second_values = conditional_selector_values(&probe, &selectors);
            probe.state.resources[resource] = previous;
            first_values != second_values
        }) else {
            continue;
        };
        poses = Some((first, second));
        break;
    }
    let (first_pose, second_pose) = poses.expect("resource poses toggle a compiled conditional selector");

    // Try each root independently. Some roots do not visit a conditional wire;
    // only assert stale rejection for a root proven to have captured that guard.
    let mut found_used_selector = false;
    for root in 0..net.wires.len() as u32 {
        let mut candidate_state = initial.clone();
        candidate_state.resources[resource] = ResourceState {
            at: first_pose,
            enabled: true,
            revision: candidate_state.resources[resource].revision,
        };
        if let ComponentState::Electrical(state) = &mut candidate_state.components[source.index()] {
            state.powered = false;
            state.output_strength = 0;
        } else {
            unreachable!();
        }
        for wire in &net.wires {
            if let ComponentState::Wire(state) = &mut candidate_state.components[wire.node.index()] {
                state.power = 0;
            } else {
                unreachable!();
            }
        }
        let reference_state = candidate_state.clone();
        let mut candidate = GraphBackend::new(candidate_program.clone(), candidate_state, 0).unwrap();
        let mut reference = GraphBackend::new(reference_program.clone(), reference_state, 0).unwrap();
        candidate.set_test_recording(true);
        reference.set_test_recording(true);

        let run_both = |candidate: &mut GraphBackend, reference: &mut GraphBackend| {
            candidate.run_instant_transport(net_index as u32, root, 0).unwrap();
            reference.run_instant_transport(net_index as u32, root, 0).unwrap();
            candidate.assert_same_execution_state(reference);
            assert_eq!(candidate.observed_operations, reference.observed_operations);
        };
        run_both(&mut candidate, &mut reference);
        if candidate.execution_counts().instant_transition_programs == 0 {
            continue;
        }
        run_both(&mut candidate, &mut reference);
        if candidate.execution_counts().instant_transition_hits == 0 {
            continue;
        }
        assert!(net.wires.iter().all(|wire| candidate.wire_state(wire.node).power == 0));

        let queries = transition.wire_queries.iter().copied().collect::<std::collections::BTreeSet<_>>();
        let old_queries = queries
            .iter()
            .map(|&query| candidate.instant_drive_query_power(query))
            .collect::<Vec<_>>();
        for backend in [&mut candidate, &mut reference] {
            let previous = backend.state.resources[resource];
            backend.state.resources[resource] = ResourceState {
                at: second_pose,
                enabled: true,
                revision: previous.revision,
            };
        }
        assert_eq!(
            queries
                .iter()
                .map(|&query| candidate.instant_drive_query_power(query))
                .collect::<Vec<_>>(),
            old_queries,
            "resource pose change must leave typed source drives unchanged"
        );
        assert!(net.wires.iter().all(|wire| candidate.wire_state(wire.node).power == 0));

        let misses_before_pose_change = candidate.execution_counts().instant_transition_misses;
        run_both(&mut candidate, &mut reference);
        if candidate.execution_counts().instant_transition_misses > misses_before_pose_change {
            found_used_selector = true;
            assert!(net.wires.iter().all(|wire| candidate.wire_state(wire.node).power == 0));
            break;
        }
    }
    assert!(found_used_selector, "no root that used the changed selector was found in this fixture");
}

fn conditional_selector_values(backend: &GraphBackend, selectors: &[ValueGeometrySelector]) -> Vec<bool> {
    selectors
        .iter()
        .map(|selector| {
            let horizontal = backend.live_site(selector.horizontal_site);
            if let Some(up) = selector.ascent_up_site {
                horizontal.solid && !horizontal.transparent && !backend.live_site(up).solid
            } else {
                !horizontal.solid
            }
        })
        .collect()
}
