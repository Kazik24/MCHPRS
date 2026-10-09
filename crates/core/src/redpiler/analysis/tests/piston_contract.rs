//! Synchronized inputs and observable outputs, independent of internal piston motion.
use super::*;

const EXAMPLES: &str = "test_data/piston-research/test-prefix-20261008";

fn schematic(path: &Path) -> (PlotWorld, (BlockPos, BlockPos)) {
    let clipboard = load_schematic(std::io::Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let last = BASE
        + BlockPos::new(
            clipboard.size_x as i32 - 1,
            clipboard.size_y as i32 - 1,
            clipboard.size_z as i32 - 1,
        );
    (world, (BASE, last))
}

fn compile(world: &PlotWorld, assume_instant: bool, optimize: bool) -> Compiler {
    let mut compiler = Compiler::default();
    compiler
        .compile(
            world,
            world.get_corners(),
            CompilerOptions {
                assume_instant,
                optimize,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    compiler
}

#[test]
fn test_examples_compile_with_proof_and_weird_examples_with_assumption() {
    let mut paths: Vec<_> = std::fs::read_dir(root().join(EXAMPLES))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "schem")
        })
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 18);
    let mut failures = Vec::new();
    for path in paths {
        let (world, _) = schematic(&path);
        let name = path.file_name().unwrap().to_string_lossy();
        let assume_instant = name.contains("WEIRD");
        for optimize in [false, true] {
            let mut compiler = Compiler::default();
            if let Err(error) = compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    optimize,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            ) {
                failures.push(format!("{name} O={optimize} A={assume_instant}: {error}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn adders_and_counter_compile_with_proof() {
    for name in ["adder_1bit", "adder_11bits", "counter_basic"] {
        let (world, _, _) = fixture(name);
        for optimize in [false, true] {
            assert!(compile(&world, false, optimize).is_active(), "{name}");
        }
    }
}

#[test]
fn revised_pc_counter_rejects_unmodeled_feedback_sampling() {
    let (world, bounds) = schematic(&root().join(
        "test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem",
    ));
    reject_pc_feedback(&world, bounds);
}

fn reject_pc_feedback(world: &PlotWorld, bounds: (BlockPos, BlockPos)) {
    let before = snapshot(world, bounds);
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let mut compiler = Compiler::default();
            let error = compiler
                .compile(
                    world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    world.scheduler().iter_entries().collect(),
                    Default::default(),
                )
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("unsupported internally driven QC sampling interface"),
                "{error}"
            );
            assert!(error.contains("does not notify the base"), "{error}");
            assert!(
                error.contains("without an explicit sampled boundary"),
                "{error}"
            );
            assert!(!compiler.is_active());
            assert_eq!(snapshot(world, bounds), before);
        }
    }
}

fn tick_pair(native: &mut PlotWorld, world: &mut PlotWorld, compiler: &mut Compiler) {
    native.tick_interpreted();
    compiler.tick_with_world(world);
    compiler.flush(world);
}

fn use_pair(
    native: &mut PlotWorld,
    world: &mut PlotWorld,
    compiler: &mut Compiler,
    action: &Value,
) {
    let pos = local_pos(&action["pos"]);
    let desired = action["powered"].as_bool().unwrap();
    lever_action(native, pos, desired);
    if matches!(world.get_block(pos), Block::Lever { lever } if lever.powered != desired) {
        compiler.on_use_block(pos);
        compiler.flush(world);
    }
}

fn assert_ports(native: &PlotWorld, world: &PlotWorld, ports: &[BlockPos], label: &str, tick: u64) {
    for &pos in ports {
        assert_eq!(
            world.get_block(pos),
            native.get_block(pos),
            "{label} output {:?} tick={tick}",
            pos - BASE
        );
    }
}

fn held_logic_inputs(name: &str) {
    for optimize in [false, true] {
        let path = root().join(EXAMPLES).join(format!("{name}.schem"));
        let (mut native, bounds) = schematic(&path);
        let (mut world, _) = schematic(&path);
        let mut compiler = compile(&world, false, optimize);
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        crate::world::for_each_block_optimized(&native, bounds.0, bounds.1, |pos| {
            let block = native.get_block(pos);
            if matches!(block, Block::Lever { .. }) {
                inputs.push(pos);
            }
            if matches!(block, Block::RedstoneRepeater { .. }) || block.is_copper_bulb() {
                outputs.push(pos);
            }
        });
        assert!(!inputs.is_empty() && !outputs.is_empty());
        for _ in 0..32 {
            tick_pair(&mut native, &mut world, &mut compiler);
        }
        // Deliver the input bank together, then hold it through every reset cycle.
        for pos in inputs {
            let Block::Lever { lever } = native.get_block(pos) else {
                unreachable!()
            };
            lever_action(&mut native, pos, !lever.powered);
            compiler.on_use_block(pos);
        }
        compiler.flush(&mut world);
        for tick in 1..=96 {
            tick_pair(&mut native, &mut world, &mut compiler);
            assert_ports(&native, &world, &outputs, name, tick);
        }
    }
}

#[test]
fn held_instant_chain_preserves_output_ticks() {
    held_logic_inputs("TEST_INSTANT_3");
}

#[test]
fn held_or1_preserves_output_ticks() {
    held_logic_inputs("TEST_OR1");
}

#[test]
fn held_or2_preserves_output_ticks() {
    held_logic_inputs("TEST_OR2");
}

#[test]
fn held_piston_preserves_bulb_toggles() {
    held_logic_inputs("TEST_PISTION");
}

#[test]
fn held_piston_chain_preserves_output_ticks() {
    held_logic_inputs("TEST_PISTION_2");
}

#[test]
fn prepared_adders_preserve_sum_and_carry_output_ticks() {
    for name in ["adder_1bit", "adder_11bits"] {
        let (_, _, manifest) = fixture(name);
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            for optimize in [false, true] {
                let (mut native, _, _) = fixture(name);
                let (mut world, _, _) = fixture(name);
                let mut compiler = compile(&world, false, optimize);
                let mut outputs = Vec::new();
                for key in ["sum_repeater", "carry_repeater"] {
                    let ports = &manifest["ports"]["observations"][key];
                    if ports[0].is_number() {
                        outputs.push(local_pos(ports));
                    } else if let Some(ports) = ports.as_array() {
                        outputs.extend(ports.iter().map(local_pos));
                    }
                }
                assert!(!outputs.is_empty());
                let label = format!("{name} {} O={optimize}", case["id"]);
                for action in case["actions"].as_array().unwrap() {
                    if action["op"] == "lever" {
                        use_pair(&mut native, &mut world, &mut compiler, action);
                    } else {
                        assert_eq!(action["op"], "wait_ready");
                        for _ in 0..action["ticks"].as_u64().unwrap() {
                            tick_pair(&mut native, &mut world, &mut compiler);
                        }
                    }
                }
                for tick in 1..=24 {
                    tick_pair(&mut native, &mut world, &mut compiler);
                    assert_ports(&native, &world, &outputs, &label, tick);
                }
            }
        }
    }
}

#[test]
fn bud_data_hold_and_resampling_preserve_output_ticks() {
    for name in ["bud_pistonupdate", "bud_noninstantinputs"] {
        let (_, _, manifest) = fixture(name);
        for case in manifest["cases"].as_array().unwrap().iter().filter(|case| {
            matches!(
                case["id"].as_str(),
                Some("data-only" | "sample-1" | "store-hold-resample")
            )
        }) {
            for optimize in [false, true] {
                let (mut native, _, _) = fixture(name);
                let (mut world, _, _) = fixture(name);
                let mut compiler = compile(&world, false, optimize);
                let output = local_pos(&manifest["ports"]["observations"]["repeater"]);
                let label = format!("{name} {} O={optimize}", case["id"]);
                let mut tick = 0;
                for action in case["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .chain(std::iter::once(
                        &json!({"op": "wait_ready", "ticks": case["ticks"]}),
                    ))
                {
                    if action["op"] == "lever" {
                        use_pair(&mut native, &mut world, &mut compiler, action);
                    } else {
                        assert_eq!(action["op"], "wait_ready");
                        for _ in 0..action["ticks"].as_u64().unwrap() {
                            tick += 1;
                            tick_pair(&mut native, &mut world, &mut compiler);
                            assert_ports(&native, &world, &[output], &label, tick);
                        }
                    }
                }
                let cell = local_pos(&manifest["ports"]["observations"]["memory"]);
                let Block::Piston { piston } = native.get_block(cell) else {
                    panic!("{label}: memory must be stationary at the endpoint")
                };
                let banks = compiler.backend.as_ref().unwrap().logical_stats();
                let stored = banks
                    .iter()
                    .flat_map(|(_, _, memory)| memory)
                    .find(|(pos, _)| *pos == cell);
                assert_eq!(
                    stored.map(|(_, value)| *value),
                    Some(!piston.extended),
                    "{label}: explicit BUD bit"
                );
            }
        }
    }
}

#[test]
fn counter_preserves_native_output_waveform_when_started_stopped_and_resumed() {
    for optimize in [false, true] {
        let (mut native, _, manifest) = fixture("counter_basic");
        let (mut world, _, _) = fixture("counter_basic");
        let mut compiler = compile(&world, false, optimize);
        let trigger = &manifest["ports"]["inputs"]["trigger"];
        let outputs: Vec<_> = manifest["ports"]["observations"]["repeater"]
            .as_array()
            .unwrap()
            .iter()
            .map(local_pos)
            .collect();
        let mut tick = 0;
        for (powered, ticks) in [(false, 24), (true, 102), (false, 48), (true, 102)] {
            use_pair(
                &mut native,
                &mut world,
                &mut compiler,
                &json!({"pos": trigger, "powered": powered}),
            );
            for _ in 0..ticks {
                tick += 1;
                tick_pair(&mut native, &mut world, &mut compiler);
                assert_ports(&native, &world, &outputs, "counter_basic", tick);
            }
        }
    }
}

#[test]
fn running_counter_preserves_output_ticks_across_repeated_domain_changes() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let (mut native, _, manifest) = fixture("counter_basic");
            let (mut world, fixture_bounds, _) = fixture("counter_basic");
            let bounds = world.get_corners();
            let options = || CompilerOptions {
                optimize,
                io_only,
                ..Default::default()
            };
            let mut compiler = Compiler::default();
            compiler
                .compile(&world, bounds, options(), vec![], Default::default())
                .unwrap();
            world.clear_scheduled_ticks();
            let outputs: Vec<_> = manifest["ports"]["observations"]["repeater"]
                .as_array()
                .unwrap()
                .iter()
                .map(local_pos)
                .collect();
            let label = format!("counter handoff O={optimize} i={io_only}");
            let mut tick = 0;
            for _ in 0..24 {
                tick_pair(&mut native, &mut world, &mut compiler);
                tick += 1;
            }
            use_pair(
                &mut native,
                &mut world,
                &mut compiler,
                &json!({"pos": manifest["ports"]["inputs"]["trigger"], "powered": true}),
            );
            // Vary the reset point through every phase of the six-tick clock.
            for run_ticks in 6..12 {
                for _ in 0..run_ticks {
                    tick_pair(&mut native, &mut world, &mut compiler);
                    tick += 1;
                    assert_ports(&native, &world, &outputs, &label, tick);
                }
                compiler.reset(&mut world, bounds);
                assert!(!compiler.is_active());
                assert_ports(&native, &world, &outputs, &label, tick);
                for _ in 0..24 {
                    native.tick_interpreted();
                    world.tick_interpreted();
                    tick += 1;
                    assert_ports(&native, &world, &outputs, &label, tick);
                }
                let before = snapshot(&world, fixture_bounds);
                assert!(
                    compiler
                        .compile(
                            &world,
                            bounds,
                            options(),
                            world.scheduler().iter_entries().collect(),
                            Default::default(),
                        )
                        .is_err(),
                    "active movement must stay in the interpreter"
                );
                assert_eq!(snapshot(&world, fixture_bounds), before);
                assert!(!compiler.is_active());
                let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
                lever_action(&mut native, trigger, false);
                lever_action(&mut world, trigger, false);
                for _ in 0..24 {
                    native.tick_interpreted();
                    world.tick_interpreted();
                    tick += 1;
                    assert_ports(&native, &world, &outputs, &label, tick);
                }
                compiler
                    .compile(
                        &world,
                        bounds,
                        options(),
                        world.scheduler().iter_entries().collect(),
                        Default::default(),
                    )
                    .unwrap();
                world.clear_scheduled_ticks();
                for _ in 0..12 {
                    tick_pair(&mut native, &mut world, &mut compiler);
                    tick += 1;
                    assert_ports(&native, &world, &outputs, &label, tick);
                }
                use_pair(
                    &mut native,
                    &mut world,
                    &mut compiler,
                    &json!({"pos": manifest["ports"]["inputs"]["trigger"], "powered": true}),
                );
            }
        }
    }
}

#[test]
fn activated_pc_counter_rejects_unmodeled_sampling_transactionally() {
    let path = root().join(
        "test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem",
    );
    let (mut world, bounds) = schematic(&path);
    let mut controls = Vec::new();
    crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
        if matches!(world.get_block(pos), Block::Lever { .. }) {
            controls.push(pos);
        }
    });
    assert_eq!(controls.len(), 23);
    // The author's recorded counter protocol enables all inputs before its first tick.
    for pos in controls {
        lever_action(&mut world, pos, true);
    }
    world.tick_interpreted();
    reject_pc_feedback(&world, bounds);
}

#[test]
fn held_instant_circuits_preserve_output_ticks_across_repeated_domain_changes() {
    for name in ["TEST_INSTANT_3", "TEST_OR1", "TEST_OR2"] {
        let path = root().join(EXAMPLES).join(format!("{name}.schem"));
        for optimize in [false, true] {
            for io_only in [false, true] {
                let (mut native, fixture_bounds) = schematic(&path);
                let (mut world, _) = schematic(&path);
                let bounds = world.get_corners();
                let options = || CompilerOptions {
                    optimize,
                    io_only,
                    ..Default::default()
                };
                let mut inputs = Vec::new();
                let mut outputs = Vec::new();
                crate::world::for_each_block_optimized(
                    &native,
                    fixture_bounds.0,
                    fixture_bounds.1,
                    |pos| {
                        let block = native.get_block(pos);
                        if let Block::Lever { lever } = block {
                            inputs.push((pos, lever.powered));
                        }
                        if matches!(block, Block::RedstoneRepeater { .. }) || block.is_copper_bulb()
                        {
                            outputs.push(pos);
                        }
                    },
                );
                assert!(!inputs.is_empty() && !outputs.is_empty());
                let label = format!("{name} handoff O={optimize} i={io_only}");
                let mut compiler = Compiler::default();
                compiler
                    .compile(&world, bounds, options(), vec![], Default::default())
                    .unwrap();
                world.clear_scheduled_ticks();
                let mut tick = 0;
                for _ in 0..32 {
                    tick_pair(&mut native, &mut world, &mut compiler);
                    tick += 1;
                    assert_ports(&native, &world, &outputs, &label, tick);
                }
                for handoff_at in 1..=6 {
                    for &(pos, powered) in &inputs {
                        lever_action(&mut native, pos, !powered);
                        compiler.on_use_block(pos);
                    }
                    compiler.flush(&mut world);
                    for step in 1..=handoff_at + 56 {
                        if step == handoff_at + 1 {
                            compiler.reset(&mut world, bounds);
                            assert_ports(&native, &world, &outputs, &label, tick);
                        }
                        // Hold inputs through continuation, then settle before recompiling.
                        if step == handoff_at + 25 {
                            for &(pos, powered) in &inputs {
                                lever_action(&mut native, pos, powered);
                                lever_action(&mut world, pos, powered);
                            }
                        }
                        if compiler.is_active() {
                            tick_pair(&mut native, &mut world, &mut compiler);
                        } else {
                            native.tick_interpreted();
                            world.tick_interpreted();
                        }
                        tick += 1;
                        assert_ports(&native, &world, &outputs, &label, tick);
                    }
                    compiler
                        .compile(
                            &world,
                            bounds,
                            options(),
                            world.scheduler().iter_entries().collect(),
                            Default::default(),
                        )
                        .unwrap();
                    world.clear_scheduled_ticks();
                }
            }
        }
    }
}
