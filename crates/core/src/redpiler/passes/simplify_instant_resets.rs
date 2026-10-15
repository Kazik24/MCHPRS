//! Simplifies power reads in a private redstone-token/base-observer reset family.
//!
//! Recognition follows one sticky horizontal actor, its private Near/Far token,
//! downward base observer, fixed normal/QC conductors and two output wires. Each
//! piston input and conductor must have exactly one unconditional, unattenuated
//! contributor. The rewrite substitutes that contributor at the original input;
//! the observer's correctly oriented Strong query becomes its live Output query.
//! A singleton max with zero attenuation is an identity for every strength, even
//! when an observer's powered bit and stored output disagree. Wire Strong faces,
//! input ports and guards remain exact. Sampling and FIFO acceptance therefore
//! see the same power without traversing the two conductors.
//!
//! A stricter south-facing two-wire grammar also receives the existing runtime's
//! instant-reset certificate. It proves the private alternating walk and retains
//! captured repeater/head callbacks; it does not contract the actor lifecycle.
//! State, ticks, resource ownership, lane checks and projection remain general.
//! Unsupported motifs remain general. Discovery indexes tables once, then reads
//! incident edges per actor; memory is linear in those tables. Replacements are
//! planned before a final cancellation check, and repeated application is inert.

#[cfg(test)]
use super::compile_instant_resets::CompileInstantResets;
use super::{GraphPass, PassContext};
use crate::redpiler::analysis::instant_resets::discover_candidates;
#[cfg(test)]
use crate::redpiler::analysis::instant_resets::{DiscoveryIndex, InstantResetCandidate, check_two_wire_kernel, infer_instant_resets};
#[cfg(test)]
use crate::redpiler::program_graph::{CallbackReceiver, CircuitNodeKind, Guard, OperandView, PowerTarget, ProfileElectrical, SiteId, SiteSource};
use crate::redpiler::program_graph::{CompileGraph, PowerPort};
#[cfg(test)]
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};

/// Removes two proved conductor traversals from a private instant-reset motif.
pub(crate) struct SimplifyInstantResets;

impl GraphPass for SimplifyInstantResets {
    fn name(&self) -> &'static str {
        "simplify-instant-resets"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let candidates = discover_candidates(graph, context.monitor)?;
        let recognized = candidates.len();
        let mut replacements = Vec::with_capacity(candidates.len() * 2);
        for candidate in candidates {
            check_cancelled(context.monitor)?;
            for (outer, inner) in [(candidate.main_edge, candidate.normal_edge), (candidate.quasi_edge, candidate.qc_edge)] {
                let mut replacement = graph.power_links[outer];
                replacement.from = graph.power_links[inner].from;
                replacement.from_port = if inner == candidate.qc_edge {
                    PowerPort::Output
                } else {
                    graph.power_links[inner].from_port
                };
                replacements.push((outer, replacement));
            }
        }
        check_cancelled(context.monitor)?;
        for (edge, replacement) in replacements {
            graph.power_links[edge] = replacement;
        }
        context.monitor.record_preparation_tables(
            [("recognized reset cells", recognized)]
                .into_iter()
                .map(|(table, entries)| crate::redpiler::PreparationTableStatistics {
                    stage: "instant reset specialization",
                    table: table.into(),
                    entries,
                    capacity: entries,
                    estimated_bytes: 0,
                })
                .collect(),
        );
        Ok(())
    }
}

fn check_cancelled(monitor: &crate::redpiler::TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while simplifying instant resets".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::plot::worldedit::{load_schematic, paste_clipboard};
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redpiler::passes::rich;
    use crate::redpiler::program_graph::{ComponentState, EntryState};
    use crate::redpiler::{CompilerOptions, TaskMonitor};
    use crate::world::World;
    use crate::world::storage::Chunk;
    use sha2::{Digest, Sha256};

    const ORIGIN: BlockPos = BlockPos::new(8, 12, 8);

    pub(crate) fn fixture() -> (PlotWorld, (BlockPos, BlockPos)) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/test-prefix-20261008/TEST_PISTION_2.schem");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "567f61a69ff36e693916ea6069241ba2b56882f384b022fc1d88dd710b4985df"
        );
        let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..3).flat_map(|x| (0..3).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        paste_clipboard(
            &mut world,
            &clipboard,
            ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
            false,
        );
        let bounds = (
            ORIGIN - BlockPos::new(2, 2, 2),
            ORIGIN + BlockPos::new(clipboard.size_x as i32 + 1, clipboard.size_y as i32 + 1, clipboard.size_z as i32 + 1),
        );
        (world, bounds)
    }

    fn graph() -> CompileGraph {
        let (world, bounds) = fixture();
        rich::prepare(&world, bounds, vec![], &TaskMonitor::default(), AnalysisLimits::default())
            .unwrap()
            .graph
    }

    fn apply(graph: &mut CompileGraph, monitor: &TaskMonitor) -> Result<(), String> {
        use super::super::planning::PlanBuilder;
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        let context = PassContext {
            monitor,
            options: &options,
            entry: &entry,
        };
        SimplifyInstantResets.run(graph, &context)?;
        CompileInstantResets.run(graph, &context)
    }

    fn candidate(graph: &CompileGraph) -> InstantResetCandidate {
        let mut found = discover_candidates(graph, &TaskMonitor::default()).unwrap();
        assert_eq!(found.len(), 1, "the pinned original fixture must match structurally");
        found.pop().unwrap()
    }

    #[test]
    fn original_fixture_rewrites_only_two_input_occurrences_and_is_idempotent() {
        let mut graph = graph();
        graph.validate().unwrap();
        let witness = candidate(&graph);
        assert_eq!(graph.resources[witness.resource.index()].contacts.len(), 2);
        assert_eq!(
            graph.nodes[witness.output_wires[0].index()]
                .position
                .unwrap()
                .offset(graph.pistons[witness.piston.index()].facing),
            graph.nodes[witness.output_wires[1].index()].position.unwrap()
        );
        assert_eq!(
            graph.sites[graph.power_links[witness.main_edge].from.index()].position,
            graph.pistons[witness.piston.index()].position.offset(BlockFace::North)
        );
        let before = graph.clone();
        let mut expected = before.clone();
        expected.power_links[witness.main_edge].from = before.power_links[witness.normal_edge].from;
        expected.power_links[witness.main_edge].from_port = before.power_links[witness.normal_edge].from_port;
        expected.power_links[witness.quasi_edge].from = before.power_links[witness.qc_edge].from;
        expected.power_links[witness.quasi_edge].from_port = PowerPort::Output;
        let certificates = infer_instant_resets(&graph);
        assert_eq!(certificates.len(), 1, "the original two-wire boundary must certify");
        for (wire, plan) in certificates {
            expected.wires[wire].execution.instant_reset = Some(plan);
        }
        apply(&mut graph, &TaskMonitor::default()).unwrap();
        assert!(graph == expected, "only the input sources and kernel certificate may change");
        graph.validate().unwrap();
        apply(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, expected);
    }

    #[test]
    fn declines_shared_resource_extra_qc_and_guarded_or_attenuated_inputs() {
        let original = graph();
        let witness = candidate(&original);
        for case in 0..8 {
            let mut changed = original.clone();
            match case {
                0 => {
                    changed.pistons.push(changed.pistons[witness.piston.index()]);
                }
                1 => {
                    changed.power_links.push(changed.power_links[witness.qc_edge]);
                }
                2 => {
                    changed.power_links[witness.main_edge].attenuation = 1;
                }
                3 => {
                    changed.power_links[witness.normal_edge].attenuation = 1;
                }
                4 => {
                    changed.power_links[witness.qc_edge].attenuation = 1;
                }
                5 => {
                    changed.power_links[witness.qc_edge].from_port = PowerPort::Strong(BlockFace::Top);
                }
                6 => {
                    let guard = crate::redpiler::program_graph::GuardId::new(changed.guards.len() as u32);
                    changed
                        .guards
                        .push(Guard::State(crate::redpiler::program_graph::StateTest::PistonExtended(witness.piston)));
                    changed.power_links[witness.quasi_edge].guard = guard;
                }
                7 => {
                    let actor = changed.pistons[witness.piston.index()];
                    let mut foreign = changed.resources[witness.resource.index()].clone();
                    foreign.contacts = vec![actor.base, actor.far].into_boxed_slice();
                    changed.resources.push(foreign);
                    let mut state = changed.initial_resources[witness.resource.index()];
                    state.at = actor.base;
                    changed.initial_resources.push(state);
                }
                _ => unreachable!(),
            }
            let before = changed.clone();
            apply(&mut changed, &TaskMonitor::default()).unwrap();
            assert_eq!(changed, before, "unsafe near-match {case} must remain general");
        }
    }

    #[test]
    fn declines_wrong_watch_typed_or_raw_orientation_and_selectable_sources() {
        let original = graph();
        let witness = candidate(&original);
        for case in 0..6 {
            let mut changed = original.clone();
            match case {
                0 => {
                    let CircuitNodeKind::Observer { watched, .. } = &mut changed.nodes[witness.observer.index()].kind else {
                        unreachable!()
                    };
                    *watched = changed.power_links[witness.main_edge].from;
                }
                1 => {
                    let CircuitNodeKind::Observer { facing, .. } = &mut changed.nodes[witness.observer.index()].kind else {
                        unreachable!()
                    };
                    *facing = BlockFace::Top;
                }
                2 => {
                    let node = &mut changed.nodes[witness.observer.index()];
                    let Block::Observer { mut observer } = Block::from_id(node.raw_state.unwrap()) else {
                        unreachable!()
                    };
                    observer.facing = BlockFace::Top.into();
                    node.raw_state = Some(Block::Observer { observer }.get_id());
                }
                3 => {
                    let site = changed.power_links[witness.qc_edge].from;
                    changed.sites[site.index()].source = SiteSource::Contact(changed.pistons[witness.piston.index()].near);
                }
                4 => {
                    let site = changed.power_links[witness.main_edge].from;
                    changed.sites[site.index()].source = SiteSource::Contact(changed.pistons[witness.piston.index()].near);
                }
                5 => {
                    let mut extra = changed.power_links[witness.qc_edge];
                    let mut alias = changed.sites[extra.from.index()];
                    alias.position = alias.position.offset(BlockFace::East);
                    extra.from = SiteId::new(changed.sites.len() as u32);
                    changed.sites.push(alias);
                    changed.power_links.push(extra);
                }
                _ => unreachable!(),
            }
            let before = changed.clone();
            apply(&mut changed, &TaskMonitor::default()).unwrap();
            assert_eq!(changed, before, "unclosed or wrongly oriented near-match {case}");
        }
    }

    #[test]
    fn cancellation_before_discovery_or_publication_does_not_mutate() {
        let mut graph = graph();
        let before = graph.clone();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(apply(&mut graph, &monitor).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn kernel_declines_changed_capture_boundaries_emitters_and_site_aliases() {
        let original = graph();
        let certificates = infer_instant_resets(&original);
        let [(wire, plan)] = certificates.as_slice() else {
            panic!("the original fixture must have one certificate")
        };
        let (wire, plan) = (*wire, *plan);
        let a = original.wires[wire].node;
        let b = plan.partner;
        let ar = original.wires[wire].turbo_route.unwrap();
        let east = original.wires[wire].canonical_neighbors[0];
        let actor = original.pistons[plan.piston.index()];
        for case in 0..11 {
            let mut changed = original.clone();
            match case {
                0 => {
                    // A constant emitter cannot masquerade as inert through electrical labels.
                    let block = Block::RedstoneBlock;
                    let profile = crate::redpiler::program_graph::ProfileId::new(changed.profiles.len() as u32);
                    changed.profiles.push(crate::redpiler::program_graph::Profile {
                        raw_state: block.get_id(),
                        registry_type: block.registry_id(),
                        solid: block.is_solid(),
                        transparent: block.is_transparent(),
                        cube: block.is_cube(),
                        electrical: ProfileElectrical::None,
                    });
                    changed.sites[east.index()].source = SiteSource::Constant(profile);
                }
                1 => {
                    // A contact alias at another position adds a second source to B.
                    let CircuitNodeKind::Wire(bi) = changed.nodes[b.index()].kind else {
                        unreachable!()
                    };
                    let site = changed.wires[bi].canonical_neighbors[0];
                    changed.sites[site.index()].source = SiteSource::Contact(actor.far);
                }
                2 => {
                    // Distinct same-position capture roots change visited-heading state.
                    let site = changed.wires[wire].canonical_neighbors[5];
                    changed.sites.push(changed.sites[site.index()]);
                }
                3 => {
                    // Repeater admission must be captured, never a live operand.
                    let mut rows = changed.routes[ar.index()].deliveries.to_vec();
                    rows.iter_mut()
                        .find(|row| row.receiver == Some(CallbackReceiver::Component(b)))
                        .unwrap()
                        .view = OperandView::Live;
                    changed.routes[ar.index()].deliveries = rows.into_boxed_slice();
                }
                4 => {
                    // A second incoming callback from a retained input is outside closure.
                    let mut extra = changed.routes[ar.index()]
                        .deliveries
                        .iter()
                        .find(|row| row.receiver == Some(CallbackReceiver::Component(b)))
                        .copied()
                        .unwrap();
                    extra.sequence = u32::MAX;
                    let route = changed
                        .nodes
                        .iter()
                        .find(|node| matches!(node.kind, CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)))
                        .unwrap()
                        .output_route
                        .unwrap();
                    let mut rows = changed.routes[route.index()].deliveries.to_vec();
                    rows.push(extra);
                    changed.routes[route.index()].deliveries = rows.into_boxed_slice();
                }
                5 => {
                    let ComponentState::Wire(state) = &mut changed.nodes[a.index()].state else {
                        unreachable!()
                    };
                    state.east = mchprs_blocks::blocks::RedstoneWireSide::Side;
                }
                6 => {
                    let air = changed.contacts[actor.far.index()]
                        .variants
                        .iter()
                        .find(|variant| Block::from_id(changed.profiles[variant.profile.index()].raw_state) == Block::Air)
                        .unwrap()
                        .profile;
                    changed.profiles[air.index()].solid = true;
                }
                7 => {
                    let mut link = changed.power_links[0];
                    link.to = PowerTarget::Conductor {
                        site: east,
                        include_dust: false,
                    };
                    changed.power_links.push(link);
                }
                8 => {
                    let mut rows = changed.routes[ar.index()].deliveries.to_vec();
                    let duplicate = *rows.iter().find(|row| row.receiver == Some(CallbackReceiver::Component(b))).unwrap();
                    rows.push(duplicate);
                    changed.routes[ar.index()].deliveries = rows.into_boxed_slice();
                }
                9 => {
                    changed.plans.dust_paths.push(crate::redpiler::program_graph::DustPathPlan {
                        input_node: plan.observer,
                        input_site: east,
                        input_face: BlockFace::North,
                        wires: vec![a, b].into_boxed_slice(),
                    });
                }
                10 => {
                    let mut rows = changed.routes[ar.index()].deliveries.to_vec();
                    rows.iter_mut()
                        .find(|row| row.receiver == Some(CallbackReceiver::PistonHead(plan.piston)))
                        .unwrap()
                        .guard = crate::redpiler::program_graph::GuardId::new(0);
                    changed.routes[ar.index()].deliveries = rows.into_boxed_slice();
                }
                _ => unreachable!(),
            }
            assert!(infer_instant_resets(&changed).is_empty(), "kernel near-match {case} must decline");
        }
    }

    #[test]
    fn two_wire_heading_grammar_declines_every_non_south_facing() {
        let original = graph();
        let witness = candidate(&original);
        for facing in [BlockFace::North, BlockFace::East, BlockFace::West, BlockFace::Top, BlockFace::Bottom] {
            let mut changed = original.clone();
            changed.pistons[witness.piston.index()].facing = facing;
            let index = DiscoveryIndex::build(&changed, &TaskMonitor::default()).unwrap();
            assert!(check_two_wire_kernel(&changed, &index, &witness).is_none(), "unproved heading {facing:?}");
        }
    }

    fn assert_cells_equal(left: &PlotWorld, right: &PlotWorld, bounds: (BlockPos, BlockPos)) {
        for y in bounds.0.y..=bounds.1.y {
            for z in bounds.0.z..=bounds.1.z {
                for x in bounds.0.x..=bounds.1.x {
                    let position = BlockPos::new(x, y, z);
                    assert_eq!(left.get_block(position), right.get_block(position), "cell {position:?}");
                    assert_eq!(
                        serde_json::to_value(left.get_block_entity(position)).unwrap(),
                        serde_json::to_value(right.get_block_entity(position)).unwrap(),
                        "entity {position:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn all_strengths_and_observer_power_disagreement_preserve_execution_and_reset() {
        let original = graph();
        let witness = candidate(&original);
        let input_site = original.power_links[witness.normal_edge].from;
        let SiteSource::Component(input_wire) = original.sites[input_site.index()].source else {
            unreachable!()
        };
        for strength in 0..=15 {
            for powered in [false, true] {
                let mut general = original.clone();
                let ComponentState::Electrical(observer) = &mut general.nodes[witness.observer.index()].state else {
                    unreachable!()
                };
                observer.powered = powered;
                observer.output_strength = strength;
                let ComponentState::Wire(wire) = &mut general.nodes[input_wire.index()].state else {
                    unreachable!()
                };
                wire.power = 15 - strength;
                let mut simplified = general.clone();
                apply(&mut simplified, &TaskMonitor::default()).unwrap();
                assert_ne!(general.power_links, simplified.power_links);
                let (program, state) = general.lower().unwrap();
                let mut baseline = GraphBackend::new(program, state, 0).unwrap();
                let (program, state) = simplified.lower().unwrap();
                let mut optimized = GraphBackend::new(program, state, 0).unwrap();
                let (mut baseline_world, bounds) = fixture();
                let (mut optimized_world, _) = fixture();
                for tick in 0..12 {
                    if [0, 1, 4, 5, 8].contains(&tick) {
                        baseline.use_block(ORIGIN + BlockPos::new(0, 2, 0)).unwrap();
                        optimized.use_block(ORIGIN + BlockPos::new(0, 2, 0)).unwrap();
                    }
                    baseline.tick().unwrap();
                    optimized.tick().unwrap();
                    baseline.flush(&mut baseline_world, false);
                    optimized.flush(&mut optimized_world, false);
                    assert_cells_equal(&baseline_world, &optimized_world, bounds);
                    assert_eq!(baseline.execution_counts().piston_actions, optimized.execution_counts().piston_actions);
                    assert_eq!(baseline.execution_counts().piston_samples, optimized.execution_counts().piston_samples);
                }
                baseline.reset(&mut baseline_world);
                optimized.reset(&mut optimized_world);
                assert_cells_equal(&baseline_world, &optimized_world, bounds);
                assert_eq!(
                    format!("{:?}", baseline_world.scheduler().iter_entries().collect::<Vec<_>>()),
                    format!("{:?}", optimized_world.scheduler().iter_entries().collect::<Vec<_>>())
                );
            }
        }
    }
}
