//! Opt-in characterization of larger author fixtures. Never an execution fallback.
use super::*;
use crate::redstone::piston::trace;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

mod notification_routes;

#[path = "../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[test]
#[ignore = "explicit symbolic admission diagnosis for larger devices"]
fn divider_and_counter_report_symbolic_kernel_admission() {
    use crate::redpiler::analysis::instant_regions::compiler::aot;
    use crate::redpiler::{CompilerOptions, passes};

    for name in ["fpu_divider", "counter_basic"] {
        let descriptor = if name == "fpu_divider" {
            manifest(name)
        } else {
            let bytes = std::fs::read(root().join("test_data/instant-pistons-io/fixtures/counter_basic.json")).unwrap();
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["loader_offset"] = value["coordinates"]["loader_offset"].clone();
            value["origin"] = value["coordinates"]["origin"].clone();
            value
        };
        let trigger = if name == "fpu_divider" {
            descriptor["protocol"]["trigger"].clone()
        } else {
            descriptor["ports"]["inputs"]["trigger"].clone()
        };
        let (world, bounds) = load(&descriptor);
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
        let options = CompilerOptions {
            optimization_level: Some(3),
            stable_instant_inputs: true,
            ..Default::default()
        };
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &passes::default_graph_pipeline(&options),
            &monitor,
        )
        .unwrap();
        let graph = &prepared.graph;
        let position = origin(&descriptor) + local_pos(&trigger) - BASE;
        let trigger =
            crate::redpiler::program_graph::NodeId::new(graph.nodes.iter().position(|node| node.position == Some(position)).unwrap() as u32);
        let candidates = aot::activation_candidates(graph);
        assert!(candidates.contains(&trigger), "{name}: an indirect control must still be a candidate");
        eprintln!(
            "{name}: {} stable regions, {} candidates, trigger {position:?} eligible={}",
            graph.plans.instant.regions.iter().filter(|region| region.stable_inputs).count(),
            candidates.len(),
            candidates.contains(&trigger)
        );
        {
            use crate::redpiler::analysis::instant_regions::compiler::{Compiler as SymbolicCompiler, entry::Entry};
            let mut symbolic = SymbolicCompiler::new(
                graph,
                mchprs_redpiler_opt::Limits {
                    max_work: 256 * 1024 * 1024,
                    ..Default::default()
                },
            )
            .unwrap();
            let entry = Entry::derive(&mut symbolic, trigger, name != "counter_basic").unwrap();
            eprintln!(
                "{name}: ready inputs={}, constant actor poses={}/{}, constant resource locations={}/{}, decisions={}, work={}",
                entry.inputs.len(),
                symbolic.state.actors.iter().filter(|actor| actor.extended.constant().is_some()).count(),
                symbolic.state.actors.len(),
                symbolic
                    .state
                    .resources
                    .iter()
                    .filter(|resource| resource.contacts.iter().all(|(_, at)| at.constant().is_some()))
                    .count(),
                symbolic.state.resources.len(),
                symbolic.logic.node_count(),
                symbolic.logic.work()
            );
        }
        let result = if name == "fpu_divider" {
            aot::compile_for_entry(
                graph,
                trigger,
                false,
                crate::redpiler::analysis::instant_regions::compiler::entry::ShapeEntry::Captured,
                &monitor,
            )
        } else {
            aot::compile_candidate(graph, trigger, true, &monitor)
        };
        match result {
            Ok(kernel) => {
                assert!(kernel.validate(graph));
                eprintln!(
                    "{name}: {} inputs, prefix {}, period {}",
                    kernel.inputs.len(),
                    kernel.prefix,
                    kernel.period
                );
            }
            Err(reason) => eprintln!("{name}: kernel declined: {reason}"),
        }
    }
}

fn origin(manifest: &Value) -> BlockPos {
    manifest.get("origin").map_or(BASE, |p| local_pos(p) - BASE)
}

fn manifest(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(root().join("test_data/piston-research/fixtures").join(format!("{name}.json"))).unwrap()).unwrap()
}

fn load(manifest: &Value) -> (PlotWorld, (BlockPos, BlockPos)) {
    let bytes = std::fs::read(root().join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(manifest["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(manifest["dimensions"], json!([clipboard.size_x, clipboard.size_y, clipboard.size_z]));
    assert_eq!(
        manifest["loader_offset"],
        json!([clipboard.offset_x, clipboard.offset_y, clipboard.offset_z])
    );
    let mut world = empty();
    let first = origin(manifest);
    let last = first + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1);
    let (world_first, world_last) = world.get_corners();
    assert_eq!(first.max(world_first), first, "fixture must fit the plot");
    assert_eq!(last.min(world_last), last, "fixture must fit the plot");
    paste_clipboard(
        &mut world,
        &clipboard,
        first + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    (world, (first, last))
}

#[test]
#[ignore = "bounded static census of the large Divider's composed instant regions"]
fn divider_stable_regions_report_complete_and_incomplete_groups() {
    use crate::redpiler::{CompilerOptions, passes};
    let (world, bounds) = load(&manifest("fpu_divider"));
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let recognized: BTreeSet<_> = passes::utils::instant_mechanisms::discover_mechanisms(graph, &monitor)
        .unwrap()
        .iter()
        .map(|m| m.piston)
        .collect();
    let mut owned = BTreeSet::new();
    for region in &graph.plans.instant.regions {
        for actor in &region.actors {
            assert!(owned.insert(*actor), "an actor has only one owner");
            if region.stable_inputs {
                assert!(recognized.contains(actor));
            }
        }
    }
    let mut composed_sizes: Vec<_> = graph
        .plans
        .instant
        .regions
        .iter()
        .filter(|r| r.stable_inputs)
        .map(|r| r.actors.len())
        .collect();
    composed_sizes.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "Divider actors={} recognized={} owned={} regions={} proxies={} composed_sizes={composed_sizes:?}",
        graph.pistons.len(),
        recognized.len(),
        owned.len(),
        graph.plans.instant.regions.len(),
        graph.plans.instant.proxies.len()
    );
    graph.validate().unwrap();
}

fn block_state(world: &PlotWorld, pos: BlockPos, first: BlockPos) -> Value {
    let block = world.get_block(pos);
    json!({"pos": pos - first, "name": block.get_name(), "properties": block.properties(),
    "entity": world.get_block_entity(pos), "power": match block {
        Block::Piston { piston } => Some(crate::redstone::piston::should_piston_extend(world, piston.facing, pos)),
        _ => None,
    }})
}

fn observations(world: &PlotWorld, manifest: &Value) -> Value {
    let ports: BTreeMap<_, _> = manifest["observations"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, positions)| {
            (
                name,
                positions
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| block_state(world, local_pos(p) - BASE + origin(manifest), origin(manifest)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    json!({"ports": ports, "tick": world.piston_state().logical_tick,
        "phase": world.piston_state().phase, "pending_ticks": world.scheduler().iter_entries().collect::<Vec<_>>(),
        "piston_events": world.piston_state().events, "motions": world.piston_state().motions})
}

fn action(world: &mut PlotWorld, manifest: &Value, operation: &Value) {
    let pos = local_pos(&operation["pos"]) - BASE + origin(manifest);
    match operation["op"].as_str().unwrap() {
        "lever" => lever_action(world, pos, operation["powered"].as_bool().unwrap()),
        "button" => cpus::click(world, pos - BlockPos::new(8, 8, 8)),
        "notify" => crate::redstone::update_surrounding_blocks(world, pos),
        "sample" => {
            let block = world.get_block(pos);
            crate::redstone::update(block, world, pos, None);
        }
        _ => panic!("unsupported research action {operation}"),
    }
}

#[test]
fn bubblesort_import_preserves_full_memory_and_button_protocol() {
    let fixture = manifest("cpu_bubblesort");
    let (mut world, bounds) = load(&fixture);
    assert_eq!(bounds, (BlockPos::new(1, 8, 1), BlockPos::new(225, 49, 254)));
    for row in 0..16 {
        let mut word = 0u16;
        for bit in 0..12 {
            let pos = bounds.0 + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row);
            let Block::Piston { piston } = world.get_block(pos) else {
                panic!("missing saved RAM cell at {pos}");
            };
            word |= u16::from(!piston.extended) << bit;
        }
        assert_eq!(word, 15 - row as u16);
    }
    action(&mut world, &fixture, &json!({"op": "button", "pos": [166,7,158]}));
    let pos = bounds.0 + BlockPos::new(166, 7, 158);
    assert!(matches!(world.get_block(pos), Block::StoneButton { button } if button.powered));
    assert!(world.pending_tick_at(pos));
    advance(&mut world, 20);
    assert!(matches!(world.get_block(pos), Block::StoneButton { button } if !button.powered));
}

#[test]
fn cpu_bubblesort_head_matches_piston_and_missing_variant_is_rejected_transactionally() {
    let fixture = manifest("cpu_bubblesort");
    let (mut world, _) = load(&fixture);
    let actor = BlockPos::new(180, 14, 166);
    let head = actor.offset(BlockFace::East);
    let Block::Piston { piston } = world.get_block(actor) else {
        panic!("expected piston at {actor:?}");
    };
    assert!(piston.extended);
    assert!(matches!(
        world.get_block(head),
        Block::PistonHead { head: actual }
            if actual.facing == piston.facing
                && actual.sticky == piston.sticky
                && !actual.short
    ));
    assert!(world.set_block(head, Block::Air));
    let before = cpus::checkpoint(&world, 0, &[]);
    let mut compiler = crate::redpiler::Compiler::default();
    let error = compiler
        .compile(
            &world,
            (actor, head.offset(BlockFace::East)),
            crate::redpiler::CompilerOptions::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("extended piston"), "{message}");
    assert!(message.contains(&format!("{actor:?}")), "{message}");
    assert!(message.contains(&format!("{head:?}")), "{message}");
    assert!(message.contains("missing its head"), "{message}");
    assert!(!compiler.is_active());
    assert!(compiler.stats().is_none());
    assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
}

#[test]
#[ignore = "explicit Graph-versus-interpreter timing episode"]
fn cpu_bubblesort_graph_and_interpreter_match_short_sign_free_episode() {
    compare_bubblesort_execution(100, 0);
}

#[test]
#[ignore = "full O4 CPU sort and interpreter continuation; explicit expensive validity check"]
fn cpu_bubblesort_o4_completes_sort_and_preserves_interpreter_continuation() {
    compare_bubblesort_execution(u64::from(cpus::CPUS[2].active_ticks), 4);
}

fn compare_bubblesort_execution(active_ticks: u64, optimization_level: usize) {
    let cpu = cpus::CPUS[2];
    let mut fixture = manifest("cpu_bubblesort");
    fixture["origin"] = json!([cpu.origin.x, cpu.origin.y, cpu.origin.z]);
    let benchmark_sign = cpu.origin + BlockPos::new(186, 2, 184);
    let mut checkpoints: BTreeSet<_> = cpus::CHECKPOINTS
        .into_iter()
        .map(u64::from)
        .filter(|&tick| tick <= active_ticks)
        .collect();
    checkpoints.insert(active_ticks);

    let mut interpreter = cpus::load_cpu(cpu);
    assert!(matches!(interpreter.get_block(benchmark_sign), Block::WallSign { .. }));
    assert!(interpreter.set_block(benchmark_sign, Block::Air));
    let expected_initial = cpus::checkpoint(&interpreter, 0, &[]);
    let mut interpreter_ram = BTreeMap::from([(0, cpu_words(&interpreter, &fixture))]);
    cpus::click_cpu(&mut interpreter, cpu, cpu.start);
    let interpreter_started = Instant::now();
    for tick in 1..=active_ticks {
        interpreter.tick_interpreted();
        if checkpoints.contains(&tick) {
            interpreter_ram.insert(tick, cpu_words(&interpreter, &fixture));
        }
    }
    let interpreter_runtime = interpreter_started.elapsed();
    if active_ticks == u64::from(cpu.active_ticks) {
        assert_eq!(
            cpu_words(&interpreter, &fixture)["ram_y37"],
            json!((0..16).collect::<Vec<u16>>()),
            "completed interpreter sort"
        );
    }
    for tick in 1..=32 {
        interpreter.tick_interpreted();
        interpreter_ram.insert(active_ticks + tick, cpu_words(&interpreter, &fixture));
    }
    drop(interpreter);

    let mut graph_world = cpus::load_cpu(cpu);
    assert!(matches!(graph_world.get_block(benchmark_sign), Block::WallSign { .. }));
    assert!(graph_world.set_block(benchmark_sign, Block::Air));
    assert_eq!(
        cpus::checkpoint(&graph_world, 0, &[]),
        expected_initial,
        "Graph and interpreter inputs must differ only by the same sign removal"
    );
    let dimensions = fixture["dimensions"].as_array().unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin
            + BlockPos::new(
                dimensions[0].as_i64().unwrap() as i32 - 1,
                dimensions[1].as_i64().unwrap() as i32 - 1,
                dimensions[2].as_i64().unwrap() as i32 - 1,
            ),
    );
    let monitor = std::sync::Arc::new(crate::redpiler::TaskMonitor::default());
    let ticks = graph_world.scheduler().iter_entries().collect();
    let mut graph = crate::redpiler::backend::graph::GraphBackend::compile_with_options(
        &graph_world,
        bounds,
        ticks,
        &crate::redpiler::CompilerOptions {
            optimization_level: Some(optimization_level),
            ..Default::default()
        },
        monitor.clone(),
    )
    .unwrap_or_else(|error| {
        panic!(
            "Graph preparation failed: {error}; preparation tables: {:?}",
            monitor.graph_statistics().preparation_tables
        )
    });
    let tables = monitor.graph_statistics().preparation_tables;
    for stage in ["geometry", "electrical", "callbacks", "lowering input", "lowering"] {
        let estimate: usize = tables
            .iter()
            .filter(|table| table.stage == stage)
            .map(|table| table.estimated_bytes)
            .sum();
        eprintln!("Graph {stage} tables: {:.1} MiB", estimate as f64 / 1_048_576.0);
    }
    let mut largest_callbacks: Vec<_> = tables.iter().filter(|table| table.stage == "callbacks").collect();
    largest_callbacks.sort_by_key(|table| std::cmp::Reverse(table.estimated_bytes));
    eprintln!(
        "Largest callback tables: {}",
        largest_callbacks
            .iter()
            .take(3)
            .map(|table| format!("{} {:.1} MiB", table.table, table.estimated_bytes as f64 / 1_048_576.0))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut graph_ram = BTreeMap::from([(0, cpu_words(&graph_world, &fixture))]);
    let button = cpu.origin + cpu.start;
    graph.use_block(button).unwrap();
    graph.set_test_recording(false);
    let graph_started = Instant::now();
    for tick in 1..=active_ticks {
        graph
            .tick_with_world(&mut graph_world)
            .unwrap_or_else(|error| panic!("Graph failed at active tick {tick}: {error}"));
        if tick % 1_000 == 0 {
            eprintln!("Graph BubbleSort active tick {tick}");
        }
        if checkpoints.contains(&tick) {
            graph.flush(&mut graph_world, false);
            graph_ram.insert(tick, cpu_words(&graph_world, &fixture));
        }
    }
    let graph_runtime = graph_started.elapsed();
    if active_ticks == u64::from(cpu.active_ticks) {
        assert_eq!(
            cpu_words(&graph_world, &fixture)["ram_y37"],
            json!((0..16).collect::<Vec<u16>>()),
            "completed Graph sort"
        );
    }
    graph.reset(&mut graph_world);
    for tick in 1..=32 {
        graph_world.tick_interpreted();
        graph_ram.insert(active_ticks + tick, cpu_words(&graph_world, &fixture));
    }
    assert_eq!(graph_ram, interpreter_ram, "checkpoint RAM parity");
    assert_eq!(
        graph.execution_counts().instant_transitions,
        0,
        "this CPU validity run exercises the general fallback"
    );
    eprintln!("Graph work counts: {:?}", graph.execution_counts());
    eprintln!(
        "CPU sign-free replay at {benchmark_sign:?}: Graph O{optimization_level} {} ticks in {:.2}s; interpreter {} ticks in {:.2}s",
        active_ticks,
        graph_runtime.as_secs_f64(),
        active_ticks,
        interpreter_runtime.as_secs_f64()
    );
}

#[test]
fn cpu_bubblesort_piston_base_sign_passes_geometry_but_external_callbacks_still_reject() {
    let fixture = manifest("cpu_bubblesort");
    let (world, _) = load(&fixture);
    let sign = BlockPos::new(187, 10, 185);
    let piston = BlockPos::new(188, 10, 185);
    let head = piston.offset(BlockFace::East);
    assert!(matches!(
        world.get_block(sign),
        Block::WallSign {
            facing: BlockDirection::West,
            ..
        }
    ));
    assert!(matches!(world.get_block(piston), Block::Piston { piston } if piston.facing == BlockFacing::East && piston.extended));
    assert!(matches!(world.get_block(head), Block::PistonHead { .. }));

    let before = cpus::checkpoint(&world, 0, &[]);
    let bounds = (sign, head.offset(BlockFace::East));
    assert!(crate::interaction::is_valid_position(world.get_block(sign), &world, sign));
    prepare_geometry(&world, bounds, &[], &TaskMonitor::default(), AnalysisLimits::default()).unwrap();
    let mut compiler = crate::redpiler::Compiler::default();
    let error = compiler
        .compile(
            &world,
            bounds,
            crate::redpiler::CompilerOptions::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("uncompiled receiver"), "{message}");
    assert!(message.contains("BlockPos { x: 188, y: 10, z: 185 }"), "{message}");
    assert!(message.contains("BlockPos { x: 191, y: 10, z: 185 }"), "{message}");
    assert!(!compiler.is_active());
    assert!(compiler.stats().is_none());
    assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
}

fn memory_pos(address: u8, bit: u8) -> BlockPos {
    BASE + BlockPos::new(14 - 2 * i32::from(address), 9, 24 - 2 * i32::from(bit))
}

fn memory_words(world: &PlotWorld) -> [u8; 8] {
    std::array::from_fn(|address| {
        (0..8).fold(0, |word, bit| {
            let pos = memory_pos(address as u8, bit);
            let Block::Piston { piston } = world.get_block(pos) else {
                panic!("memory is moving at {pos:?}; sample a settled boundary")
            };
            word | (u8::from(!piston.extended) << bit)
        })
    })
}

fn output_word(world: &PlotWorld) -> u8 {
    (0..8).fold(0, |word, bit| {
        let pos = BASE + BlockPos::new(15, 1, 24 - 2 * bit);
        let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
            panic!("missing output at {pos:?}")
        };
        word | (u8::from(repeater.powered) << bit)
    })
}

fn advance(world: &mut PlotWorld, ticks: usize) {
    for _ in 0..ticks {
        world.tick_interpreted();
    }
}

fn data(world: &mut PlotWorld, value: u8) {
    for bit in 0..8 {
        lever_action(world, BASE + BlockPos::new(14, 14, 25 - 2 * bit), value & (1 << bit) != 0);
    }
}

fn address(world: &mut PlotWorld, value: u8, write: bool) {
    for bit in 0..3 {
        lever_action(
            world,
            BASE + BlockPos::new(if write { 19 } else { 20 }, if write { 12 } else { 7 }, 3 + 2 * bit),
            value & (1 << bit) != 0,
        );
    }
}

const WRITE_ENABLE: BlockPos = BlockPos::new(19, 12, 1);
const READ_ENABLE: BlockPos = BlockPos::new(18, 7, 1);

#[test]
fn fpu_strict_import_preserves_the_948_analog_reference_values() {
    let (world, bounds) = load(&manifest("fpu_legal"));
    let mut strengths = BTreeMap::<u8, usize>::new();
    let mut positions = BTreeSet::new();
    crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
        let block = world.get_block(pos);
        if matches!(block, Block::Furnace { .. }) {
            assert!(positions.insert((pos.x, pos.y, pos.z)), "duplicate furnace visit at {pos:?}");
            let strength = crate::redstone::comparator::get_override(block, &world, pos);
            *strengths.entry(strength).or_default() += 1;
        }
    });
    // Independent raw NBT contains 948 unique furnaces, including 637 strength-1 inventories.
    assert_eq!(positions.len(), 948);
    assert_eq!(
        strengths,
        BTreeMap::from([(1, 637), (2, 1), (3, 1), (4, 128), (6, 64), (11, 1), (12, 16), (15, 100)])
    );
}

#[test]
fn rilax_memory_preserves_prepared_data_until_neighbor_movement_samples_it() {
    let fixture = manifest("rilax_memory_bank_bud");
    for selected in [0, 2, 3, 4, 5, 6, 7] {
        let (mut world, _) = load(&fixture);
        address(&mut world, selected, true);
        advance(&mut world, 24);
        let before = memory_words(&world);
        data(&mut world, 0xa5);
        advance(&mut world, 24);
        assert_eq!(memory_words(&world), before, "preparation at address {selected}");
        assert!(!crate::redstone::piston::should_piston_extend(
            &world,
            BlockFacing::Down,
            memory_pos(selected, 0)
        ));
        let operations = trace::capture(|| {
            lever_action(&mut world, BASE + WRITE_ENABLE, true);
            advance(&mut world, 24);
        });
        let mut expected = before;
        expected[usize::from(selected)] = 0xa5;
        assert_eq!(memory_words(&world), expected);
        assert_eq!(output_word(&world), 0, "writing alone does not expose the output");
        assert!(
            operations
                .iter()
                .any(|entry| matches!(entry.operation, trace::Operation::Applied(event) if event.pos == memory_pos(selected, 0)))
        );
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
        address(&mut world, selected, false);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + READ_ENABLE, true);
        advance(&mut world, 9);
        assert_eq!(output_word(&world), 0);
        advance(&mut world, 1);
        assert_eq!(output_word(&world), 0xa5, "ten-tick read at address {selected}");
        advance(&mut world, 14);
        lever_action(&mut world, BASE + READ_ENABLE, false);
        advance(&mut world, 11);
        assert_eq!(output_word(&world), 0xa5);
        advance(&mut world, 1);
        assert_eq!(output_word(&world), 0, "read release after twelve ticks");
        assert_eq!(memory_words(&world), expected, "read must preserve stored data");
    }
}

#[test]
fn rilax_seven_update_pistons_sample_all_eight_bits_on_both_movement_edges() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    lever_action(&mut world, BASE + WRITE_ENABLE, true);
    advance(&mut world, 24);
    data(&mut world, 255);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world)[0], 0, "holding the update level does not continuously sample");
    let operations = trace::capture(|| {
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
    });
    let mut samplers = 0;
    let mut cells = FxHashSet::default();
    for entry in operations {
        if let trace::Operation::Applied(event) = entry.operation {
            if !event.sticky && event.action == PistonAction::Retract {
                samplers += 1;
            }
            if event.sticky && event.pos.y == BASE.y + 9 {
                cells.insert(event.pos);
            }
        }
    }
    assert_eq!(samplers, 7);
    assert_eq!(cells.len(), 8);
    assert_eq!(memory_words(&world)[0], 255, "the falling enable causes another sampling movement");
}

#[test]
fn rilax_decoder_omission_is_a_reproducible_destructive_address_change() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    assert_eq!(world.get_block(BASE + BlockPos::new(13, 11, 2)), Block::Air);
    assert_eq!(memory_words(&world)[1], 3);
    address(&mut world, 1, true);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world)[1], 0);
    let Block::Lever { lever } = world.get_block(BASE + WRITE_ENABLE) else {
        unreachable!()
    };
    assert!(!lever.powered, "the missing torch admits updates while enable is off");
}

#[test]
fn rilax_derived_missing_torch_probe_restores_address_one_enable_gating() {
    use mchprs_blocks::BlockDirection;
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    // A one-block diagnostic in a fresh test world, never a replacement schematic.
    let pos = BASE + BlockPos::new(13, 11, 2);
    world.set_block(
        pos,
        Block::RedstoneWallTorch {
            lit: true,
            facing: BlockDirection::South,
        },
    );
    crate::redstone::update_surrounding_blocks(&mut world, pos);
    let before = memory_words(&world);
    address(&mut world, 1, true);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world), before);
    data(&mut world, 0xa5);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world), before);
    lever_action(&mut world, BASE + WRITE_ENABLE, true);
    advance(&mut world, 24);
    let mut expected = before;
    expected[1] = 0xa5;
    assert_eq!(memory_words(&world), expected);
    lever_action(&mut world, BASE + WRITE_ENABLE, false);
    advance(&mut world, 24);
    address(&mut world, 1, false);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + READ_ENABLE, true);
    advance(&mut world, 24);
    assert_eq!(output_word(&world), 0xa5);
}

#[test]
fn rilax_held_read_does_not_resample_a_later_write() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    lever_action(&mut world, BASE + READ_ENABLE, true);
    advance(&mut world, 24);
    data(&mut world, 255);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + WRITE_ENABLE, true);
    advance(&mut world, 24);
    assert_eq!(memory_words(&world)[0], 255);
    assert_eq!(output_word(&world), 0, "held read retains its sampled output");
    lever_action(&mut world, BASE + WRITE_ENABLE, false);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + READ_ENABLE, false);
    advance(&mut world, 24);
    lever_action(&mut world, BASE + READ_ENABLE, true);
    advance(&mut world, 24);
    assert_eq!(output_word(&world), 255, "a new read movement exposes the new word");
}

#[test]
fn rilax_short_enable_pulses_are_filtered_by_the_decoder() {
    for width in [1, 2, 4, 6, 8, 12] {
        let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
        data(&mut world, 255);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + WRITE_ENABLE, true);
        advance(&mut world, width);
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
        assert_eq!(memory_words(&world)[0], if width <= 2 { 0 } else { 255 }, "enable width {width}");
    }
}

#[test]
fn rilax_unchanged_data_keeps_memory_but_still_samples_on_each_update() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    advance(&mut world, 24);
    let before = memory_words(&world);
    let operations = trace::capture(|| {
        for _ in 0..2 {
            lever_action(&mut world, BASE + WRITE_ENABLE, true);
            advance(&mut world, 24);
            lever_action(&mut world, BASE + WRITE_ENABLE, false);
            advance(&mut world, 24);
        }
    });
    let samples = operations
        .iter()
        .filter(|entry| matches!(entry.operation, trace::Operation::Sample {pos, ..} if (0..8).any(|bit| pos == memory_pos(0, bit))))
        .count();
    let writes = operations
        .iter()
        .filter(|entry| matches!(entry.operation, trace::Operation::Applied(event) if event.sticky && event.pos.y == BASE.y + 9))
        .count();
    assert_eq!(memory_words(&world), before);
    assert!(samples >= 32, "both movements of both pulses sample all eight cells");
    assert_eq!(writes, 0);
}

#[test]
fn rilax_memory_writes_are_reusable_with_independent_read_address() {
    let (mut world, _) = load(&manifest("rilax_memory_bank_bud"));
    let mut expected = memory_words(&world);
    for (selected, value) in [(0, 0xa5), (2, 0x5a), (7, 0xff), (0, 0), (2, 0x81)] {
        address(&mut world, selected, true);
        advance(&mut world, 24);
        data(&mut world, value);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + WRITE_ENABLE, true);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + WRITE_ENABLE, false);
        advance(&mut world, 24);
        expected[usize::from(selected)] = value;
        assert_eq!(memory_words(&world), expected);
    }
    for selected in [7, 2, 0] {
        address(&mut world, selected, false);
        advance(&mut world, 24);
        lever_action(&mut world, BASE + READ_ENABLE, true);
        advance(&mut world, 24);
        assert_eq!(output_word(&world), expected[usize::from(selected)]);
        lever_action(&mut world, BASE + READ_ENABLE, false);
        advance(&mut world, 24);
    }
    assert_eq!(memory_words(&world), expected);
}

#[test]
fn rilax_pico_and_game_steps_agree_at_completed_tick_boundaries() {
    let fixture = manifest("rilax_memory_bank_bud");
    let (mut game, _) = load(&fixture);
    let (mut pico, _) = load(&fixture);
    for world in [&mut game, &mut pico] {
        data(world, 255);
        advance(world, 24);
        lever_action(world, BASE + WRITE_ENABLE, true);
    }
    for tick in 25..=40 {
        game.tick_interpreted();
        let mut completed = false;
        for _ in 0..20_000 {
            pico.picotick_advance(1);
            if pico.piston_state().logical_tick == tick && pico.piston_state().phase == AdvancePhase::BetweenTicks {
                completed = true;
                break;
            }
        }
        assert!(completed, "bounded pico stepping at tick {tick}");
        assert_eq!(observations(&pico, &fixture), observations(&game, &fixture), "aligned tick {tick}");
    }
}

fn cpu_words(world: &PlotWorld, fixture: &Value) -> Value {
    let words: BTreeMap<_, _> = fixture["observations"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| name.starts_with("ram_") || name.starts_with("register"))
        .map(|(name, positions)| {
            let words: Vec<Option<u16>> = positions
                .as_array()
                .unwrap()
                .chunks(12)
                .map(|bits| {
                    bits.iter()
                        .enumerate()
                        .try_fold(0, |word, (bit, p)| match world.get_block(local_pos(p) - BASE + origin(fixture)) {
                            Block::Piston { piston } => Some(word | (u16::from(!piston.extended) << bit)),
                            _ => None,
                        })
                })
                .collect();
            (name, words)
        })
        .collect();
    json!(words)
}

fn diagnose(world: &PlotWorld) -> Value {
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut reports = Vec::new();
    let before = cpus::checkpoint(world, world.piston_state().logical_tick as u32, &[]);
    for budget in [1, 2, 4, 8] {
        let start = Instant::now();
        let result = analyze(
            world,
            world.get_corners(),
            &ticks,
            &Default::default(),
            AnalysisLimits::for_budget(budget),
        );
        reports.push(match result {
            Ok(report) => json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "report": report}),
            Err(error) => json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "error": error.to_string()}),
        });
    }
    let mut attempts = Vec::new();
    for budget in [1, 2, 4, 8] {
        let mut compiler = Compiler::default();
        let start = Instant::now();
        let result = compiler.compile(
            world,
            world.get_corners(),
            CompilerOptions {
                budget_multiplier: budget,
                optimization_level: None,
                io_only: true,
                ..Default::default()
            },
            ticks.clone(),
            Default::default(),
        );
        assert_eq!(
            cpus::checkpoint(world, world.piston_state().logical_tick as u32, &[]),
            before,
            "read-only admission must preserve physical state and work"
        );
        attempts.push(
            json!({"budget": budget, "elapsed_ms": start.elapsed().as_millis(), "active": compiler.is_active(),
            "error": result.err().map(|error| error.to_string())}),
        );
    }
    json!({"analysis": reports, "compile": attempts})
}

#[test]
#[ignore = "bounded CPU protocol capture; explicit new output file required"]
fn capture_bubblesort_execution() {
    let output = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture = manifest("cpu_bubblesort");
    let selected = std::env::var("MCHPRS_PISTON_RESEARCH_CASE").unwrap();
    let case = fixture["cases"].as_array().unwrap().iter().find(|c| c["id"] == selected).unwrap();
    let (mut world, _) = load(&fixture);
    let watched: rustc_hash::FxHashSet<_> = fixture["observations"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|p| p.as_array().unwrap())
        .map(|p| local_pos(p) - BASE + origin(&fixture))
        .collect();
    let mut samples = vec![json!({"label":"import", "tick":0, "words":cpu_words(&world, &fixture)})];
    for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        let count = step["advance"].as_u64().or_else(|| step["wait_quiet"].as_u64());
        if let Some(count) = count {
            assert!(count <= 50_000, "bounded CPU research run");
            let mut quiet = 0;
            for elapsed in 1..=count {
                let entries = trace::capture(|| world.tick_interpreted());
                let total = entries.len();
                let sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(&entries).unwrap()));
                let entries: Vec<_> = entries
                    .into_iter()
                    .filter(|entry| {
                        watched.contains(&match entry.operation {
                            trace::Operation::Sample { pos, .. } => pos,
                            trace::Operation::Applied(event) => event.pos,
                        })
                    })
                    .collect();
                let pending = world.scheduler().iter_entries().count();
                let motions = world.piston_state().motions.len();
                let events = world.piston_state().events.len();
                quiet = if pending == 0 && motions == 0 && events == 0 { quiet + 1 } else { 0 };
                let words = cpu_words(&world, &fixture);
                samples.push(json!({"label":"tick", "step":index, "tick":world.piston_state().logical_tick,
                    "words":words, "pending":pending, "motions":motions, "events":events,
                    "operation_count":total, "operation_sha256":sha256, "operations":entries}));
                if elapsed % 500 == 0 {
                    println!("{selected}: step {index}, {elapsed} ticks, pending {pending}, motions {motions}");
                }
                if step.get("wait_quiet").is_some() && quiet >= 20 {
                    break;
                }
            }
            samples.push(json!({"label":"boundary", "step":index, "tick":world.piston_state().logical_tick,
                "quiet_ticks":quiet, "state":observations(&world, &fixture), "checkpoint":cpus::checkpoint(&world, world.piston_state().logical_tick as u32, &[])}));
            println!(
                "{selected}: boundary {index}, tick {}, quiet {quiet}, RAM {}",
                world.piston_state().logical_tick,
                cpu_words(&world, &fixture)["ram_y37"]
            );
        } else if step.get("diagnose").is_some() {
            samples.push(json!({"label":"diagnostics", "step":index, "tick":world.piston_state().logical_tick,
                "words":cpu_words(&world, &fixture), "diagnostics":diagnose(&world)}));
        } else {
            let entries = trace::capture(|| action(&mut world, &fixture, step));
            samples.push(json!({"label":"action", "step":index, "tick":world.piston_state().logical_tick,
                "action":step, "words":cpu_words(&world, &fixture), "operations":entries}));
        }
    }
    if let Some(expected) = case.get("expected_final_ram_y37") {
        assert_eq!(&cpu_words(&world, &fixture)["ram_y37"], expected, "completed CPU sort");
        assert_eq!(world.scheduler().iter_entries().count(), 0);
        assert!(world.piston_state().motions.is_empty());
        assert!(world.piston_state().events.is_empty());
    }
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(output).unwrap();
    serde_json::to_writer(std::io::BufWriter::new(file), &json!({"schema_version":1, "engine":"MCHPRS interpreter",
        "fixture":fixture["fixture"], "fixture_sha256":fixture["sha256"], "origin":fixture["origin"], "case":case,
        "coordinates":"words selection-local; operations absolute; null word means moving or missing base",
        "trace_scope":"full ordered operations SHA-256 each tick; explicit ordered entries only at manifest observation positions", "samples":samples})).unwrap();
}

#[test]
#[ignore = "explicit new-file research capture; MCHPRS_PISTON_RESEARCH_OUTPUT required"]
fn capture_author_research_fixtures() {
    let output = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let fixture = std::env::var("MCHPRS_PISTON_RESEARCH_FIXTURE").unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/piston-research/fixtures").join(format!("{fixture}.json"))).unwrap()).unwrap();
    assert!(!Path::new(&output).exists(), "capture never replaces a frozen artifact");
    let selected = std::env::var("MCHPRS_PISTON_RESEARCH_CASE").ok();
    let (initial, _) = load(&manifest);
    let diagnostics = if selected.is_none() || selected.as_deref() == Some("diagnostics") {
        diagnose(&initial)
    } else {
        Value::Null
    };
    println!("{fixture}: diagnostics {}", diagnostics["compile"]);
    let mut episodes = Vec::new();
    for case in manifest["cases"].as_array().unwrap() {
        if selected.as_deref().is_some_and(|id| case["id"] != id) {
            continue;
        }
        let (mut world, _) = load(&manifest);
        let mut samples = vec![json!({"label": "import", "state": observations(&world, &manifest), "operations": []})];
        for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            if let Some(ticks) = step["advance"].as_u64() {
                assert!(ticks <= 512, "bounded research episode");
                for _ in 0..ticks {
                    let operations = trace::capture(|| world.tick_interpreted());
                    samples.push(json!({"label": "tick", "step": index, "state": observations(&world, &manifest), "operations": operations}));
                }
            } else if step.get("diagnose").is_some() {
                samples.push(json!({"label": "diagnostics", "step": index, "state": observations(&world, &manifest), "operations": [], "diagnostics": diagnose(&world)}));
            } else {
                let operations = trace::capture(|| action(&mut world, &manifest, step));
                samples.push(
                    json!({"label": "action", "step": index, "action": step, "state": observations(&world, &manifest), "operations": operations}),
                );
            }
        }
        println!("{fixture}: {} captured {} samples", case["id"], samples.len());
        episodes.push(json!({"case": case, "samples": samples}));
    }
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(output).unwrap();
    serde_json::to_writer(
        std::io::BufWriter::new(file),
        &json!({"schema_version": 1, "engine": "MCHPRS interpreter",
        "fixture_sha256": manifest["sha256"], "fixture": manifest["fixture"],
        "origin": [origin(&manifest).x, origin(&manifest).y, origin(&manifest).z], "setup": "strict paste, no notifications or implicit settling",
        "coordinates": "observations selection-local; operation entries and queued work absolute",
        "diagnostics": diagnostics, "episodes": episodes}),
    )
    .unwrap();
}
