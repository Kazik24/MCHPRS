use super::*;
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::{
    BlockEntity, CommandBlockEntity, ContainerType, InventoryEntry,
};
use mchprs_blocks::blocks::Block;
use mchprs_save_data::plot_data::{ChunkData, WorldSendRate};
use mchprs_world::{PistonAction, PistonEvent, TickEntry, TickPriority};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

struct TempRoot(PathBuf);
impl TempRoot {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mchprs-git-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join("p-1,2/repository.sqlite")).unwrap()
    }
    fn repo(&self) -> Repository {
        Repository::open(&self.0, (-1, 2), test_limits()).unwrap()
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn test_limits() -> Limits {
    Limits {
        snapshot: 16 * 1048576,
        plot_bytes: 64 * 1048576,
        total_bytes: 128 * 1048576,
    }
}
fn empty() -> Snapshot {
    Snapshot {
        version: 1,
        data_version: mchprs_save_data::plot_data::MC_DATA_VERSION,
        plot: (-1, 2),
        data: PlotData {
            tps: Tps::Limited(20),
            world_send_rate: WorldSendRate(20),
            piston_animation: Default::default(),
            chunk_data: vec![
                ChunkData {
                    sections: std::array::from_fn(|_| None),
                    block_entities: Default::default()
                };
                super::super::NUM_CHUNKS
            ],
            pending_ticks: Vec::new(),
            piston_state: Default::default(),
        },
    }
}
fn pos(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(-256 + x, y, 512 + z)
}
fn set(snapshot: &mut Snapshot, x: i32, y: i32, z: i32, block: Block, entity: Option<BlockEntity>) {
    let index = (x / 16 * super::super::PLOT_WIDTH + z / 16) as usize;
    let mut chunk = Chunk::load(
        -16 + x / 16,
        32 + z / 16,
        snapshot.data.chunk_data[index].clone(),
    );
    chunk.set_block((x & 15) as u32, y as u32, (z & 15) as u32, block.get_id());
    if let Some(entity) = entity {
        chunk.set_block_entity(BlockPos::new(x & 15, y, z & 15), entity);
    }
    snapshot.data.chunk_data[index] = chunk.save();
}
fn compare(a: Snapshot, b: Snapshot) -> Diff {
    Diff::new(
        "a".repeat(64),
        "b".repeat(64),
        a,
        b,
        Reservation::new(0).unwrap(),
    )
    .unwrap()
}
fn command_entity(command: &str) -> BlockEntity {
    BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
        command: command.into(),
        ..Default::default()
    }))
}

#[test]
fn persistent_commits_branch_divergence_and_ordered_search() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    assert!(repo.resolve("HEAD").is_err());
    assert_eq!(repo.sidebar_head().unwrap(), "");
    repo.commit(&snapshot, 42, "Alice", "root build").unwrap();
    assert_eq!(repo.sidebar_head().unwrap(), "main");
    let first = repo.resolve("HEAD").unwrap();
    repo.branch("experiment", "HEAD").unwrap();
    assert_eq!(repo.head().unwrap().0, "main");
    set(&mut snapshot, 3, 64, 5, Block::Stone {}, None);
    repo.commit(&snapshot, 42, "Alice", "improved main")
        .unwrap();
    let main = repo.resolve("main").unwrap();
    assert_ne!(main, first);
    assert_eq!(repo.resolve(&first[..8]).unwrap(), first);
    let (mut branch, _, _reservation) = repo
        .checkout("experiment", &snapshot, 42, "Alice", &root.0.join("plot"))
        .unwrap();
    assert_eq!(branch.block(pos(3, 64, 5)), 0);
    set(&mut branch, 4, 64, 5, Block::Glass, None);
    repo.commit(&branch, 99, "Bob", "divergent experiment")
        .unwrap();
    let experiment = repo.resolve("HEAD").unwrap();
    assert_eq!(repo.resolve("main").unwrap(), main);
    let log = repo.log(false, None, 1).unwrap().to_string();
    assert!(log.contains("divergent experiment") && log.contains("root build"));
    assert!(!log.contains("improved main"));
    assert!(repo
        .log(true, Some("IMPROVED"), 1)
        .unwrap()
        .to_string()
        .contains("improved main"));
    assert!(repo.show(&experiment).unwrap().contains("Bob"));
    drop(repo);
    let repo = root.repo();
    assert_eq!(repo.sidebar_head().unwrap(), "experiment");
    assert_eq!(
        repo.head().unwrap(),
        ("experiment".into(), Some(experiment.clone()))
    );
    assert_eq!(
        repo.load(&experiment).unwrap().block(pos(4, 64, 5)),
        Block::Glass.get_id()
    );
}

#[test]
fn checkout_preserves_dirty_work_in_a_recoverable_branch() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    repo.branch("other", "HEAD").unwrap();
    set(
        &mut snapshot,
        8,
        64,
        9,
        Block::from_name("command_block").unwrap(),
        Some(command_entity("say unfinished")),
    );
    snapshot.data.pending_ticks.push(TickEntry {
        pos: pos(8, 64, 9),
        ticks_left: 2,
        tick_priority: TickPriority::High,
        block_type: None,
    });
    let dirty = snapshot.fingerprints().unwrap().full;
    let (restored, message, _reservation) = repo
        .checkout("other", &snapshot, 1, "Alice", &root.0.join("plot"))
        .unwrap();
    assert_eq!(restored.data.tps, Tps::Limited(0));
    assert_eq!(restored.data.world_send_rate, snapshot.data.world_send_rate);
    assert!(message.contains("Recovery:"));
    assert_eq!(restored.block(pos(8, 64, 9)), 0);
    let recovery: String = root
        .db()
        .query_row("SELECT id FROM recoveries", [], |r| r.get(0))
        .unwrap();
    repo.recover_branch(&recovery[..8], "unfinished", 1, "Alice")
        .unwrap();
    let recovered = repo.load(&repo.resolve("unfinished").unwrap()).unwrap();
    assert_eq!(recovered.fingerprints().unwrap().full, dirty);
    assert_eq!(recovered.data.pending_ticks, snapshot.data.pending_ticks);
    assert!(repo.recoveries(1).unwrap().to_string().contains("Alice"));
    assert!(repo
        .checkout("other", &snapshot, 1, "Alice", &root.0.join("plot"))
        .is_err());
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM recoveries", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn failed_checkout_restores_save_and_branch_after_target_write() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    repo.branch("other", "HEAD").unwrap();
    set(&mut snapshot, 1, 60, 1, Block::Stone {}, None);
    let save = root.0.join("plot");
    snapshot.data.save_to_file(&save).unwrap();
    root.db().execute_batch("CREATE TRIGGER reject_checkout BEFORE UPDATE ON meta WHEN NEW.key='active' BEGIN SELECT RAISE(ABORT,'injected metadata failure'); END;").unwrap();
    assert!(repo
        .checkout("other", &snapshot, 1, "Alice", &save)
        .is_err());
    assert_eq!(repo.head().unwrap().0, "main");
    assert!(!repo.has_pending().unwrap());
    let loaded = PlotData::<{ super::super::PLOT_SECTIONS }>::load_from_file(&save, false).unwrap();
    assert_eq!(
        bincode::serialize(&loaded).unwrap(),
        bincode::serialize(&snapshot.data).unwrap()
    );
}

#[test]
fn interrupted_checkout_completes_on_reopen_before_or_after_target_save() {
    for target_written in [false, true] {
        let root = TempRoot::new();
        let mut repo = root.repo();
        let base = empty();
        repo.commit(&base, 1, "Alice", "base").unwrap();
        let old = repo.resolve("HEAD").unwrap();
        repo.branch("other", "HEAD").unwrap();
        let mut snapshot = base.clone();
        set(&mut snapshot, 2, 60, 2, Block::Stone {}, None);
        repo.commit(&snapshot, 1, "Alice", "main").unwrap();
        let save = root.0.join("plot");
        if target_written {
            base.data.save_to_file(&save).unwrap();
        } else {
            snapshot.data.save_to_file(&save).unwrap();
        }
        root.db()
            .execute(
                "INSERT INTO checkout SELECT 1,'other',?1,snapshot FROM commits WHERE id=?2",
                rusqlite::params![old, repo.resolve("HEAD").unwrap()],
            )
            .unwrap();
        drop(repo);
        let mut repo = root.repo();
        assert!(repo.has_pending().unwrap());
        let target = repo.finish_checkout(&save).unwrap();
        assert_eq!(target.block(pos(2, 60, 2)), 0);
        assert_eq!(repo.head().unwrap().0, "other");
        assert!(!repo.has_pending().unwrap());
        assert_eq!(
            PlotData::<{ super::super::PLOT_SECTIONS }>::load_from_file(save, false)
                .unwrap()
                .tps,
            Tps::Limited(0)
        );
    }
}

#[test]
fn rollback_io_failure_keeps_durable_checkout_record() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    repo.branch("other", "HEAD").unwrap();
    set(&mut snapshot, 3, 60, 3, Block::Stone {}, None);
    fs::write(root.0.join("blocked"), "file").unwrap();
    assert!(repo
        .checkout("other", &snapshot, 1, "Alice", &root.0.join("blocked/plot"))
        .is_err());
    assert!(repo.has_pending().unwrap());
    assert_eq!(repo.head().unwrap().0, "main");
    drop(repo);
    let mut repo = root.repo();
    repo.finish_checkout(&root.0.join("plot")).unwrap();
    assert_eq!(repo.head().unwrap().0, "other");
    assert!(!repo.has_pending().unwrap());
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM recoveries", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn quota_and_transaction_failures_never_advance_head_or_leave_objects() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    let head = repo.head().unwrap();
    set(&mut snapshot, 2, 50, 2, Block::Stone {}, None);
    root.db().execute_batch("CREATE TRIGGER reject_commit BEFORE INSERT ON commits BEGIN SELECT RAISE(ABORT,'injected commit failure'); END;").unwrap();
    assert!(repo.commit(&snapshot, 1, "Alice", "failed").is_err());
    assert_eq!(repo.head().unwrap(), head);
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM objects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    root.db()
        .execute_batch("DROP TRIGGER reject_commit")
        .unwrap();
    drop(repo);
    let mut quota = test_limits();
    quota.plot_bytes = 1;
    let mut repo = Repository::open(&root.0, (-1, 2), quota).unwrap();
    assert!(repo
        .commit(&snapshot, 1, "Alice", "over quota")
        .unwrap_err()
        .to_string()
        .contains("quota"));
    assert_eq!(repo.head().unwrap(), head);
    assert!(repo.branch("../escape", "HEAD").is_err());
}

#[test]
fn canonical_hashes_ignore_palette_layout_inventory_order_and_nbt_key_order() {
    let mut a = empty();
    let mut b = empty();
    let mut section = mchprs_save_data::plot_data::ChunkSectionData {
        data: vec![0; 256],
        palette: vec![0],
        bits_per_block: 4,
        block_count: 0,
        entries: 4096,
    };
    b.data.chunk_data[0].sections[0] = Some(section.clone());
    assert_eq!(
        a.fingerprints().unwrap().full,
        b.fingerprints().unwrap().full
    );
    section.palette = vec![Block::Stone {}.get_id() as i32, 0];
    section.data.fill(0x1111111111111111);
    b.data.chunk_data[0].sections[0] = Some(section);
    assert_eq!(
        a.fingerprints().unwrap().full,
        b.fingerprints().unwrap().full
    );
    let tag = |reverse: bool| {
        let mut blob = nbt::Blob::new();
        for (k, v) in if reverse {
            [("z", 2), ("a", 1)]
        } else {
            [("a", 1), ("z", 2)]
        } {
            blob.insert(k, nbt::Value::Int(v)).unwrap();
        }
        let mut bytes = Vec::new();
        blob.to_writer(&mut bytes).unwrap();
        bytes
    };
    let inventory = |reverse: bool| {
        let mut entries = vec![
            InventoryEntry {
                id: 1,
                count: 1,
                slot: 0,
                nbt: Some(tag(reverse)),
            },
            InventoryEntry {
                id: 1,
                count: 2,
                slot: 1,
                nbt: None,
            },
        ];
        if reverse {
            entries.reverse();
        }
        BlockEntity::Container {
            inventory: entries.into_iter().collect(),
            ty: ContainerType::Chest,
            comparator_override: 1,
        }
    };
    set(
        &mut a,
        4,
        60,
        4,
        Block::from_name("chest").unwrap(),
        Some(inventory(false)),
    );
    set(
        &mut b,
        4,
        60,
        4,
        Block::from_name("chest").unwrap(),
        Some(inventory(true)),
    );
    assert_eq!(
        a.fingerprints().unwrap().content,
        b.fingerprints().unwrap().content
    );
    b.data.tps = Tps::Unlimited;
    b.data.world_send_rate = WorldSendRate(9);
    assert_eq!(
        a.fingerprints().unwrap().full,
        b.fingerprints().unwrap().full
    );
}

#[test]
fn validation_and_checksums_reject_corruption_without_changing_history() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let snapshot = empty();
    let encoded = snapshot.encode(test_limits().snapshot).unwrap();
    assert!(Snapshot::decode(&encoded, (0, 0), test_limits().snapshot).is_err());
    assert!(Snapshot::decode(&encoded, (-1, 2), 1).is_err());
    let mut invalid = snapshot.clone();
    invalid.data.chunk_data.pop();
    assert!(invalid.validate((-1, 2)).is_err());
    invalid = snapshot.clone();
    invalid.data.chunk_data[0].sections[0] = Some(mchprs_save_data::plot_data::ChunkSectionData {
        data: vec![],
        palette: vec![0],
        bits_per_block: 0,
        block_count: 0,
        entries: 4096,
    });
    assert!(invalid.fingerprints().is_err());
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    let head = repo.resolve("HEAD").unwrap();
    let mut changed = snapshot.clone();
    set(&mut changed, 1, 1, 1, Block::Stone {}, None);
    root.db()
        .execute(
            "UPDATE objects SET blob=?1",
            [changed.encode(test_limits().snapshot).unwrap()],
        )
        .unwrap();
    assert!(repo
        .load(&head)
        .unwrap_err()
        .to_string()
        .contains("checksum"));
    assert_eq!(repo.resolve("HEAD").unwrap(), head);
}

#[test]
fn diff_classifies_add_remove_state_and_data_and_inspects_removed_positions() {
    let mut a = empty();
    let mut b = empty();
    set(&mut a, 2, 64, 3, Block::Stone {}, None);
    set(&mut a, 3, 64, 3, Block::Stone {}, None);
    set(&mut b, 3, 64, 3, Block::Glass, None);
    set(
        &mut a,
        4,
        64,
        3,
        Block::from_name("command_block").unwrap(),
        Some(command_entity("say before")),
    );
    set(
        &mut b,
        4,
        64,
        3,
        Block::from_name("command_block").unwrap(),
        Some(command_entity("say after")),
    );
    set(&mut b, 1, 64, 3, Block::Stone {}, None);
    b.data.piston_state.logical_tick = 1;
    let diff = compare(a, b);
    assert_eq!(diff.counts, [1, 1, 1, 1]);
    let summary = diff.summary().to_string();
    assert!(summary.contains("2 changed") && summary.contains("execution changed"));
    assert!(!summary.contains("Page"));
    assert_eq!(
        diff.summary()["extra"][4]["click_event"]["command"],
        "/git diff show"
    );
    let removed = diff.inspect(pos(2, 64, 3), None).unwrap().to_string();
    assert!(removed.contains("From: stone") && removed.contains("To: air"));
    let inspected = diff.inspect(pos(4, 64, 3), None).unwrap();
    let text = inspected["text"].as_str().unwrap();
    assert!(text.contains("say before") && text.contains("say after"));
    assert!(text.contains("From data:") && text.contains("To data:"));
    assert!(inspected.get("extra").is_none());
    assert!(diff
        .inspect(pos(4, 64, 3), Some("from"))
        .unwrap()
        .to_string()
        .contains("say before"));
    assert!(diff
        .inspect(pos(4, 64, 3), Some("to"))
        .unwrap()
        .to_string()
        .contains("say after"));
    assert!(diff.inspect(pos(8, 64, 3), None).is_err());
    assert!(diff
        .inspect(BlockPos::new(i32::MAX, 64, i32::MIN), None)
        .is_err());
}

#[test]
fn nearby_markers_are_capped_sorted_and_include_deleted_blocks() {
    let mut a = empty();
    let mut b = empty();
    for z in 1..100 {
        set(&mut b, 8, 64, z, Block::Stone {}, None);
    }
    set(&mut a, 8, 64, 0, Block::Stone {}, None);
    let diff = compare(a, b);
    let center = PlayerPos::new(-247.5, 64.5, 512.5);
    let nearby = diff.near(center, 20.0, 8).unwrap();
    assert_eq!(nearby.len(), 8);
    assert_eq!(
        nearby[0],
        Marker {
            pos: pos(8, 64, 0),
            kind: 1
        }
    );
    assert_eq!(nearby[7].pos, pos(8, 64, 7));
    assert!(diff.near(center, 20.0, 0).unwrap().is_empty());
    let eye = PlayerPos::new(-247.5, 64.5, 510.5);
    assert_eq!(
        diff::aimed(eye, 0.0, 0.0, nearby.into_iter(), 64.0),
        Some(pos(8, 64, 0))
    );
    assert!(diff::aimed(eye, f32::NAN, 0.0, std::iter::empty(), 64.0).is_none());
}

#[test]
fn snapshots_restore_scheduled_ticks_piston_events_and_entities_exactly() {
    let mut snapshot = empty();
    set(
        &mut snapshot,
        8,
        64,
        8,
        Block::from_name("command_block").unwrap(),
        Some(command_entity("say saved")),
    );
    snapshot.data.pending_ticks = vec![
        TickEntry {
            ticks_left: 2,
            tick_priority: TickPriority::Highest,
            pos: pos(8, 64, 8),
            block_type: None,
        },
        TickEntry {
            ticks_left: 2,
            tick_priority: TickPriority::High,
            pos: pos(9, 64, 8),
            block_type: None,
        },
    ];
    snapshot.data.piston_state.logical_tick = 42;
    snapshot.data.piston_state.events.push_back(PistonEvent {
        pos: pos(10, 64, 8),
        sticky: true,
        facing: mchprs_blocks::BlockFace::Top,
        action: PistonAction::Extend,
    });
    let loaded = Snapshot::decode(
        &snapshot.encode(test_limits().snapshot).unwrap(),
        snapshot.plot,
        test_limits().snapshot,
    )
    .unwrap();
    assert_eq!(
        bincode::serialize(&loaded).unwrap(),
        bincode::serialize(&snapshot).unwrap()
    );
    let root = TempRoot::new();
    let mut repo = root.repo();
    repo.commit(&snapshot, 1, "Alice", "runtime").unwrap();
    assert_eq!(
        repo.load(&repo.resolve("HEAD").unwrap())
            .unwrap()
            .fingerprints()
            .unwrap()
            .full,
        snapshot.fingerprints().unwrap().full
    );
}

#[test]
fn streamed_execution_hash_keeps_existing_object_ids() {
    let mut snapshot = empty();
    snapshot.data.pending_ticks = (0..2000)
        .map(|n| TickEntry {
            ticks_left: 2,
            tick_priority: TickPriority::High,
            pos: pos(n % 256, 64, n / 256),
            block_type: None,
        })
        .collect();
    let state = &mut snapshot.data.piston_state;
    state.logical_tick = 42;
    state.next_identity = 1;
    state.events.push_back(PistonEvent {
        pos: pos(10, 64, 8),
        sticky: true,
        facing: mchprs_blocks::BlockFace::Top,
        action: PistonAction::Extend,
    });
    let mut tag = nbt::Blob::new();
    tag.insert("data", nbt::Value::ByteArray(vec![-1, 0, 1]))
        .unwrap();
    let mut nbt = Vec::new();
    tag.to_writer(&mut nbt).unwrap();
    state.motions.push_back(mchprs_world::PistonMotion {
        pos: pos(10, 64, 8),
        identity: 1,
        progress: 0.1,
        previous_progress: 0.05,
        last_tick: 40,
        carried_entity: Some(Box::new(BlockEntity::Container {
            ty: ContainerType::Chest,
            comparator_override: 1,
            inventory: [InventoryEntry {
                id: 1,
                slot: 0,
                count: 1,
                nbt: Some(nbt),
            }]
            .into_iter()
            .collect(),
        })),
    });
    state.phase = mchprs_world::AdvancePhase::MovingEntities;
    state.movement_work.push((pos(10, 64, 8), 1));
    for scheduled in [false, true] {
        snapshot.data.piston_state.scheduled_advanced = scheduled;
        let mut legacy =
            serde_json::to_value((&snapshot.data.pending_ticks, &snapshot.data.piston_state))
                .unwrap();
        snapshot::normalize(&mut legacy).unwrap();
        let expected = snapshot::hex(Sha256::digest(serde_json::to_vec(&legacy).unwrap()));
        assert_eq!(snapshot.fingerprints().unwrap().execution, expected);
    }
}

#[test]
fn malformed_lengths_are_rejected_before_snapshot_and_blob_allocations() {
    for tps in [Tps::Limited(20), Tps::Unlimited] {
        let mut snapshot = empty();
        snapshot.data.tps = tps;
        let mut bytes = bincode::serialize(&snapshot).unwrap();
        let offset = if tps == Tps::Unlimited { 24 } else { 28 };
        bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let packed = lz4_flex::compress_prepend_size(&bytes);
        assert!(
            Snapshot::decode(&packed, snapshot.plot, test_limits().snapshot)
                .unwrap_err()
                .to_string()
                .contains("plot size")
        );
    }
    let root = TempRoot::new();
    let mut repo = root.repo();
    repo.commit(&empty(), 1, "Alice", "base").unwrap();
    let head = repo.head().unwrap();
    let mut forged = vec![0; 1024 * 1024];
    forged[..4].copy_from_slice(&32u32.to_le_bytes());
    root.db()
        .execute("UPDATE objects SET blob=?1", [forged])
        .unwrap();
    let error = repo.load(head.1.as_ref().unwrap()).unwrap_err();
    assert!(error.to_string().contains("oversized"));
    assert_eq!(repo.head().unwrap(), head);
    assert!(Reservation::snapshot(mib(MAX_SNAPSHOT_MIB)).is_err());
    assert!(Reservation::snapshot(usize::MAX).is_err());
    assert!(limits().snapshot <= mib(MAX_SNAPSHOT_MIB));
}

#[test]
fn nbt_resource_guards_reject_length_depth_and_tag_bombs() {
    resources::check_nbt(&[10, 0, 0, 0]).unwrap();
    for tag in [7, 9, 11, 12] {
        let mut bytes = vec![10, 0, 0, tag, 0, 0];
        if tag == 9 {
            bytes.push(1);
        }
        bytes.extend_from_slice(&i32::MAX.to_be_bytes());
        bytes.push(0);
        assert!(resources::check_nbt(&bytes).is_err());
    }
    let mut nested = vec![10, 0, 0];
    for _ in 0..65 {
        nested.extend_from_slice(&[10, 0, 0]);
    }
    nested.extend_from_slice(&[0; 66]);
    assert!(resources::check_nbt(&nested).is_err());
    let mut tags = vec![10, 0, 0, 9, 0, 0, 1];
    tags.extend_from_slice(&4096i32.to_be_bytes());
    tags.extend_from_slice(&[0; 4097]);
    assert!(resources::check_nbt(&tags).is_err());
    assert!(resources::check_nbt(&vec![0; 65537]).is_err());

    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    let head = repo.head().unwrap();
    set(
        &mut snapshot,
        2,
        64,
        2,
        Block::from_name("command_block").unwrap(),
        Some(BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            last_output: Some("x".repeat(256 * 1024)),
            ..Default::default()
        }))),
    );
    assert!(repo.commit(&snapshot, 1, "Alice", "too much data").is_err());
    assert_eq!(repo.head().unwrap(), head);
}

#[test]
fn creation_and_concurrent_writes_obey_disk_quota_before_growing_files() {
    let root = TempRoot::new();
    let mut quota = test_limits();
    quota.plot_bytes = 1;
    assert!(Repository::open(&root.0, (-1, 2), quota).is_err());
    assert_eq!(
        fs::metadata(root.0.join("p-1,2/repository.sqlite"))
            .unwrap()
            .len(),
        0
    );
    let repo = root.repo();
    let size = fs::metadata(root.0.join("p-1,2/repository.sqlite"))
        .unwrap()
        .len();
    drop(repo);
    quota = test_limits();
    quota.total_bytes = size;
    let error = Repository::open(&root.0, (0, 0), quota).err().unwrap();
    assert!(error.to_string().contains("server's Git storage quota"));
    assert_eq!(
        fs::metadata(root.0.join("p0,0/repository.sqlite"))
            .unwrap()
            .len(),
        0
    );
    // Existing history remains readable even when its owner has been downgraded.
    quota.plot_bytes = 1;
    assert!(Repository::open(&root.0, (-1, 2), quota).is_ok());

    let simultaneous = TempRoot::new();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    quota.plot_bytes = test_limits().plot_bytes;
    let handles: Vec<_> = (0..2)
        .map(|x| {
            let path = simultaneous.0.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Repository::open(&path, (x, 0), quota).is_ok()
            })
        })
        .collect();
    let admitted = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(admitted, 1);
    let total: u64 = fs::read_dir(&simultaneous.0)
        .unwrap()
        .map(|dir| {
            fs::metadata(dir.unwrap().path().join("repository.sqlite"))
                .unwrap()
                .len()
        })
        .sum();
    assert!(total <= quota.total_bytes);
}

#[test]
fn logs_paginate_with_literal_search_and_modern_clicks() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    for n in 0..12 {
        snapshot.data.piston_state.logical_tick = n;
        repo.commit(&snapshot, 1, "Alice", &format!("commit {n} % literal"))
            .unwrap();
    }
    let page = repo.log(false, None, 1).unwrap();
    assert_eq!(page["extra"].as_array().unwrap().len(), 11);
    assert_eq!(page["extra"][10]["click_event"]["command"], "/git log 2");
    assert_eq!(
        repo.log(false, None, 2).unwrap()["extra"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(repo
        .log(true, Some("% literal"), 1)
        .unwrap()
        .to_string()
        .contains("commit 11"));
    assert_eq!(
        repo.log(true, Some("_' OR 1=1"), 1).unwrap()["extra"],
        json!([])
    );
    assert!(repo.log(false, None, 0).is_err());
    snapshot.data.piston_state.logical_tick = 12;
    repo.commit(&snapshot, 1, "Alice", "ŚWIATŁO działa")
        .unwrap();
    assert!(repo
        .log(true, Some("światło"), 1)
        .unwrap()
        .to_string()
        .contains("ŚWIATŁO działa"));
}

#[test]
fn reused_objects_still_obey_metadata_storage_quota() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let a = empty();
    let mut b = a.clone();
    set(&mut b, 1, 1, 1, Block::Stone {}, None);
    repo.commit(&a, 1, "Alice", "first").unwrap();
    repo.commit(&b, 1, "Alice", "second").unwrap();
    let bytes = fs::metadata(root.0.join("p-1,2/repository.sqlite"))
        .unwrap()
        .len();
    drop(repo);
    let mut limits = test_limits();
    limits.plot_bytes = bytes;
    let mut repo = Repository::open(&root.0, (-1, 2), limits).unwrap();
    let mut rejected = false;
    for n in 0..100 {
        let head = repo.head().unwrap();
        let snapshot = if n % 2 == 0 { &a } else { &b };
        if let Err(error) = repo.commit(snapshot, 1, "Alice", &format!("{n}: {}", "x".repeat(200)))
        {
            assert!(error.to_string().contains("quota"));
            assert_eq!(repo.head().unwrap(), head);
            rejected = true;
            break;
        }
    }
    assert!(
        rejected,
        "Metadata growth must be rejected even when no new snapshot is stored"
    );
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM objects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn no_op_commits_branch_validation_and_memory_limit() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "base").unwrap();
    let before = repo.head().unwrap();
    assert!(repo.commit(&snapshot, 1, "Alice", "unchanged").is_err());
    assert_eq!(repo.head().unwrap(), before);
    for name in [
        "",
        "HEAD",
        "../bad",
        "with space",
        "deadbeef",
        "branch-name-too-long-1",
    ] {
        assert!(!repository::valid_branch(name));
    }
    for name in ["main", "experiment-1", "trial_2", "branch-name-length20"] {
        assert!(repository::valid_branch(name));
    }
    let longest = "b".repeat(19) + "z";
    assert!(repo.branch(&longest, "HEAD").is_ok());
    assert!(repo.branch(&(longest + "z"), "HEAD").is_err());
    assert!(Reservation::new(usize::MAX).is_err());
    assert_eq!(
        parse_search(&["--all", "--page", "2", "hello", "world"]).unwrap(),
        (true, 2, "hello world".into())
    );
    assert!(parse_search(&["--page"]).is_err());
}

#[test]
fn display_metadata_matches_checked_in_protocol_and_ids_are_unique() {
    let entities: Value =
        serde_json::from_str(include_str!("../../../../../mc_data/1.21.5/entities.json")).unwrap();
    let display = entities
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "block_display")
        .unwrap();
    assert_eq!(display["id"], 15);
    assert_eq!(display["metadataKeys"][11], "translation");
    assert_eq!(display["metadataKeys"][12], "scale");
    assert_eq!(display["metadataKeys"][22], "glow_color_override");
    assert_eq!(display["metadataKeys"][23], "block_state");
    for (kind, color, glass) in [
        (0, 0x55ff55, "lime_stained_glass"),
        (1, 0xff5555, "red_stained_glass"),
        (2, 0xffff55, "yellow_stained_glass"),
        (3, 0xffff55, "yellow_stained_glass"),
    ] {
        let packet = visuals::metadata(7, kind);
        assert_eq!(packet.metadata[0].value, [0x40]);
        let glow = packet.metadata.iter().find(|m| m.index == 22).unwrap();
        use mchprs_network::packets::PacketDecoderExt;
        for (index, expected) in [(11, -0.005f32), (12, 1.01f32)] {
            let transform = packet.metadata.iter().find(|m| m.index == index).unwrap();
            assert_eq!(transform.metadata_type, 33);
            assert_eq!(transform.value.len(), 12);
            let mut values = std::io::Cursor::new(&transform.value);
            for _ in 0..3 {
                assert_eq!(values.read_float().unwrap(), expected);
            }
        }
        assert_eq!(
            std::io::Cursor::new(&glow.value).read_varint().unwrap(),
            color
        );
        let state = packet.metadata.iter().find(|m| m.index == 23).unwrap();
        assert_eq!(state.metadata_type, 14);
        let id = std::io::Cursor::new(&state.value).read_varint().unwrap();
        assert_eq!(Block::from_id(id as u32).get_name(), glass);
    }
    assert_ne!(
        crate::player::allocate_entity_id(),
        crate::player::allocate_entity_id()
    );
}

#[test]
fn commit_checkout_detaches_head_and_keeps_branch_tips_and_dirty_work() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let base = empty();
    repo.commit(&base, 1, "Alice", "base").unwrap();
    let first = repo.resolve("HEAD").unwrap();
    let mut latest = base.clone();
    set(&mut latest, 2, 64, 2, Block::Stone {}, None);
    repo.commit(&latest, 1, "Alice", "latest").unwrap();
    let main = repo.resolve("main").unwrap();
    let mut dirty = latest.clone();
    set(&mut dirty, 3, 64, 3, Block::Stone {}, None);
    let save = root.0.join("plot");

    let (restored, message, reservation) = repo
        .checkout(&first[..8].to_uppercase(), &dirty, 1, "Alice", &save)
        .unwrap();
    assert!(message.contains("detached HEAD") && message.contains("Recovery:"));
    assert_eq!(restored.block(pos(2, 64, 2)), 0);
    assert_eq!(repo.resolve("HEAD").unwrap(), first);
    assert_eq!(repo.resolve("main").unwrap(), main);
    assert!(repo.status(&restored).unwrap().contains("detached HEAD"));
    assert_eq!(repo.sidebar_head().unwrap(), format!("@{first}"));
    assert!(repo.branches().unwrap().contains("detached HEAD"));
    let recovery: String = root
        .db()
        .query_row("SELECT id FROM recoveries", [], |r| r.get(0))
        .unwrap();
    drop(reservation);
    drop(repo);

    let mut repo = root.repo();
    assert_eq!(repo.resolve("HEAD").unwrap(), first);
    assert_eq!(repo.sidebar_head().unwrap(), format!("@{first}"));
    let mut experiment = restored;
    set(&mut experiment, 4, 64, 4, Block::Stone {}, None);
    repo.commit(&experiment, 1, "Alice", "detached experiment")
        .unwrap();
    let detached = repo.resolve("HEAD").unwrap();
    assert_eq!(repo.sidebar_head().unwrap(), format!("@{detached}"));
    assert_ne!(detached, first);
    assert_eq!(repo.resolve("main").unwrap(), main);
    repo.branch("experiment", "HEAD").unwrap();
    let (_, _, reservation) = repo
        .checkout("experiment", &experiment, 1, "Alice", &save)
        .unwrap();
    assert_eq!(repo.head().unwrap().0, "experiment");
    assert_eq!(repo.resolve("HEAD").unwrap(), detached);
    drop(reservation);
    repo.recover_branch(&recovery, "unfinished", 1, "Alice")
        .unwrap();
    assert_eq!(
        repo.load(&repo.resolve("unfinished").unwrap())
            .unwrap()
            .fingerprints()
            .unwrap()
            .full,
        dirty.fingerprints().unwrap().full
    );
}

#[test]
fn interrupted_commit_checkout_restores_detached_head_on_restart() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let base = empty();
    repo.commit(&base, 1, "Alice", "base").unwrap();
    let first = repo.resolve("HEAD").unwrap();
    let mut latest = base.clone();
    set(&mut latest, 2, 64, 2, Block::Stone {}, None);
    repo.commit(&latest, 1, "Alice", "latest").unwrap();
    let main = repo.resolve("main").unwrap();
    let save = root.0.join("plot");
    latest.data.save_to_file(&save).unwrap();
    root.db()
        .execute(
            "INSERT INTO checkout SELECT 1,?1,?2,snapshot FROM commits WHERE id=?3",
            rusqlite::params![format!("@{first}"), first, main],
        )
        .unwrap();
    drop(repo);

    let mut repo = root.repo();
    assert!(repo.has_pending().unwrap());
    let restored = repo.finish_checkout(&save).unwrap();
    assert_eq!(restored.block(pos(2, 64, 2)), 0);
    assert_eq!(repo.resolve("HEAD").unwrap(), first);
    assert_eq!(repo.resolve("main").unwrap(), main);
    assert!(!repo.has_pending().unwrap());
}

#[test]
fn lz4_snapshots_are_compact_bounded_and_round_trip() {
    let snapshot = empty();
    let raw = bincode::serialize(&snapshot).unwrap();
    let compressed = snapshot.encode(raw.len()).unwrap();
    assert_eq!(
        u32::from_le_bytes(compressed[..4].try_into().unwrap()) as usize,
        raw.len()
    );
    assert!(compressed.len() < raw.len() / 10);
    let expected = snapshot.fingerprints().unwrap().full;
    for bytes in [&compressed] {
        assert_eq!(
            Snapshot::decode(bytes, (-1, 2), raw.len())
                .unwrap()
                .fingerprints()
                .unwrap()
                .full,
            expected
        );
        assert!(Snapshot::decode(bytes, (-1, 2), raw.len() - 1).is_err());
    }
    let mut short_header = compressed.clone();
    short_header[..4].copy_from_slice(&((raw.len() - 1) as u32).to_le_bytes());
    assert!(Snapshot::decode(&short_header, (-1, 2), raw.len()).is_err());
    let mut corrupt = compressed.clone();
    corrupt.truncate(corrupt.len() / 2);
    assert!(Snapshot::decode(&corrupt, (-1, 2), raw.len()).is_err());
    assert!(Reservation::new(mib(MAX_WORK_MEMORY_MIB) + 1).is_err());
}

#[test]
fn owner_storage_tiers_apply_server_ceiling_and_default() {
    let default = CONFIG
        .git_default_plot_storage_mib
        .min(CONFIG.git_plot_storage_mib);
    assert_eq!(
        owner_limits(None, None).unwrap().plot_bytes,
        default * 1048576
    );
    for (grant, expected) in [
        (None, 100),
        (Some(100), 100),
        (Some(1024), 1024),
        (Some(2048), 1024),
        (Some(0), 0),
    ] {
        // Simulate a cached owner's effective grant. An administrator/member's
        // allowance is never an input to resolving this plot's quota.
        let storage = storage_limit_mib(grant, 100, 1024);
        let limits = owner_limits(Some(1), Some(storage)).unwrap();
        assert_eq!(limits.plot_bytes, expected * 1048576);
    }
    assert_eq!(storage_limit_mib(Some(1024), 100, 50), 50);
    assert_eq!(storage_limit_mib(None, 100, 50), 50);
}

#[test]
fn storage_rank_downgrade_keeps_history_readable_and_rejects_growth() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let mut snapshot = empty();
    repo.commit(&snapshot, 1, "Alice", "first").unwrap();
    set(&mut snapshot, 1, 64, 1, Block::Stone {}, None);
    repo.commit(&snapshot, 1, "Alice", "second").unwrap();
    let head = repo.head().unwrap();
    let fingerprint = snapshot.fingerprints().unwrap().full;
    drop(repo);
    let mut repo = Repository::open(
        &root.0,
        (-1, 2),
        Limits {
            plot_bytes: 0,
            ..test_limits()
        },
    )
    .unwrap();
    assert_eq!(
        repo.load(head.1.as_ref().unwrap())
            .unwrap()
            .fingerprints()
            .unwrap()
            .full,
        fingerprint
    );
    assert!(repo
        .log(false, None, 1)
        .unwrap()
        .to_string()
        .contains("second"));
    assert!(repo.status(&snapshot).is_ok());
    set(&mut snapshot, 2, 64, 2, Block::Glass, None);
    assert_eq!(
        repo.commit(&snapshot, 1, "Alice", "third")
            .unwrap_err()
            .to_string(),
        messages::GIT_PLOT_STORAGE_FULL
    );
    assert_eq!(repo.head().unwrap(), head);
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM commits", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(repo);
    let mut repo = root.repo();
    repo.commit(&snapshot, 1, "Alice", "third").unwrap();
    assert_eq!(
        repo.load(&repo.resolve("HEAD").unwrap())
            .unwrap()
            .fingerprints()
            .unwrap()
            .full,
        snapshot.fingerprints().unwrap().full
    );
}

fn rebase_branches(
    repo: &mut Repository,
    ours: &Snapshot,
    theirs: &Snapshot,
    save: &Path,
) -> (String, String) {
    repo.commit(&empty(), 1, "Alice", "base").unwrap();
    repo.branch("revisit", "HEAD").unwrap();
    repo.commit(ours, 1, "Alice", "our changes").unwrap();
    let ours_id = repo.resolve("HEAD").unwrap();
    drop(repo.checkout("revisit", ours, 1, "Alice", save).unwrap());
    repo.commit(theirs, 2, "Bob", "source changes").unwrap();
    let theirs_id = repo.resolve("HEAD").unwrap();
    drop(repo.checkout("main", theirs, 1, "Alice", save).unwrap());
    (ours_id, theirs_id)
}

#[test]
fn rebase_copies_whole_plot_uncommitted_then_commit_advances_current_branch() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let save = root.0.join("plot");
    let mut ours = empty();
    set(&mut ours, 1, 64, 1, Block::Stone {}, None);
    let mut theirs = empty();
    set(
        &mut theirs,
        2,
        64,
        2,
        Block::from_name("command_block").unwrap(),
        Some(command_entity("say source")),
    );
    theirs.data.pending_ticks.push(TickEntry {
        pos: pos(2, 64, 2),
        ticks_left: 2,
        tick_priority: TickPriority::High,
        block_type: None,
    });
    theirs.data.piston_state.logical_tick = 42;
    let (old_head, source_head) = rebase_branches(&mut repo, &ours, &theirs, &save);
    let mut dirty = ours.clone();
    set(&mut dirty, 4, 64, 4, Block::Glass, None);
    dirty.data.world_send_rate = WorldSendRate(7);
    let (mut copied, message, reservation) =
        repo.rebase("revisit", &dirty, 1, "Alice", &save).unwrap();
    assert!(
        message.contains("revisit") && message.contains("main") && message.contains("Recovery:")
    );
    assert_eq!(copied.block(pos(1, 64, 1)), 0);
    assert_eq!(copied.block(pos(4, 64, 4)), 0);
    assert_eq!(
        copied.fingerprints().unwrap().full,
        theirs.fingerprints().unwrap().full
    );
    assert_eq!(copied.data.pending_ticks, theirs.data.pending_ticks);
    assert_eq!(copied.data.piston_state.logical_tick, 42);
    assert_eq!(copied.data.tps, Tps::Limited(0));
    assert_eq!(copied.data.world_send_rate, WorldSendRate(7));
    assert_eq!(
        repo.head().unwrap(),
        ("main".into(), Some(old_head.clone()))
    );
    assert_eq!(repo.resolve("revisit").unwrap(), source_head);
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM commits", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(repo
        .status(&copied)
        .unwrap()
        .contains("Build changes: true"));
    assert!(!repo.has_pending().unwrap());
    drop(reservation);
    drop(repo);
    let mut repo = root.repo();
    assert_eq!(
        repo.head().unwrap(),
        ("main".into(), Some(old_head.clone()))
    );
    let saved = Snapshot {
        data: PlotData::load_from_file(&save, false).unwrap(),
        ..empty()
    };
    assert_eq!(
        saved.fingerprints().unwrap().full,
        copied.fingerprints().unwrap().full
    );
    set(&mut copied, 3, 64, 3, Block::Glass, None);
    repo.commit(&copied, 1, "Alice", "Bring revisit into main")
        .unwrap();
    let new_head = repo.resolve("HEAD").unwrap();
    assert_ne!(new_head, old_head);
    assert_eq!(
        root.db()
            .query_row("SELECT parent FROM commits WHERE id=?1", [&new_head], |r| r
                .get::<_, String>(0))
            .unwrap(),
        old_head
    );
    assert_eq!(repo.resolve("revisit").unwrap(), source_head);
    assert_eq!(
        repo.load(&new_head).unwrap().fingerprints().unwrap().full,
        copied.fingerprints().unwrap().full
    );
    let recovery: String = root
        .db()
        .query_row("SELECT id FROM recoveries", [], |r| r.get(0))
        .unwrap();
    repo.recover_branch(&recovery, "unfinished", 1, "Alice")
        .unwrap();
    assert_eq!(
        repo.load(&repo.resolve("unfinished").unwrap())
            .unwrap()
            .fingerprints()
            .unwrap()
            .full,
        dirty.fingerprints().unwrap().full
    );
}

#[test]
fn rebase_requires_named_branches_and_allows_copying_same_branch() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let save = root.0.join("plot");
    let mut ours = empty();
    set(&mut ours, 1, 64, 1, Block::Stone {}, None);
    let mut theirs = empty();
    set(&mut theirs, 2, 64, 2, Block::Glass, None);
    let (old_head, source_head) = rebase_branches(&mut repo, &ours, &theirs, &save);
    let bytes = fs::read(&save).unwrap();
    for reference in ["missing", "HEAD", &source_head[..8]] {
        let error = repo
            .rebase(reference, &ours, 1, "Alice", &save)
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            messages::GIT_REBASE_SOURCE_BRANCH_REQUIRED
        );
        assert_eq!(repo.resolve("main").unwrap(), old_head);
        assert_eq!(fs::read(&save).unwrap(), bytes);
        assert!(!repo.has_pending().unwrap());
    }
    let mut dirty = ours.clone();
    set(&mut dirty, 3, 64, 3, Block::Glass, None);
    let (restored, _, reservation) = repo.rebase("main", &dirty, 1, "Alice", &save).unwrap();
    assert_eq!(
        restored.fingerprints().unwrap().full,
        ours.fingerprints().unwrap().full
    );
    assert_eq!(repo.resolve("main").unwrap(), old_head);
    drop(reservation);
    drop(
        repo.checkout(&source_head[..8], &restored, 1, "Alice", &save)
            .unwrap(),
    );
    let error = repo
        .rebase("main", &theirs, 1, "Alice", &save)
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        messages::GIT_REBASE_NAMED_BRANCH_REQUIRED
    );
    assert_eq!(
        repo.head().unwrap(),
        (format!("@{source_head}"), Some(source_head))
    );
    assert_eq!(repo.resolve("main").unwrap(), old_head);
    assert!(!repo.has_pending().unwrap());
}

#[test]
fn rebase_recovery_quota_failure_preserves_work_and_history() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let save = root.0.join("plot");
    let mut ours = empty();
    set(&mut ours, 1, 64, 1, Block::Stone {}, None);
    let mut theirs = empty();
    set(&mut theirs, 2, 64, 2, Block::Glass, None);
    let (old_head, source_head) = rebase_branches(&mut repo, &ours, &theirs, &save);
    set(&mut ours, 3, 64, 3, Block::Glass, None);
    ours.data.save_to_file(&save).unwrap();
    let bytes = fs::read(&save).unwrap();
    drop(repo);
    let mut repo = Repository::open(
        &root.0,
        (-1, 2),
        Limits {
            plot_bytes: 0,
            ..test_limits()
        },
    )
    .unwrap();
    let error = repo
        .rebase("revisit", &ours, 1, "Alice", &save)
        .err()
        .unwrap();
    assert_eq!(error.to_string(), messages::GIT_PLOT_STORAGE_FULL);
    assert_eq!(repo.head().unwrap(), ("main".into(), Some(old_head)));
    assert_eq!(repo.resolve("revisit").unwrap(), source_head);
    assert_eq!(fs::read(&save).unwrap(), bytes);
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM commits", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM recoveries", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(!repo.has_pending().unwrap());
}

#[test]
fn rebase_interrupted_save_resumes_copy_without_moving_branch_tips() {
    let root = TempRoot::new();
    let mut repo = root.repo();
    let save = root.0.join("plot");
    let mut ours = empty();
    set(&mut ours, 1, 64, 1, Block::Stone {}, None);
    let mut theirs = empty();
    set(&mut theirs, 2, 64, 2, Block::Glass, None);
    let (old_head, source_head) = rebase_branches(&mut repo, &ours, &theirs, &save);
    let unavailable_save = root.0.join("unavailable");
    fs::create_dir(&unavailable_save).unwrap();
    assert!(repo
        .rebase("revisit", &ours, 1, "Alice", &unavailable_save)
        .is_err());
    assert_eq!(
        repo.head().unwrap(),
        ("main".into(), Some(old_head.clone()))
    );
    assert!(repo.has_pending().unwrap());
    drop(repo);
    fs::remove_dir(&unavailable_save).unwrap();
    let mut repo = root.repo();
    let restored = repo.finish_checkout(&unavailable_save).unwrap();
    assert_eq!(
        restored.fingerprints().unwrap().full,
        theirs.fingerprints().unwrap().full
    );
    assert_eq!(restored.data.tps, Tps::Limited(0));
    assert_eq!(repo.head().unwrap(), ("main".into(), Some(old_head)));
    assert_eq!(repo.resolve("revisit").unwrap(), source_head);
    assert_eq!(
        root.db()
            .query_row("SELECT count(*) FROM commits", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(!repo.has_pending().unwrap());
}
