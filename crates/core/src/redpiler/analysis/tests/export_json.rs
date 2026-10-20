use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::export_json as adapter;
use crate::redpiler::passes;
use crate::redpiler::program_graph::CompileGraph;
use crate::world::{BlockAction, World};
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::{Block, ComparatorMode, Lever, LeverFace, RedstoneComparator};
use mchprs_world::TickPriority;
use serde_json::Value;
use std::cell::Cell;

fn exported(world: &PlotWorld) -> adapter::GraphExport {
    exported_with_options(world, &CompilerOptions::default())
}

fn exported_with_options(world: &PlotWorld, options: &CompilerOptions) -> adapter::GraphExport {
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    adapter::prepare(
        world,
        world.get_corners(),
        &ticks,
        options,
        &crate::redpiler::TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
}

fn prepared_graph_export(world: &PlotWorld, optimize: bool) -> (CompileGraph, Vec<u8>) {
    let monitor = TaskMonitor::default();
    let ticks = world.scheduler().iter_entries().collect();
    let mut prepared = passes::rich::prepare(world, world.get_corners(), ticks, &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: if optimize { None } else { Some(0) },
        ..Default::default()
    };
    let pipeline = passes::default_graph_pipeline(&options);
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    prepared.finish_pipeline_statistics(&monitor);
    let statistics = monitor.graph_statistics();
    assert!(statistics.parser_complete.is_some());
    assert_eq!(statistics.final_counts, Some(crate::redpiler::GraphCounts::of(&prepared.graph)));
    let bytes = adapter::from_graph(&prepared.report, &prepared.graph).unwrap().to_bytes().unwrap();
    (prepared.graph, bytes)
}

struct CancelAtReadWorld<'a> {
    inner: PlotWorld,
    monitor: &'a TaskMonitor,
    cancel_on_read: usize,
    raw_reads: Cell<usize>,
    reads_after_cancel: Cell<usize>,
}

impl World for CancelAtReadWorld<'_> {
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        let reads = self.raw_reads.get() + 1;
        self.raw_reads.set(reads);
        if reads == self.cancel_on_read {
            self.monitor.cancel();
        }
        if self.monitor.cancelled() {
            self.reads_after_cancel.set(self.reads_after_cancel.get() + 1);
        }
        self.inner.get_block_raw(pos)
    }

    fn set_block_raw(&mut self, pos: BlockPos, block: u32) -> bool {
        self.inner.set_block_raw(pos, block)
    }

    fn delete_block_entity(&mut self, pos: BlockPos) {
        self.inner.delete_block_entity(pos);
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity> {
        self.inner.get_block_entity(pos)
    }

    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity> {
        self.inner.get_block_entity_mut(pos)
    }

    fn piston_state(&self) -> &mchprs_world::PistonState {
        self.inner.piston_state()
    }

    fn piston_state_mut(&mut self) -> &mut mchprs_world::PistonState {
        self.inner.piston_state_mut()
    }

    fn set_block_entity(&mut self, pos: BlockPos, entity: BlockEntity) {
        self.inner.set_block_entity(pos, entity);
    }

    fn get_chunk(&self, x: i32, z: i32) -> Option<&crate::world::storage::Chunk> {
        self.inner.get_chunk(x, z)
    }

    fn get_chunk_mut(&mut self, x: i32, z: i32) -> Option<&mut crate::world::storage::Chunk> {
        self.inner.get_chunk_mut(x, z)
    }

    fn schedule_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.inner.schedule_tick(pos, delay, priority);
    }

    fn schedule_half_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.inner.schedule_half_tick(pos, delay, priority);
    }

    fn pending_tick_at(&mut self, pos: BlockPos) -> bool {
        self.inner.pending_tick_at(pos)
    }

    fn block_action(&mut self, pos: BlockPos, action: BlockAction) {
        self.inner.block_action(pos, action);
    }

    fn play_sound(&mut self, pos: BlockPos, id: i32, category: i32, volume: f32, pitch: f32) {
        self.inner.play_sound(pos, id, category, volume, pitch);
    }

    fn container_opened(&mut self, pos: BlockPos, kind: ContainerType) {
        self.inner.container_opened(pos, kind);
    }
}

#[test]
fn electrical_preparation_stops_after_cancellation_and_export_reports_it() {
    let mut inner = PlotWorld::from_chunks(0, 0, vec![crate::world::storage::Chunk::empty(0, 0)], Default::default());
    inner.set_block(BlockPos::new(1, 20, 1), Block::RedstoneLamp { lit: false });
    let monitor = TaskMonitor::default();
    let world = CancelAtReadWorld {
        inner,
        monitor: &monitor,
        // Geometry inspects the occupied 16x16x16 section before electrical preparation starts.
        cancel_on_read: 4_097,
        raw_reads: Cell::new(0),
        reads_after_cancel: Cell::new(0),
    };
    let bounds = (BlockPos::new(0, 16, 0), BlockPos::new(15, 31, 15));

    let directory = std::env::temp_dir().join(format!("mchprs-redpiler-cancel-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&directory).unwrap();
    let result = adapter::prepare_and_write(
        &world,
        bounds,
        &[],
        &CompilerOptions::default(),
        &monitor,
        AnalysisLimits::default(),
        &directory,
        0,
        0,
    );
    let cancelled = result.as_ref().err().is_some_and(|error| error.code == "cancelled");
    let published_files = std::fs::read_dir(&directory).unwrap().count();
    std::fs::remove_dir_all(&directory).unwrap();

    assert!(cancelled, "cancelled preparation must not publish an export");
    assert_eq!(published_files, 0);
    assert!(world.reads_after_cancel.get() < 64);
}

#[test]
fn electrical_graph_expansion_obeys_the_preparation_budget() {
    let mut world = PlotWorld::from_chunks(0, 0, vec![crate::world::storage::Chunk::empty(0, 0)], Default::default());
    let lamp = BlockPos::new(5, 20, 5);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });

    let result = adapter::prepare(
        &world,
        (lamp, lamp),
        &[],
        &CompilerOptions::default(),
        &TaskMonitor::default(),
        AnalysisLimits {
            max_cells: 1,
            ..Default::default()
        },
    );

    assert!(result.is_err(), "the expanded electrical graph exceeds one cell of preparation budget");
}

#[test]
fn callback_preparation_stops_reading_after_cancellation() {
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
    let mut inner = PlotWorld::from_chunks(0, 0, vec![crate::world::storage::Chunk::empty(0, 0)], Default::default());
    for index in 0..6 {
        let lever = BlockPos::new(1 + index * 2, 20, 2);
        inner.set_block(lever.offset(mchprs_blocks::BlockFace::Bottom), Block::Stone {});
        inner.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, Default::default(), false),
            },
        );
        inner.set_block(lever.offset(mchprs_blocks::BlockFace::East), Block::RedstoneLamp { lit: false });
    }
    let preparation_monitor = TaskMonitor::default();
    let geometry = super::super::geometry::prepare_geometry(&inner, bounds, &[], &preparation_monitor, AnalysisLimits::default()).unwrap();
    let mut electrical = super::super::electrical::prepare_electrical(&inner, bounds, &geometry.draft, &preparation_monitor).unwrap();
    let monitor = TaskMonitor::default();
    let world = CancelAtReadWorld {
        inner,
        monitor: &monitor,
        cancel_on_read: 1,
        raw_reads: Cell::new(0),
        reads_after_cancel: Cell::new(0),
    };

    let result = super::super::callbacks::prepare_callbacks(&world, &geometry.draft, &mut electrical, &monitor);

    assert!(matches!(
        result,
        Err(super::super::callbacks::CallbackError::Preparation(AnalysisError::Cancelled))
    ));
    assert!(world.reads_after_cancel.get() <= 1, "only the read that triggers cancellation may finish");
}

#[test]
fn adder_1bit_fixture_matches_graph_json() {
    for level in 0..=crate::redpiler::MAX_OPTIMIZATION_LEVEL {
        let bytes = adder_1bit_export(Some(level));
        let expected = std::fs::read(adder_1bit_export_path(level)).unwrap();
        assert!(
            bytes == expected,
            "ADDER_1BIT O{level} export differs from its checked-in fixture; inspect the change before running write_adder_1bit_graph_json_fixtures"
        );
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["version"]["major"], 1);
        assert_eq!(document["stage"], "callback_draft");
        assert!(!document["graph"]["routes"].as_array().unwrap().is_empty());
        let graph = &document["graph"];
        assert!(!graph["callback_links"].as_array().unwrap().is_empty());
        assert!(
            graph["callback_links"].as_array().unwrap().iter().any(|link| {
                link["kind"] == "update"
                    && graph["nodes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|node| node["id"] == link["from"] && node["kind"]["kind"] == "piston")
                    && graph["nodes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|node| node["id"] == link["to"] && node["kind"]["kind"] == "observer")
            }),
            "the adder export retains direct piston-to-observer updates"
        );
        assert!(document["trace"].is_null());
    }
    assert_eq!(adder_1bit_export(None), adder_1bit_export(Some(crate::redpiler::MAX_OPTIMIZATION_LEVEL)));
}

#[test]
fn stable_o3_adder_export_keeps_the_existing_proxy_program() {
    let world = geometry_io_fixture("ADDER_1BIT.schem");
    let export = exported_with_options(
        &world,
        &CompilerOptions {
            optimization_level: Some(3),
            stable_instant_inputs: true,
            ..Default::default()
        },
    );
    let document: Value = serde_json::from_slice(&export.to_bytes().unwrap()).unwrap();
    let graph = &document["graph"];
    assert_eq!(graph["instant_regions"].as_array().unwrap().len(), 1);
    let region = &graph["instant_regions"][0];
    assert_eq!(region["stable_inputs"], true);
    assert_eq!(region["actors"].as_array().unwrap().len(), 13);
    assert_eq!(region["wires"].as_array().unwrap().len(), 32);
    let proxy = &graph["instant_proxies"][0];
    assert_eq!(proxy["private_cells"].as_array().unwrap().len(), 4);
    let mut sampled = std::collections::BTreeSet::new();
    for route in proxy["routes"].as_array().unwrap() {
        for operation in route["operations"].as_array().unwrap() {
            if operation["kind"] == "sample" {
                sampled.insert(operation["piston"].as_str().unwrap());
            }
            if operation["kind"] == "arm_reset" {
                assert!(region["resets"].as_array().unwrap().contains(&operation["node"]));
            }
        }
    }
    assert_eq!(sampled.len(), 13);
    for removed in ["instant_machine", "instant_kernel", "symbolic_region_declines", "symbolic_regions"] {
        assert!(graph.get(removed).is_none(), "removed synthesis artifact {removed} must not be exported");
    }
}

#[test]
fn stable_o4_adder_export_reports_selected_kernels_and_retained_observations() {
    let world = geometry_io_fixture("ADDER_1BIT.schem");
    let export = exported_with_options(
        &world,
        &CompilerOptions {
            optimization_level: Some(4),
            stable_instant_inputs: true,
            ..Default::default()
        },
    );
    let document: Value = serde_json::from_slice(&export.to_bytes().unwrap()).unwrap();
    let graph = &document["graph"];
    assert!(!graph["instant_cells"].as_array().unwrap().is_empty());
    assert!(graph.get("instant_layer_regions").is_none());
    let kernels = graph["instant_event_kernels"].as_array().unwrap();
    assert_eq!(kernels.len(), 1);
    assert_eq!(kernels[0]["actors"].as_array().unwrap().len(), 13);
    assert_eq!(kernels[0]["entries"].as_array().unwrap().len(), 13);
    assert_eq!(kernels[0]["wire_propagation_removed"], false);
    assert!(
        graph["wire_plans"]
            .as_array()
            .unwrap()
            .iter()
            .all(|wire| wire["mutable_captures"].is_u64())
    );
    assert!(graph["instant_proxies"].as_array().unwrap().is_empty());
    assert!(
        graph.get("instant_kernel").is_none(),
        "the removed learned kernel is not the layered compiler"
    );
}

#[test]
#[ignore = "regenerates the checked-in ADDER_1BIT graph JSON fixtures for every optimization level"]
fn write_adder_1bit_graph_json_fixtures() {
    std::fs::create_dir_all(adder_1bit_export_path(0).parent().unwrap()).unwrap();
    for level in 0..=crate::redpiler::MAX_OPTIMIZATION_LEVEL {
        std::fs::write(adder_1bit_export_path(level), adder_1bit_export(Some(level))).unwrap();
    }
}

#[test]
#[ignore = "regenerates O0 and O1 graph JSON exports for test_data/graph-fixtures"]
fn write_graph_fixture_json_variants() {
    let directory = root().join("test_data/graph-fixtures");
    let mut schematics: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "schem"))
        .collect();
    schematics.sort();
    assert!(!schematics.is_empty(), "no graph fixture schematics found");

    let mut rejected = Vec::new();
    for schematic in schematics {
        let name = schematic.file_name().unwrap().to_string_lossy();
        let world = graph_fixture_world(&schematic);
        for (level, suffix) in [(0, ""), (1, "-o1")] {
            let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
            let result = adapter::prepare(
                &world,
                world.get_corners(),
                &ticks,
                &CompilerOptions {
                    optimization_level: Some(level),
                    ..Default::default()
                },
                &crate::redpiler::TaskMonitor::default(),
                AnalysisLimits::default(),
            )
            .and_then(|export| export.to_bytes());
            let bytes = match result {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("{name} O{level} rejected: {error}");
                    rejected.push(format!("{name} O{level}: {error}"));
                    continue;
                }
            };
            let stem = schematic.file_stem().unwrap().to_string_lossy();
            let destination = directory.join(format!("{stem}{suffix}.graph.json"));
            std::fs::write(destination, bytes).unwrap();
        }
    }
    assert!(rejected.is_empty(), "fixtures not exported:\n{}", rejected.join("\n"));
}

fn graph_fixture_world(path: &std::path::Path) -> PlotWorld {
    let clipboard = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    if path.file_name().is_some_and(|name| name == "adder_non_instant_pistons_4bit.schem") {
        // This fixture uses wall signs as labels, including labels on moving piston supports.
        let last = BASE + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1);
        let mut removed = 0;
        for y in BASE.y..=last.y {
            for z in BASE.z..=last.z {
                for x in BASE.x..=last.x {
                    let pos = BlockPos::new(x, y, z);
                    if matches!(world.get_block(pos), Block::WallSign { .. }) {
                        world.set_block(pos, Block::Air);
                        removed += 1;
                    }
                }
            }
        }
        assert!(removed > 0, "expected labeled graph fixture");
    }
    world
}

fn adder_1bit_export(level: Option<usize>) -> Vec<u8> {
    let world = geometry_io_fixture("ADDER_1BIT.schem");
    exported_with_options(
        &world,
        &CompilerOptions {
            optimization_level: level,
            ..Default::default()
        },
    )
    .to_bytes()
    .unwrap()
}

fn adder_1bit_export_path(level: usize) -> std::path::PathBuf {
    let name = if level == 0 {
        "adder-1bit.graph.json".to_owned()
    } else {
        format!("adder-1bit-o{level}.graph.json")
    };
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/graph-fixtures/adder-1bit")
        .join(name)
}

fn site_at(graph: &Value, position: BlockPos) -> String {
    graph["graph"]["sites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|site| site["position"]["x"] == position.x && site["position"]["y"] == position.y && site["position"]["z"] == position.z)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn node_at(graph: &Value, position: BlockPos) -> String {
    graph["graph"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["position"]["x"] == position.x && node["position"]["y"] == position.y && node["position"]["z"] == position.z)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn export_is_deterministic_and_exposes_a_lever_to_lamp_power_path() {
    let mut world = empty();
    let lever = BASE;
    let lamp = BASE.offset(BlockFace::East);
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    );
    world.set_block(lamp, Block::RedstoneLamp { lit: true });
    let before = (world.get_block(lever), world.get_block(lamp));
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();

    let first = exported(&world);
    let second = exported(&world);
    let bytes = first.to_bytes().unwrap();
    assert_eq!(bytes, second.to_bytes().unwrap());
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["format"], "mchprs.redpiler.graph");
    assert_eq!(document["version"]["major"], 1);
    assert_eq!(document["stage"], "callback_draft");
    assert!(document["graph"]["routes"].is_array());
    assert!(document["graph"]["callback_links"].is_array());
    assert!(document["trace"].is_null());
    assert!(document["snapshots"][0]["execution"].is_null());

    let lever_site = site_at(&document, lever);
    let lamp_node = node_at(&document, lamp);
    assert!(
        document["graph"]["power_links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["from"]["site"] == lever_site && link["to"]["node"] == lamp_node)
    );
    assert_eq!((world.get_block(lever), world.get_block(lamp)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn fixed_classic_plan_exports_and_validates_real_executable_tables() {
    let mut world = empty();
    let source = BASE;
    let first_wire = source.offset(BlockFace::East);
    let second_wire = first_wire.offset(BlockFace::East);
    let repeater_pos = second_wire.offset(BlockFace::East);
    world.set_block(source, Block::RedstoneBlock {});
    world.set_block(first_wire, Block::RedstoneWire { wire: Default::default() });
    world.set_block(second_wire, Block::RedstoneWire { wire: Default::default() });
    world.set_block(
        repeater_pos,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                ..Default::default()
            },
        },
    );
    let options = CompilerOptions {
        optimization_level: Some(2),
        ..Default::default()
    };
    let export = exported_with_options(&world, &options);
    let document: Value = serde_json::from_slice(&export.to_bytes().unwrap()).unwrap();
    let plan = &document["graph"]["fixed_classic"];
    assert_eq!(plan["status"], "compiled");
    assert_eq!(
        plan["program"]["nodes"].as_array().unwrap().len(),
        document["graph"]["nodes"].as_array().unwrap().len()
    );
    assert_eq!(
        plan["program"]["wires"].as_array().unwrap().len(),
        document["graph"]["wire_plans"].as_array().unwrap().len()
    );
    assert_eq!(
        plan["program"]["route_ranges"].as_array().unwrap().len(),
        document["graph"]["routes"].as_array().unwrap().len()
    );
    assert_eq!(
        plan["inventory"]["table_payload_bytes"],
        plan["inventory"]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| table["payload_bytes"].as_u64().unwrap())
            .sum::<u64>()
    );
}

#[test]
fn export_preserves_guarded_shared_resource_references() {
    let mut world = empty();
    let first = BASE;
    let second = BASE + BlockPos::new(0, 0, 4);
    let payload = BASE + BlockPos::new(0, 0, 2);
    world.set_block(first, piston_block(BlockFace::South, true, false));
    world.set_block(second, piston_block(BlockFace::North, true, false));
    world.set_block(payload, Block::RedstoneBlock {});

    let export = exported(&world);
    let document: Value = serde_json::from_slice(&export.to_bytes().unwrap()).unwrap();
    let resources = document["graph"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["contacts"].as_array().unwrap().len(), 3);
    assert_eq!(document["snapshots"][0]["resources"].as_array().unwrap().len(), 1);
    assert!(
        document["graph"]["guards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guard| { guard["kind"] == "state" && guard["test"]["kind"] == "resource_at" })
    );
    assert!(document["graph"]["routes"].is_array());
    assert!(
        document["graph"]["guards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guard| guard["test"]["kind"] == "resource_at")
    );
}

#[test]
fn pinned_sticky_shared_graph_export_matches_the_pipeline_artifact() {
    let world = geometry_io_fixture("ADDER_1BIT.schem");
    let (plain_graph, plain_bytes) = prepared_graph_export(&world, false);
    let (optimized_graph, optimized_bytes) = prepared_graph_export(&world, true);
    assert_graph_reduced(&plain_graph, &optimized_graph);
    assert_export_matches_draft(&plain_bytes, &plain_graph);
    assert_export_matches_draft(&optimized_bytes, &optimized_graph);
    let monitor = TaskMonitor::default();
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let standalone = adapter::prepare(
        &world,
        world.get_corners(),
        &ticks,
        &CompilerOptions::default(),
        &monitor,
        AnalysisLimits::default(),
    )
    .unwrap();
    assert_eq!(plain_bytes, standalone.to_bytes().unwrap());
    assert_eq!(monitor.progress(), monitor.max_progress());

    let document: Value = serde_json::from_slice(&plain_bytes).unwrap();
    let graph = &document["graph"];
    assert!(
        graph["pistons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|piston| { piston["sticky"] == true && piston["lane"]["kind"] == "single_resource" })
    );
    assert!(
        graph["guards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guard| { guard["kind"] == "state" && guard["test"]["kind"] == "resource_at" })
    );
    assert!(!document["snapshots"][0]["resources"].as_array().unwrap().is_empty());
    for selection in document["snapshots"][0]["resources"].as_array().unwrap() {
        let resource = graph["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["id"] == selection["resource"])
            .unwrap();
        assert!(resource["contacts"].as_array().unwrap().contains(&selection["selected_contact"]));
    }

    assert!(graph["routes"].as_array().unwrap().iter().any(|route| {
        let deliveries = route["deliveries"].as_array().unwrap();
        deliveries.iter().enumerate().all(|(index, delivery)| delivery["sequence"] == index)
            && deliveries
                .iter()
                .enumerate()
                .any(|(index, delivery)| deliveries[..index].iter().any(|earlier| earlier["target"] == delivery["target"]))
    }));
}

#[test]
fn ordinary_comparator_and_dust_graph_is_complete_in_both_modes_and_exports_from_itself() {
    let mut world = empty();
    let comparator_pos = BASE;
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    world.set_block(comparator_pos, Block::RedstoneComparator { comparator });
    world.set_block_entity(comparator_pos, BlockEntity::Comparator { output_strength: 6 });
    let wire = BASE + BlockPos::new(8, 0, 0);
    let conductor = wire.offset(BlockFace::East);
    let lamp = conductor.offset(BlockFace::East);
    world.set_block(wire, Block::RedstoneWire { wire: Default::default() });
    world.set_block(conductor, Block::Stone {});
    world.set_block(lamp, Block::RedstoneLamp { lit: false });

    let (plain_graph, plain_bytes) = prepared_graph_export(&world, false);
    let (optimized_graph, optimized_bytes) = prepared_graph_export(&world, true);
    assert_graph_reduced(&plain_graph, &optimized_graph);
    assert_export_matches_draft(&plain_bytes, &plain_graph);
    assert_export_matches_draft(&optimized_bytes, &optimized_graph);
    assert_eq!(plain_bytes, exported(&world).to_bytes().unwrap());
    assert!(!plain_graph.wires.is_empty());
    assert!(!plain_graph.wire_relations.is_empty());
    assert!(!plain_graph.comparator_inputs.is_empty());

    let document: Value = serde_json::from_slice(&plain_bytes).unwrap();
    assert!(!document["graph"]["wire_plans"].as_array().unwrap().is_empty());
    assert!(!document["graph"]["wire_relations"].as_array().unwrap().is_empty());
    assert!(!document["graph"]["comparator_plans"].as_array().unwrap().is_empty());
    let comparator_node = node_at(&document, comparator_pos);
    assert!(
        document["snapshots"][0]["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|component| { component["node"] == comparator_node && component["state"]["output_strength"] == 6 })
    );
}

fn assert_graph_reduced(plain: &CompileGraph, optimized: &CompileGraph) {
    plain.validate().unwrap();
    optimized.validate().unwrap();
    let deliveries = |graph: &CompileGraph| graph.routes.iter().map(|route| route.deliveries.len()).sum::<usize>();
    assert!(optimized.guards.len() <= plain.guards.len());
    assert!(optimized.links.len() <= plain.links.len());
    assert!(optimized.power_links.len() <= plain.power_links.len());
    assert!(deliveries(optimized) <= deliveries(plain));
    assert!(
        optimized.guards.len() < plain.guards.len()
            || optimized.links.len() < plain.links.len()
            || optimized.power_links.len() < plain.power_links.len()
            || deliveries(optimized) < deliveries(plain),
        "the fixture should exercise at least one certified reduction"
    );
}

fn assert_export_matches_draft(bytes: &[u8], graph: &CompileGraph) {
    let document: Value = serde_json::from_slice(bytes).unwrap();
    let exported = &document["graph"];
    assert_eq!(exported["guards"].as_array().unwrap().len(), graph.guards.len());
    assert_eq!(exported["callback_links"].as_array().unwrap().len(), graph.links.len());
    assert_eq!(exported["power_links"].as_array().unwrap().len(), graph.power_links.len());
    let routes = exported["routes"].as_array().unwrap();
    assert_eq!(routes.len(), graph.routes.len());
    for (route, draft) in routes.iter().zip(&graph.routes) {
        let deliveries = route["deliveries"].as_array().unwrap();
        assert_eq!(deliveries.len(), draft.deliveries.len());
        for (ordinal, (delivery, draft)) in deliveries.iter().zip(draft.deliveries.iter()).enumerate() {
            assert_eq!(delivery["ordinal"].as_u64(), Some(ordinal as u64));
            assert_eq!(delivery["sequence"].as_u64(), Some(draft.sequence.into()));
        }
    }
}

#[test]
fn export_keeps_quasi_and_dust_conductor_policies_separate() {
    let mut qc_world = empty();
    qc_world.set_block(BASE, piston_block(BlockFace::South, true, false));
    let front = BASE.offset(BlockFace::South);
    let qc_source = BASE.offset(BlockFace::Top).offset(BlockFace::East);
    let normal_source = BASE.offset(BlockFace::West);
    qc_world.set_block(front, Block::RedstoneBlock {});
    qc_world.set_block(qc_source, Block::RedstoneBlock {});
    qc_world.set_block(normal_source, Block::RedstoneBlock {});
    let qc = exported(&qc_world);
    let qc_json: Value = serde_json::from_slice(&qc.to_bytes().unwrap()).unwrap();
    let qc_node = node_at(&qc_json, BASE);
    let qc_site = site_at(&qc_json, qc_source);
    let normal_site = site_at(&qc_json, normal_source);
    assert!(
        qc_json["graph"]["power_links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| { link["from"]["site"] == qc_site && link["to"]["node"] == qc_node && link["to"]["port"] == "quasi" })
    );
    assert!(
        qc_json["graph"]["power_links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| { link["from"]["site"] == normal_site && link["to"]["node"] == qc_node && link["to"]["port"] == "main" })
    );

    let mut dust_world = empty();
    let wire = BASE;
    let conductor = wire.offset(BlockFace::East);
    let lamp = conductor.offset(BlockFace::East);
    dust_world.set_block(wire, Block::RedstoneWire { wire: Default::default() });
    dust_world.set_block(conductor, Block::Stone {});
    dust_world.set_block(lamp, Block::RedstoneLamp { lit: false });
    let dust = exported(&dust_world);
    let dust_json: Value = serde_json::from_slice(&dust.to_bytes().unwrap()).unwrap();
    let conductor_site = site_at(&dust_json, conductor);
    let wire_node = node_at(&dust_json, wire);
    let lamp_node = node_at(&dust_json, lamp);
    let links = dust_json["graph"]["power_links"].as_array().unwrap();
    assert!(links.iter().any(|link| {
        link["from"]["site"] == conductor_site
            && link["to"]["node"] == wire_node
            && link["from"]["port"]["kind"] == "conducted"
            && link["from"]["port"]["include_dust"] == false
    }));
    assert!(links.iter().any(|link| {
        link["from"]["site"] == conductor_site
            && link["to"]["node"] == lamp_node
            && link["from"]["port"]["kind"] == "conducted"
            && link["from"]["port"]["include_dust"] == true
    }));
}

#[test]
fn export_keeps_fixed_and_live_comparator_overrides_distinct() {
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);

    let mut fixed_world = empty();
    fixed_world.set_block(BASE, Block::RedstoneComparator { comparator });
    fixed_world.set_block(BASE.offset(BlockFace::South), Block::Cauldron { level: 3 });
    let fixed = exported(&fixed_world);
    let fixed_json: Value = serde_json::from_slice(&fixed.to_bytes().unwrap()).unwrap();
    assert!(
        fixed_json["graph"]["comparator_plans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|plan| plan["direct_override"]["kind"] == "literal" && plan["direct_override"]["value"] == 3)
    );

    let mut live_world = empty();
    live_world.set_block(BASE, Block::RedstoneComparator { comparator });
    live_world.set_block(BASE.offset(BlockFace::South), Block::Stone {});
    live_world.set_block(BASE + BlockPos::new(0, 0, 2), Block::from_name("copper_bulb").unwrap());
    let live = exported(&live_world);
    let live_json: Value = serde_json::from_slice(&live.to_bytes().unwrap()).unwrap();
    assert!(
        live_json["graph"]["comparator_plans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|plan| plan["far_override"]["value"]["kind"] == "live_component_output")
    );
}

#[test]
fn failed_preparation_creates_no_export_and_preserves_world_and_ticks() {
    let mut world = empty();
    world.set_block(BASE, Block::Stone {});
    let before = world.get_block(BASE);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let directory = std::env::temp_dir().join(format!(
        "mchprs-graph-export-failed-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    let error = adapter::prepare_and_write(
        &world,
        (BASE, BlockPos::new(BASE.x, -1, BASE.z)),
        &ticks,
        &CompilerOptions::default(),
        &crate::redpiler::TaskMonitor::default(),
        AnalysisLimits::default(),
        &directory,
        0,
        0,
    )
    .unwrap_err();
    assert_eq!(error.code, "analysis");
    assert!(!directory.exists());
    assert_eq!(world.get_block(BASE), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn temp_publication_does_not_replace_an_existing_export() {
    let directory = std::env::temp_dir().join(format!(
        "mchprs-graph-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let destination = directory.join("review.json");
    adapter::publish_temp_file(&directory, &destination, "first", b"original").unwrap();
    let error = adapter::publish_temp_file(&directory, &destination, "second", b"replacement").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&destination).unwrap(), b"original");
    std::fs::remove_dir_all(directory).unwrap();
}
