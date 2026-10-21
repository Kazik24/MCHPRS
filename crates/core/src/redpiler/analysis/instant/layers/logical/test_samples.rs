//! Ignored coverage probe for the saved TEST_ schematics. Admission is inventory, not a correctness claim.
use super::*;
use crate::plot::PlotWorld;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Block, Lever, LeverFace};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const NATIVE_REPLAY_FIXTURES: &[&str] = &[
    "TEST_BUD_1",
    "TEST_BUD_2",
    "TEST_CODER_1",
    "TEST_DEC",
    "TEST_DEC2",
    "TEST_INSTANT_3",
    "TEST_OR1",
    "TEST_OR2",
    "TEST_OR_3",
    "TEST_PISTION",
    "TEST_PISTION_2",
    "TEST_XOR",
    "TEST_XOR2",
];

#[test]
#[ignore = "O6 inventory of saved TEST_ schematics; report coverage without treating parsing as a correctness oracle"]
fn report_settled_extraction_for_all_test_schematics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research");
    let dirs = [root.join("test-prefix-20261008"), root.join("test-potados-counter-revised-20261008")];
    let mut fixtures = dirs
        .iter()
        .flat_map(|dir| {
            std::fs::read_dir(dir)
                .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.extension().is_some_and(|ext| ext == "schem")
                        && path.file_stem().is_some_and(|stem| stem.to_string_lossy().starts_with("TEST_"))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    fixtures.sort();
    println!("TEST_ schematic inventory: {} files", fixtures.len());

    for path in fixtures {
        report_fixture(&root, &path);
    }
}

fn report_fixture(root: &Path, path: &Path) {
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let bytes = std::fs::read(path).unwrap();
    let clipboard = match load_schematic(std::io::Cursor::new(&bytes)) {
        Ok(clipboard) => clipboard,
        Err(error) => {
            println!("{name}: parse declined: {error:?}");
            return;
        }
    };
    let prefix = root.join("test-prefix-20261008");
    let manifest_path = path.parent().unwrap().join("download-manifest.json");
    let manifest_record = if manifest_path.exists() {
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy();
        manifest["files"]
            .as_array()
            .and_then(|files| files.iter().find(|entry| entry["name"] == filename.as_ref()))
            .cloned()
    } else {
        None
    };
    if let Some(record) = &manifest_record {
        let actual_hash = format!("{:x}", Sha256::digest(&bytes));
        if record["bytes"].as_u64() != Some(bytes.len() as u64) || record["sha256"].as_str() != Some(actual_hash.as_str()) {
            println!("{name}: manifest byte/hash mismatch; inventory continues with the local file");
        }
    }

    let summary_path = prefix.join("output-summary.json");
    let summary: Value = serde_json::from_slice(&std::fs::read(summary_path).unwrap()).unwrap();
    let summary_name = format!("{name}.schem");
    let record = summary["schematics"]
        .as_array()
        .and_then(|schematics| schematics.iter().find(|entry| entry["name"] == summary_name.as_str()));
    let (added_controls, output_contracts, native_samples) = record.map_or((0, 0, 0), |record| {
        let added = record["added_controls"].as_array().map_or(0, Vec::len);
        let outputs = record["outputs"].as_array().map_or(0, Vec::len);
        let samples = record["episodes"].as_array().map_or(0, |episodes| {
            episodes
                .iter()
                .map(|episode| episode["native_final_outputs"].as_array().map_or(0, Vec::len))
                .sum()
        });
        (added, outputs, samples)
    });

    let size = BlockPos::new(clipboard.size_x as i32, clipboard.size_y as i32, clipboard.size_z as i32);
    let mut world = empty_world();
    paste_clipboard(
        &mut world,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    if let Some(record) = record {
        if let Some(controls) = record["added_controls"].as_array() {
            for control in controls {
                let local = BlockPos::new(
                    control["x"].as_i64().unwrap() as i32,
                    control["y"].as_i64().unwrap() as i32,
                    control["z"].as_i64().unwrap() as i32,
                );
                let position = ORIGIN + local;
                let support = position.offset(BlockFace::South);
                if world.get_block(support).is_solid() {
                    world.set_block(
                        position,
                        Block::Lever {
                            lever: Lever::new(LeverFace::Wall, BlockDirection::North, false),
                        },
                    );
                }
            }
        }
    }
    // The archived selections omit the floor in some cases; restore only the
    // bottom-layer support used by the independent TEST-family harness.
    for z in 0..size.z {
        for x in 0..size.x {
            let position = ORIGIN + BlockPos::new(x, 0, z);
            let below = position.offset(BlockFace::Bottom);
            if crate::interaction::attachment_support(world.get_block(position), position).is_some_and(|(support, _)| support == below)
                && world.get_block(below) == Block::Air
            {
                world.set_block(below, Block::Stone {});
            }
        }
    }
    let bounds = (
        ORIGIN - BlockPos::new(2, 2, 2),
        ORIGIN + BlockPos::new(size.x + 2, size.y + 2, size.z + 2),
    );
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = match passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)) {
        Ok(prepared) => prepared,
        Err(error) => {
            println!(
                "{name}: dimensions={}x{}x{} controls={} outputs={} native_samples={} O6 prepare declined: {error:?}",
                size.x, size.y, size.z, added_controls, output_contracts, native_samples
            );
            return;
        }
    };
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        ..Default::default()
    };
    if let Err(error) = passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    ) {
        println!(
            "{name}: dimensions={}x{}x{} controls={} outputs={} native_samples={} O6 pipeline declined: {error:?}",
            size.x, size.y, size.z, added_controls, output_contracts, native_samples
        );
        return;
    }

    let graph = &prepared.graph;
    let cells = graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len());
    let actor_kinds = actor_kind_counts(graph);
    match compile_settled(graph, &monitor) {
        Ok(circuit) => {
            let register_outputs = circuit
                .outputs
                .iter()
                .filter(|output| matches!(output.target, SettledTarget::Register(_)))
                .count();
            let native_oracle_test = NATIVE_REPLAY_FIXTURES.contains(&name.as_str());
            println!(
                "{name}: dims={}x{}x{} nodes={} cells={} inputs={} exprs={} outputs={} registers={} retained_effects={} declines={} controls={} output_contracts={} native_samples={} independent_native_replay_test={} actor_kinds={actor_kinds:?}",
                size.x,
                size.y,
                size.z,
                graph.nodes.len(),
                cells,
                circuit.program.inputs.len(),
                circuit.program.expressions.len(),
                circuit.outputs.len(),
                register_outputs,
                circuit.retained_effects.len(),
                circuit.declines.len(),
                added_controls,
                output_contracts,
                native_samples,
                native_oracle_test,
            );
            let mut causes = BTreeMap::<String, usize>::new();
            for decline in &circuit.declines {
                *causes.entry(decline_cause(decline)).or_default() += 1;
            }
            for (cause, count) in causes {
                println!("  decline {count}x: {cause}");
            }
        }
        Err(error) => println!(
            "{name}: dims={}x{}x{} nodes={} cells={} controls={} outputs={} native_samples={} settled compile error={error}",
            size.x,
            size.y,
            size.z,
            graph.nodes.len(),
            cells,
            added_controls,
            output_contracts,
            native_samples,
        ),
    }
}

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..crate::plot::PLOT_WIDTH)
            .flat_map(|x| (0..crate::plot::PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}

fn actor_kind_counts(graph: &CompileGraph) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for node in &graph.nodes {
        let name = match &node.kind {
            CircuitNodeKind::Electrical(kind) => format!("Electrical::{kind:?}"),
            CircuitNodeKind::Piston(_) => "Piston".into(),
            CircuitNodeKind::Observer { .. } => "Observer".into(),
            CircuitNodeKind::BuiltinReset { .. } => "BuiltinReset".into(),
            CircuitNodeKind::Contact(_) => "Contact".into(),
            CircuitNodeKind::Wire(_) => "Wire".into(),
        };
        *counts.entry(name).or_default() += 1;
    }
    counts
}

fn decline_cause(decline: &str) -> String {
    decline.split_once(": ").map_or_else(|| decline.to_owned(), |(_, cause)| cause.to_owned())
}
