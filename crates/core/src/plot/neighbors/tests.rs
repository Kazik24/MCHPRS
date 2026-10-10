use super::*;
use crate::plot::PlotWorld;
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_save_data::plot_data::{ChunkData, PistonAnimation, Tps, WorldSendRate};

fn packet(state: u8) -> Snapshot {
    Arc::new(PacketEncoder::new(vec![state], 0x27))
}

#[test]
fn neighbor_refreshes_are_grouped_throttled_and_skip_unchanged_packets() {
    let mut views = Views::default();
    let now = Instant::now();
    let interval = Duration::from_secs(2);
    views.load(1, (16, 0));
    views.load(2, (16, 0));
    let mut requests = Vec::new();
    assert!(views.update(now, interval, |r| requests.push(r)).is_empty());
    assert_eq!(requests.len(), 1);
    let request = requests.pop().unwrap();
    assert_eq!(request.plot, (1, 0));
    assert_eq!(request.chunks, vec![(16, 0)]);
    request.reply.send(vec![((16, 0), packet(1))]).unwrap();
    let delivered = views.update(now, interval, |r| requests.push(r));
    assert_eq!(delivered.len(), 2);
    assert!(Arc::ptr_eq(&delivered[0].1, &delivered[1].1));
    assert!(
        views
            .update(now + Duration::from_millis(1999), interval, |r| requests
                .push(r))
            .is_empty()
    );
    assert!(requests.is_empty());
    views.update(now + interval, interval, |r| requests.push(r));
    assert_eq!(requests.len(), 1);
    requests
        .pop()
        .unwrap()
        .reply
        .send(vec![((16, 0), packet(1))])
        .unwrap();
    assert!(
        views
            .update(now + interval, interval, |r| requests.push(r))
            .is_empty()
    );
    views.update(now + interval * 2, interval, |r| requests.push(r));
    requests
        .pop()
        .unwrap()
        .reply
        .send(vec![((16, 0), packet(2))])
        .unwrap();
    assert_eq!(
        views
            .update(now + interval * 2, interval, |r| requests.push(r))
            .len(),
        2
    );
}

#[test]
fn stale_replies_cannot_reload_unloaded_chunks_or_follow_a_player_to_another_plot() {
    let mut views = Views::default();
    let now = Instant::now();
    let interval = Duration::from_secs(2);
    views.load(1, (-1, -1));
    views.load(2, (-1, -1));
    let mut requests = Vec::new();
    views.update(now, interval, |r| requests.push(r));
    let old = requests.pop().unwrap();
    assert_eq!(old.plot, (-1, -1));
    views.unload(1, (-1, -1));
    views.load(1, (-1, -1));
    views.remove_player(2);
    old.reply.send(vec![((-1, -1), packet(1))]).unwrap();
    assert!(views.update(now, interval, |r| requests.push(r)).is_empty());
    assert!(views.packet(1, (-1, -1)).is_none());
    assert_eq!(requests.len(), 1); // New visibility requests its own initial snapshot.
    requests
        .pop()
        .unwrap()
        .reply
        .send(vec![((-1, -1), packet(2))])
        .unwrap();
    let deliveries = views.update(now, interval, |_| panic!("unexpected request"));
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].0, 1);
    views.unload(1, (-1, -1));
    views.update(now, interval, |_| panic!("no visible neighbors"));
    assert!(views.pending.is_empty());
    assert!(views.last_request.is_empty());
}

#[test]
fn failed_and_timed_out_requests_retry_without_flooding_the_service() {
    let mut views = Views::default();
    let now = Instant::now();
    let interval = Duration::from_secs(2);
    views.load(1, (16, 0));
    let mut requests = Vec::new();
    views.update(now, interval, |r| requests.push(r));
    drop(requests.pop());
    views.update(now + Duration::from_secs(1), interval, |_| {
        panic!("retry too soon")
    });
    views.update(now + interval, interval, |r| requests.push(r));
    let stalled = requests.pop().unwrap();
    views.update(now + interval + REQUEST_TIMEOUT, interval, |r| {
        requests.push(r)
    });
    assert_eq!(requests.len(), 1);
    assert!(stalled.reply.try_send(vec![]).is_err());
}

#[test]
fn snapshot_cache_obeys_its_memory_limit_and_evicts_the_least_recently_used_chunk() {
    let mut cache = Cache::new(258); // Two one-byte packets plus entry accounting.
    let version = Version::Live((1, 0));
    cache.insert((0, 0), version.clone(), packet(1));
    cache.insert((0, 1), version.clone(), packet(2));
    assert!(cache.get((0, 0), &version).is_some());
    cache.insert((0, 2), version.clone(), packet(3));
    assert!(cache.get((0, 1), &version).is_none());
    assert!(cache.get((0, 0), &version).is_some());
    assert!(cache.get((0, 0), &Version::Live((1, 1))).is_none());
    assert!(cache.bytes <= cache.limit);
    cache.insert(
        (0, 3),
        version,
        Arc::new(PacketEncoder::new(vec![0; 1024], 1)),
    );
    assert!(cache.bytes <= cache.limit);
}

#[test]
fn live_snapshots_observe_changes_after_local_flushes_and_screen_only_filtering() {
    let (sender, requests) = mpsc::sync_channel(1);
    let mut source = LiveSource {
        plot: (0, 0),
        requests,
        registration: 0,
        work: None,
        cache: Cache::new(LIVE_CACHE_BYTES),
    };
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let read = |source: &mut LiveSource, world: &PlotWorld| {
        let (reply, response) = mpsc::sync_channel(1);
        sender
            .send(Request {
                plot: (0, 0),
                chunks: vec![(0, 0)],
                reply,
            })
            .unwrap();
        source.poll(&world.chunks);
        response.try_recv().unwrap().pop().unwrap().1
    };
    let initial = read(&mut source, &world);
    assert!(Arc::ptr_eq(&initial, &read(&mut source, &world)));
    world.set_screen_only(true);
    world.set_block_raw(BlockPos::new(1, 2, 3), 1);
    world.flush_block_changes();
    let changed = read(&mut source, &world);
    assert_ne!(initial.buffer, changed.buffer);
    assert_eq!(
        changed.buffer,
        world.chunks[0].encode_packet_for_client(true).buffer
    );
    world.chunks[0].set_block_entity(
        BlockPos::new(1, 2, 3),
        BlockEntity::Comparator { output_strength: 1 },
    );
    let version = world.chunks[0].snapshot_version();
    if let Some(BlockEntity::Comparator { output_strength }) =
        world.get_block_entity_mut(BlockPos::new(1, 2, 3))
    {
        *output_strength = 2;
    }
    assert_ne!(version, world.chunks[0].snapshot_version());
    world.chunks[0].delete_block_entity(BlockPos::new(1, 2, 3));
    world.chunks[0] = Chunk::empty(0, 0); // Rewind replaces chunks with fresh instances.
    assert_eq!(read(&mut source, &world).buffer, initial.buffer);
}

#[test]
fn live_snapshots_project_moving_pistons_and_limit_work_per_poll() {
    let (sender, requests) = mpsc::sync_channel(1);
    let mut source = LiveSource {
        plot: (0, 0),
        requests,
        registration: 0,
        work: None,
        cache: Cache::new(LIVE_CACHE_BYTES),
    };
    let mut chunk = Chunk::empty(0, 0);
    let pos = BlockPos::new(1, 20, 2);
    let moving = mchprs_blocks::blocks::Block::from_name("moving_piston")
        .unwrap()
        .get_id();
    chunk.set_block(1, 20, 2, moving);
    chunk.set_block_entity(pos, BlockEntity::MovingPiston(Default::default()));
    let normal = chunk.encode_packet_for_client(false);
    let projected = chunk.encode_packet_for_client(true);
    assert_ne!(normal.buffer, projected.buffer);
    let (reply, response) = mpsc::sync_channel(1);
    sender
        .send(Request {
            plot: (0, 0),
            chunks: vec![(0, 0); 9],
            reply,
        })
        .unwrap();
    source.poll(std::slice::from_ref(&chunk));
    assert!(matches!(response.try_recv(), Err(TryRecvError::Empty)));
    while source.work.is_some() {
        source.poll(std::slice::from_ref(&chunk));
    }
    let packets = response.try_recv().unwrap();
    assert_eq!(packets.len(), 9);
    assert!(packets.iter().all(|(_, p)| p.buffer == projected.buffer));
}

struct TempRoot(std::path::PathBuf);
impl TempRoot {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mchprs-neighbors-{}-{unique}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn saved_plot() -> PlotData<PLOT_SECTIONS> {
    PlotData {
        tps: Tps::Limited(20),
        world_send_rate: WorldSendRate::default(),
        chunk_data: vec![
            ChunkData {
                sections: std::array::from_fn(|_| None),
                block_entities: Default::default()
            };
            NUM_CHUNKS
        ],
        pending_ticks: Vec::new(),
        piston_state: Default::default(),
        piston_animation: PistonAnimation::Auto,
    }
}

#[test]
fn saved_neighbor_reads_use_correct_negative_coordinates_and_invalidate_changed_files() {
    let root = TempRoot::new();
    let path = root.0.join("p-1,-1");
    let mut data = saved_plot();
    data.save_to_file(&path).unwrap();
    let (reply, _) = mpsc::sync_channel(1);
    let request = Request {
        plot: (-1, -1),
        chunks: vec![(-1, -1), (0, 0)],
        reply,
    };
    let mut cache = Cache::new(DISK_CACHE_BYTES);
    let initial = disk_snapshots(&root.0, &request, &mut cache).unwrap();
    assert_eq!(initial.len(), 1); // Never return a chunk from another plot.
    let original_save = fs::read(&path).unwrap();
    let unchanged = disk_snapshots(&root.0, &request, &mut cache).unwrap();
    assert!(Arc::ptr_eq(&initial[0].1, &unchanged[0].1));
    assert_eq!(fs::read(&path).unwrap(), original_save);
    let mut chunk = Chunk::empty(-1, -1);
    chunk.set_block(1, 20, 2, 1);
    data.chunk_data[NUM_CHUNKS - 1] = chunk.save();
    data.save_to_file(&path).unwrap();
    let updated = disk_snapshots(&root.0, &request, &mut cache).unwrap();
    assert_ne!(updated[0].1.buffer, initial[0].1.buffer);
    assert_eq!(
        updated[0].1.buffer,
        chunk.encode_packet_for_client(true).buffer
    );
    fs::write(&path, b"invalid save").unwrap();
    assert!(disk_snapshots(&root.0, &request, &mut cache).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"invalid save");
}

#[test]
fn missing_neighbors_use_the_template_or_generated_ground_without_creating_saves() {
    let root = TempRoot::new();
    let (reply, _) = mpsc::sync_channel(1);
    let request = Request {
        plot: (1, -1),
        chunks: vec![(16, -16)],
        reply,
    };
    let mut cache = Cache::new(DISK_CACHE_BYTES);
    let generated = disk_snapshots(&root.0, &request, &mut cache).unwrap();
    assert_eq!(
        generated[0].1.buffer,
        Plot::generate_chunk(8, 16, -16)
            .encode_packet_for_client(true)
            .buffer
    );
    assert!(!root.0.join("p1,-1").exists());
    let mut data = saved_plot();
    let mut chunk = Chunk::empty(16, -16);
    chunk.set_block(1, 20, 2, 1);
    data.chunk_data[0] = chunk.save();
    data.save_to_file(root.0.join("pTEMPLATE")).unwrap();
    let templated = disk_snapshots(&root.0, &request, &mut cache).unwrap();
    assert_eq!(
        templated[0].1.buffer,
        chunk.encode_packet_for_client(true).buffer
    );
    assert!(!root.0.join("p1,-1").exists());
}

#[test]
fn dropping_an_old_live_source_does_not_unregister_its_replacement() {
    let plot = (12345, -12345);
    let old = LiveSource::register(plot);
    let replacement = LiveSource::register(plot);
    drop(old);
    let (reply, _) = mpsc::sync_channel(1);
    SERVICE.request(Request {
        plot,
        chunks: vec![],
        reply,
    });
    assert!(replacement.requests.try_recv().is_ok());
    drop(replacement);
    assert!(!SERVICE.sources.lock().unwrap().contains_key(&plot));
}
