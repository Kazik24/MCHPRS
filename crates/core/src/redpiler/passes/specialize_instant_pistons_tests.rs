use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::AnalysisLimits;
#[cfg(test)]
use crate::redpiler::passes::Pass;
use crate::redpiler::passes::{PassEntry, PassPipeline, rich, run_graph_pipeline};
use crate::redpiler::program_graph::EntryState;
use crate::redpiler::program_graph::*;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::BlockFace;
use mchprs_blocks::{BlockPos, blocks::RotateAmt};
use sha2::{Digest, Sha256};

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    )
}

fn fixture(rotation: Option<RotateAmt>, anchor: BlockPos) -> CompileGraph {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes = std::fs::read(root.join("test_data/instant-pistons-io/ADDER_1BIT.schem")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "e80092c97fc4637c313a735d38d0241b69b876f41dd2db7de571a6458369e526"
    );
    let clip = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let origin = BlockPos::new(40, 30, 40);
    let mut original = empty_world();
    paste_clipboard(
        &mut original,
        &clip,
        origin + BlockPos::new(clip.offset_x, clip.offset_y, clip.offset_z),
        false,
    );
    let mut world = empty_world();
    let mut low = anchor;
    let mut high = anchor;
    for y in 0..clip.size_y {
        for z in 0..clip.size_z {
            for x in 0..clip.size_x {
                let mut block = original.get_block(origin + BlockPos::new(x as i32, y as i32, z as i32));
                if let Some(rotation) = rotation {
                    block.rotate(rotation);
                }
                let p = BlockPos::new(x as i32, y as i32, z as i32);
                let p = match rotation {
                    Some(RotateAmt::Rotate90) => BlockPos::new(-p.z, p.y, p.x),
                    Some(RotateAmt::Rotate180) => BlockPos::new(-p.x, p.y, -p.z),
                    Some(RotateAmt::Rotate270) => BlockPos::new(p.z, p.y, -p.x),
                    _ => p,
                } + anchor;
                low = low.min(p);
                high = high.max(p);
                world.set_block(p, block);
            }
        }
    }
    rich::prepare(&world, (low, high), vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph
}

fn apply(graph: &mut CompileGraph) {
    run_graph_pipeline(
        graph,
        &CompilerOptions::default(),
        &EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        },
        &PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(SpecializeInstantPistons),
            enabled: true,
        }]),
        &TaskMonitor::default(),
    )
    .unwrap();
}

#[test]
fn recognizes_and_specializes_every_adder_actor_in_all_horizontal_rotations() {
    for rotation in [None, Some(RotateAmt::Rotate90), Some(RotateAmt::Rotate180), Some(RotateAmt::Rotate270)] {
        let mut graph = fixture(rotation, BlockPos::new(80, 50, 80));
        let recognized = discover_mechanisms(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph.pistons.len(), 13);
        assert_eq!(recognized.len(), 13, "rotation {rotation:?}");
        assert_eq!(recognized.iter().filter(|m| m.piston != m.watched_actor).count(), 4);
        let callbacks = graph.routes.clone();
        let resources = graph.resources.clone();
        let before = graph.power_links.clone();
        apply(&mut graph);
        for mechanism in &recognized {
            let node = graph.pistons[mechanism.piston.index()].node;
            let inputs = |links: &[PowerLink]| {
                links
                    .iter()
                    .filter(|link| matches!(link.to,PowerTarget::Input{node:target,..} if target==node))
                    .copied()
                    .collect::<Vec<_>>()
            };
            assert_ne!(
                inputs(&before),
                inputs(&graph.power_links),
                "actor {:?} must receive a real rewrite",
                mechanism.piston
            );
            assert!(!mechanism.resources.is_empty());
        }
        assert_eq!(graph.routes, callbacks);
        assert_eq!(graph.resources, resources);
        assert_eq!(discover_mechanisms(&graph, &TaskMonitor::default()).unwrap(), recognized);
        let once = graph.clone();
        apply(&mut graph);
        assert_eq!(graph, once);
        graph.lower().unwrap();
    }
}

#[test]
fn wrong_observer_power_face_or_missing_change_callback_declines_recognition() {
    let graph = fixture(None, BlockPos::new(40, 30, 40));
    let mechanisms = discover_mechanisms(&graph, &TaskMonitor::default()).unwrap();
    let target = &mechanisms[0];
    let mut changed = graph.clone();
    for link in &mut changed.power_links {
        if matches!(changed.sites[link.from.index()].source,SiteSource::Component(node) if node==target.observer) {
            link.from_port = PowerPort::Strong(BlockFace::Top);
        }
    }
    assert!(
        !discover_mechanisms(&changed, &TaskMonitor::default())
            .unwrap()
            .iter()
            .any(|m| m.piston == target.piston)
    );
    let watched = graph.pistons[target.watched_actor.index()];
    let mut changed = graph.clone();
    changed.routes[watched.routes.base_changed.index()].deliveries = Box::new([]);
    assert!(
        !discover_mechanisms(&changed, &TaskMonitor::default())
            .unwrap()
            .iter()
            .any(|m| m.piston == target.piston)
    );
}

#[test]
fn cancellation_and_disabled_pipeline_leave_the_graph_unchanged() {
    let original = fixture(None, BlockPos::new(40, 30, 40));
    let mut graph = original.clone();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    assert!(
        SpecializeInstantPistons
            .run(
                &mut graph,
                &PassContext {
                    monitor: &monitor,
                    options: &CompilerOptions::default(),
                    entry: &EntryState {
                        logical_tick: 0,
                        pending_ticks: vec![]
                    }
                }
            )
            .is_err()
    );
    assert_eq!(graph, original);
    run_graph_pipeline(
        &mut graph,
        &CompilerOptions::default(),
        &EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        },
        &PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(SpecializeInstantPistons),
            enabled: false,
        }]),
        &TaskMonitor::default(),
    )
    .unwrap();
    assert_eq!(graph, original);
}

#[test]
fn shared_high_fan_in_cannot_explode_the_power_table() {
    let mut graph = fixture(None, BlockPos::new(40, 30, 40));
    let actor = graph.pistons[0].node;
    let input = *graph
        .power_links
        .iter()
        .find(|link| {
            link.to
                == (PowerTarget::Input {
                    node: actor,
                    port: InputPort::Main,
                })
                && matches!(link.from_port, PowerPort::Conducted { .. })
        })
        .unwrap();
    let PowerPort::Conducted { include_dust } = input.from_port else {
        unreachable!()
    };
    let target = PowerTarget::Conductor {
        site: input.from,
        include_dust,
    };
    let contributor = *graph.power_links.iter().find(|link| link.to == target).unwrap();
    for link in &mut graph.power_links {
        if matches!(link.to, PowerTarget::Input {node,port:InputPort::Main} if graph.pistons.iter().any(|actor|actor.node==node)) {
            link.from = input.from;
            link.from_port = input.from_port;
        }
    }
    graph.power_links.extend(std::iter::repeat_n(contributor, 100));
    let before = graph.power_links.len();
    apply(&mut graph);
    assert!(graph.power_links.len() <= before * 2);
    assert_eq!(discover_mechanisms(&graph, &TaskMonitor::default()).unwrap().len(), 13);
}
