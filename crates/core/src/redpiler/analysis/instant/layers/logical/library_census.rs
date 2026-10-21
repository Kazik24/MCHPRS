//! Ignored O6 extraction census for every checked-in schematic; it is not a correctness oracle.
use super::*;
use crate::plot::worldedit::load_schematic;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{LayerInputSource, PowerPort, SettledTarget, SiteSource, StateTest};
use crate::redpiler::{CompilerOptions, passes};
use mchprs_blocks::BlockPos;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[test]
#[ignore = "O6 logical extraction census for all local schematic sources; outputs are unverified frontier queries"]
fn report_o6_logical_census_for_every_schematic_source() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let data = repository.join("test_data");
    let mut files = Vec::new();
    collect_files(&data, "schem", &mut files).unwrap();
    files.sort();

    let mut groups = BTreeMap::<String, Vec<(PathBuf, usize)>>::new();
    for path in files {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let hash = format!("{:x}", Sha256::digest(&bytes));
                groups.entry(hash).or_default().push((path, bytes.len()));
            }
            Err(error) => eprintln!("schematic census read failed {}: {error}", path.display()),
        }
    }

    let json_files = collect_files(&data, "json", &mut Vec::new()).map(|mut paths| {
        paths.sort();
        paths
    });
    let json_files = match json_files {
        Ok(paths) => paths,
        Err(error) => panic!("could not enumerate JSON manifests: {error}"),
    };
    let output_path = repository.join("target/logical-inference/settled-library-census.jsonl");
    std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();
    let output = std::fs::File::create(&output_path).unwrap();
    let mut output = BufWriter::new(output);
    write_row(
        &mut output,
        &json!({
            "record":"scope",
            "inventory":"recursive test_data .schem files, SHA-256 deduplicated",
            "saved_worlds":"not included; CHUNGUS requires a separate saved-world loader",
            "correctness":"extraction census only; no output-port or native-correctness claim",
        }),
    );
    let mut completed = 0usize;

    for (hash, aliases) in groups {
        let primary = &aliases[0].0;
        let alias_names = aliases
            .iter()
            .map(|(path, _)| path.strip_prefix(&repository).unwrap_or(path).to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();
        let manifest = find_manifest(&json_files, &repository, &alias_names);
        let bytes = match std::fs::read(primary) {
            Ok(bytes) => bytes,
            Err(error) => {
                let row = json!({"hash":hash,"aliases":alias_names,"parse":"not_attempted","error":error.to_string()});
                write_row(&mut output, &row);
                completed += 1;
                continue;
            }
        };
        let clipboard = match load_schematic(std::io::Cursor::new(&bytes)) {
            Ok(clipboard) => clipboard,
            Err(error) => {
                let row = json!({"hash":hash,"aliases":alias_names,"bytes":bytes.len(),"parse":"failed","error":format!("{error:?}")});
                write_row(&mut output, &row);
                completed += 1;
                continue;
            }
        };
        let dimensions = [clipboard.size_x, clipboard.size_y, clipboard.size_z];
        let offset = [clipboard.offset_x, clipboard.offset_y, clipboard.offset_z];
        let Some(mut descriptor) = manifest else {
            let row = json!({"hash":hash,"aliases":alias_names,"bytes":bytes.len(),"dimensions":dimensions,"loader_offset":offset,"parse":"ok","manifest":false,"prepare":{"status":"not_attempted","reason":"no fixture manifest; origin and ports are unverified"}});
            write_row(&mut output, &row);
            completed += 1;
            continue;
        };
        descriptor["fixture"] = json!(primary.strip_prefix(&repository).unwrap_or(primary).to_string_lossy().replace('\\', "/"));
        // Keep manifest coordinates and ports authoritative; never guess them.
        let origin = super::cpus::manifest_origin(&descriptor);
        let anchor = origin + BlockPos::new(offset[0], offset[1], offset[2]);
        let size = BlockPos::new(dimensions[0] as i32, dimensions[1] as i32, dimensions[2] as i32);
        let bounds = (anchor - BlockPos::new(2, 2, 2), anchor + size + BlockPos::new(2, 2, 2));
        let mut row = json!({
            "hash": hash,
            "aliases": alias_names,
            "bytes": bytes.len(),
            "dimensions": dimensions,
            "loader_offset": offset,
            "parse": "ok",
            "manifest": true,
        });
        let world = match super::cpus::load_manifest(&descriptor) {
            Ok(world) => world,
            Err(error) => {
                row["prepare"] = json!({"status":"not_attempted","reason":error.to_string()});
                write_row(&mut output, &row);
                completed += 1;
                continue;
            }
        };
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let mut prepared = match passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)) {
            Ok(prepared) => prepared,
            Err(error) => {
                row["prepare"] = json!({"status":"failed","reason":format!("{error:?}")});
                write_row(&mut output, &row);
                completed += 1;
                continue;
            }
        };
        row["prepare"] = json!({"status":"ok","nodes":prepared.graph.nodes.len(),"pistons":prepared.graph.pistons.len(),"resources":prepared.graph.resources.len()});
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            stable_instant_inputs: false,
            ..Default::default()
        };
        if let Err(error) = passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &passes::default_graph_pipeline(&options),
            &monitor,
        ) {
            row["pipeline"] = json!({"status":"failed","reason":format!("{error:?}")});
            write_row(&mut output, &row);
            completed += 1;
            continue;
        }
        let graph = &prepared.graph;
        let primary_controls = graph
            .nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                )
            })
            .count();
        row["pipeline"] = json!({
            "status":"ok",
            "graph_nodes":graph.nodes.len(),
            "pistons":graph.pistons.len(),
            "resources":graph.resources.len(),
            "recognized_cells":graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len()),
            "primary_controls":primary_controls,
            "bud_memory_reads":graph.plans.bud_memory_reads.len(),
        });
        let settled = match compile_settled(graph, &monitor) {
            Ok(settled) => settled,
            Err(error) => {
                row["compile_settled"] = json!({"status":"failed","reason":error});
                write_row(&mut output, &row);
                completed += 1;
                continue;
            }
        };
        let frontier = settled
            .outputs
            .iter()
            .filter_map(|output| match output.target {
                SettledTarget::Power(target) => Some(target),
                _ => None,
            })
            .collect::<Vec<_>>();
        row["compile_settled"] = json!({
            "status":"ok",
            "input_count":settled.program.inputs.len(),
            "expression_count":settled.program.expressions.len(),
            "power_frontier_count":frontier.len(),
            "true_feedback_bits":settled.outputs.iter().filter(|output| matches!(output.target, SettledTarget::ResourceState { .. })).count(),
            "bud_register_bits":settled.outputs.iter().filter(|output| matches!(output.target, SettledTarget::Register(_))).count(),
            "retained_effects":settled.retained_effects.len(),
            "declines":settled.declines.len(),
        });
        if frontier.is_empty() {
            row["compile_computation"] = json!({"status":"not_attempted","reason":"no graph frontier power targets"});
            write_row(&mut output, &row);
            completed += 1;
            continue;
        }
        match compile_computation(graph, &frontier, &monitor) {
            Ok(computation) => {
                let (unresolved, samples) = closure_sources(graph, &computation);
                row["compile_computation"] = json!({
                    "status":"ok",
                    "target_scope":"graph frontier; not verified as public outputs",
                    "frontier_targets":frontier.len(),
                    "input_count":computation.program.inputs.len(),
                    "expression_count":computation.program.expressions.len(),
                    "true_feedback_bits":computation.outputs.iter().filter(|output| matches!(output.target, SettledTarget::ResourceState { .. })).count(),
                    "retained_effects":computation.retained_effects.len(),
                    "declines":computation.declines.len(),
                    "closure":{"closed":unresolved.values().sum::<usize>() == 0,"unresolved_source_classes":unresolved,"first_unresolved_samples":samples},
                });
            }
            Err(error) => {
                row["compile_computation"] = json!({"status":"failed","target_scope":"graph frontier; not verified as public outputs","frontier_targets":frontier.len(),"reason":error})
            }
        }
        write_row(&mut output, &row);
        completed += 1;
    }

    println!("library logical census wrote {completed} hash groups to {}", output_path.display());
}

fn write_row(output: &mut BufWriter<std::fs::File>, row: &Value) {
    serde_json::to_writer(&mut *output, row).unwrap();
    writeln!(output).unwrap();
    output.flush().unwrap();
    if let Some(alias) = row.get("aliases").and_then(Value::as_array).and_then(|aliases| aliases.first()) {
        println!("library logical census: {alias}");
    } else {
        println!("library logical census: scope note");
    }
}

fn collect_files(directory: &Path, extension: &str, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, extension, files)?;
        } else if path.extension().is_some_and(|value| value == extension) {
            files.push(path);
        }
    }
    Ok(())
}

fn find_manifest(manifests: &[PathBuf], repository: &Path, aliases: &[String]) -> Option<Value> {
    for path in manifests {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
        let Some(fixture) = value.get("fixture").and_then(Value::as_str) else {
            continue;
        };
        let fixture = fixture.replace('\\', "/");
        if aliases.iter().any(|alias| alias == &fixture) {
            return Some(value);
        }
    }
    let _ = repository;
    None
}

fn source_class(graph: &CompileGraph, node: NodeId) -> &'static str {
    match &graph.nodes[node.index()].kind {
        CircuitNodeKind::Wire(_) => "wire",
        CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Observer { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. }) => {
            "observer_reset"
        }
        CircuitNodeKind::Electrical(NodeType::Lever) => "lever",
        CircuitNodeKind::Electrical(NodeType::Button) => "button",
        CircuitNodeKind::Electrical(NodeType::PressurePlate) => "pressure_plate",
        CircuitNodeKind::Electrical(NodeType::Torch) => "torch",
        CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => "repeater",
        CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => "comparator",
        _ => "other",
    }
}

fn closure_sources(graph: &CompileGraph, circuit: &SettledCircuit) -> (BTreeMap<String, usize>, Vec<String>) {
    let feedback = circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut samples = Vec::new();
    for input in circuit.program.inputs.iter() {
        let class = match input.source {
            LayerInputSource::SourceAbove { site, port, threshold } => match graph.sites[site.index()].source {
                SiteSource::Component(node) => {
                    let kind = source_class(graph, node);
                    let declared = matches!(kind, "lever" | "button" | "pressure_plate") && port == PowerPort::Output && threshold == 0;
                    if declared { "primary_control" } else { kind }
                }
                SiteSource::Constant(_) => "constant",
                _ => "unknown_site_source",
            },
            LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                if feedback.contains_key(&resource) && graph.resources[resource.index()].contacts.contains(&contact) =>
            {
                "feedback_resource"
            }
            LayerInputSource::State(_) => "other_state",
        };
        if class != "primary_control" && class != "constant" && class != "feedback_resource" {
            *counts.entry(class.to_owned()).or_default() += 1;
            if samples.len() < 12 {
                samples.push(format!("class={class} source={:?}", input.source));
            }
        }
    }
    (counts, samples)
}
