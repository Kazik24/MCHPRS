use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::{TaskMonitor, passes};
use crate::world::storage::Chunk;
use mchprs_blocks::BlockPos;
use serde_json::json;
use std::path::Path;

#[test]
#[ignore = "read-only full fixture and ANPU classic-net discovery census"]
fn classic_discovery_census() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths: Vec<_> = std::fs::read_dir(root.join("test_data/graph-fixtures"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "schem"))
        .collect();
    paths.sort();
    paths.push(root.join("test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem"));
    let mut rows = Vec::new();
    for path in paths {
        let mut clip = load_schematic(std::fs::File::open(&path).unwrap()).unwrap();
        clip.offset_x = 0;
        clip.offset_y = 0;
        clip.offset_z = 0;
        let origin = BlockPos::new(8, 8, 8);
        let bounds = (
            origin,
            origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
        );
        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
        paste_clipboard(&mut world, &clip, origin, false);
        let started = std::time::Instant::now();
        let name = path.file_name().unwrap().to_string_lossy();
        let row = match passes::rich::prepare(&world, bounds, vec![], &TaskMonitor::default(), AnalysisLimits::for_budget(8)) {
            Ok(prepared) => {
                let report = census(&prepared.graph);
                println!("CLASSIC {name}: {}", serde_json::to_string(&report).unwrap());
                let monitor = TaskMonitor::default();
                let options = Default::default();
                let output = CompileClassicRedstone
                    .build(
                        &prepared.graph,
                        &PassContext {
                            monitor: &monitor,
                            options: &options,
                            entry: &prepared.entry,
                        },
                    )
                    .unwrap();
                let PlanOutput::ClassicNets(plans) = output else { unreachable!() };
                println!(
                    "CLASSIC PROOF {name}: {} nets, {} wires; {:?}",
                    plans.len(),
                    plans.iter().map(|plan| plan.wires.len()).sum::<usize>(),
                    monitor.graph_statistics().preparation_tables
                );
                let accepted:Vec<_> = plans.iter().map(|plan|json!({"source":plan.source.index(),"position":prepared.graph.nodes[plan.source.index()].position,"wires":plan.wires.len(),"receivers":plan.inputs.len(),"entries":plan.entries.len(),"ops":plan.entries.iter().map(|entry|entry.ops.len()).sum::<usize>()})).collect();
                let tables: Vec<_> = monitor
                    .graph_statistics()
                    .preparation_tables
                    .into_iter()
                    .map(|table| json!({"table":table.table,"entries":table.entries}))
                    .collect();
                json!({"fixture":name,"report":report,"accepted":accepted,"proof_tables":tables,"seconds":started.elapsed().as_secs_f64()})
            }
            Err(error) => {
                println!("CLASSIC {name}: preparation declined: {error}");
                json!({"fixture":name,"preparation_error":error.to_string(),"seconds":started.elapsed().as_secs_f64()})
            }
        };
        rows.push(row);
        std::fs::write(root.join("target/classic-discovery.json"), serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    }
    assert!(!rows.is_empty());
}
