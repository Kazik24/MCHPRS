//! Supplementary physical BUD oracle; never replace the original CPU references.
use crate::redstone::piston::trace::{self, Operation};
use crate::world::{for_each_block_optimized, World};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use rustc_hash::FxHashSet;

#[path = "../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BudTick {
    tick: u32,
    samples: usize,
    accepted: usize,
    /// SHA-256 of JSON serialization of the ordered, filtered trace entries.
    sha256: String,
}

fn sample(tick: u32, entries: &[trace::Entry], memory: &FxHashSet<BlockPos>) -> Option<BudTick> {
    let entries: Vec<_> = entries
        .iter()
        .filter(|entry| {
            memory.contains(&match entry.operation {
                Operation::Sample { pos, .. } => pos,
                Operation::Applied(event) => event.pos,
            })
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    let samples = entries
        .iter()
        .filter(|entry| matches!(entry.operation, Operation::Sample { .. }))
        .count();
    Some(BudTick {
        tick,
        samples,
        accepted: entries.len() - samples,
        sha256: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&entries).unwrap())
        ),
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct BudReference {
    schema: u32,
    schematic_sha256: String,
    game_ticks: u32,
    coordinates: String,
    memory: Vec<BlockPos>,
    updates: Vec<BudTick>,
}

fn read(memory: &FxHashSet<BlockPos>) -> BudReference {
    let reference: BudReference = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test_data/cpu-references/anpu_bud_updates.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(reference.schema, 1);
    assert_eq!(reference.schematic_sha256, cpus::CPUS[1].sha256);
    assert_eq!(reference.game_ticks, 50_000);
    assert_eq!(
        reference.memory.iter().copied().collect::<FxHashSet<_>>(),
        *memory
    );
    reference
}

fn capture_episode() -> BudReference {
    let cpu = cpus::CPUS[1];
    let frozen = cpus::reference(cpu);
    let mut world = cpus::load_cpu(cpu);
    let mut memory = FxHashSet::default();
    let (first, last) = world.get_corners();
    for_each_block_optimized(&world, first, last, |pos| {
        if matches!(world.get_block(pos), Block::Piston { piston } if piston.sticky && piston.facing == BlockFacing::Down)
            && matches!(
                world.get_block(pos.offset(BlockFace::Top)),
                Block::NoteBlock { .. }
            )
        {
            memory.insert(pos);
        }
    });
    assert_eq!(memory.len(), 896);
    let mut updates = Vec::new();
    let mut frames = Vec::new();
    cpus::collect_screen(&world, 0, &mut frames);
    assert_eq!(cpus::checkpoint(&world, 0, &[]), frozen.checkpoints[0]);
    if let Some(entry) = sample(
        0,
        &trace::capture(|| cpus::click(&mut world, cpu.start)),
        &memory,
    ) {
        updates.push(entry);
    }
    for tick in 1..=50_000 {
        if let Some(entry) = sample(tick, &trace::capture(|| world.tick_interpreted()), &memory) {
            updates.push(entry);
        }
        cpus::collect_screen(&world, tick, &mut frames);
        if let Some(expected) = frozen.checkpoints.iter().find(|c| c.tick == tick) {
            assert_eq!(cpus::checkpoint(&world, tick, &[]), *expected);
        }
    }
    let frozen_screen: Vec<cpus::ScreenFrame> = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test_data/cpu-references/anpu_screen.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        frames, frozen_screen,
        "capture must preserve the existing screen baseline"
    );
    let mut memory: Vec<_> = memory.into_iter().collect();
    memory.sort_by_key(|pos| (pos.x, pos.y, pos.z));
    BudReference {
        schema: 1,
        schematic_sha256: cpu.sha256.into(),
        game_ticks: 50_000,
        coordinates: "world positions after placing selection minimum at (8,8,8); ordered entries preserve sample tick, phase, position, old extension and sampled power, and accepted event kind".into(),
        memory,
        updates,
    }
}

#[test]
#[ignore = "ANPU 50,000-tick frozen physical memory/screen replay"]
fn anpu_interpreter_preserves_frozen_bud_updates_and_screen() {
    let captured = capture_episode();
    let memory = captured.memory.iter().copied().collect();
    let frozen = read(&memory);
    assert_eq!(
        captured.updates, frozen.updates,
        "frozen ANPU BUD sampling/write pattern"
    );
    println!(
        "ANPU interpreter preserved {} active BUD sample ticks and the frozen screen/checkpoints",
        captured.updates.len()
    );
}

#[test]
#[ignore = "explicit new-file baseline capture; requires MCHPRS_ANPU_BUD_CAPTURE"]
fn capture_anpu_bud_reference_to_new_file() {
    let destination = std::env::var("MCHPRS_ANPU_BUD_CAPTURE")
        .expect("set MCHPRS_ANPU_BUD_CAPTURE to a new output file");
    assert!(
        !Path::new(&destination).exists(),
        "refusing to replace a BUD reference"
    );
    let captured = capture_episode();
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .unwrap();
    serde_json::to_writer_pretty(output, &captured).unwrap();
    println!(
        "captured {} active BUD sample ticks",
        captured.updates.len()
    );
}

#[test]
fn anpu_cannot_bypass_compiled_graph_admission() {
    use crate::redpiler::analysis::{self, AnalysisLimits};
    use crate::redpiler::{Compiler, CompilerOptions};
    use std::collections::BTreeMap;

    let world = cpus::load_cpu(cpus::CPUS[1]);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let report = analysis::analyze(
        &world,
        world.get_corners(),
        &ticks,
        &Default::default(),
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let mut unsupported = BTreeMap::<_, usize>::new();
    for actor in &report.recognition {
        for failure in &actor.failures {
            if let analysis::families::RecognitionFailure::UnsupportedPayload { block, .. } =
                failure
            {
                *unsupported.entry(block).or_default() += 1;
            }
        }
    }
    println!(
        "ANPU live inventory: {}",
        serde_json::json!({
            "pistons": report.pistons.len(), "observers": report.observers.len(),
            "ordinary": report.pistons.iter().filter(|p| !p.piston.sticky).count(),
            "retracted": report.pistons.iter().filter(|p| !p.piston.extended).count(),
            "matched_reset_mechanisms": report.recognition.iter().filter(|r| r.is_matched()).count(),
            "payload_groups": report.payload_groups.len(), "unsupported_payloads": unsupported,
            "inspected_cells": report.inspected_cells, "dependency_steps": report.dependency_steps,
        })
    );
    for budget_multiplier in [1, 2, 4, 8] {
        for flags in [
            "",
            "--optimize",
            "--io-only",
            "--optimize --io-only",
            "--assume-instant",
        ] {
            let mut compiler = Compiler::default();
            let mut options = CompilerOptions::parse(flags).unwrap();
            options.budget_multiplier = budget_multiplier;
            let error = compiler
                .compile(
                    &world,
                    world.get_corners(),
                    options,
                    ticks.clone(),
                    Default::default(),
                )
                .unwrap_err();
            println!("ANPU admission budget={budget_multiplier}, flags={flags:?}: {error}");
            assert!(!compiler.is_active());
            assert!(compiler.current_flags().is_none());
            assert_eq!(cpus::checkpoint(&world, 0, &[]), before,
                "failed admission must preserve physical state and queued work; budget={budget_multiplier}, flags={flags}");
        }
    }
    assert!(CompilerOptions::parse("--piston-events").is_err());
}
