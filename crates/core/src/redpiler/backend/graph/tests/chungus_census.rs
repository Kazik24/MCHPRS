//! Bounded CHUNGUS inventory and required-preparation probe; no runtime optimization.
use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::components::identify_block;
use crate::world::storage::Chunk;
use mchprs_save_data::plot_data::PlotData;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Instant;

pub(super) const START_BUTTON: BlockPos = BlockPos::new(187, 99, 115);

pub(super) fn load_chungus_world() -> PlotWorld {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/chungus_mandelbrot_plot");
    let data = PlotData::load_from_file(path, false).unwrap();
    let chunks = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| Chunk::load(index as i32 / PLOT_WIDTH, index as i32 % PLOT_WIDTH, chunk))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect())
}

#[test]
#[ignore = "research: loads CHUNGUS and probes normal bounded full graph preparation"]
fn chungus_fixed_topology_inventory_and_preparation_census() {
    let started = Instant::now();
    let world = load_chungus_world();
    let load_seconds = started.elapsed().as_secs_f64();
    let bounds = world.get_corners();
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let mut blocks = BTreeMap::<String, usize>::new();
    let mut actors = BTreeMap::<String, usize>::new();
    let mut powered_actors = BTreeMap::<String, usize>::new();
    let mut wires = Vec::new();
    let mut movers = Vec::new();
    crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |position| {
        let block = world.get_block(position);
        if block == Block::Air {
            return;
        }
        *blocks.entry(block.get_name().to_string()).or_default() += 1;
        if let Some((kind, state)) = identify_block(block, position, &world) {
            let name = match kind {
                NodeType::Repeater { .. } => "repeater",
                NodeType::Torch => "torch",
                NodeType::Comparator { .. } => "comparator",
                NodeType::Observer { .. } => "observer",
                NodeType::Lamp => "lamp",
                NodeType::CopperBulb => "copper_bulb",
                NodeType::Hopper => "hopper",
                NodeType::Button => "button",
                NodeType::Lever => "lever",
                NodeType::PressurePlate => "pressure_plate",
                NodeType::Trapdoor => "trapdoor",
                NodeType::Wire => "wire",
                NodeType::Constant => "constant",
                NodeType::CommandBlock { .. } => "command_block",
                NodeType::NoteBlock { .. } => "note_block",
            };
            *actors.entry(name.into()).or_default() += 1;
            if state.output_strength > 0 {
                *powered_actors.entry(name.into()).or_default() += 1;
            }
        }
        if matches!(block, Block::RedstoneWire { .. }) {
            wires.push(position);
        }
        if matches!(block, Block::Piston { .. } | Block::PistonHead { .. } | Block::MovingPiston { .. }) {
            movers.push(position);
        }
    });
    let index = wires
        .iter()
        .copied()
        .enumerate()
        .map(|(index, position)| (position, index))
        .collect::<FxHashMap<_, _>>();
    let mut parents = (0..wires.len()).collect::<Vec<_>>();
    fn root(parents: &mut [usize], mut node: usize) -> usize {
        while parents[node] != node {
            parents[node] = parents[parents[node]];
            node = parents[node];
        }
        node
    }
    let mut edges = 0;
    for (to, &position) in wires.iter().enumerate() {
        let up_solid = world.get_block(position.offset(BlockFace::Top)).is_solid();
        let mut include = |source: BlockPos| {
            if let Some(&from) = index.get(&source) {
                edges += 1;
                let a = root(&mut parents, to);
                let b = root(&mut parents, from);
                parents[a] = b;
            }
        };
        for face in BlockFace::values() {
            let adjacent = position.offset(face);
            let block = world.get_block(adjacent);
            include(adjacent);
            if face.is_horizontal() {
                if !up_solid && !block.is_transparent() {
                    include(adjacent.offset(BlockFace::Top));
                }
                if !block.is_solid() {
                    include(adjacent.offset(BlockFace::Bottom));
                }
            }
        }
    }
    let mut component_sizes = BTreeMap::<usize, usize>::new();
    for wire in 0..wires.len() {
        *component_sizes.entry(root(&mut parents, wire)).or_default() += 1;
    }
    let monitor = TaskMonitor::default();
    let limits = AnalysisLimits::for_budget(8);
    let analysis_started = Instant::now();
    let admission = crate::redpiler::analysis::analyze(&world, bounds, &ticks, &monitor, limits).unwrap();
    let analysis_seconds = analysis_started.elapsed().as_secs_f64();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/chungus_mandelbrot_plot");
    let inventory = json!({"fixture_sha256":format!("{:x}",Sha256::digest(std::fs::read(fixture).unwrap())),
        "bounds":format!("{bounds:?}"),"start_button":format!("{START_BUTTON:?}"),"start_block":format!("{:?}",world.get_block(START_BUTTON)),
        "load_seconds":load_seconds,"analysis_seconds":analysis_seconds,"admission":admission,
        "blocks":blocks,"actors":actors,"initial_nonzero_output_actors":powered_actors,"mover_positions":movers,
        "wiring":{"wires":wires.len(),"directed_dust_power_dependencies":edges,"weak_components":component_sizes.len(),
            "largest_component":component_sizes.values().max(),"interpretation":"initial fixed-world dust dependencies, not synthesized source channels"},
        "entry_ticks":ticks.len(),"protocol":"existing bench presses start once, no prewarm; full historical benchmark 12,411,975 ticks; no frozen reference supplied"});
    println!("CHUNGUS INVENTORY {inventory}");
    let root_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::write(
        root_path.join("target/chungus-fixed-topology-inventory.json"),
        serde_json::to_vec_pretty(&inventory).unwrap(),
    )
    .unwrap();
    assert!(!wires.is_empty());
    let preparation_started = Instant::now();
    // Use the existing hard table-memory bound; do not raise it to obtain a successful census.
    let prepared = passes::rich::prepare(&world, bounds, ticks, &monitor, limits);
    let result = match prepared {
        Ok(prepared) => json!({"ok":true,"nodes":prepared.graph.nodes.len(),"guards":prepared.graph.guards.len(),
            "contacts":prepared.graph.contacts.len(),"pistons":prepared.graph.pistons.len(),"resources":prepared.graph.resources.len(),
            "wires":prepared.graph.wires.len(),"power_links":prepared.graph.power_links.len(),"wire_relations":prepared.graph.wire_relations.len(),
            "callback_routes":prepared.graph.routes.len(),"callback_links":prepared.graph.links.len(),
            "state_guards":prepared.graph.guards.iter().filter(|guard|matches!(guard,Guard::State(_))).count()}),
        Err(error) => json!({"ok":false,"error":error.to_string(),"debug_error":format!("{error:?}")}),
    };
    let statistics = monitor.graph_statistics();
    let tables = statistics.preparation_tables.iter().map(|table|json!({"stage":table.stage,"table":table.table,"entries":table.entries,"capacity":table.capacity,"estimated_bytes":table.estimated_bytes})).collect::<Vec<_>>();
    let report = json!({"inventory":inventory,"preparation":result,"preparation_seconds":preparation_started.elapsed().as_secs_f64(),
        "hard_table_limit_bytes":crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES,"tables":tables,"statistics":format!("{statistics:?}"),
        "semantic_claim":"discovery only; preparation or tape-cache declines do not establish impossibility of fixed graph synthesis"});
    println!("CHUNGUS PREPARATION {report}");
    std::fs::write(
        root_path.join("target/chungus-fixed-topology-census.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

fn lowered_inventory(graph: &GraphBackend) -> serde_json::Value {
    let plans = graph.program.wires();
    let facts = plans.iter().filter_map(|wire| wire.execution.query_facts).collect::<Vec<_>>();
    json!({"nodes":graph.program.nodes().len(),"wires":plans.len(),"sites":graph.program.sites().len(),
        "guards":graph.program.guards().len(),"routes":graph.program.routes().len(),
        "route_deliveries":graph.program.routes().iter().map(|route|route.deliveries.len()).sum::<usize>(),
        "power_links":graph.program.power_links().len(),"query_facts":facts.len(),
        "fully_known_24_slots":facts.iter().filter(|facts|facts.known==(1<<24)-1).count(),
        "compiled_transfers":facts.iter().filter(|facts|facts.transfer.is_some()).count(),
        "compiled_raw_sides":facts.iter().filter(|facts|facts.raw_sides.is_some()).count(),
        "transfers_without_runtime_geometry":facts.iter().filter(|facts|facts.transfer.is_some_and(|transfer|transfer.seed.conditional==0&&transfer.turbo.conditional==0)).count(),
        "isolated":facts.iter().filter(|facts|facts.isolated).count(),"hop_roots":plans.iter().filter(|wire|wire.execution.hop_root.is_some()).count(),
        "contacts":graph.program.contacts().len(),"pistons":graph.program.pistons().len(),"resources":graph.program.resources().len(),
        "compiled_callback_wires":plans.iter().filter(|wire|wire.execution.callback_selection.is_some_and(|plan|plan.known==WireCallbackSelection::ALL)).count(),
        "compiled_callback_slots":plans.iter().filter_map(|wire|wire.execution.callback_selection).map(|plan|plan.known.count_ones() as usize).sum::<usize>(),
        "callback_index_entries":graph.program.turbo_callback_indices().len(),
        "classic_nets":graph.program.classic().nets.len(),"classic_wires":graph.program.classic().wires.len(),
        "classic_guarded_wires":graph.program.classic().guarded_wires.len(),
        "classic_outside_main_terms":graph.program.classic().outside.len(),
        "classic_guarded_receiver_inputs":graph.program.classic().inputs.iter().filter(|input|input.guarded).count(),
        "classic_active_nets":graph.state.classic_active.iter().filter(|active|**active).count(),
        "classic_deoptimized_nets":graph.state.classic_active.iter().filter(|active|!**active).count()})
}

#[test]
#[ignore = "research: bounded CHUNGUS interpreter/Graph differential after required preparation"]
fn chungus_bounded_interpreter_graph_differential() {
    use mchprs_blocks::block_entities::BlockEntity;
    use mchprs_blocks::blocks::ButtonFace;
    use std::time::Duration;
    let mut native = load_chungus_world();
    let bounds = native.get_corners();
    let ticks = std::env::var("MCHPRS_CHUNGUS_TICKS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(64);
    assert!(ticks > 0);
    let classic_ablation = std::env::var_os("MCHPRS_CHUNGUS_CLASSIC").is_some();
    let weighted = std::env::var_os("MCHPRS_CHUNGUS_UNCHECKED_WEIGHTED").is_some();
    assert!(
        !(classic_ablation && weighted),
        "select classic ablation or unchecked weighted, never both"
    );
    let mut graphs = Vec::new();
    let mut monitors = Vec::new();
    let mut compile_seconds = Vec::new();
    let mut names = vec!["normal_graph_o2"];
    for enable_classic in [false, true].into_iter().take(if classic_ablation { 2 } else { 1 }) {
        let monitor = Arc::new(TaskMonitor::default());
        monitor.set_budget_multiplier(8);
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut pipeline = passes::default_graph_pipeline(&options);
        pipeline.entries.retain(|entry| {
            !matches!(&entry.pass, passes::Pass::Build(builder)
                if builder.name() == "compile-fixed-classic" || !enable_classic && builder.name() == "compile-classic-redstone")
        });
        let started = Instant::now();
        graphs.push(
            GraphBackend::compile_with_pipeline(
                &native,
                bounds,
                native.scheduler().iter_entries().collect(),
                &options,
                &pipeline,
                monitor.clone(),
            )
            .unwrap(),
        );
        compile_seconds.push(started.elapsed().as_secs_f64());
        println!(
            "CHUNGUS COMPILED local_classic_pass={enable_classic} seconds={} inventory={} fixed_tables={:?}",
            compile_seconds.last().unwrap(),
            lowered_inventory(graphs.last().unwrap()),
            monitor
                .graph_statistics()
                .preparation_tables
                .iter()
                .filter(|table| table.stage == "fixed signals")
                .collect::<Vec<_>>()
        );
        monitors.push(monitor);
    }
    let mut unchecked = None;
    if weighted {
        let started = Instant::now();
        let mut candidate = GraphBackend::new_unactivated(graphs[0].program.clone(), graphs[0].state.clone(), 0).unwrap();
        unchecked = Some(super::unchecked_classic_anpu::UncheckedProgram::attach_all(&mut candidate));
        compile_seconds.push(started.elapsed().as_secs_f64());
        graphs.push(candidate);
        monitors.push(Arc::new(TaskMonitor::default()));
        names.push("unchecked_weighted_all_fixed");
    } else if classic_ablation {
        names.push("local_classic_pass_control");
    }
    let mut projected = (0..graphs.len()).map(|_| load_chungus_world()).collect::<Vec<_>>();
    let positions = graphs[0].program.nodes().iter().filter_map(|node| node.position).collect::<Vec<_>>();
    let comparator_positions = graphs[0]
        .program
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. })))
        .filter_map(|node| node.position)
        .collect::<Vec<_>>();
    let lamps = graphs[0]
        .program
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
        .filter_map(|node| node.position)
        .collect::<Vec<_>>();
    let mut execution = vec![Duration::ZERO; graphs.len() + 1];
    let mut activation = vec![Duration::ZERO; graphs.len() + 1];
    let mut projection = vec![Duration::ZERO; graphs.len()];
    let mut mismatches = vec![[0usize; 3]; graphs.len()];
    let mut first = vec![None; graphs.len()];
    let mut first_fifo = vec![None; graphs.len()];
    let mut first_actor = vec![None; graphs.len()];
    let mut first_analog = vec![None; graphs.len()];
    let mut first_pending = vec![None; graphs.len()];
    let mut first_component = vec![None; graphs.len()];
    let mut lamp_mismatches = vec![0usize; graphs.len()];
    let mut first_lamp = vec![None; graphs.len()];
    let mut candidate_lamp_frames = vec![0usize; graphs.len()];
    let mut previous_candidate_lamps = vec![Vec::new(); graphs.len()];
    let mut candidate_lamp_traces = (0..graphs.len()).map(|_| Sha256::new()).collect::<Vec<_>>();
    let mut internal_mismatches = vec![[0usize; 3]; graphs.len()];
    let mut first_internal = vec![None; graphs.len()];
    let mut lamp_frames = 0;
    let mut previous_lamps = Vec::new();
    let mut lamp_trace = Sha256::new();
    let start = Instant::now();
    let Block::StoneButton { mut button } = native.get_block(START_BUTTON) else {
        panic!("CHUNGUS start button changed")
    };
    assert!(!button.powered);
    button.powered = true;
    native.set_block(START_BUTTON, Block::StoneButton { button });
    native.schedule_tick(START_BUTTON, 10, TickPriority::Normal);
    crate::redstone::update_surrounding_blocks(&mut native, START_BUTTON);
    let support = match button.face {
        ButtonFace::Floor => BlockFace::Bottom,
        ButtonFace::Ceiling => BlockFace::Top,
        ButtonFace::Wall => button.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(&mut native, START_BUTTON.offset(support));
    activation[0] = start.elapsed();
    for (index, graph) in graphs.iter_mut().enumerate() {
        let started = Instant::now();
        graph.use_block(START_BUTTON).unwrap();
        activation[index + 1] = started.elapsed();
    }
    for tick in 0..=ticks {
        if tick > 0 {
            let mut order = (0..execution.len()).collect::<Vec<_>>();
            if tick % 2 == 1 {
                order.reverse();
            }
            for mode in order {
                let started = Instant::now();
                if mode == 0 {
                    native.tick_interpreted();
                } else {
                    graphs[mode - 1].tick().unwrap();
                }
                execution[mode] += started.elapsed();
            }
        }
        let frame = lamps
            .iter()
            .map(|&position| u8::from(matches!(native.get_block(position), Block::RedstoneLamp { lit: true })))
            .collect::<Vec<_>>();
        for (index, graph) in graphs.iter().enumerate() {
            let shadow = &mut projected[index];
            let started = Instant::now();
            shadow.clear_scheduled_ticks();
            graph.project_future(shadow);
            projection[index] += started.elapsed();
            if let Some(&position) = positions
                .iter()
                .find(|&&position| native.get_block_raw(position) != shadow.get_block_raw(position))
            {
                mismatches[index][0] += 1;
                first_actor[index].get_or_insert_with(||json!({"tick":tick,"position":position,"native":format!("{:?}",native.get_block(position)),"graph":format!("{:?}",shadow.get_block(position))}));
                first[index].get_or_insert_with(||json!({"tick":tick,"kind":"actor_block","position":position,"native":format!("{:?}",native.get_block(position)),"graph":format!("{:?}",shadow.get_block(position))}));
            }
            let strength = |world: &PlotWorld, position| match world.get_block_entity(position) {
                Some(BlockEntity::Comparator { output_strength }) => Some(*output_strength),
                _ => None,
            };
            if let Some(&position) = comparator_positions
                .iter()
                .find(|&&position| strength(&native, position) != strength(shadow, position))
            {
                mismatches[index][1] += 1;
                first_analog[index].get_or_insert_with(
                    || json!({"tick":tick,"position":position,"native":strength(&native,position),"graph":strength(shadow,position)}),
                );
                first[index].get_or_insert_with(||json!({"tick":tick,"kind":"comparator_entity","position":position,"native":strength(&native,position),"graph":strength(shadow,position)}));
            }
            let a = native.scheduler().iter_entries().collect::<Vec<_>>();
            let b = shadow.scheduler().iter_entries().collect::<Vec<_>>();
            if a != b || native.piston_state().logical_tick != shadow.piston_state().logical_tick {
                mismatches[index][2] += 1;
                let ordinal = a
                    .iter()
                    .zip(&b)
                    .position(|(left, right)| left != right)
                    .or_else(|| (a.len() != b.len()).then_some(a.len().min(b.len())));
                let witness = json!({"tick":tick,"kind":"ordered_future","ordinal":ordinal,"native_queue_len":a.len(),"graph_queue_len":b.len(),
                    "native_entry":ordinal.map(|ordinal|format!("{:?}",a.get(ordinal))),"graph_entry":ordinal.map(|ordinal|format!("{:?}",b.get(ordinal))),
                    "native_clock":native.piston_state().logical_tick,"graph_clock":shadow.piston_state().logical_tick});
                first_fifo[index].get_or_insert_with(|| witness.clone());
                first[index].get_or_insert(witness);
            }
            let candidate_frame = lamps
                .iter()
                .map(|&position| u8::from(matches!(shadow.get_block(position), Block::RedstoneLamp { lit: true })))
                .collect::<Vec<_>>();
            if let Some(pixel) = frame.iter().zip(&candidate_frame).position(|(left, right)| left != right) {
                lamp_mismatches[index] += 1;
                first_lamp[index].get_or_insert_with(
                    || json!({"tick":tick,"lamp_index":pixel,"position":lamps[pixel],"native":frame[pixel],"graph":candidate_frame[pixel]}),
                );
            }
            if candidate_frame != previous_candidate_lamps[index] {
                candidate_lamp_frames[index] += 1;
                previous_candidate_lamps[index] = candidate_frame.clone();
            }
            candidate_lamp_traces[index].update(&candidate_frame);
            if index > 0 {
                let normal = &graphs[0];
                let different =
                    (0..normal.state.components.len()).find(|&node| match (&normal.state.components[node], &graph.state.components[node]) {
                        (ComponentState::Wire(_), ComponentState::Wire(_)) => {
                            normal.wire_state(NodeId::new(node as u32)) != graph.wire_state(NodeId::new(node as u32))
                        }
                        (left, right) => left != right,
                    });
                if let Some(node) = different {
                    internal_mismatches[index][0] += 1;
                    first_component[index].get_or_insert_with(||json!({"tick":tick,"node":node,"position":normal.program.nodes()[node].position,"normal_storage":format!("{:?}",normal.state.components[node]),"candidate_storage":format!("{:?}",graph.state.components[node]),"normal_projected":format!("{:?}",normal.node_block(NodeId::new(node as u32))),"candidate_projected":format!("{:?}",graph.node_block(NodeId::new(node as u32)))}));
                    first_internal[index].get_or_insert_with(
                        || json!({"tick":tick,"kind":"reconstructed_component_state","node":node,"position":normal.program.nodes()[node].position}),
                    );
                }
                if normal.future.pending != graph.future.pending {
                    internal_mismatches[index][1] += 1;
                    first_pending[index].get_or_insert_with(|| {
                        json!({"tick":tick,"normal_count":normal.future.pending.len(),"candidate_count":graph.future.pending.len(),
                        "first_missing":normal.future.pending.keys().find(|key|!graph.future.pending.contains_key(*key)).map(|key|format!("{key:?}")),
                        "first_extra":graph.future.pending.keys().find(|key|!normal.future.pending.contains_key(*key)).map(|key|format!("{key:?}"))})
                    });
                    first_internal[index].get_or_insert_with(|| json!({"tick":tick,"kind":"pending_membership"}));
                }
                if format!("{:?}", normal.future.clock) != format!("{:?}", graph.future.clock) {
                    internal_mismatches[index][2] += 1;
                    first_internal[index].get_or_insert_with(|| json!({"tick":tick,"kind":"graph_clock"}));
                }
            }
        }
        if frame != previous_lamps {
            lamp_frames += 1;
            previous_lamps = frame.clone();
        }
        lamp_trace.update(&frame);
    }
    let modes=graphs.iter().enumerate().map(|(index,graph)|{
        let statistics=monitors[index].graph_statistics();
        let tables=statistics.preparation_tables.iter().filter(|table|table.stage=="classic redstone"||table.stage=="lowering input"||table.stage=="fixed signals"||table.stage=="compiled wire callbacks").map(|table|json!({"stage":table.stage,"table":table.table,"entries":table.entries,"estimated_bytes":table.estimated_bytes})).collect::<Vec<_>>();
        json!({"mode":names[index],"compile_seconds":compile_seconds[index],"plans":lowered_inventory(graph),"execution_seconds":execution[index+1].as_secs_f64(),
            "activation_seconds":activation[index+1].as_secs_f64(),"projection_seconds":projection[index].as_secs_f64(),"mismatch_observations":mismatches[index],
            "first_mismatch":first[index],"first_actor_mismatch":first_actor[index],"first_analog_mismatch":first_analog[index],
            "first_pending_mismatch":first_pending[index],"first_reconstructed_component_mismatch":first_component[index],"first_fifo_mismatch":first_fifo[index],"lamp_mismatch_observations":lamp_mismatches[index],
            "first_lamp_mismatch":first_lamp[index],"lamp_changed_frames_including_initial":candidate_lamp_frames[index],
            "lamp_trace_sha256":format!("{:x}",candidate_lamp_traces[index].clone().finalize()),
            "vs_normal_internal_mismatch_observations":internal_mismatches[index],"first_internal_mismatch":first_internal[index],"graph_work":format!("{:?}",graph.execution_counts()),"tables":tables,"compile_statistics":format!("{statistics:?}")})
    }).collect::<Vec<_>>();
    let report = json!({"experiment":"CHUNGUS interpreter/normal Graph/optional candidate differential; weighted candidate deliberately unsafe, not certified",
        "ticks":ticks,"profile":if cfg!(debug_assertions){"debug"}else{"release"},
        "protocol":"saved full plot, no prewarm, one original start-button press; alternating interpreter/Graph tick order; validation/projection outside execution timers",
        "interpreter_execution_seconds":execution[0].as_secs_f64(),"interpreter_activation_seconds":activation[0].as_secs_f64(),
        "checked_actor_positions":positions.len(),"checked_comparator_entities":comparator_positions.len(),"lamps":lamps.len(),"lamp_changed_frames_including_initial":lamp_frames,
        "lamp_trace_sha256":format!("{:x}",lamp_trace.finalize()),"modes":modes,"weighted_work":unchecked.as_ref().map(|program|program.counts()),"weighted_coverage":unchecked.as_ref().map(|program|program.coverage())});
    println!("CHUNGUS DIFFERENTIAL {report}");
    let suffix = if classic_ablation {
        "-classic"
    } else if weighted {
        "-unchecked-weighted"
    } else {
        ""
    };
    let root_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::write(
        root_path.join(format!("target/chungus-differential-{ticks}{suffix}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert_eq!(mismatches[0], [0, 0, 0], "normal baseline mismatch; report preserved");
    if classic_ablation {
        assert_eq!(mismatches[1], [0, 0, 0], "certified candidate mismatch; report preserved");
        assert_eq!(
            internal_mismatches[1],
            [0, 0, 0],
            "certified internal future/state mismatch; report preserved"
        );
        assert_eq!(lamp_mismatches[1], 0);
    }
    assert!(
        graphs[0].execution_counts().callbacks > 0 && graphs[0].execution_counts().wire_walks > 0,
        "CPU must do real work"
    );
}

#[test]
#[ignore = "research: CHUNGUS electrical opportunity census, no reference replay or execution"]
fn chungus_binary_classic_candidate_census() {
    use crate::redpiler::passes::{GraphPass, PassContext};
    use std::collections::BTreeSet;
    let started = Instant::now();
    let world = load_chungus_world();
    let monitor = TaskMonitor::default();
    let mut graph = passes::rich::prepare(
        &world,
        world.get_corners(),
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap()
    .graph;
    let options = crate::redpiler::CompilerOptions::default();
    let entry = EntryState {
        logical_tick: 0,
        pending_ticks: vec![],
    };
    passes::simplify_guards::GuardAndUpdateReduction
        .run(&mut graph, &PassContext::new(&monitor, &options, &entry))
        .unwrap();
    let analysis = ClassicAnalysis::new(&graph);
    let regions = passes::compile_classic_redstone::census_regions(&graph);
    let mut cardinalities = BTreeMap::<String, [usize; 2]>::new();
    let mut source_kinds = BTreeMap::<String, usize>::new();
    let mut declines = BTreeMap::<String, [usize; 2]>::new();
    let mut existing_declines = BTreeMap::<String, [usize; 2]>::new();
    let mut candidates = [0usize; 2];
    let mut one_to_four = [0usize; 2];
    for members in &regions {
        let owned = members.iter().copied().collect::<BTreeSet<_>>();
        let mut sources = BTreeSet::new();
        let mut reasons = BTreeSet::<String>::new();
        for &wire in members {
            match analysis.census_expression(wire, InputPort::Main) {
                Ok((constant, leaves)) => {
                    if constant != 0 {
                        reasons.insert("nonzero fixed electrical contribution".into());
                    }
                    sources.extend(leaves.into_iter().filter(|source| !owned.contains(source)));
                }
                Err(reason) => {
                    reasons.insert(format!("electrical query: {reason}"));
                }
            }
        }
        let bucket = if sources.len() > 4 { ">4".into() } else { sources.len().to_string() };
        let row = cardinalities.entry(bucket).or_default();
        row[0] += 1;
        row[1] += members.len();
        for &source in &sources {
            let node = &graph.nodes[source.index()];
            *source_kinds.entry(format!("{:?}", node.kind)).or_default() += 1;
            if !matches!(
                node.kind,
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::Torch | NodeType::Repeater { .. })
            ) {
                reasons.insert("unsupported source kind (including analog/comparator)".into());
            }
            match &node.state {
                ComponentState::Electrical(state) => {
                    if state.output_strength != u8::from(state.powered) * 15 {
                        reasons.insert("source output is not consistent binary 0/15".into());
                    }
                    if matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) {
                        if state.repeater_locked {
                            reasons.insert("saved source repeater locked".into());
                        }
                        match analysis.census_expression(source, InputPort::Side) {
                            Ok((0, leaves)) if leaves.is_empty() => {}
                            Ok(_) => {
                                reasons.insert("source repeater structural side input".into());
                            }
                            Err(reason) => {
                                reasons.insert(format!("source side query: {reason}"));
                            }
                        }
                    }
                }
                _ => {
                    reasons.insert("source is not electrical state".into());
                }
            }
        }
        if members.len() > CLASSIC_MAX_WIRES {
            reasons.insert("region wire bound".into());
        }
        if !(1..=4).contains(&sources.len()) {
            reasons.insert("source cardinality outside 1..4".into());
        }
        if reasons.is_empty() {
            one_to_four[0] += 1;
            one_to_four[1] += members.len();
            if sources.len() >= 2 {
                candidates[0] += 1;
                candidates[1] += members.len();
            }
        }
        for reason in reasons {
            let row = declines.entry(reason).or_default();
            row[0] += 1;
            row[1] += members.len();
        }
        if let Err(reason) = analysis.infer(members) {
            let row = existing_declines.entry(reason.into()).or_default();
            row[0] += 1;
            row[1] += members.len();
        }
    }
    let report = json!({"regions":regions.len(),"wires":graph.wires.len(),"seconds":started.elapsed().as_secs_f64(),
        "histogram_value_order":["regions","wires"],"source_cardinalities":cardinalities,"source_region_occurrences_by_kind":source_kinds,
        "binary_1_to_4_electrical_candidates":one_to_four,"binary_2_to_4_extension_opportunity":candidates,
        "overlapping_electrical_declines":declines,"existing_first_family_first_decline":existing_declines,
        "claim":"Opportunity only: external nonwire source union over exact fixed Main queries in each current electrical region after guard reduction. No boundary legality, row representability, runtime binary invariant, source route prefixes, receiver ownership or 256 transition proof is certified. Decline reasons overlap; source kind counts count each source once per region. No CPU state enumeration."});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chungus-binary-classic-candidate-census.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("CHUNGUS BINARY CANDIDATES {report}");
    assert_eq!(cardinalities.values().map(|row| row[1]).sum::<usize>(), graph.wires.len());
}
