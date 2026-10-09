//! Opt-in timing inventory and interpreter/Redpiler output comparison for TEST_ fixtures.
use super::*;
use crate::redstone::instant_piston_tests::capture_at;
use crate::redstone::piston::trace;
use mchprs_blocks::blocks::Block;
use sha2::{Digest, Sha256};

const SOURCE: &str = "test_data/piston-research/test-prefix-20261008";

fn fixture(
    name: &str,
) -> (
    PlotWorld,
    Vec<BlockPos>,
    Vec<BlockPos>,
    (BlockPos, BlockPos),
) {
    let bytes = std::fs::read(root().join(SOURCE).join(name)).unwrap();
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let first = BASE;
    let last = first
        + BlockPos::new(
            clipboard.size_x as i32 - 1,
            clipboard.size_y as i32 - 1,
            clipboard.size_z as i32 - 1,
        );
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        first + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let mut levers = Vec::new();
    let mut traced = Vec::new();
    for y in first.y..=last.y {
        for z in first.z..=last.z {
            for x in first.x..=last.x {
                let pos = BlockPos::new(x, y, z);
                let block = world.get_block(pos);
                if matches!(block, Block::Lever { .. }) {
                    levers.push(pos);
                }
                if matches!(
                    block,
                    Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::StonePressurePlate { .. }
                        | Block::RedstoneWire { .. }
                        | Block::Piston { .. }
                        | Block::PistonHead { .. }
                        | Block::Observer { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::RedstoneLamp { .. }
                ) || block.is_copper_bulb()
                {
                    traced.push(pos);
                }
            }
        }
    }
    (world, levers, traced, (first, last))
}

fn event(entry: &trace::Entry) -> Value {
    let mut value = serde_json::to_value(entry).unwrap();
    let operation = value.get_mut("operation").unwrap();
    if let Some(sample) = operation.get_mut("Sample") {
        let pos: BlockPos = serde_json::from_value(sample["pos"].clone()).unwrap();
        sample["pos"] = json!(pos - BASE);
    } else if let Some(applied) = operation.get_mut("Applied") {
        let pos: BlockPos = serde_json::from_value(applied["pos"].clone()).unwrap();
        applied["pos"] = json!(pos - BASE);
    }
    value
}

fn state(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Vec<Value> {
    let mut blocks = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                let block = world.get_block(pos);
                if matches!(
                    block,
                    Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::StonePressurePlate { .. }
                        | Block::RedstoneWire { .. }
                        | Block::Piston { .. }
                        | Block::PistonHead { .. }
                        | Block::Observer { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::RedstoneLamp { .. }
                ) || block.is_copper_bulb()
                {
                    blocks.push(json!({"pos":pos-BASE,"name":block.get_name(),"properties":block.properties(),
                        "power":if let Block::Piston { piston }=block { Some(crate::redstone::piston::should_piston_extend(world,piston.facing,pos)) } else { None }}));
                }
            }
        }
    }
    blocks
}

fn ticks(world: &mut PlotWorld, bounds: (BlockPos, BlockPos), count: usize) -> Vec<Value> {
    (1..=count)
        .map(|step| {
            let operations = trace::capture(|| world.tick_interpreted());
            json!({"step":step,"operations":operations.iter().map(event).collect::<Vec<_>>(),
                "state":state(world,bounds),"pending_ticks":world.scheduler().iter_entries().count(),
                "piston_events":world.piston_state().events.len(),"motions":world.piston_state().motions.len()})
        })
        .collect()
}

fn baseline(name: &str, bounds: (BlockPos, BlockPos)) -> Value {
    let (mut world, _, traced, _) = fixture(name);
    let initial = state(&world, bounds);
    let mut idle = Vec::new();
    let callbacks = capture_at(&traced, || idle = ticks(&mut world, bounds, 12));
    json!({"initial":initial,"idle_callbacks":callbacks,"idle_ticks":idle,
        "idle_state":state(&world,bounds),"idle_pending_ticks":world.scheduler().iter_entries().count(),
        "idle_piston_events":world.piston_state().events.len(),"idle_motions":world.piston_state().motions.len()})
}

#[test]
#[ignore = "author TEST_ schematic native timing report; requires explicit output path"]
fn test_prefixed_schematics_have_native_timing_report() {
    let output =
        std::path::PathBuf::from(std::env::var("MCHPRS_TEST_PREFIX_TIMING_OUTPUT").unwrap());
    assert!(!output.exists(), "choose a new report output");
    let directory = root().join(SOURCE);
    let mut names: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("TEST_") && name.ends_with(".schem"))
        .collect();
    names.sort();
    let mut reports = Vec::new();
    for name in names {
        let bytes = std::fs::read(directory.join(&name)).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let (world, levers, traced, bounds) = fixture(&name);
        let lever_states: Vec<_> = levers
            .iter()
            .map(|&pos| matches!(world.get_block(pos), Block::Lever { lever } if lever.powered))
            .collect();
        let mechanisms: Vec<_> = traced
            .iter()
            .map(|&pos| {
                let block = world.get_block(pos);
                json!({"pos":pos-BASE,"name":block.get_name(),"properties":block.properties()})
            })
            .collect();
        let idle = baseline(&name, bounds);
        let mut compilation = Vec::new();
        for budget_multiplier in [1, 8] {
            for optimize in [false, true] {
                for assume_instant in [false, true] {
                    let (world, _, _, _) = fixture(&name);
                    let mut compiler = Compiler::default();
                    let started = std::time::Instant::now();
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
                    compilation.push(json!({"budget_multiplier":budget_multiplier,"optimize":optimize,
                        "assume_instant":assume_instant,"elapsed_seconds":started.elapsed().as_secs_f64(),
                        "result":result.map(|_| "compiled".to_owned()).unwrap_or_else(|error| error.to_string())}));
                }
            }
        }
        let mut combinations = Vec::new();
        let count = 1usize << levers.len();
        for mask in 0..count {
            let (mut world, _, traced, _) = fixture(&name);
            let mut action_edges = Vec::new();
            for (index, &pos) in levers.iter().enumerate() {
                let desired = mask & (1 << index) != 0;
                if desired != lever_states[index] {
                    action_edges.push(trace::capture(|| lever_action(&mut world, pos, desired)));
                }
            }
            let mut frames = Vec::new();
            let callbacks = capture_at(&traced, || frames = ticks(&mut world, bounds, 12));
            combinations.push(json!({"lever_mask":mask,"target_levers":levers.iter().enumerate().map(|(i,p)| json!({"pos":*p-BASE,"powered":mask&(1<<i)!=0})).collect::<Vec<_>>(),
                "action_edges":action_edges.iter().map(|x|x.iter().map(event).collect::<Vec<_>>()).collect::<Vec<_>>(),
                "callbacks":callbacks,"ticks":frames}));
        }
        let mut pulses = Vec::new();
        for (index, &pos) in levers.iter().enumerate() {
            let (mut world, _, traced, _) = fixture(&name);
            let start = lever_states[index];
            let mut idle_ticks = Vec::new();
            let idle_callbacks = capture_at(&traced, || idle_ticks = ticks(&mut world, bounds, 4));
            let rising = trace::capture(|| lever_action(&mut world, pos, !start));
            let first_tick = trace::capture(|| world.tick_interpreted());
            let cancelled = trace::capture(|| lever_action(&mut world, pos, start));
            let mut settle = Vec::new();
            let callbacks = capture_at(&traced, || settle = ticks(&mut world, bounds, 12));
            pulses.push(json!({"lever":pos-BASE,"saved_powered":start,"idle_ticks":idle_ticks,
                "idle_callbacks":idle_callbacks,"first_edge":rising.iter().map(event).collect::<Vec<_>>(),
                "one_tick_later":first_tick.iter().map(event).collect::<Vec<_>>(),
                "return_edge":cancelled.iter().map(event).collect::<Vec<_>>(),"callbacks":callbacks,"settle":settle}));
        }
        reports.push(json!({"name":name,"sha256":hash,"bounds":{"first":bounds.0-BASE,"last":bounds.1-BASE},
            "levers":levers.iter().enumerate().map(|(i,p)|json!({"pos":*p-BASE,"powered":lever_states[i]})).collect::<Vec<_>>(),
            "actuators_and_observers":mechanisms,"compilation":compilation,"idle":idle,
            "input_combinations":combinations,"one_game_tick_pulses":pulses}));
    }
    std::fs::write(output, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}

fn comparison_fixture(name: &str) -> (PlotWorld, Vec<BlockPos>, Vec<BlockPos>) {
    let (mut world, mut levers, traced, _) = fixture(name);
    if levers.is_empty() {
        // Missing controls are attached to the existing input torches' fixed supports.
        for &pos in &traced {
            if let Block::RedstoneWallTorch { facing, .. } = world.get_block(pos) {
                let support = pos.offset(facing.opposite().block_face());
                let control = support.offset(facing.opposite().block_face());
                assert!(world.get_block(support).is_solid());
                assert_eq!(world.get_block(control), Block::Air);
                world.set_block(
                    control,
                    Block::Lever {
                        lever: mchprs_blocks::blocks::Lever {
                            face: mchprs_blocks::blocks::LeverFace::Wall,
                            facing: facing.opposite(),
                            powered: false,
                        },
                    },
                );
                levers.push(control);
            }
        }
    }
    (world, levers, traced)
}

fn output_frame(
    world: &PlotWorld,
    compiler: Option<&Compiler>,
    outputs: &[BlockPos],
    tick: usize,
    label: &str,
) -> Value {
    let sources = compiler.map(|compiler| compiler.ordinary_sources());
    let observations: Vec<_> = outputs
        .iter()
        .map(|&pos| {
            let mut state = block_state(world, pos, BASE);
            if matches!(world.get_block(pos), Block::RedstoneComparator { .. }) {
                if let Some(sources) = &sources {
                    // Comparator entities are materialized only on reset; compare live strength.
                    let strength = sources.iter().find(|(source, _)| *source == pos).unwrap().1;
                    state["entity"] = json!({"Comparator":{"output_strength":strength}});
                }
            }
            state
        })
        .collect();
    json!({"tick":tick,"label":label,"outputs":observations})
}

fn replay_outputs(
    name: &str,
    outputs: &[BlockPos],
    protocol: &[(usize, Vec<(BlockPos, bool)>)],
    flags: Option<(bool, bool)>,
) -> Result<Value, String> {
    let (mut world, _, _) = comparison_fixture(name);
    let mut compiler = Compiler::default();
    if let Some((optimize, assume_instant)) = flags {
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    optimize,
                    assume_instant,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .map_err(|error| error.to_string())?;
    }
    let mut frames = vec![output_frame(
        &world,
        flags.is_some().then_some(&compiler),
        outputs,
        0,
        "import",
    )];
    let mut operations = Vec::new();
    let mut tick = 0;
    for (advance, actions) in protocol {
        for &(pos, desired) in actions {
            if flags.is_some() {
                let Block::Lever { lever } = world.get_block(pos) else {
                    panic!("missing lever at {pos}")
                };
                if lever.powered != desired {
                    compiler.on_use_block(pos);
                    compiler.flush(&mut world);
                }
            } else {
                operations.extend(trace::capture(|| lever_action(&mut world, pos, desired)));
            }
        }
        if flags.is_some() {
            compiler.flush(&mut world);
        }
        frames.push(output_frame(
            &world,
            flags.is_some().then_some(&compiler),
            outputs,
            tick,
            "input",
        ));
        for _ in 0..*advance {
            tick += 1;
            if flags.is_some() {
                compiler.tick_with_world(&mut world);
                compiler.flush(&mut world);
            } else {
                operations.extend(trace::capture(|| world.tick_interpreted()));
            }
            frames.push(output_frame(
                &world,
                flags.is_some().then_some(&compiler),
                outputs,
                tick,
                "tick",
            ));
        }
    }
    let logical = flags.map(|_| compiler.backend.as_ref().unwrap().logical_stats());
    Ok(
        json!({"frames":frames,"operations":operations.iter().map(event).collect::<Vec<_>>(),
        "logical_stats":logical,"warnings":compiler.warnings()}),
    )
}

#[test]
#[ignore = "ordered interpreter/Redpiler output comparison; explicit new output path required"]
fn test_prefixed_schematics_compare_interpreter_outputs() {
    let output =
        std::path::PathBuf::from(std::env::var("MCHPRS_TEST_PREFIX_COMPARISON_OUTPUT").unwrap());
    assert!(!output.exists(), "choose a new report output");
    let recorded: Vec<Value> = serde_json::from_slice(
        &std::fs::read(root().join(SOURCE).join("timing-full.json")).unwrap(),
    )
    .unwrap();
    let mut reports = Vec::new();
    for reference in recorded {
        let name = reference["name"].as_str().unwrap();
        let bytes = std::fs::read(root().join(SOURCE).join(name)).unwrap();
        assert_eq!(reference["sha256"], format!("{:x}", Sha256::digest(bytes)));
        let (world, levers, traced) = comparison_fixture(name);
        let added_controls: Vec<_> = if reference["levers"].as_array().unwrap().is_empty() {
            levers.iter().map(|p| *p - BASE).collect()
        } else {
            Vec::new()
        };
        let outputs: Vec<_> = traced
            .iter()
            .copied()
            .filter(|&pos| {
                let block = world.get_block(pos);
                block.is_copper_bulb()
                    || matches!(
                        block,
                        Block::RedstoneRepeater { .. }
                            | Block::RedstoneComparator { .. }
                            | Block::RedstoneLamp { .. }
                    )
            })
            .collect();
        let saved: Vec<_> = levers
            .iter()
            .map(|&pos| {
                let Block::Lever { lever } = world.get_block(pos) else {
                    unreachable!()
                };
                (pos, lever.powered)
            })
            .collect();
        let mut protocols = Vec::new();
        for mask in 0..1usize << levers.len() {
            protocols.push((
                format!("mask_{mask}"),
                vec![
                    (
                        96,
                        levers
                            .iter()
                            .enumerate()
                            .map(|(i, &pos)| (pos, mask & (1 << i) != 0))
                            .collect(),
                    ),
                    (32, saved.clone()),
                ],
            ));
        }
        for (index, &(pos, powered)) in saved.iter().enumerate() {
            for width in [1, 2, 3, 4, 6, 8] {
                protocols.push((
                    format!("lever_{index}_pulse_{width}"),
                    vec![
                        (4, vec![]),
                        (width, vec![(pos, !powered)]),
                        (32, vec![(pos, powered)]),
                    ],
                ));
            }
        }
        if name.starts_with("TEST_BUD_") {
            let update = levers[0];
            let data = levers[1];
            protocols.push((
                "data_only_high_low".into(),
                vec![(16, vec![(data, true)]), (32, vec![(data, false)])],
            ));
            protocols.push((
                "data_before_update_high_low".into(),
                vec![
                    (8, vec![(data, true)]),
                    (24, vec![(update, true)]),
                    (8, vec![(update, false)]),
                    (8, vec![(data, false)]),
                    (24, vec![(update, true)]),
                    (32, vec![(update, false)]),
                ],
            ));
            protocols.push((
                "update_before_data_high_low".into(),
                vec![
                    (8, vec![(update, true)]),
                    (24, vec![(data, true)]),
                    (8, vec![(update, false)]),
                    (8, vec![(data, false)]),
                    (24, vec![(update, true)]),
                    (32, vec![(update, false)]),
                ],
            ));
        }
        let mut episodes = Vec::new();
        for (label, protocol) in protocols {
            let native = replay_outputs(name, &outputs, &protocol, None).unwrap();
            if let Some(original) = label.strip_prefix("mask_").and_then(|mask| {
                reference["input_combinations"].get(mask.parse::<usize>().unwrap())
            }) {
                for step in 1..=12 {
                    // Check this replay against the existing native evidence before comparison.
                    for observed in native["frames"][step + 1]["outputs"].as_array().unwrap() {
                        let expected = original["ticks"][step - 1]["state"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|block| block["pos"] == observed["pos"])
                            .unwrap();
                        assert_eq!(
                            observed["properties"], expected["properties"],
                            "{name} {label} step {step}"
                        );
                    }
                }
            }
            let mut compiled = Vec::new();
            for (optimize, assume_instant) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                match replay_outputs(name, &outputs, &protocol, Some((optimize, assume_instant))) {
                    Err(error) => compiled.push(
                        json!({"optimize":optimize,"assume_instant":assume_instant,"error":error}),
                    ),
                    Ok(replay) => {
                        let mismatches: Vec<_> = native["frames"].as_array().unwrap().iter()
                            .zip(replay["frames"].as_array().unwrap()).enumerate()
                            .filter_map(|(index, (native, compiled))| (native["outputs"] != compiled["outputs"])
                                .then_some(json!({"frame":index,"tick":native["tick"],"label":native["label"]})))
                            .collect();
                        assert!(
                            mismatches.is_empty(),
                            "{name} {label} O={optimize} A={assume_instant}: {mismatches:?}"
                        );
                        assert_eq!(
                            native["frames"].as_array().unwrap().len(),
                            replay["frames"].as_array().unwrap().len()
                        );
                        compiled.push(json!({"optimize":optimize,"assume_instant":assume_instant,"mismatches":mismatches,"replay":replay}));
                    }
                }
            }
            episodes.push(json!({"label":label,"native":native,"compiled":compiled}));
        }
        eprintln!("Compared {name}: {} protocols", episodes.len());
        reports.push(json!({"name":name,"sha256":reference["sha256"],"added_controls":added_controls,"outputs":outputs.iter().map(|p|*p-BASE).collect::<Vec<_>>(),"episodes":episodes}));
    }
    std::fs::write(output, serde_json::to_vec(&reports).unwrap()).unwrap();
}

#[test]
fn test_prefixed_schematics_keep_native_callbacks_and_motion() {
    let recorded: Vec<Value> = serde_json::from_slice(
        &std::fs::read(root().join(SOURCE).join("timing-full.json")).unwrap(),
    )
    .unwrap();
    for reference in recorded {
        let name = reference["name"].as_str().unwrap();
        for (optimize, assume_instant) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (mut native, levers, traced) = comparison_fixture(name);
            let (mut compiled, _, _) = comparison_fixture(name);
            let mut compiler = Compiler::default();
            let result = compiler.compile(
                &compiled,
                compiled.get_corners(),
                CompilerOptions {
                    optimize,
                    assume_instant,
                    ..Default::default()
                },
                compiled.scheduler().iter_entries().collect(),
                Default::default(),
            );
            if let Err(error) = result {
                assert!(
                    !assume_instant
                        && matches!(
                            name,
                            "TEST_CODER_1.schem"
                                | "TEST_WEIRD.schem"
                                | "TEST_WEIRD_INSTANT.schem"
                                | "TEST_WEIRD_INSTANT_2.schem"
                                | "TEST_WEIRD_INSTANT_3.schem"
                        ),
                    "{name}: {error}"
                );
                assert!(!compiler.is_active());
                continue;
            }
            assert_eq!(compiler.stats().unwrap().regions.timed_regions, 1);
            for pos in &levers {
                let expected = capture_at(&traced, || lever_action(&mut native, *pos, true));
                let actual = capture_at(&traced, || {
                    compiler.on_use_block(*pos);
                    compiler.flush(&mut compiled);
                });
                assert_eq!(
                    actual, expected,
                    "{name} input {pos} O={optimize} A={assume_instant}"
                );
            }
            for tick in 1..=40 {
                let expected = capture_at(&traced, || native.tick_interpreted());
                let actual = capture_at(&traced, || {
                    compiler.tick_with_world(&mut compiled);
                    compiler.flush(&mut compiled);
                });
                assert_eq!(
                    actual, expected,
                    "{name} tick={tick} O={optimize} A={assume_instant}"
                );
                for &pos in &traced {
                    assert_eq!(
                        block_state(&compiled, pos, BASE),
                        block_state(&native, pos, BASE),
                        "{name} tick={tick} at {pos}"
                    );
                }
                assert_eq!(
                    serde_json::to_value(compiled.piston_state()).unwrap(),
                    serde_json::to_value(native.piston_state()).unwrap(),
                    "{name} tick={tick}"
                );
            }
        }
    }
}

#[test]
fn test_prefixed_piston_handoff_preserves_pending_work() {
    for name in [
        "TEST_PISTION.schem",
        "TEST_BUD_1.schem",
        "TEST_BUD_2.schem",
        "TEST_WEIRD_INSTANT.schem",
    ] {
        for handoff in [2, 7] {
            let (mut native, levers, traced) = comparison_fixture(name);
            let (mut compiled, _, _) = comparison_fixture(name);
            let options = || CompilerOptions {
                assume_instant: true,
                ..Default::default()
            };
            let mut compiler = Compiler::default();
            let bounds = compiled.get_corners();
            compiler
                .compile(&compiled, bounds, options(), vec![], Default::default())
                .unwrap();
            for pos in levers {
                lever_action(&mut native, pos, true);
                compiler.on_use_block(pos);
            }
            for tick in 1..=32 {
                native.tick_interpreted();
                compiler.tick_with_world(&mut compiled);
                compiler.flush(&mut compiled);
                if tick == handoff {
                    compiler.reset(&mut compiled, bounds);
                    assert_eq!(
                        serde_json::to_value(compiled.piston_state()).unwrap(),
                        serde_json::to_value(native.piston_state()).unwrap(),
                        "{name} handoff tick={tick}"
                    );
                    let native_ticks: Vec<_> = native.scheduler().iter_entries().collect();
                    let ticks: Vec<_> = compiled.scheduler().iter_entries().collect();
                    assert_eq!(ticks, native_ticks, "{name} handoff scheduler");
                    compiler
                        .compile(&compiled, bounds, options(), ticks, Default::default())
                        .unwrap();
                    compiled.native_scheduler().clear();
                }
                for &pos in &traced {
                    assert_eq!(
                        block_state(&compiled, pos, BASE),
                        block_state(&native, pos, BASE),
                        "{name} tick={tick} at {pos}"
                    );
                }
            }
        }
    }
}
