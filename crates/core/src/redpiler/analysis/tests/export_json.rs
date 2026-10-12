use super::*;
use crate::redpiler::analysis::export_json as adapter;
use crate::world::{BlockAction, World};
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::{Block, ComparatorMode, Lever, LeverFace, RedstoneComparator};
use mchprs_world::TickPriority;
use serde_json::Value;
use std::cell::Cell;

fn exported(world: &PlotWorld) -> adapter::GraphExport {
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    adapter::prepare(
        world,
        world.get_corners(),
        &ticks,
        &crate::redpiler::TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
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
            self.reads_after_cancel
                .set(self.reads_after_cancel.get() + 1);
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
    let mut inner = PlotWorld::from_chunks(
        0,
        0,
        vec![crate::world::storage::Chunk::empty(0, 0)],
        Default::default(),
    );
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

    let directory = std::env::temp_dir().join(format!(
        "mchprs-redpiler-cancel-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let result = adapter::prepare_and_write(
        &world,
        bounds,
        &[],
        &monitor,
        AnalysisLimits::default(),
        &directory,
        0,
        0,
    );
    let cancelled = result
        .as_ref()
        .err()
        .is_some_and(|error| error.code == "cancelled");
    let published_files = std::fs::read_dir(&directory).unwrap().count();
    std::fs::remove_dir_all(&directory).unwrap();

    assert!(
        cancelled,
        "cancelled preparation must not publish an export"
    );
    assert_eq!(published_files, 0);
    assert!(world.reads_after_cancel.get() < 64);
}

#[test]
fn electrical_graph_expansion_obeys_the_preparation_budget() {
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        vec![crate::world::storage::Chunk::empty(0, 0)],
        Default::default(),
    );
    let lamp = BlockPos::new(5, 20, 5);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });

    let result = adapter::prepare(
        &world,
        (lamp, lamp),
        &[],
        &TaskMonitor::default(),
        AnalysisLimits {
            max_cells: 1,
            ..Default::default()
        },
    );

    assert!(
        result.is_err(),
        "the expanded electrical graph exceeds one cell of preparation budget"
    );
}

#[test]
fn callback_preparation_stops_reading_after_cancellation() {
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
    let mut inner = PlotWorld::from_chunks(
        0,
        0,
        vec![crate::world::storage::Chunk::empty(0, 0)],
        Default::default(),
    );
    for index in 0..6 {
        let lever = BlockPos::new(1 + index * 2, 20, 2);
        inner.set_block(
            lever.offset(mchprs_blocks::BlockFace::Bottom),
            Block::Stone {},
        );
        inner.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, Default::default(), false),
            },
        );
        inner.set_block(
            lever.offset(mchprs_blocks::BlockFace::East),
            Block::RedstoneLamp { lit: false },
        );
    }
    let preparation_monitor = TaskMonitor::default();
    let geometry = super::super::geometry::prepare_geometry(
        &inner,
        bounds,
        &[],
        &preparation_monitor,
        AnalysisLimits::default(),
    )
    .unwrap();
    let mut electrical =
        super::super::electrical::prepare_electrical(&inner, bounds, &geometry.draft, &preparation_monitor).unwrap();
    let monitor = TaskMonitor::default();
    let world = CancelAtReadWorld {
        inner,
        monitor: &monitor,
        cancel_on_read: 1,
        raw_reads: Cell::new(0),
        reads_after_cancel: Cell::new(0),
    };

    let result = super::super::callbacks::prepare_callbacks(&world, &geometry.draft, &mut electrical, &monitor);

    assert!(matches!(result, Err(super::super::callbacks::CallbackError::Preparation(AnalysisError::Cancelled))));
    assert!(world.reads_after_cancel.get() <= 1, "only the read that triggers cancellation may finish");
}

#[test]
fn adder_1bit_fixture_matches_graph_json() {
    let bytes = adder_1bit_export();
    assert_eq!(bytes, std::fs::read(adder_1bit_export_path()).unwrap());
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["version"]["major"], 1);
    assert_eq!(document["stage"], "electrical_draft");
    assert!(document["graph"]["routes"].is_null());
    assert!(document["trace"].is_null());
}

#[test]
#[ignore = "regenerates the checked-in ADDER_1BIT graph JSON fixture"]
fn write_adder_1bit_graph_json_fixture() {
    std::fs::create_dir_all(adder_1bit_export_path().parent().unwrap()).unwrap();
    std::fs::write(adder_1bit_export_path(), adder_1bit_export()).unwrap();
}

fn adder_1bit_export() -> Vec<u8> {
    let world = geometry_io_fixture("ADDER_1BIT.schem");
    exported(&world).to_bytes().unwrap()
}

fn adder_1bit_export_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json")
}

fn site_at(graph: &Value, position: BlockPos) -> String {
    graph["graph"]["sites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|site| {
            site["position"]["x"] == position.x
                && site["position"]["y"] == position.y
                && site["position"]["z"] == position.z
        })
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
        .find(|node| {
            node["position"]["x"] == position.x
                && node["position"]["y"] == position.y
                && node["position"]["z"] == position.z
        })
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
    assert_eq!(document["stage"], "electrical_draft");
    assert!(document["graph"]["routes"].is_null());
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
    assert_eq!(
        document["snapshots"][0]["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        document["graph"]["guards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guard| { guard["kind"] == "state" && guard["test"]["kind"] == "resource_at" })
    );
    assert!(document["graph"]["routes"].is_null());
    assert!(
        document["graph"]["guards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|guard| guard["test"]["kind"] == "resource_at")
    );
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
            .any(|link| {
                link["from"]["site"] == qc_site
                    && link["to"]["node"] == qc_node
                    && link["to"]["port"] == "quasi"
            })
    );
    assert!(
        qc_json["graph"]["power_links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| {
                link["from"]["site"] == normal_site
                    && link["to"]["node"] == qc_node
                    && link["to"]["port"] == "main"
            })
    );

    let mut dust_world = empty();
    let wire = BASE;
    let conductor = wire.offset(BlockFace::East);
    let lamp = conductor.offset(BlockFace::East);
    dust_world.set_block(
        wire,
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
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
            .any(|plan| plan["direct_override"]["kind"] == "literal"
                && plan["direct_override"]["value"] == 3)
    );

    let mut live_world = empty();
    live_world.set_block(BASE, Block::RedstoneComparator { comparator });
    live_world.set_block(BASE.offset(BlockFace::South), Block::Stone {});
    live_world.set_block(
        BASE + BlockPos::new(0, 0, 2),
        Block::from_name("copper_bulb").unwrap(),
    );
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
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let error = adapter::prepare_and_write(
        &world,
        (BASE, BlockPos::new(BASE.x, -1, BASE.z)),
        &ticks,
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
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let destination = directory.join("review.json");
    adapter::publish_temp_file(&directory, &destination, "first", b"original").unwrap();
    let error =
        adapter::publish_temp_file(&directory, &destination, "second", b"replacement").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&destination).unwrap(), b"original");
    std::fs::remove_dir_all(directory).unwrap();
}
