#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use mchprs_core::{
    plot::PlotWorld,
    plot::worldedit::load_schematic,
    redpiler::{CompileError, Compiler, CompilerOptions, PublicationMode, TaskMonitor},
    world::World,
};
use std::{fs::File, sync::Arc, time::Instant};
use sha2::{Digest, Sha256};

fn difference(reference: &mut PlotWorld, compiled: &mut PlotWorld, tick: u32, phase: &str) -> Option<serde_json::Value> {
    // Combine both change queues, extract their positions, then sort and deduplicate below.
    let mut positions: Vec<_> = reference.take_block_changes().into_iter().chain(compiled.take_block_changes()).map(|(pos, _)| pos).collect();
    positions.sort_unstable_by_key(|pos| (pos.y, pos.z, pos.x));
    positions.dedup();
    let blocks: Vec<_> = positions.into_iter().filter_map(|pos| {
        let expected = reference.get_block(pos);
        let actual = compiled.get_block(pos);
        (expected != actual).then_some((pos, expected, actual))
    }).collect();
    // Merge block entities from both worlds, flatten chunk-local positions, then compare each unique world position.
    let mut positions: Vec<_> = reference.get_chunks().iter().chain(compiled.get_chunks())
        .flat_map(|chunk| chunk.block_entities.keys().map(move |pos|
            BlockPos::new(chunk.x * 16 + pos.x, pos.y, chunk.z * 16 + pos.z))).collect();
    positions.sort_unstable_by_key(|pos| (pos.y, pos.z, pos.x));
    positions.dedup();
    let entities: Vec<_> = positions.into_iter().filter_map(|pos| {
        let block = reference.get_block(pos);
        if !block.is_command_block() && !matches!(block, Block::RedstoneComparator { .. }) { return None; }
        let expected = serde_json::to_value(reference.get_block_entity(pos)).unwrap();
        let actual = serde_json::to_value(compiled.get_block_entity(pos)).unwrap();
        (expected != actual).then_some((pos, expected, actual))
    }).collect();
    let expected: Vec<_> = reference.command_output().map(str::to_owned).collect();
    let actual: Vec<_> = compiled.command_output().map(str::to_owned).collect();
    if blocks.is_empty() && entities.is_empty() && expected == actual { return None; }
    // Check aligned messages first and fall back to the first unmatched index when lengths differ.
    let first_message = expected.iter().zip(&actual).position(|(a, b)| a != b)
        .or_else(|| (expected.len() != actual.len()).then_some(expected.len().min(actual.len())));
    let at = blocks.first().map(|row| row.0).or_else(|| entities.first().map(|row| row.0));
    // Expand the first mismatch to its six neighboring blocks for a compact local diagnostic.
    let neighbours: Vec<_> = at.into_iter().flat_map(|pos| BlockFace::values().map(move |face| pos.offset(face)))
        .map(|pos| serde_json::json!({"position":pos,"interpreter":format!("{:?}", reference.get_block(pos)),"compiled":format!("{:?}", compiled.get_block(pos))})).collect();
    // Cap block detail to keep the saved diagnostic readable; the counts above retain full totals.
    Some(serde_json::json!({"tick":tick,"phase":phase,"block_difference_count":blocks.len(),
        "blocks":blocks.iter().take(32).map(|(pos,a,b)| serde_json::json!({"position":pos,"interpreter":format!("{a:?}"),"compiled":format!("{b:?}")})).collect::<Vec<_>>(),
        "entity_difference_count":entities.len(),"entities":entities.into_iter().take(32).collect::<Vec<_>>(),
        "interpreter_message_count":expected.len(),"compiled_message_count":actual.len(),"first_message_index":first_message,
        "interpreter_message":first_message.and_then(|i|expected.get(i)),"compiled_message":first_message.and_then(|i|actual.get(i)),"neighbours":neighbours}))
}

fn compare_interpreter(world: &mut PlotWorld, compiler: &mut Compiler, cpu: cpus::Cpu, bounds: (BlockPos, BlockPos), ticks: u32) {
    let mut reference = cpus::load_cpu(cpu);
    eprintln!("interpreter_output_state={:?} compiled_output_state={:?}", reference.command_block_output_state(), world.command_block_output_state());
    reference.take_block_changes();
    world.take_block_changes();
    compiler.try_flush(world).unwrap();
    let mut found = difference(&mut reference, world, 0, "before start");
    if found.is_none() {
        cpus::click_cpu(&mut reference, cpu, cpu.start);
        compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
        compiler.try_flush(world).unwrap();
        found = difference(&mut reference, world, 0, "after start, before first tick");
    }
    for tick in 1..=ticks {
        if found.is_some() { break; }
        reference.tick_interpreted();
        compiler.tick().unwrap_or_else(|error| panic!("compiled execution tick {tick}: {error}"));
        compiler.try_flush(world).unwrap();
        found = difference(&mut reference, world, tick, "after tick");
        if tick % 256 == 0 { eprintln!("interpreter/compiled compared tick {tick}"); }
    }
    if let Some(mut report) = found {
        compiler.try_reset(world, bounds).unwrap();
        let expected: Vec<_> = reference.scheduler().iter_entries().collect();
        let actual: Vec<_> = world.scheduler().iter_entries().collect();
        report["pending_equal"] = serde_json::json!(expected == actual);
        report["interpreter_pending"] = serde_json::json!(expected);
        report["compiled_pending"] = serde_json::json!(actual);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zcore-recovery-trials/pm1-first-difference.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("first_difference={report}");
        println!("wrote {}", path.display());
    } else {
        println!("no observable difference through {ticks} ticks");
    }
}

fn main() {
    let interpreter = std::env::args().any(|arg| arg == "--interpreter");
    let llvm = !interpreter && !std::env::args().any(|arg| arg == "--rust");
    let backend = if interpreter { "interpreter" } else if llvm { "llvm" } else { "rust" };
    let compare = std::env::args().any(|arg| arg == "--compare-interpreter");
    let cpu = if std::env::args().any(|arg| arg == "--pm1") {
        cpus::Cpu {
            schematic: "PM1_SORT_FIXED.schem",
            sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
            ..cpus::CPUS[0]
        }
    } else {
        cpus::CPUS[3]
    };
    let mut world = cpus::load_cpu(cpu);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let ticks = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    let started = Instant::now();
    let result = if interpreter { Ok(()) } else { compiler.compile(
        &world,
        bounds,
        CompilerOptions {
            llvm,
            publication: compare.then_some(PublicationMode::All),
            ..Default::default()
        },
        ticks,
        Arc::new(TaskMonitor::default()),
    ) };
    if let Err(CompileError::Unsupported(error)) = &result
        && let Some(pos) = error.position
    {
        eprintln!("rejected position={pos:?} block={:?} entity={:?}", world.get_block(pos), world.get_block_entity(pos));
    }
    println!(
        "fixture={} schematic={} backend={} setup_ms={:.3} active={} result={:?}",
        cpu.name,
        cpu.schematic,
        backend,
        started.elapsed().as_secs_f64() * 1000.0,
        compiler.is_active(),
        result.map_err(|error| error.to_string())
    );
    if compare {
        assert!(!interpreter && compiler.is_active(), "comparison requires a compiled backend");
        compare_interpreter(&mut world, &mut compiler, cpu, bounds, 12_151);
        return;
    }
    if let Some(ticks) = std::env::args().find_map(|arg| arg.strip_prefix("--ticks=").map(|ticks| ticks.parse::<u32>().unwrap())) {
        assert!(interpreter || compiler.is_active(), "fixture did not compile");
        assert!(ticks > 0);
        if interpreter { cpus::click_cpu(&mut world, cpu, cpu.start); }
        else { compiler.try_on_use_block(cpu.origin + cpu.start).unwrap(); }
        let execution = Instant::now();
        for tick in 1..=ticks {
            if interpreter { world.tick_interpreted(); }
            else { compiler.tick().unwrap_or_else(|error| panic!("execution tick {tick}: {error}")); }
        }
        let elapsed = execution.elapsed();
        println!("backend={backend} ticks={ticks} kernel_ms={:.3} tps={:.3}", elapsed.as_secs_f64() * 1000.0, ticks as f64 / elapsed.as_secs_f64());
        if !interpreter {
            assert!(compiler.is_active());
            compiler.try_reset(&mut world, bounds).unwrap();
        }
        let messages: Vec<_> = world.command_output().map(|message| message.to_owned()).collect();
        if cpu.name == "pm1_sort" { assert!(!messages.is_empty(), "PM1 start produced no output"); }
        println!("messages={} messages_sha256={:x} halt_observed={} last_message={:?}",
            messages.len(), Sha256::digest(serde_json::to_vec(&messages).unwrap()),
            messages.iter().any(|message| message.contains("shut")), messages.last());
        println!("checkpoint={}", serde_json::to_string(&cpus::checkpoint(&world, ticks, &[])).unwrap());
    }
}
