//! Explicit inspection artifact from the same public pipeline selected by --optimize.
use super::*;

#[test]
#[ignore = "write the complete current FPU O5 stable-input graph for the viewer"]
fn export_current_fpu_o5_graph_for_viewer() {
    let mut fixture = manifest("fpu_legal");
    fixture["origin"] = json!([40, 30, 40]);
    let (world, bounds) = load(&fixture);
    let mut options = CompilerOptions::parse("--optimize --stable-instant-inputs").unwrap();
    options.budget_multiplier = 8;
    let monitor = std::sync::Arc::new(TaskMonitor::default());
    println!("Preparing FPU O5 + stable-input programs...");
    let candidate = crate::redpiler::analysis::graph::prepare_candidate_graph(
        &world,
        bounds,
        &world.scheduler().iter_entries().collect::<Vec<_>>(),
        &options,
        monitor.clone(),
    )
    .unwrap();
    assert!(!candidate.graph.plans.cpu_kernels.entries.is_empty());
    assert!(!candidate.graph.plans.stable_instant_logic.nets.is_empty());
    println!("Exporting validated FPU inspection tables...");
    let export = crate::redpiler::analysis::export_json::from_graph(&candidate.report, &candidate.graph).unwrap();
    println!("Serializing FPU graph JSON...");
    let bytes = serde_json::to_vec(&export).unwrap();
    let directory = root().join("target/graph-viewer-current");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("fpu-o5-stable.graph.json"), &bytes).unwrap();
    std::fs::write(
        directory.join("fpu-export-provenance.json"),
        serde_json::to_vec_pretty(&json!({
            "schematic":fixture["fixture"], "schematic_sha256":fixture["sha256"],
            "flags":"--optimize --stable-instant-inputs", "budget_multiplier":8,
            "bytes":bytes.len(), "graph_sha256":format!("{:x}",Sha256::digest(&bytes)),
            "nodes":candidate.summary().nodes,"links":candidate.summary().links,
            "scope":"compile-time programs and physical mappings; runtime-learned transcripts are unavailable",
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "FPU O5 graph: {:.2} MiB; {} nodes, {} links; target/graph-viewer-current/fpu-o5-stable.graph.json",
        bytes.len() as f64 / (1024.0 * 1024.0),
        candidate.summary().nodes,
        candidate.summary().links,
    );
}
