//! Ignored end-to-end checks for O6 CPU piston-power query plans on real CPU fixtures.
use super::instant_cpu_tests::cpus;
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::redstone::piston::trace::{Entry as PistonTraceEntry, Operation as PistonTraceOperation};
use crate::world::World;
use mchprs_blocks::{BlockPos, blocks::Block};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Arc;

fn compile_o6(world: &PlotWorld) -> GraphBackend {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let bounds = world.get_corners();
    GraphBackend::compile_with_options(world, bounds, world.scheduler().iter_entries().collect(), &options, Arc::new(monitor)).unwrap()
}

fn assert_full_cpu_power_coverage(backend: &GraphBackend, cpu: cpus::Cpu) {
    let plans = backend.program.cpu_power_queries();
    assert_eq!(
        plans.entries.len(),
        backend.program.pistons().len(),
        "{}: expected an O6 Main/Quasi query for every piston actor",
        cpu.name
    );
    for index in 0..backend.program.pistons().len() {
        let actor = PistonId::new(index as u32);
        assert!(
            backend.program.cpu_power_query_entry(actor).is_some(),
            "{}: missing actor {index}",
            cpu.name
        );
    }
    println!(
        "{} O6 power-query coverage: {}/{} piston actors; {} original-input fallbacks among {} packed reads",
        cpu.name,
        plans.entries.len(),
        backend.program.pistons().len(),
        plans.packed.reads.iter().filter(|read| matches!(read, CpuRead::Input { .. })).count(),
        plans.packed.reads.len(),
    );
}

fn report_logical_bud_coverage(backend: &GraphBackend, cpu: cpus::Cpu, stage: &str) {
    let active = backend.logical_bud_owner_by_actor.iter().filter(|owner| owner.is_some()).count();
    let counts = backend.execution_counts();
    println!(
        "{} O6 logical BUD {stage}: compiled owners={}, runtime-active owners={}, logical samples={}, accepts={}, publications={}, physical samples={}, physical actions={}",
        cpu.name,
        backend.program.logical_bud_owners().len(),
        active,
        counts.logical_bud_samples,
        counts.logical_bud_accepts,
        counts.logical_bud_publications,
        counts.piston_samples,
        counts.piston_actions,
    );
}

pub(super) fn ram_word_snapshot(world: &PlotWorld, cpu: cpus::Cpu) -> Vec<Option<u16>> {
    if cpu.name != "cpu_bubblesort" {
        return Vec::new();
    }
    [7, 17, 27, 37]
        .into_iter()
        .flat_map(|y| (0..16).map(move |row| (y, row)))
        .map(|(y, row)| {
            (0..12).try_fold(0u16, |word, bit| {
                match world.get_block(cpu.origin + BlockPos::new(175 + 4 * bit, y, 62 - 4 * row)) {
                    Block::Piston { piston } => Some(word | (u16::from(!piston.extended) << bit)),
                    _ => None,
                }
            })
        })
        .collect()
}

pub(super) fn expand_ram(cpu: cpus::Cpu, ticks: u32) -> Vec<Vec<Option<u16>>> {
    let transitions = cpus::reference(cpu).ram_trace;
    assert_eq!(transitions.first().map(|row| row.0), Some(0));
    let mut expanded = Vec::with_capacity(ticks as usize + 1);
    let mut index = 0usize;
    let mut current = transitions[0].1.clone();
    expanded.push(current.clone());
    for tick in 1..=ticks {
        while transitions.get(index + 1).is_some_and(|row| row.0 <= tick) {
            index += 1;
            current.clone_from(&transitions[index].1);
        }
        expanded.push(current.clone());
    }
    expanded
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct FrozenBudTick {
    tick: u32,
    samples: usize,
    accepted: usize,
    sha256: String,
}

fn summarize_bud_tick(tick: u32, entries: &[PistonTraceEntry], memory: &HashSet<BlockPos>) -> Option<FrozenBudTick> {
    let selected: Vec<_> = entries
        .iter()
        .filter(|entry| match &entry.operation {
            PistonTraceOperation::Sample { pos, .. } => memory.contains(pos),
            PistonTraceOperation::Applied(event) => memory.contains(&event.pos),
        })
        .collect();
    if selected.is_empty() {
        return None;
    }
    let samples = selected
        .iter()
        .filter(|entry| matches!(&entry.operation, PistonTraceOperation::Sample { .. }))
        .count();
    Some(FrozenBudTick {
        tick,
        samples,
        accepted: selected.len() - samples,
        sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&selected).unwrap())),
    })
}

#[test]
#[ignore = "research: full O6 BubbleSort power-query coverage, frozen per-tick RAM, and interpreter continuation"]
fn bubblesort_o6_power_queries_match_every_frozen_ram_tick_and_handoff() {
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "cpu_bubblesort").unwrap();
    let mut world = cpus::load_cpu(cpu);
    let sign = cpu.origin + BlockPos::new(186, 2, 184);
    assert!(matches!(world.get_block(sign), Block::WallSign { .. }));
    assert!(world.set_block(sign, Block::Air));
    world.flush_block_changes();

    let mut backend = compile_o6(&world);
    assert_full_cpu_power_coverage(&backend, cpu);
    report_logical_bud_coverage(&backend, cpu, "before activity");
    backend.set_test_recording(true);
    let ticks = cpu.active_ticks + 1;
    let expected = expand_ram(cpu, ticks + 32);
    assert_eq!(ram_word_snapshot(&world, cpu), expected[0], "initial RAM");
    backend.use_block(cpu.origin + cpu.start).unwrap();
    for tick in 1..=ticks {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        assert_eq!(
            ram_word_snapshot(&world, cpu),
            expected[tick as usize],
            "O6 CPU_BUBBLESORT RAM at native tick {tick}"
        );
        backend.clear_test_traces();
        if tick % 1_000 == 0 {
            eprintln!("O6 BubbleSort tick {tick}/{ticks}");
        }
    }
    let counts = backend.execution_counts();
    report_logical_bud_coverage(&backend, cpu, "after activity");
    assert!(
        counts.piston_samples > 0 && counts.piston_actions > 0,
        "BubbleSort exercised piston power queries"
    );
    assert_eq!(
        &expected[ticks as usize][48..],
        &(0..16).map(Some).collect::<Vec<_>>(),
        "frozen trace is expected to finish the sort"
    );

    // Hand the materialized O6 world back to the interpreter and keep checking
    // public RAM against the same frozen trajectory.
    backend.reset(&mut world);
    for offset in 1..=32 {
        world.tick_interpreted();
        let tick = ticks + offset;
        let expected_tick = expected.get(tick as usize).expect("frozen RAM trace covers handoff");
        assert_eq!(ram_word_snapshot(&world, cpu), *expected_tick, "interpreter handoff RAM at tick {tick}");
    }
}

#[test]
#[ignore = "research: O6 ANPU power-query coverage and every frozen screen frame"]
fn anpu_o6_power_queries_match_all_frozen_screen_frames() {
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
    let mut world = cpus::load_cpu(cpu);
    world.disable_command_output_limits_for_replay();
    let mut backend = compile_o6(&world);
    assert_full_cpu_power_coverage(&backend, cpu);
    report_logical_bud_coverage(&backend, cpu, "before activity");
    backend.set_test_recording(true);

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/cpu-references");
    #[derive(Deserialize)]
    struct FrozenBudTrace {
        schematic_sha256: String,
        memory: Vec<BlockPos>,
        updates: Vec<FrozenBudTick>,
    }
    let bud: FrozenBudTrace = serde_json::from_slice(&std::fs::read(root.join("anpu_bud_updates.json")).unwrap()).unwrap();
    assert_eq!(bud.schematic_sha256, cpu.sha256);
    let bud_memory: HashSet<_> = bud.memory.into_iter().collect();
    assert_eq!(bud_memory.len(), 896);
    let expected_bud: Vec<_> = bud.updates.into_iter().filter(|row| row.tick <= cpu.active_ticks).collect();
    let mut bud_index = 0usize;

    let mut expected: Vec<cpus::ScreenFrame> = serde_json::from_slice(&std::fs::read(root.join("anpu_screen.json")).unwrap()).unwrap();
    expected.retain(|frame| frame.tick <= cpu.active_ticks);
    assert_eq!(expected.first().map(|frame| frame.tick), Some(0));
    let mut actual = Vec::new();
    cpus::collect_screen(&world, 0, &mut actual);
    let trace_start = backend.native_piston_trace().len();
    backend.use_block(cpu.origin + cpu.start).unwrap();
    if let Some(row) = summarize_bud_tick(0, &backend.native_piston_trace()[trace_start..], &bud_memory) {
        assert_eq!(expected_bud.get(bud_index), Some(&row), "ANPU BUD updates at activation");
        bud_index += 1;
    }
    backend.clear_test_traces();
    let mut expected_index = 0usize;
    for tick in 1..=cpu.active_ticks {
        let trace_start = backend.native_piston_trace().len();
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        let actual_bud = summarize_bud_tick(tick, &backend.native_piston_trace()[trace_start..], &bud_memory);
        if expected_bud.get(bud_index).is_some_and(|row| row.tick == tick) {
            assert_eq!(actual_bud.as_ref(), expected_bud.get(bud_index), "ANPU ordered BUD events at tick {tick}");
            bud_index += 1;
        } else {
            assert!(actual_bud.is_none(), "unexpected ANPU BUD event at tick {tick}: {actual_bud:?}");
        }
        cpus::collect_screen(&world, tick, &mut actual);
        while expected.get(expected_index + 1).is_some_and(|frame| frame.tick <= tick) {
            expected_index += 1;
        }
        assert_eq!(
            cpus::screen(&world),
            expected[expected_index].pixels,
            "O6 ANPU screen at native tick {tick}"
        );
        backend.clear_test_traces();
    }
    assert_eq!(actual, expected, "all O6 screen transition frames");
    assert_eq!(bud_index, expected_bud.len(), "all frozen ANPU BUD update ticks were compared");
    report_logical_bud_coverage(&backend, cpu, "after activity");

    println!(
        "anpu_pong O6 matched {} screen frames and {} ordered BUD update ticks through {} ticks",
        actual.len(),
        bud_index,
        cpu.active_ticks
    );
}
