//! Opt-in production-path cache measurements; artifacts are written outside the checkout.
use super::*;
#[path = "../../../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
use crate::redpilator::PublicationMode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn selected(name: &str) -> cpus::Cpu {
    match name {
        "zcore" => cpus::CPUS[3],
        "pm1" => cpus::Cpu {
            schematic: "PM1_SORT_FIXED.schem",
            sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
            screen: Some(cpus::Screen {
                x: 230,
                top_y: 71,
                bottom_y: 40,
                left_z: 81,
                right_z: 112,
                sparse: false,
            }),
            ..cpus::CPUS[0]
        },
        _ => panic!("MCHPRS_CACHE_BENCH_CPU must be pm1 or zcore"),
    }
}

fn compile(plot: &mut Plot, options: &CompilerOptions, label: &str, rows: &mut Vec<Value>) {
    drop(plot.world.take_block_changes());
    eprintln!("CACHE_BENCH_BEGIN {label}");
    let started = Instant::now();
    plot.start_redpilator_with_budget(options.clone(), None);
    while plot.compilation.is_some() {
        assert!(started.elapsed() < Duration::from_secs(900), "{label}: compiler timed out");
        plot.poll_redpilator_compilation();
        thread::sleep(Duration::from_millis(1));
    }
    let wall = started.elapsed();
    assert!(plot.redpilator.is_active(), "{label}: compilation rejected");
    let stats = plot.redpilator.stats().unwrap();
    let row = json!({"case":label,"wall_ms":wall.as_secs_f64()*1000.0,
        "compiler_ms":stats.duration.as_secs_f64()*1000.0,"whole_program_cached":stats.cached,
        "sampling":plot.redpilator.source_sampling_stats(),"components":stats.components});
    eprintln!("CACHE_BENCH_RESULT {row}");
    rows.push(row);
}

fn restore(plot: &mut Plot, entry: &PlotWorld, program: &Arc<CachedProgram>, options: &CompilerOptions) {
    plot.reset_redpilator().unwrap();
    plot.world = entry.activation_snapshot();
    plot.redpilator_cache = Some(CompilationCache {
        entry: EntryVersion::capture(&plot.world),
        options: options.clone(),
        program: program.clone(),
        suspended: None,
    });
}

fn output(world: &PlotWorld, cpu: cpus::Cpu) -> Value {
    json!({"screen":cpus::screen(world,cpu),"chat":world.command_output().collect::<Vec<_>>()})
}

fn state_hash(world: &PlotWorld) -> String {
    let mut hash = Sha256::new();
    for chunk in world.get_chunks() {
        hash.update(chunk.x.to_le_bytes());
        hash.update(chunk.z.to_le_bytes());
        for (section, data) in chunk.sections.iter().enumerate() {
            hash.update(data.block_count().to_le_bytes());
            if data.block_count() == 0 {
                continue;
            }
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        hash.update(chunk.get_block(x, section as u32 * 16 + y, z).to_le_bytes());
                    }
                }
            }
        }
        let mut entities: Vec<_> = chunk.block_entities.iter().collect();
        entities.sort_by_key(|(pos, _)| (pos.x, pos.y, pos.z));
        for (pos, entity) in entities {
            hash.update(bincode::serialize(&(pos.x, pos.y, pos.z, entity)).unwrap());
        }
    }
    hash.update(bincode::serialize(world.piston_state()).unwrap());
    hash.update(bincode::serialize(&world.scheduler().iter_entries().collect::<Vec<_>>()).unwrap());
    format!("{:x}", hash.finalize())
}

fn start(world: &mut PlotWorld, cpu: cpus::Cpu, compiler: &mut Compiler, prelude: u32) {
    cpus::click_cpu(world, cpu, cpu.start);
    compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
    for _ in 0..prelude {
        world.tick_interpreted();
        compiler.tick().unwrap();
    }
    if prelude != 0 {
        let run = BlockPos::new(207, 10, 154);
        cpus::click_cpu(world, cpu, run);
        compiler.try_on_use_block(cpu.origin + run).unwrap();
    }
}

fn validation(plot: &mut Plot, entry: &PlotWorld, cpu: cpus::Cpu, ticks: u32, prelude: u32) -> Value {
    let mut native = entry.activation_snapshot();
    start(&mut native, cpu, &mut plot.redpilator, prelude);
    let mut hash = Sha256::new();
    let mut first = None;
    for tick in 1..=ticks {
        native.tick_interpreted();
        plot.redpilator.tick().unwrap();
        plot.redpilator.try_flush(&mut plot.world).unwrap();
        let actual = output(&plot.world, cpu);
        hash.update(serde_json::to_vec(&actual).unwrap());
        if first.is_none() && actual != output(&native, cpu) {
            first = Some(tick);
        }
        if tick % 1000 == 0 {
            eprintln!("CACHE_BENCH_VALIDATION tick={tick}/{ticks} first_difference={first:?}");
        }
    }
    plot.reset_redpilator().unwrap();
    json!({"ticks":ticks,"first_output_difference":first,"output_matches_interpreter":first.is_none(),
        "trace_sha256":format!("{:x}",hash.finalize()),"final_state_sha256":state_hash(&plot.world),
        "messages":plot.world.command_output().count(),"final_output":output(&plot.world,cpu)})
}

fn quiet(world: &PlotWorld) -> bool {
    world.scheduler().iter_entries().next().is_none() && world.piston_state().events.is_empty() && world.piston_state().motions.is_empty()
}

#[test]
#[ignore = "PM1/ZCore production-path cache benchmark; set MCHPRS_CACHE_BENCH_OUTPUT and CPU"]
fn compilation_cache_scenarios() {
    let name = std::env::var("MCHPRS_CACHE_BENCH_CPU").unwrap_or_else(|_| "zcore".into());
    let path = std::path::PathBuf::from(std::env::var_os("MCHPRS_CACHE_BENCH_OUTPUT").expect("output path required"));
    let ticks: u32 = std::env::var("MCHPRS_CACHE_BENCH_TICKS").map_or(2048, |s| s.parse().unwrap());
    let repeats: u32 = std::env::var("MCHPRS_CACHE_BENCH_REPEATS").map_or(2, |s| s.parse().unwrap());
    let runtime_only = std::env::var_os("MCHPRS_CACHE_BENCH_RUNTIME_ONLY").is_some();
    let latency_only = std::env::var_os("MCHPRS_CACHE_BENCH_LATENCY_ONLY").is_some();
    assert!(!(runtime_only && latency_only));
    assert!(ticks > 0 && repeats > 0);
    let cpu = selected(&name);
    let baseline = cpus::load_cpu(cpu);
    assert!(quiet(&baseline), "benchmark starts at a stationary boundary");
    let mut prelude = 0;
    if name == "zcore" {
        let mut native = baseline.activation_snapshot();
        cpus::click_cpu(&mut native, cpu, cpu.start);
        let mut idle = 0;
        for tick in 1..=4096 {
            native.tick_interpreted();
            idle = if quiet(&native) { idle + 1 } else { 0 };
            if idle == 20 {
                prelude = tick;
                break;
            }
        }
        assert!(prelude > 0, "ZCore load did not settle");
    }
    let connection = mchprs_network::test_support::connection(false).unwrap();
    let _peer = connection.peer;
    let mut plot = benchmark::with_client(baseline.activation_snapshot(), benchmark::player(connection.player), Tps::Limited(0));
    plot.owner = Some(plot.players[0].uuid);
    plot.players[0].set_test_permissions(&["commands.redpilator.reset"]);
    let options = CompilerOptions {
        publication: Some(PublicationMode::All),
        ..Default::default()
    };
    let mut rows = Vec::new();
    let mut checks = Vec::new();
    let save = |rows: &Vec<Value>, checks: &Vec<Value>| {
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({"cpu":name,"fixture":cpu.schematic,
            "fixture_sha256":cpu.sha256,"rayon_threads":std::env::var("RAYON_NUM_THREADS").ok(),
            "prelude_ticks":prelude,"runtime_only":runtime_only,"latency_only":latency_only,"samples":rows,"checks":checks}))
            .unwrap(),
        )
        .unwrap();
    };
    compile(&mut plot, &options, "initial_cold", &mut rows);
    let original = plot.redpilator.cached_program().unwrap();
    if !runtime_only {
        if !latency_only {
            for _ in 0..3 {
                plot.reset_redpilator().unwrap();
                compile(&mut plot, &options, "unchanged_internal_handoff", &mut rows);
                assert!(plot.redpilator.stats().unwrap().cached);
            }
            save(&rows, &checks);

            restore(&mut plot, &baseline, &original, &options);
            let unrelated = BlockPos::new(400, 20, 400);
            assert_eq!(plot.world.get_block(unrelated), Block::Air);
            plot.world.set_block(unrelated, Block::Stone {});
            compile(&mut plot, &options, "unrelated_chunk_edit", &mut rows);
            assert!(!plot.redpilator.stats().unwrap().cached);
            let (hits, misses, _) = plot.redpilator.source_sampling_stats().unwrap();
            assert!(hits > 0 && misses == 0, "unrelated chunk should reuse all source observations");
            save(&rows, &checks);
        }

        let cut = if name == "pm1" {
            BlockPos::new(165, 135, 125)
        } else {
            let mut found = None;
            'scan: for y in 8..128 {
                for z in 8..256 {
                    for x in 8..256 {
                        let pos = BlockPos::new(x, y, z);
                        if !matches!(baseline.get_block(pos), Block::RedstoneWire { .. }) {
                            continue;
                        }
                        let mut candidate = baseline.activation_snapshot();
                        candidate.set_block(pos, Block::Air);
                        redstone::update_surrounding_blocks(&mut candidate, pos);
                        if quiet(&candidate) {
                            found = Some(pos);
                            break 'scan;
                        }
                    }
                }
            }
            found.expect("no stationary wire cut")
        };
        let mut edited = baseline.activation_snapshot();
        assert!(matches!(edited.get_block(cut), Block::RedstoneWire { .. }));
        edited.set_block(cut, Block::Air);
        redstone::update_surrounding_blocks(&mut edited, cut);
        assert!(quiet(&edited), "wire deletion must not require advancing the context clock");
        checks.push(json!({"case":"wire_cut_entry","position":[cut.x,cut.y,cut.z],"clock":edited.piston_state().logical_tick}));
        for pair in 0..repeats {
            let order = if pair % 2 == 0 { [true, false] } else { [false, true] };
            let mut validated = Vec::new();
            for reuse in order {
                restore(&mut plot, &baseline, &original, &options);
                plot.world = edited.activation_snapshot();
                if !reuse {
                    plot.clear_redpilator_cache();
                }
                let label = if reuse { "wire_cut_incremental" } else { "wire_cut_forced_cold" };
                compile(&mut plot, &options, label, &mut rows);
                assert!(!plot.redpilator.stats().unwrap().cached);
                rows.last_mut().unwrap()["pair"] = json!(pair);
                if pair == 0 && !latency_only {
                    let limit = if name == "pm1" { 13000 } else { ticks };
                    let mut result = validation(&mut plot, &edited, cpu, limit, prelude);
                    result["case"] = json!(label);
                    validated.push(result.clone());
                    checks.push(result);
                }
                save(&rows, &checks);
            }
            if pair == 0 && !latency_only {
                assert_eq!(
                    validated[0]["trace_sha256"], validated[1]["trace_sha256"],
                    "incremental/cold output trace"
                );
                assert_eq!(
                    validated[0]["final_state_sha256"], validated[1]["final_state_sha256"],
                    "incremental/cold full final state"
                );
            }
        }
        if latency_only {
            return;
        }

        restore(&mut plot, &baseline, &original, &options);
        compile(&mut plot, &options, "before_idle_roundtrip", &mut rows);
        for _ in 0..3 {
            plot.redpilator.tick().unwrap();
        }
        plot.reset_redpilator().unwrap();
        for _ in 0..3 {
            plot.world.tick_interpreted();
        }
        assert!(quiet(&plot.world));
        compile(&mut plot, &options, "after_compiled_and_interpreter_ticks", &mut rows);
        assert_eq!(
            plot.redpilator.source_sampling_stats().unwrap().0,
            0,
            "clock change must invalidate source cache"
        );
        save(&rows, &checks);

        restore(&mut plot, &baseline, &original, &options);
        compile(&mut plot, &options, "before_manual_reset", &mut rows);
        let reset = Instant::now();
        plot.handle_command(0, "/rp", vec!["reset"]);
        rows.push(json!({"case":"manual_reset","wall_ms":reset.elapsed().as_secs_f64()*1000.0}));
        assert!(plot.redpilator_cache.is_none());
        compile(&mut plot, &options, "after_manual_reset_cold", &mut rows);
        assert!(!plot.redpilator.stats().unwrap().cached);
        assert_eq!(plot.redpilator.source_sampling_stats().unwrap().0, 0);
        let clear = Instant::now();
        plot.handle_command(0, "/rp", vec!["clear"]);
        rows.push(json!({"case":"manual_clear","wall_ms":clear.elapsed().as_secs_f64()*1000.0}));
        assert!(plot.redpilator_cache.is_none());
        save(&rows, &checks);
    }

    restore(&mut plot, &baseline, &original, &options);
    let native_options = CompilerOptions {
        llvm: true,
        ..options.clone()
    };
    compile(&mut plot, &native_options, "switch_to_llvm_cold", &mut rows);
    assert!(!plot.redpilator.stats().unwrap().cached);
    let native_program = plot.redpilator.cached_program().unwrap();
    save(&rows, &checks);
    let mut reference = None;
    for (backend, program, flags) in [("rust", original, options), ("llvm", native_program, native_options)] {
        for mode in [
            None,
            Some(PublicationMode::All),
            Some(PublicationMode::Redstone),
            Some(PublicationMode::Io),
        ] {
            for sample in 0..3 {
                restore(&mut plot, &baseline, &program, &flags);
                let mut setup = Vec::new();
                compile(&mut plot, &flags, "runtime_setup_cached", &mut setup);
                assert!(plot.redpilator.stats().unwrap().cached);
                if let Some(mode) = mode {
                    assert!(plot.redpilator.set_publication_mode(mode));
                }
                plot.redpilator.try_on_use_block(cpu.origin + cpu.start).unwrap();
                for _ in 0..prelude {
                    plot.redpilator.tick().unwrap();
                }
                if prelude != 0 {
                    plot.redpilator.try_on_use_block(cpu.origin + BlockPos::new(207, 10, 154)).unwrap();
                }
                let timer = Instant::now();
                for _ in 0..ticks {
                    plot.redpilator.tick().unwrap();
                    if mode.is_some() {
                        plot.redpilator.try_flush(&mut plot.world).unwrap();
                    }
                }
                let elapsed = timer.elapsed();
                plot.reset_redpilator().unwrap();
                let actual = output(&plot.world, cpu);
                if let Some(expected) = &reference {
                    assert_eq!(&actual, expected, "backend/publication outcome");
                } else {
                    reference = Some(actual);
                }
                rows.push(
                    json!({"case":"runtime","backend":backend,"publication":mode.map_or("none",PublicationMode::name),
                    "sample":sample,"ticks":ticks,"wall_ms":elapsed.as_secs_f64()*1000.0,"tps":ticks as f64/elapsed.as_secs_f64()}),
                );
                save(&rows, &checks);
            }
        }
    }
    assert!(
        checks.iter().all(|check| check["output_matches_interpreter"] != json!(false)),
        "interpreter output divergence; see report"
    );
}
