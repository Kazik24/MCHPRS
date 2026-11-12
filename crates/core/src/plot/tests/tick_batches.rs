use super::*;

#[test]
fn publication_clock_keeps_cadence_and_skips_expired_frames() {
    let start = Instant::now();
    let period = Duration::from_millis(100);
    assert_eq!(publication_clock(start, start + period, period), start + period);
    assert_eq!(publication_clock(start, start + Duration::from_millis(105), period), start + period);
    assert_eq!(
        publication_clock(start, start + Duration::from_millis(450), period),
        start + Duration::from_millis(400)
    );
}

#[test]
fn native_batches_require_high_configured_and_observed_rate() {
    for uses_llvm in [false, true] {
        for tps in [Tps::Limited(0), Tps::Limited(20), Tps::Limited(99), Tps::Limited(100), Tps::Unlimited] {
            for nspt in [
                None,
                Some(Duration::ZERO),
                Some(Duration::from_millis(10)),
                Some(Duration::from_nanos(10_000_001)),
            ] {
                let eligible = uses_llvm
                    && matches!(tps, Tps::Unlimited | Tps::Limited(100..))
                    && nspt.is_none_or(|duration| duration <= Duration::from_millis(10));
                assert_eq!(normal_tick_batch_width(tps, nspt, uses_llvm), if eligible { 16 } else { 1 });
            }
        }
    }
}

#[test]
fn native_budget_waits_for_full_credit_and_does_not_starve_at_small_caps() {
    for available in 0..64 {
        assert_eq!(normal_tick_budget(available, 1, 16), if available >= 16 { 16 } else { 0 });
        assert_eq!(normal_tick_budget(available, 63, 16), (available.min(63) / 16 * 16) as u32);
        assert_eq!(normal_tick_budget(available, 7, 1), available.min(7) as u32);
    }
    assert_eq!(normal_tick_budget(u64::MAX, u64::MAX, 16), 50_000);
    assert_eq!(normal_tick_budget(u64::MAX, u64::MAX, 1), 50_000);
}

#[test]
fn llvm_normal_update_batches_but_exact_and_stopped_ticks_do_not() {
    let (mut plot, _peer) = client_sync_tests::fixture(false);
    let connection = mchprs_network::test_support::connection(false).unwrap();
    let _incoming = connection.incoming;
    let _live_peer = connection.peer;
    plot.players[0].client = connection.player;
    plot.world.packet_senders.clear();
    plot.world.packet_senders.push(PlayerPacketSender::new(&plot.players[0].client));
    let lever = BlockPos::new(34, 21, 32);
    plot.world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    plot.world.set_block(
        lever,
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(mchprs_blocks::blocks::LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
        },
    );
    plot.world.set_block(lever.offset(BlockFace::East), Block::RedstoneLamp { lit: false });
    let bounds = (BlockPos::new(30, 19, 30), BlockPos::new(38, 23, 38));
    plot.redpilator
        .compile(
            &plot.world,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    assert!(plot.redpilator.uses_llvm());
    plot.tps = Tps::Unlimited;
    plot.last_nspt = None;
    plot.world_send_rate = WorldSendRate(0);
    let initial_clock = plot.world.piston_state().logical_tick;
    let initial_ticks = plot.world.update_stats.simulated_ticks;
    plot.update();
    assert_eq!(plot.world.update_stats.simulated_ticks - initial_ticks, 16);
    assert!(plot.tick());
    plot.tps = Tps::Limited(0);
    plot.update();
    assert_eq!(plot.world.update_stats.simulated_ticks - initial_ticks, 16);
    plot.redpilator.try_reset(&mut plot.world, bounds).unwrap();
    assert_eq!(plot.world.piston_state().logical_tick, initial_clock + 17);
}

#[test]
fn benchmark_tick_limits_preserve_native_batch_tails() {
    let (mut plot, _old_peer) = client_sync_tests::fixture(false);
    let connection = mchprs_network::test_support::connection(false).unwrap();
    let _incoming = connection.incoming;
    let _peer = connection.peer;
    plot.players[0].client = connection.player;
    plot.world.packet_senders.clear();
    plot.world.packet_senders.push(PlayerPacketSender::new(&plot.players[0].client));
    plot.world.set_block(
        BlockPos::new(32, 21, 32),
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(mchprs_blocks::blocks::LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
        },
    );
    plot.world.set_block(BlockPos::new(33, 21, 32), Block::RedstoneLamp { lit: false });
    plot.redpilator
        .compile(
            &plot.world,
            (BlockPos::new(30, 19, 30), BlockPos::new(38, 23, 38)),
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    let clock = plot.world.piston_state().logical_tick;
    let mut total = 0;
    for batch in [16, 32, 64] {
        for ticks in [1, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
            benchmark::run_ticks_batched(&mut plot, ticks, batch);
            total += u64::from(ticks);
            assert_eq!(plot.world.update_stats.simulated_ticks, u64::from(ticks));
            if ticks >= batch {
                assert_eq!(plot.world.update_stats.max_native_batch, batch);
            }
        }
    }
    plot.redpilator
        .try_reset(&mut plot.world, (BlockPos::new(30, 19, 30), BlockPos::new(38, 23, 38)))
        .unwrap();
    assert_eq!(plot.world.piston_state().logical_tick, clock + total);
}
