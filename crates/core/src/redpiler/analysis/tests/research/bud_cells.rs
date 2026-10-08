//! Opt-in evidence for extracted author cells; no admission overrides.
use super::*;
use crate::redpiler::instant::{clocked, logic, regions, sampling};
use crate::redstone::instant_piston_tests::capture_at;
use rustc_hash::FxHashSet;

fn fixture(name: &str) -> Value {
    let directory = if name == "BUD_VERTICAL_ERROR_UPDATE" {
        "test_data/piston-research/bud-vertical-20261008"
    } else {
        "test_data/piston-research/bud-cells-20261008"
    };
    let inspection: Value = serde_json::from_slice(
        &std::fs::read(root().join(format!(
            "{directory}/inspection/{}.json",
            name.to_lowercase()
        )))
        .unwrap(),
    )
    .unwrap();
    json!({"fixture": format!("{directory}/{name}.schem"),
        "sha256": inspection["sha256"], "dimensions": inspection["dimensions"],
        "loader_offset": inspection["loader_offset"]})
}

fn state(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Value {
    let mut blocks = Vec::new();
    crate::world::for_each_block_optimized(world, bounds.0, bounds.1, |pos| {
        if world.get_block(pos) != Block::Air {
            blocks.push(block_state(world, pos, BASE));
        }
    });
    json!({"blocks": blocks, "tick": world.piston_state().logical_tick,
        "phase": world.piston_state().phase, "events": world.piston_state().events,
        "motions": world.piston_state().motions,
        "scheduled": world.scheduler().iter_entries().collect::<Vec<_>>()})
}

fn step(
    world: &mut PlotWorld,
    bounds: (BlockPos, BlockPos),
    label: &str,
    click: Option<BlockPos>,
    ticks: usize,
) -> Value {
    let positions: Vec<_> = (bounds.0.y..=bounds.1.y)
        .flat_map(|y| {
            (bounds.0.z..=bounds.1.z)
                .flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z)))
        })
        .collect();
    let before = state(world, bounds);
    let mut samples = Vec::new();
    let mut frames = Vec::new();
    let callbacks = capture_at(&positions, || {
        samples.extend(trace::capture(|| {
            if let Some(pos) = click {
                let pos = BASE + pos;
                let Block::Lever { lever } = world.get_block(pos) else {
                    panic!("missing lever at {pos}")
                };
                lever_action(world, pos, !lever.powered);
            }
        }));
        frames.push(state(world, bounds));
        for _ in 0..ticks {
            samples.extend(trace::capture(|| world.tick_interpreted()));
            frames.push(state(world, bounds));
        }
    });
    json!({"label": label, "click_local": click, "ticks": ticks,
        "before": before, "frames": frames, "callbacks": callbacks, "samples": samples})
}

fn compare(fixture: &Value, bounds: (BlockPos, BlockPos), schedule: &[Value]) -> Vec<Value> {
    let mut comparisons = Vec::new();
    for budget in [1, 8] {
        for (optimize, assume_instant) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (mut world, _) = load(fixture);
            let mut compiler = Compiler::default();
            let monitor = std::sync::Arc::new(TaskMonitor::default());
            monitor.set_budget_multiplier(budget);
            let result = compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    optimize,
                    assume_instant,
                    budget_multiplier: budget,
                    ..Default::default()
                },
                vec![],
                monitor,
            );
            if let Err(error) = result {
                comparisons.push(json!({"budget":budget,"optimize":optimize,"assume_instant":assume_instant,"error":error.to_string()}));
                continue;
            }
            let mut phases = Vec::new();
            for action in schedule {
                if let Ok(pos) = serde_json::from_value::<BlockPos>(action["click_local"].clone()) {
                    compiler.on_use_block(BASE + pos);
                }
                compiler.flush(&mut world);
                let mut frames = vec![state(&world, bounds)];
                for _ in 0..action["ticks"].as_u64().unwrap() {
                    compiler.tick_with_world(&mut world);
                    compiler.flush(&mut world);
                    frames.push(state(&world, bounds));
                }
                phases.push(json!({"label":action["label"],"frames":frames,
                    "logical_stats":compiler.backend.as_ref().unwrap().logical_stats()}));
            }
            comparisons.push(json!({"budget":budget,"optimize":optimize,"assume_instant":assume_instant,"phases":phases}));
        }
    }
    comparisons
}

#[test]
#[ignore = "counter comparison diagnostics; fresh output required"]
fn investigate_counter_bud_sources() {
    let output = root().join(std::env::var("MCHPRS_BUD_COUNTER_OUTPUT").unwrap());
    assert!(!output.exists());
    let (world, _, fixture) = super::super::fixture("counter_basic");
    let report = analyze_world(&world);
    let (_, programs) = crate::redpiler::instant::program::prepare(
        &world,
        &report,
        &[],
        &CompilerOptions::default(),
        Default::default(),
    )
    .unwrap();
    let mut diagnostics = Vec::new();
    for program in programs {
        let Some(clock) = program.clocked.as_ref() else {
            continue;
        };
        let logic = &program.logic;
        diagnostics.push(json!({"clock":program.pistons[clock.clock].pos,"memory_count":clock.memory.len(),
            "sources":logic.response_sources.iter().map(|&p|block_state(&world,p,BASE)).collect::<Vec<_>>(),
            "clock_dependencies":format!("{:?}",crate::redpiler::instant::sequential::dependencies(&logic.arena,[logic.responses[clock.clock]]))}));
    }
    let mut attempts = Vec::new();
    for budget_multiplier in [1, 8] {
        for (optimize, assume_instant) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut compiler = Compiler::default();
            let start = Instant::now();
            let result = compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    budget_multiplier,
                    optimize,
                    assume_instant,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            );
            assert!(result.is_ok(), "{result:?}");
            attempts.push(json!({"budget":budget_multiplier,"optimize":optimize,"assume_instant":assume_instant,"elapsed_us":start.elapsed().as_micros(),"active":compiler.is_active()}));
        }
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(
            &json!({"fixture":fixture,"diagnostics":diagnostics,"compilation":attempts}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "read-only compiler diagnostics and native author-cell traces; fresh output required"]
fn investigate_extracted_bud_cells() {
    let output = root().join(std::env::var("MCHPRS_BUD_CELL_OUTPUT").unwrap());
    assert!(!output.exists(), "preserve existing investigation evidence");
    let mut records = Vec::new();
    for name in [
        "BUD_MEMORY_PM1",
        "BUD_MEMORY_ANPU_NONINSTANT",
        "BUD_MEMORY_PISTION_UPDATE_INSTANT",
        "BUD_VERTICAL_ERROR_UPDATE",
    ] {
        if std::env::var("MCHPRS_BUD_CELL_CASE").is_ok_and(|case| case != name) {
            continue;
        }
        let fixture = fixture(name);
        let (world, bounds) = load(&fixture);
        let initial = state(&world, bounds);
        let mut attempts = Vec::new();
        for (selection, compile_bounds) in
            [("schematic", bounds), ("whole_plot", world.get_corners())]
        {
            for budget in [1, 8] {
                for (optimize, assume_instant) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    let monitor = std::sync::Arc::new(TaskMonitor::default());
                    monitor.set_budget_multiplier(budget);
                    let mut compiler = Compiler::default();
                    let start = Instant::now();
                    let result = compiler.compile(
                        &world,
                        compile_bounds,
                        CompilerOptions {
                            optimize,
                            assume_instant,
                            budget_multiplier: budget,
                            ..Default::default()
                        },
                        world.scheduler().iter_entries().collect(),
                        monitor,
                    );
                    attempts.push(json!({"selection":selection,"bounds":compile_bounds,"budget":budget,"optimize":optimize,"assume_instant":assume_instant,
                    "elapsed_us":start.elapsed().as_micros(),"error":result.err().map(|e|e.to_string()),
                    "active":compiler.is_active()}));
                    assert_eq!(state(&world, bounds), initial, "compilation mutated source");
                }
            }
        }
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let report = analyze(
            &world,
            world.get_corners(),
            &[],
            &monitor,
            AnalysisLimits::for_budget(8),
        )
        .unwrap();
        let mut classified = Vec::new();
        for region in regions::split(&world, &report, &monitor).unwrap() {
            let mut diagnostic = json!({"pistons":region.pistons});
            let clock = clocked::recognize(&world, &region, &monitor, false);
            match clock {
                Err(e) => {
                    diagnostic["clock_error"] = json!(e);
                }
                Ok(clock) => {
                    diagnostic["clock"] =
                        json!(clock.as_ref().map(|c| region.pistons[c.clock].pos));
                    let resets = sampling::reset_candidates(&world, &region);
                    match sampling::recognize(&world, &region, &monitor, clock.as_ref(), &resets) {
                        Err(e) => {
                            diagnostic["sampling_error"] = json!(e);
                        }
                        Ok(mut independent) => {
                            let mut memory: FxHashSet<_> =
                                independent.memory.iter().map(|m| m.actor).collect();
                            let mut owned = crate::redpiler::analysis::families::reset_internals(
                                &region.recognition,
                            );
                            let mut reset_owned = resets;
                            if let Some(c) = &clock {
                                memory.extend(c.memory.iter().map(|m| m.actor));
                                reset_owned.extend(c.observers.iter().copied());
                            }
                            owned.extend(reset_owned.iter().copied());
                            for p in &region.pistons {
                                owned.extend([p.pos, p.head, p.payload]);
                            }
                            diagnostic["memory_actors"] =
                                json!(memory.iter().copied().collect::<Vec<_>>());
                            diagnostic["generators"] = json!(independent.generators);
                            diagnostic["sampling"] = json!(independent.events.iter().map(|e| {
                                let source = match &e.source {
                                    sampling::SamplingSource::Generator(id) => json!({"generator":id}),
                                    sampling::SamplingSource::Power {writer,pos,initial,..} => json!({"writer":writer,"pos":pos,"initial":initial}),
                                };
                                json!({"source":source,"targets":e.targets.iter().map(|t|json!({"actor":t.actor,"requires_extended":t.requires_extended})).collect::<Vec<_>>()})
                            }).collect::<Vec<_>>());
                            match logic::extract_ideal_with_state(
                                &world,
                                &region,
                                &monitor,
                                memory,
                                clock.as_ref().map(|c| c.clock),
                                &owned,
                                &reset_owned,
                                &independent.generators,
                                &mut independent.events,
                            ) {
                                Err(e) => {
                                    diagnostic["extraction_error"] = json!(e);
                                }
                                Ok(logic) => {
                                    diagnostic["response_dependencies"] = json!(logic
                                        .responses
                                        .iter()
                                        .map(|&root| format!(
                                            "{:?}",
                                            crate::redpiler::instant::sequential::dependencies(
                                                &logic.arena,
                                                [root]
                                            )
                                        ))
                                        .collect::<Vec<_>>());
                                    diagnostic["response_sources"] = json!(logic
                                        .response_sources
                                        .iter()
                                        .map(|&p| block_state(&world, p, BASE))
                                        .collect::<Vec<_>>());
                                    diagnostic["sampling_validation"] = json!(sampling::validate(
                                        &independent.events,
                                        &logic,
                                        &region
                                    )
                                    .err());
                                    diagnostic["clock_validation"] = json!(clock
                                        .as_ref()
                                        .and_then(|c| c.validate(&world, &logic).err()));
                                }
                            }
                        }
                    }
                }
            }
            // Also inspect independent sampling directly if specialization rejected.
            let generic = sampling::recognize(
                &world,
                &region,
                &monitor,
                None,
                &sampling::reset_candidates(&world, &region),
            );
            diagnostic["generic_sampling"] = json!(generic.map(|mut c| {
                let mut result = json!({"generators":c.generators,
                    "memory":c.memory.iter().map(|m|m.actor).collect::<Vec<_>>(),"events":c.events.len()});
                if diagnostic.get("clock_error").is_some() {
                    let reset_owned = sampling::reset_candidates(&world,&region);
                    let mut owned = crate::redpiler::analysis::families::reset_internals(&region.recognition);
                    owned.extend(reset_owned.iter().copied());
                    for p in &region.pistons {owned.extend([p.pos,p.head,p.payload]);}
                    match logic::extract_ideal_with_state(&world,&region,&monitor,c.memory.iter().map(|m|m.actor).collect(),
                        None,&owned,&reset_owned,&c.generators,&mut c.events) {
                        Err(error) => {result["extraction_error"] = json!(error);}
                        Ok(logic) => {
                            result["response_sources"] = json!(logic.response_sources.iter().map(|&p|block_state(&world,p,BASE)).collect::<Vec<_>>());
                            result["sampling_validation"] = json!(sampling::validate(&c.events,&logic,&region).err());
                            result["sampling"] = json!(c.events.iter().map(|e|match &e.source {
                                sampling::SamplingSource::Generator(id) => json!({"generator":id,"targets":e.targets.iter().map(|t|t.actor).collect::<Vec<_>>()}),
                                sampling::SamplingSource::Power {writer,pos,..} => json!({"writer":writer,"pos":pos,"targets":e.targets.iter().map(|t|t.actor).collect::<Vec<_>>()}),
                            }).collect::<Vec<_>>());
                        }
                    }
                }
                result
            }).map_err(|e|e.to_string()));
            classified.push(diagnostic);
        }
        let (mut world, bounds) = load(&fixture);
        let mut native = vec![step(&mut world, bounds, "original saved idle", None, 12)];
        let (data, trigger) = if name == "BUD_MEMORY_PM1" {
            (
                vec![
                    BlockPos::new(6, 5, 2),
                    BlockPos::new(6, 5, 4),
                    BlockPos::new(6, 5, 6),
                ],
                BlockPos::new(0, 2, 0),
            )
        } else if name == "BUD_MEMORY_ANPU_NONINSTANT" {
            (
                vec![BlockPos::new(1, 6, 3), BlockPos::new(1, 6, 5)],
                BlockPos::new(4, 1, 0),
            )
        } else if name == "BUD_MEMORY_PISTION_UPDATE_INSTANT" {
            (
                vec![BlockPos::new(1, 5, 3), BlockPos::new(1, 5, 5)],
                BlockPos::new(4, 5, 1),
            )
        } else {
            (
                vec![
                    BlockPos::new(6, 4, 4),
                    BlockPos::new(6, 4, 6),
                    BlockPos::new(13, 5, 3),
                    BlockPos::new(13, 5, 5),
                    BlockPos::new(13, 5, 7),
                ],
                BlockPos::new(7, 3, 0),
            )
        };
        for &p in &data {
            native.push(step(
                &mut world,
                bounds,
                "data changes without trigger",
                Some(p),
                8,
            ));
        }
        native.push(step(
            &mut world,
            bounds,
            "trigger first edge / start",
            Some(trigger),
            24,
        ));
        native.push(step(
            &mut world,
            bounds,
            "trigger second edge / stop",
            Some(trigger),
            12,
        ));
        for &p in &data {
            native.push(step(
                &mut world,
                bounds,
                "opposite data without trigger",
                Some(p),
                8,
            ));
        }
        native.push(step(
            &mut world,
            bounds,
            "trigger third edge / restart",
            Some(trigger),
            24,
        ));
        native.push(step(
            &mut world,
            bounds,
            "trigger fourth edge / stop",
            Some(trigger),
            12,
        ));
        native.push(step(
            &mut world,
            bounds,
            "repeated trigger first edge",
            Some(trigger),
            12,
        ));
        native.push(step(
            &mut world,
            bounds,
            "repeated trigger second edge",
            Some(trigger),
            12,
        ));
        let compiled = compare(&fixture, bounds, &native);
        let mut controlled = Vec::new();
        for protocol in [
            "cancel_trigger_before_execution",
            "data_after_delivery_before_execution",
            "data_between_pulse_edges",
        ] {
            let (mut world, bounds) = load(&fixture);
            let mut episode = vec![step(&mut world, bounds, "original saved idle", None, 12)];
            if protocol == "cancel_trigger_before_execution" {
                episode.push(step(&mut world, bounds, "trigger queued", Some(trigger), 0));
                episode.push(step(
                    &mut world,
                    bounds,
                    "trigger cancelled before event phase",
                    Some(trigger),
                    12,
                ));
            } else {
                episode.push(step(
                    &mut world,
                    bounds,
                    "trigger queued",
                    Some(trigger),
                    if protocol == "data_between_pulse_edges" {
                        1
                    } else {
                        0
                    },
                ));
                for &p in &data {
                    episode.push(step(
                        &mut world,
                        bounds,
                        "data changed after notification",
                        Some(p),
                        0,
                    ));
                }
                episode.push(step(&mut world, bounds, "finish pending work", None, 18));
            }
            let compiled = compare(&fixture, bounds, &episode);
            controlled.push(json!({"protocol":protocol,"native":episode,"compiled":compiled}));
        }
        // Actual head callbacks, fetched from the live world, in both saved and
        // interpreter-established committed poses. No synthetic power is used.
        let cells: Vec<_> = report
            .pistons
            .iter()
            .filter(|p| p.piston.sticky)
            .map(|p| p.pos)
            .collect();
        let mut heads = Vec::new();
        for &cell in &cells {
            let Block::Piston { piston } = world.get_block(cell) else {
                unreachable!()
            };
            let head = cell.offset(piston.facing.into());
            for extended in [false, true] {
                let (mut world, bounds) = load(&fixture);
                for &p in &data {
                    lever_action(&mut world, BASE + p, extended);
                }
                let active = name == "BUD_MEMORY_ANPU_NONINSTANT";
                lever_action(&mut world, BASE + trigger, active);
                for _ in 0..18 {
                    world.tick_interpreted();
                }
                lever_action(&mut world, BASE + trigger, !active);
                for _ in 0..6 {
                    world.tick_interpreted();
                }
                assert!(
                    matches!(world.get_block(cell),Block::Piston {piston} if piston.extended==extended)
                );
                for &p in &data {
                    lever_action(&mut world, BASE + p, !extended);
                }
                let head_block = world.get_block(head);
                let mut samples = Vec::new();
                let callbacks = capture_at(&[cell, head], || {
                    samples = trace::capture(|| {
                        crate::redstone::update(
                            world.get_block(head),
                            &mut world,
                            head,
                            Some(BlockFace::South),
                        );
                        for _ in 0..6 {
                            world.tick_interpreted();
                        }
                    });
                });
                let sampled = samples
                    .iter()
                    .any(|e| matches!(e.operation,trace::Operation::Sample {pos,..} if pos==cell));
                assert_eq!(sampled, extended, "head-only callback eligibility");
                heads.push(json!({"cell":cell,"head":head,"prepared_extended":extended,"head_block":head_block.get_name(),
                    "callbacks":callbacks,"samples":samples,"final":state(&world,bounds)}));
            }
        }
        for action in native.iter().filter(|a| {
            a["label"]
                .as_str()
                .unwrap()
                .contains("data without trigger")
                || a["label"] == "data changes without trigger"
        }) {
            for entry in action["samples"].as_array().unwrap() {
                if let Some(sample) = entry["operation"].get("Sample") {
                    assert!(
                        !cells.iter().any(|&cell| json!(cell) == sample["pos"]),
                        "data delivery sampled storage"
                    );
                }
            }
        }
        records.push(
            json!({"name":name,"fixture":fixture,"origin":BASE,"bounds":bounds,
            "initial":initial,"compilation":attempts,"analysis":report,"classification":classified,
            "native":native,"compiled":compiled,"controlled":controlled,"heads":heads}),
        );
    }
    std::fs::write(output, serde_json::to_vec_pretty(&records).unwrap()).unwrap();
}
