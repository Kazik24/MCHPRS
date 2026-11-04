use super::*;
use crate::plot::client_sync_tests::fixture;
use mchprs_network::packets::serverbound::SKeepAlive;
use std::sync::mpsc;

impl Plot {
    pub(in crate::plot) fn wait_for_redpiler(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.compilation.is_some() {
            assert!(Instant::now() < deadline, "compiler worker did not finish");
            self.poll_redpiler_compilation();
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn install_job(plot: &mut Plot, work: impl FnOnce(PlotWorld, Arc<TaskMonitor>) -> CompilationResult + Send + 'static) {
    let monitor = Arc::new(TaskMonitor::default());
    let worker_monitor = monitor.clone();
    let world = plot.world.activation_snapshot();
    plot.compilation = Some(CompilationJob {
        started: Instant::now(),
        last_progress: Instant::now(),
        last_phase: 0,
        entry: EntryVersion::capture(&plot.world),
        options: Default::default(),
        cacheable: false,
        monitor,
        worker: thread::spawn(move || work(world, worker_monitor)),
    });
}

fn blocked_compile(plot: &mut Plot) -> mpsc::Sender<()> {
    let (release, gate) = mpsc::channel();
    install_job(plot, move |world, monitor| {
        gate.recv_timeout(Duration::from_secs(20)).unwrap();
        let mut compiler = Compiler::default();
        let ticks = world.scheduler().iter_entries().collect();
        let result = compiler.compile(&world, world.get_corners(), Default::default(), ticks, monitor);
        CompilationResult { world, compiler, result }
    });
    release
}

#[test]
fn blocked_compiler_reports_phase_counts_and_elapsed_time_without_blocking_poll() {
    for phase_changed in [true, false] {
        let (mut plot, mut peer) = fixture(false);
        let release = blocked_compile(&mut plot);
        let job = plot.compilation.as_mut().unwrap();
        job.started = Instant::now() - Duration::from_secs(10);
        job.last_progress = Instant::now() - Duration::from_secs(if phase_changed { 1 } else { 4 });
        job.monitor.set_phase("source transition sampling");
        job.monitor.set_counts(120, 80, 4);
        job.monitor.set_progress(2, 10, "probes");
        if !phase_changed { job.last_phase = job.monitor.phase_number(); }
        plot.poll_redpiler_compilation();
        assert!(!plot.redpiler.is_active());
        assert!(plot.compilation.is_some());
        let (id, packet) = mchprs_network::test_support::read_frame(&mut peer, false).unwrap();
        plot.cancel_redpiler_compilation();
        release.send(()).unwrap();
        plot.wait_for_redpiler();
        assert_eq!(id, 0x72);
        let text = String::from_utf8_lossy(packet.get_ref());
        assert!(text.contains("source transition sampling"));
        assert!(text.contains("phase 1:"));
        assert!(text.contains("2/10 probes"));
        assert!(text.contains("120 nodes, 80 wires, 4 pistons"));
    }
}

#[test]
fn compilation_errors_report_phase_block_coordinates_and_specific_recovery() {
    let (mut plot, _peer) = fixture(false);
    let position = BlockPos::new(34, 20, 34);
    plot.world.set_block(position, Block::from_name("piston").unwrap());
    plot.world.piston_state.events.push_back(mchprs_world::PistonEvent {
        pos: position, sticky: false, facing: mchprs_blocks::BlockFace::East, action: mchprs_world::PistonAction::Extend,
    });
    let monitor = Arc::new(TaskMonitor::default());
    let error = Compiler::default().compile(&plot.world, plot.world.get_corners(),
        CompilerOptions { llvm: true, ..Default::default() }, vec![], monitor.clone()).unwrap_err();
    let text = compilation_error(&plot.world, &error, &monitor);
    assert!(text.contains("phase 1: entry validation"));
    assert!(text.contains("piston at (34, 20, 34)"));
    assert!(text.contains("settle"));
    assert!(crate::player::diagnostic_component(&text).to_string().contains("/hl 34 20 34"));
}

#[test]
fn activation_waits_for_poll_and_preserves_imported_deadlines() {
    let (mut plot, _peer) = fixture(false);
    let pos = BlockPos::new(34, 20, 34);
    plot.world.set_block(pos, Block::RedstoneLamp { lit: true });
    plot.world.schedule_tick(pos, 4, TickPriority::High);
    let pending: Vec<_> = plot.world.scheduler().iter_entries().collect();

    plot.start_redpiler(Default::default());
    assert!(plot.compilation.is_some());
    let monitor = plot.compilation.as_ref().unwrap().monitor.clone();
    assert!(!plot.redpiler.is_active());
    assert_eq!(plot.world.scheduler().iter_entries().collect::<Vec<_>>(), pending);
    assert!(!plot.tick(), "manual ticks must not advance the compilation input");

    plot.wait_for_redpiler();
    assert!(monitor.status().contains("1 nodes, 0 wires, 0 pistons"));
    assert!(plot.redpiler.is_active());
    assert!(plot.world.scheduler().iter_entries().next().is_none());
    plot.reset_redpiler().unwrap();
    assert_eq!(plot.world.scheduler().iter_entries().collect::<Vec<_>>(), pending);
}

#[test]
fn unchanged_plot_reuses_program_but_edits_and_pending_work_recompile() {
    let (mut plot, _peer) = fixture(false);
    let pos = BlockPos::new(34, 20, 34);
    plot.world.set_block(pos, Block::RedstoneLamp { lit: false });
    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(!plot.redpiler.stats().unwrap().cached);
    plot.reset_redpiler().unwrap();

    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(plot.redpiler.stats().unwrap().cached);
    plot.reset_redpiler().unwrap();

    plot.world.set_block(pos, Block::Stone {});
    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(!plot.redpiler.stats().unwrap().cached);
    plot.reset_redpiler().unwrap();

    plot.world.schedule_tick(pos, 4, TickPriority::High);
    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(!plot.redpiler.stats().unwrap().cached);
    plot.reset_redpiler().unwrap();

    let pending: Vec<_> = plot.world.scheduler().iter_entries().collect();
    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(plot.redpiler.stats().unwrap().cached);
    plot.reset_redpiler().unwrap();
    assert_eq!(plot.world.scheduler().iter_entries().collect::<Vec<_>>(), pending);

    plot.start_redpiler(CompilerOptions { io_only: true, ..Default::default() });
    plot.wait_for_redpiler();
    assert!(!plot.redpiler.stats().unwrap().cached);
}

#[test]
fn blocked_compiler_keeps_packets_and_reset_commands_responsive() {
    let (mut plot, _peer) = fixture(false);
    plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.redpiler.reset", "mchprs.build"]);
    let connection = mchprs_network::test_support::connection(false).unwrap();
    plot.players[0].client = connection.player;
    let received = Instant::now() - Duration::from_secs(20);
    plot.players[0].last_keep_alive_received = received;
    connection.incoming.send(Box::new(SKeepAlive { id: 7 })).unwrap();
    let release = blocked_compile(&mut plot);
    plot.tps = Tps::Unlimited;
    let logical_tick = plot.world.piston_state.logical_tick;

    plot.update();

    assert!(plot.players[0].last_keep_alive_received > received);
    assert!(plot.players[0].client.alive());
    assert_eq!(plot.world.piston_state.logical_tick, logical_tick);
    assert!(plot.compiling_redpiler());
    assert!(!plot.handle_command(0, "/rp", vec!["reset"]));
    assert!(!plot.compiling_redpiler());
    assert!(plot.compilation.as_ref().unwrap().monitor.cancelled());
    assert!(plot.tick(), "cancelled work must not freeze the world");
    release.send(()).unwrap();
    plot.wait_for_redpiler();
    assert!(!plot.redpiler.is_active());
}

#[test]
fn edits_and_new_pending_work_prevent_stale_activation() {
    for edit_block in [false, true] {
        let (mut plot, _peer) = fixture(false);
        let release = blocked_compile(&mut plot);
        let pos = BlockPos::new(34, 20, 34);
        if edit_block {
            plot.world.set_block(pos, Block::Stone {});
        } else {
            plot.world.schedule_tick(pos, 7, TickPriority::Higher);
        }
        let pending: Vec<_> = plot.world.scheduler().iter_entries().collect();
        release.send(()).unwrap();
        plot.wait_for_redpiler();
        assert!(!plot.redpiler.is_active());
        assert_eq!(plot.world.scheduler().iter_entries().collect::<Vec<_>>(), pending);
        assert_eq!(plot.world.get_block(pos), if edit_block { Block::Stone {} } else { Block::Air });
    }
}

#[test]
fn reset_discards_even_an_already_completed_compilation() {
    let (mut plot, _peer) = fixture(false);
    plot.start_redpiler(Default::default());
    let deadline = Instant::now() + Duration::from_secs(20);
    while !plot.compilation.as_ref().unwrap().worker.is_finished() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    plot.reset_redpiler().unwrap();
    plot.poll_redpiler_compilation();
    assert!(plot.compilation.is_none());
    assert!(!plot.redpiler.is_active());
}

#[test]
fn cancelling_and_recompiling_cannot_overlap_large_workers() {
    let (mut plot, _peer) = fixture(false);
    let release = blocked_compile(&mut plot);
    plot.reset_redpiler().unwrap();
    let original = plot.compilation.as_ref().unwrap().worker.thread().id();
    plot.start_redpiler(Default::default());
    assert_eq!(plot.compilation.as_ref().unwrap().worker.thread().id(), original);
    release.send(()).unwrap();
    plot.wait_for_redpiler();
    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(plot.redpiler.is_active());
}

#[test]
fn worker_panics_preserve_live_work_and_allow_retry() {
    let (mut plot, _peer) = fixture(false);
    let pos = BlockPos::new(34, 20, 34);
    plot.world.schedule_tick(pos, 3, TickPriority::High);
    let pending: Vec<_> = plot.world.scheduler().iter_entries().collect();
    install_job(&mut plot, |_, _| panic!("deliberate compiler failure"));
    plot.wait_for_redpiler();
    assert!(!plot.redpiler.is_active());
    assert_eq!(plot.world.scheduler().iter_entries().collect::<Vec<_>>(), pending);
    plot.start_redpiler(Default::default());
    plot.wait_for_redpiler();
    assert!(plot.redpiler.is_active());
}

#[test]
fn entry_versions_ignore_flushes_but_detect_piston_state_changes() {
    let (mut plot, _peer) = fixture(false);
    let entry = EntryVersion::capture(&plot.world);
    plot.world.flush_block_changes();
    assert!(entry.matches(&plot.world));
    plot.world.piston_state.logical_tick += 1;
    assert!(!entry.matches(&plot.world));
    let entry = EntryVersion::capture(&plot.world);
    plot.world.x += 1;
    assert!(!entry.matches(&plot.world));
}

#[test]
fn entry_versions_detect_edits_in_read_only_probe_context() {
    let (mut plot, _peer) = fixture(false);
    let entry = EntryVersion::capture(&plot.world);
    let selected_cell = BlockPos::new(16, 20, 34);
    let outside_selection = selected_cell.offset(mchprs_blocks::BlockFace::West);
    plot.world.set_block(outside_selection, Block::Stone {});
    assert!(!entry.matches(&plot.world));
}

#[test]
fn entry_versions_detect_entity_changes() {
    let (mut plot, _peer) = fixture(false);
    let pos = BlockPos::new(2, 20, 2);
    plot.world.set_block_entity(pos, BlockEntity::Comparator { output_strength: 5 });
    let entry = EntryVersion::capture(&plot.world);
    let Some(BlockEntity::Comparator { output_strength }) = plot.world.get_block_entity_mut(pos) else {
        panic!("comparator entity missing");
    };
    *output_strength = 7;
    assert!(!entry.matches(&plot.world));
}
