//! In-memory plot adapter for production-profile client benchmarks and packet tests.
use super::*;

pub fn with_client(mut world: PlotWorld, player: Player, tps: Tps) -> Plot {
    world.packet_senders.push(PlayerPacketSender::new(&player.client));
    let (message_sender, _) = std::sync::mpsc::channel();
    let (_, priv_message_receiver) = std::sync::mpsc::channel();
    Plot {
        transient_test_fixture: true,
        world,
        players: vec![player],
        redpiler: Default::default(),
        compilation: None,
        message_receiver: bus::Bus::new(4).add_rx(),
        message_sender,
        priv_message_receiver,
        locked_players: Default::default(),
        tps,
        world_send_rate: WorldSendRate(20),
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
        auto_redpiler: false,
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
        assert!(plot.redpiler.is_active(), "compiled execution stopped");
    }
    started.elapsed()
}

pub fn publish(plot: &mut Plot) {
    plot.redpiler.flush(&mut plot.world);
    plot.world.flush_block_changes();
}

pub fn timings(plot: &Plot) -> serde_json::Value {
    let p = &plot.world.update_stats;
    json!({
        "ticks": p.simulated_ticks,
        "plot_update_seconds": p.plot.as_secs_f64(),
        "engine_seconds": p.simulation.saturating_sub(p.projection).as_secs_f64(),
        "projection_seconds": p.projection.as_secs_f64(),
        "collection_seconds": p.collection.as_secs_f64(),
        "enqueue_seconds": p.enqueue.as_secs_f64(),
        "other_plot_seconds": p.plot.saturating_sub(p.simulation + p.collection + p.enqueue).as_secs_f64(),
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
