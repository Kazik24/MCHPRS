//! In-memory plot adapter for production-profile client benchmarks and packet tests.
use super::*;

pub fn pin_cpu(cpu: usize) {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn SetThreadAffinityMask(thread: *mut std::ffi::c_void, mask: usize) -> usize;
        }
        assert!(cpu < usize::BITS as usize);
        // SAFETY: This pseudo-handle addresses only the calling benchmark thread.
        assert_ne!(
            unsafe { SetThreadAffinityMask(GetCurrentThread(), 1usize << cpu) },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
    }
    #[cfg(not(windows))]
    {
        let _ = cpu;
        panic!("benchmark affinity is supported on Windows only");
    }
}

pub fn with_client(mut world: PlotWorld, player: Player, tps: Tps) -> Plot {
    world.packet_senders.push(PlayerPacketSender::new(&player.client));
    let (message_sender, _) = std::sync::mpsc::channel();
    let (_, priv_message_receiver) = std::sync::mpsc::channel();
    Plot {
        transient_test_fixture: true,
        waveform_source: None,
        world,
        players: vec![player],
        snowballs: Vec::new(),
        redpilator: Default::default(),
        compilation: None,
        redpilator_cache: None,
        redpilator_handoff_failed: false,
        message_receiver: bus::Bus::new(4).add_rx(),
        message_sender,
        priv_message_receiver,
        locked_players: Default::default(),
        tps,
        world_send_rate: WorldSendRate(20),
        publication_setting: None,
        redpilator_auto_publication: false,
        piston_animation: Default::default(),
        last_update_time: Instant::now(),
        lag_time: Duration::ZERO,
        last_nspt: None,
        timings: TimingsMonitor::new(tps),
        last_player_time: Instant::now(),
        last_world_send_time: Instant::now(),
        sleep_time: Duration::ZERO,
        running: false,
        always_running: false,
        owner: Some(1),
        async_rt: Plot::create_async_rt(),
        scoreboard: Default::default(),
        last_sidebar_update: Instant::now(),
        last_redbox_state: Instant::now(),
        neighbor_views: Default::default(),
        neighbor_source: None,
        git: Default::default(),
        wire_cursor: 0,
    }
}

/// Run the same updates as the unlimited-TPS plot worker, without startup or shutdown.
pub fn run_for(plot: &mut Plot, duration: Duration) -> Duration {
    assert!(!duration.is_zero());
    plot.tps = Tps::Unlimited;
    plot.timings.set_tps(Tps::Unlimited);
    plot.world.update_stats = Default::default();
    plot.last_update_time = Instant::now();
    plot.last_world_send_time = Instant::now();
    let handle = plot.async_rt.handle().clone();
    let _entered = handle.enter();
    let started = Instant::now();
    while started.elapsed() < duration {
        plot.update();
        assert_eq!(plot.players.len(), 1, "benchmark client disconnected or left the plot");
        assert!(plot.redpilator.is_active(), "compiled execution stopped");
    }
    started.elapsed()
}

/// Run a fixed number of game ticks through the actual plot update loop.
pub fn run_ticks(plot: &mut Plot, ticks: u32) -> Duration {
    run_ticks_batched(plot, ticks, 16)
}

pub fn run_ticks_batched(plot: &mut Plot, ticks: u32, batch: u32) -> Duration {
    assert!([1, 16, 32, 64].contains(&batch));
    assert!(ticks > 0);
    plot.tps = Tps::Unlimited;
    plot.timings.set_tps(Tps::Unlimited);
    plot.world.update_stats = Default::default();
    plot.lag_time = Duration::ZERO;
    plot.last_update_time = Instant::now();
    plot.last_world_send_time = Instant::now();
    let handle = plot.async_rt.handle().clone();
    let _entered = handle.enter();
    let started = Instant::now();
    while plot.world.update_stats.simulated_ticks < u64::from(ticks) {
        let remaining = ticks - plot.world.update_stats.simulated_ticks as u32;
        plot.update_with_limits(remaining, batch);
        assert_eq!(plot.players.len(), 1, "benchmark client disconnected or left the plot");
        assert!(plot.redpilator.is_active(), "compiled execution stopped");
    }
    started.elapsed()
}

pub fn publication(plot: &mut Plot, mode: Option<PublicationMode>, send_rate: u32) {
    plot.world_send_rate = WorldSendRate(if mode.is_some() { send_rate } else { 0 });
    plot.redpilator_auto_publication = false;
    assert!(plot.redpilator.set_publication_mode(mode.unwrap_or(PublicationMode::Io)));
    plot.world.clear_scheduled_ticks();
}

pub fn live_command_output(world: &mut PlotWorld) {
    world.command_output_limits_enabled = true;
}

pub fn drain_commands(plot: &mut Plot) -> Duration {
    let started = Instant::now();
    plot.tps = Tps::Limited(0);
    let handle = plot.async_rt.handle().clone();
    let _entered = handle.enter();
    while !plot.world.command_messages.is_empty() {
        assert!(started.elapsed() < Duration::from_secs(10), "command delivery did not finish");
        plot.update();
        if !plot.world.command_messages.is_empty() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    started.elapsed()
}

/// Stop client block updates before the untimed complete-state correctness read.
pub fn materialize(plot: &mut Plot, bounds: (BlockPos, BlockPos)) {
    plot.world.packet_senders.clear();
    plot.redpilator.try_reset(&mut plot.world, bounds).unwrap();
}

pub fn publish(plot: &mut Plot) {
    plot.redpilator.flush(&mut plot.world);
    plot.world.flush_block_changes();
}

pub fn timings(plot: &Plot) -> serde_json::Value {
    let p = &plot.world.update_stats;
    json!({
        "ticks": p.simulated_ticks,
        "plot_update_seconds": p.plot.as_secs_f64(),
        "engine_seconds": p.simulation.as_secs_f64(),
        "projection_seconds": p.projection.as_secs_f64(),
        "collection_seconds": p.collection.as_secs_f64(),
        "enqueue_seconds": p.enqueue.as_secs_f64(),
        "other_plot_seconds": p.plot.saturating_sub(p.simulation + p.projection + p.collection + p.enqueue).as_secs_f64(),
        "native_batches": p.native_batches, "max_native_batch": p.max_native_batch,
        "publications": p.publications, "last_publication_tick": p.last_publication_tick,
        "maximum_publication_gap_ms": p.maximum_publication_gap.as_secs_f64()*1000.0,
        "visual_flushes": p.flushes, "sections": p.sections, "block_records": p.records,
        "visual_send_rate_hz": plot.effective_send_rate(),
        "screen_only": plot.world.screen_only(), "fast_rendering": plot.world.fast_rendering,
    })
}

pub fn player(client: mchprs_network::PlayerConn) -> Player {
    let mut player = Player::test_player(client);
    player.pos = PlayerPos::new(32.5, 21.0, 35.5);
    player.last_chunk_x = 2;
    player.last_chunk_z = 2;
    player
}
