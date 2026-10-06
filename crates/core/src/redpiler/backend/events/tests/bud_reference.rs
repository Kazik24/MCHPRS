//! Supplementary physical BUD oracle; never replace the original CPU references.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BudTick {
    tick: u32,
    samples: usize,
    accepted: usize,
    /// SHA-256 of JSON serialization of the ordered, filtered trace entries.
    sha256: String,
}

pub(super) fn sample(
    tick: u32,
    entries: &[trace::Entry],
    memory: &FxHashSet<BlockPos>,
) -> Option<BudTick> {
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
pub(super) struct BudReference {
    schema: u32,
    schematic_sha256: String,
    game_ticks: u32,
    coordinates: String,
    memory: Vec<BlockPos>,
    pub updates: Vec<BudTick>,
}

pub(super) fn read(memory: &FxHashSet<BlockPos>) -> BudReference {
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

#[test]
#[ignore = "explicit new-file baseline capture; requires MCHPRS_ANPU_BUD_CAPTURE"]
fn capture_anpu_bud_reference_to_new_file() {
    let destination = std::env::var("MCHPRS_ANPU_BUD_CAPTURE")
        .expect("set MCHPRS_ANPU_BUD_CAPTURE to a new output file");
    assert!(
        !Path::new(&destination).exists(),
        "refusing to replace a BUD reference"
    );
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
    let captured = BudReference {
        schema: 1,
        schematic_sha256: cpu.sha256.into(),
        game_ticks: 50_000,
        coordinates: "world positions after placing selection minimum at (8,8,8); ordered entries preserve sample tick, phase, position, old extension and sampled power, and accepted event kind".into(),
        memory,
        updates,
    };
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
