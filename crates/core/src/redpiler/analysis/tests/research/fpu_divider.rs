use super::*;

#[test]
fn divider_cross_and_dot_sample_the_same_power_on_comparator_edges() {
    use crate::redpiler::analysis::topology::PowerRoute;

    for name in ["fpu_divider", "divider_explanation"] {
        let fixture = manifest(name);
        let base = origin(&fixture) + BlockPos::new(26, 9, 2);
        let data = base + BlockPos::new(1, 1, 2);
        let wire_pos = base + BlockPos::new(0, 1, 1);
        let conductor = base + BlockPos::new(0, 1, 2);
        let mut traces = Vec::new();
        for dot in [false, true] {
            let (mut world, _) = load(&fixture);
            let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                panic!("missing cross");
            };
            assert!(crate::redstone::wire::is_cross(wire));
            if dot {
                crate::redstone::wire::on_use(wire, &mut world, wire_pos);
                let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                    unreachable!();
                };
                assert!(
                    crate::redstone::wire::is_dot(wire),
                    "click must produce a valid dot"
                );
            }
            let report = analyze_world(&world);
            let actor = report.pistons.iter().position(|p| p.pos == base).unwrap();
            let inputs = &report.recognition[actor].inputs.sources;
            assert!(inputs
                .iter()
                .any(|s| s.source == data && s.route == PowerRoute::Direct));
            assert_eq!(
                inputs
                    .iter()
                    .any(|s| s.source == data && s.route == PowerRoute::QuasiConnectivity),
                !dot
            );
            let mut edges = Vec::new();
            for present in [true, false] {
                if !present {
                    world.set_block(conductor, Block::Air);
                    crate::redstone::update_wire_neighbors(&mut world, conductor);
                }
                for strength in [0, 15] {
                    let Block::RedstoneComparator { comparator } = world.get_block(data) else {
                        unreachable!();
                    };
                    world.set_block(
                        data.offset(comparator.facing.block_face()),
                        if strength == 0 {
                            Block::Air
                        } else {
                            Block::RedstoneBlock
                        },
                    );
                    let samples: Vec<_> = trace::capture(|| crate::redstone::comparator::tick(comparator, &mut world, data))
                        .into_iter().filter(|e| matches!(e.operation, trace::Operation::Sample { pos, .. } if pos == base)).collect();
                    let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                        unreachable!();
                    };
                    let expected = if present { strength } else { 0 };
                    assert_eq!(wire.power, expected);
                    assert_eq!(
                        crate::redstone::get_redstone_power(
                            world.get_block(wire_pos.offset(BlockFace::Bottom)),
                            &world,
                            wire_pos.offset(BlockFace::Bottom),
                            BlockFace::South
                        ),
                        expected
                    );
                    assert_eq!(
                        crate::redstone::get_redstone_power(
                            world.get_block(wire_pos),
                            &world,
                            wire_pos,
                            BlockFace::South
                        ),
                        if dot { 0 } else { expected }
                    );
                    assert_eq!(samples.is_empty(), !present);
                    edges.push(samples);
                }
            }
            traces.push(edges);
        }
        assert_eq!(
            traces[0], traces[1],
            "{name}: ordered piston samples must match"
        );
    }
}

#[test]
#[ignore = "cross/dot interpreter comparison; new MCHPRS_PISTON_RESEARCH_OUTPUT required"]
fn capture_divider_cross_dot_comparison() {
    use mchprs_blocks::blocks::RedstoneWire;

    let output = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    let protocol = manifest("fpu_divider");
    let mut evidence = Vec::new();
    for name in ["fpu_divider", "divider_explanation"] {
        let fixture = manifest(name);
        let (world, _) = load(&fixture);
        let report = analyze_world(&world);
        let dots: Vec<_> = report
            .pistons
            .iter()
            .filter_map(|p| {
                if p.piston.facing != BlockFacing::Down {
                    return None;
                }
                let pos = p.pos + BlockPos::new(0, 1, 1);
                let Block::RedstoneWire { wire } = world.get_block(pos) else {
                    return None;
                };
                let dot = RedstoneWire {
                    power: wire.power,
                    ..Default::default()
                };
                (crate::redstone::wire::is_cross(wire)
                    && crate::redstone::wire::is_dot(crate::redstone::wire::get_regulated_sides(
                        dot, &world, pos,
                    )))
                .then_some(pos)
            })
            .collect();
        assert!(!dots.is_empty());
        let load_variant = |dot: bool| {
            let (mut world, _) = load(&fixture);
            if dot {
                for &pos in &dots {
                    let Block::RedstoneWire { wire } = world.get_block(pos) else {
                        unreachable!();
                    };
                    world.set_block(
                        pos,
                        Block::RedstoneWire {
                            wire: RedstoneWire {
                                power: wire.power,
                                ..Default::default()
                            },
                        },
                    );
                }
            }
            world
        };
        let mut compile = Vec::new();
        for dot in [false, true] {
            let world = load_variant(dot);
            for optimize in [false, true] {
                for assume_instant in [false, true] {
                    let error = Compiler::default()
                        .compile(
                            &world,
                            world.get_corners(),
                            CompilerOptions {
                                optimize,
                                assume_instant,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .err()
                        .map(|e| e.to_string());
                    compile.push(json!({"dot": dot, "optimize": optimize, "assume_instant": assume_instant, "error": error}));
                }
            }
        }
        let mut cases = Vec::new();
        for case in protocol["cases"].as_array().unwrap() {
            let mut variants = Vec::new();
            for (dot, compiled, optimize) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (false, true, true),
                (true, true, true),
            ] {
                let mut world = load_variant(dot);
                let mut compiler = Compiler::default();
                if compiled {
                    compiler
                        .compile(
                            &world,
                            world.get_corners(),
                            CompilerOptions {
                                optimize,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    compiler.flush(&mut world);
                }
                let mut frames = Vec::new();
                for (step_index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                    if step.get("diagnose").is_some() {
                        continue;
                    }
                    for _ in 0..step["advance"].as_u64().unwrap_or(1) {
                        let operations = trace::capture(|| {
                            if step.get("advance").is_some() {
                                if compiled {
                                    compiler.tick_with_world(&mut world);
                                } else {
                                    world.tick_interpreted();
                                }
                            } else if compiled {
                                assert_eq!(step["op"], "lever");
                                let pos = local_pos(&step["pos"]) - BASE + origin(&protocol);
                                let Block::Lever { lever } = world.get_block(pos) else {
                                    panic!("missing lever");
                                };
                                if lever.powered != step["powered"].as_bool().unwrap() {
                                    compiler.on_use_block(pos);
                                }
                            } else {
                                action(&mut world, &protocol, step);
                            }
                            if compiled {
                                compiler.flush(&mut world);
                            }
                        });
                        frames.push(json!({"step": step_index, "state": observations(&world, &protocol), "operations": operations,
                            "pistons": report.pistons.iter().map(|p| world.get_block_raw(p.pos)).collect::<Vec<_>>()}));
                    }
                }
                variants.push(frames);
            }
            assert_eq!(variants[0].len(), variants[1].len());
            let differences: Vec<_> = variants[0]
                .iter()
                .zip(&variants[1])
                .enumerate()
                .filter_map(|(index, (cross, dot))| {
                    (cross != dot).then(|| json!({"frame": index, "cross": cross, "dot": dot}))
                })
                .collect();
            let compiled_output_differences: Vec<_> = (2..variants.len())
                .flat_map(|variant| {
                    variants[variant % 2]
                        .iter()
                        .zip(&variants[variant])
                        .enumerate()
                        .filter_map(move |(index, (native, compiled))| {
                            let native = &native["state"]["ports"]["output_msb_first"];
                            let compiled = &compiled["state"]["ports"]["output_msb_first"];
                            (native != compiled).then(|| {
                                json!({"dot": variant % 2 == 1, "optimize": variant >= 4,
                        "frame": index, "native": native, "compiled": compiled})
                            })
                        })
                })
                .collect();
            println!(
                "{name} {}: {} frames, {} cross/dot differences, {} compiled output differences",
                case["id"],
                variants[0].len(),
                differences.len(),
                compiled_output_differences.len()
            );
            cases.push(
                json!({"id": case["id"], "frames": variants[0].len(), "differences": differences, "compiled_output_differences": compiled_output_differences}),
            );
        }
        println!("{name}: {} dots; compilation {compile:?}", dots.len());
        evidence
            .push(json!({"fixture": fixture, "dots": dots, "compile": compile, "cases": cases}));
    }
    serde_json::to_writer(std::io::BufWriter::new(file), &evidence).unwrap();
    assert!(evidence.iter().all(|fixture| fixture["compile"]
        .as_array()
        .unwrap()
        .iter()
        .all(|attempt| attempt["error"].is_null())));
    assert!(
        evidence.iter().all(
            |fixture| fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|case| case["differences"].as_array().unwrap().is_empty()
                    && case["compiled_output_differences"]
                        .as_array()
                        .unwrap()
                        .is_empty())
        ),
        "cross/dot and compiled interpreter comparisons must agree"
    );
}

#[test]
fn rotated_divider_admits_notified_qc_at_the_reported_coordinates() {
    use crate::redpiler::instant::sampling;
    use mchprs_blocks::blocks::RotateAmt;

    let fixture = manifest("fpu_divider");
    let (source, bounds) = load(&fixture);
    let mut world = PlotWorld::from_chunks(
        -1,
        -1,
        (0..PLOT_WIDTH)
            .flat_map(|x| {
                (0..PLOT_WIDTH).map(move |z| Chunk::empty(x - PLOT_WIDTH, z - PLOT_WIDTH))
            })
            .collect(),
        Default::default(),
    );
    crate::world::for_each_block_optimized(&source, bounds.0, bounds.1, |pos| {
        let rotated = BlockPos::new(-70 - pos.z, pos.y + 20, pos.x - 268);
        let mut block = source.get_block(pos);
        block.rotate(RotateAmt::Rotate90);
        world.set_block(rotated, block);
        if let Some(entity) = source.get_block_entity(pos) {
            world.set_block_entity(rotated, entity.clone());
        }
    });
    let base = BlockPos::new(-112, 59, -214);
    let data = BlockPos::new(-114, 60, -213);
    assert!(matches!(world.get_block(base), Block::Piston { .. }));
    assert!(matches!(
        world.get_block(data),
        Block::RedstoneComparator { .. }
    ));
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == base).unwrap();
    for route in [
        crate::redpiler::analysis::topology::PowerRoute::Direct,
        crate::redpiler::analysis::topology::PowerRoute::QuasiConnectivity,
    ] {
        assert!(report.recognition[actor]
            .inputs
            .sources
            .iter()
            .any(|source| source.source == data && source.route == route));
    }
    sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &[base].into_iter().collect(),
        &[data].into_iter().collect(),
    )
    .unwrap();
}

#[test]
fn divider_comparator_is_saturated_and_notifies_through_the_movable_conductor() {
    use crate::redpiler::instant::sampling;

    let fixture = manifest("fpu_divider");
    let (mut world, _) = load(&fixture);
    let first = origin(&fixture);
    let base = first + BlockPos::new(14, 9, 2);
    let data = first + BlockPos::new(15, 10, 4);
    let wire_pos = first + BlockPos::new(14, 10, 3);
    let conductor = first + BlockPos::new(14, 10, 4);
    assert!(!sampling::data_notifies(&world, data, base));
    assert_eq!(world.get_block(conductor).get_name(), "white_wool");
    let Block::RedstoneComparator { comparator } = world.get_block(data) else {
        panic!("missing divider comparator");
    };
    let rear = data.offset(comparator.facing.block_face());
    assert_eq!(
        crate::redstone::comparator::get_override(world.get_block(rear), &world, rear),
        15
    );
    assert_eq!(
        comparator.mode,
        mchprs_blocks::blocks::ComparatorMode::Compare
    );
    for strength in 0..=15 {
        for side in [comparator.facing.rotate(), comparator.facing.rotate_ccw()] {
            world.set_block(
                data.offset(side.block_face()),
                Block::RedstoneWire {
                    wire: mchprs_blocks::blocks::RedstoneWire {
                        power: strength,
                        ..Default::default()
                    },
                },
            );
        }
        crate::redstone::comparator::tick(comparator, &mut world, data);
        assert_eq!(
            crate::redstone::source_strength(world.get_block(data), &world, data),
            15
        );
    }
    let (mut world, _) = load(&fixture);
    // Probe the native output edge, including notifications, in both conductor poses.
    for present in [true, false] {
        if !present {
            world.set_block(conductor, Block::Air);
            crate::redstone::update_wire_neighbors(&mut world, conductor);
        }
        for strength in [0, 15] {
            world.set_block(
                rear,
                if strength == 0 {
                    Block::Air
                } else {
                    Block::RedstoneBlock
                },
            );
            let Block::RedstoneComparator { comparator } = world.get_block(data) else {
                unreachable!();
            };
            let entries = trace::capture(|| {
                crate::redstone::comparator::tick(comparator, &mut world, data);
            });
            let samples: Vec<_> = entries
                .iter()
                .filter_map(|entry| match entry.operation {
                    trace::Operation::Sample { pos, powered, .. } if pos == base => Some(powered),
                    _ => None,
                })
                .collect();
            let expected = present && strength != 0;
            assert_eq!(
                crate::redstone::piston::should_piston_extend(&world, BlockFacing::Down, base),
                expected
            );
            let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                panic!("missing divider notification wire");
            };
            assert_eq!(wire.power, if present { strength } else { 0 });
            if present {
                assert!(
                    !samples.is_empty(),
                    "an effective comparator edge must sample the piston"
                );
                assert!(samples.iter().all(|&powered| powered == expected));
            } else {
                assert!(
                    samples.is_empty(),
                    "disconnected comparator edges cannot change piston power"
                );
            }
        }
    }
}

#[test]
fn divider_crosses_compile_without_mutating_the_imported_world() {
    for name in ["fpu_divider", "divider_explanation"] {
        let fixture = manifest(name);
        for assume_instant in [false, true] {
            for optimize in [false, true] {
                let (world, bounds) = load(&fixture);
                let before = compilation_fingerprint(&world, bounds);
                let mut compiler = Compiler::default();
                compiler
                    .compile(
                        &world,
                        world.get_corners(),
                        CompilerOptions {
                            assume_instant,
                            optimize,
                            ..Default::default()
                        },
                        vec![],
                        Default::default(),
                    )
                    .unwrap();
                assert!(compiler.is_active());
                assert!(compiler.current_flags().is_some());
                assert_eq!(compilation_fingerprint(&world, bounds), before);
            }
        }
    }
}

#[test]
fn divider_preserves_interpreter_outputs_when_power_returns_during_retraction() {
    let protocol = manifest("fpu_divider");
    let case = protocol["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "enable-falling")
        .unwrap();
    for name in ["fpu_divider", "divider_explanation"] {
        for optimize in [false, true] {
            let (mut native, _) = load(&manifest(name));
            let (mut compiled, _) = load(&manifest(name));
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            for step in case["steps"].as_array().unwrap() {
                if step.get("diagnose").is_some() {
                    continue;
                }
                if let Some(ticks) = step["advance"].as_u64() {
                    for _ in 0..ticks {
                        native.tick_interpreted();
                        compiler.tick_with_world(&mut compiled);
                        compiler.flush(&mut compiled);
                        assert_eq!(
                            observations(&native, &protocol)["ports"]["output_msb_first"],
                            observations(&compiled, &protocol)["ports"]["output_msb_first"],
                            "{name}, optimize={optimize}, tick={}",
                            native.piston_state().logical_tick
                        );
                    }
                } else {
                    action(&mut native, &protocol, step);
                    compiler.flush(&mut compiled);
                    let pos = local_pos(&step["pos"]) - BASE + origin(&protocol);
                    let Block::Lever { lever } = compiled.get_block(pos) else {
                        panic!("missing input lever");
                    };
                    if lever.powered != step["powered"].as_bool().unwrap() {
                        compiler.on_use_block(pos);
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "divider QC timing investigation; MCHPRS_PISTON_RESEARCH_OUTPUT required"]
fn capture_divider_qc_sampling() {
    use crate::redpiler::analysis::topology::{SourceKind, Topology};
    use crate::redpiler::instant::sampling;
    use crate::redstone::instant_piston_tests::capture_at;
    use rustc_hash::FxHashSet;

    let output = std::env::var("MCHPRS_PISTON_RESEARCH_OUTPUT").unwrap();
    assert!(!Path::new(&output).exists(), "preserve existing evidence");
    let fixture = manifest("fpu_divider");
    let (world, _) = load(&fixture);
    let report = analyze_world(&world);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            vec![],
            Default::default(),
        )
        .unwrap();
    let base = origin(&fixture) + BlockPos::new(14, 9, 2);
    let data = origin(&fixture) + BlockPos::new(15, 10, 4);
    let actor = report.pistons.iter().position(|p| p.pos == base).unwrap();
    let piston = &report.pistons[actor];
    assert!(!sampling::data_notifies(&world, data, piston.pos));
    let monitor = TaskMonitor::default();
    let mut topology = Topology::new(
        &world,
        report.bounds,
        &monitor,
        AnalysisLimits::default().max_dependency_steps,
        report
            .payload_groups
            .iter()
            .enumerate()
            .flat_map(|(group, descriptor)| {
                descriptor.positions.iter().map(move |&pos| (pos, group))
            })
            .collect(),
    );
    let mut positions = FxHashSet::from_iter([piston.pos, piston.head, data]);
    positions.extend(
        report.recognition[actor]
            .inputs
            .sources
            .iter()
            .map(|s| s.source),
    );
    positions.extend(
        BlockFace::values()
            .into_iter()
            .map(|face| data.offset(face)),
    );
    let mut writers = FxHashSet::default();
    let mut wires = Vec::new();
    for update in &report.ports.pistons[actor].updates {
        if update.kind != crate::redpiler::analysis::ports::UpdateKind::WireNotification {
            continue;
        }
        positions.insert(update.source);
        let inputs = topology.wire_inputs(update.source).unwrap();
        for source in &inputs.sources {
            if let SourceKind::MobilePayload { group } = source.kind {
                writers.insert(group);
            }
        }
        wires.push(json!({"update": update, "inputs": inputs}));
    }
    let mut writer_inventory = Vec::new();
    let mut sorted_writers: Vec<_> = writers.into_iter().collect();
    sorted_writers.sort_unstable();
    for group in sorted_writers {
        let descriptor = &report.payload_groups[group];
        positions.extend(descriptor.positions.iter().copied());
        writer_inventory.push(json!({"group": group, "descriptor": descriptor,
            "actors": descriptor.members.iter().map(|&id| {
                positions.insert(report.pistons[id].pos);
                json!({"piston": report.pistons[id], "recognition": report.recognition[id]})
            }).collect::<Vec<_>>()}));
    }
    let mut positions: Vec<_> = positions.into_iter().collect();
    positions.sort_by_key(|p| (p.y, p.z, p.x));
    let initial: Vec<_> = positions
        .iter()
        .map(|&pos| block_state(&world, pos, origin(&fixture)))
        .collect();
    let mut cases = Vec::new();
    let mut total_samples = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let (mut world, _) = load(&fixture);
        let mut callbacks = Vec::new();
        let mut states = Vec::new();
        let mut previous = None;
        for (step_index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            if step.get("diagnose").is_some() {
                continue;
            }
            for _ in 0..step["advance"].as_u64().unwrap_or(1) {
                let entries = capture_at(&positions, || {
                    let samples = trace::capture(|| {
                        if step.get("advance").is_some() {
                            world.tick_interpreted();
                        } else {
                            action(&mut world, &fixture, step);
                        }
                    });
                    total_samples += samples.iter().filter(|e| {
                        matches!(e.operation, trace::Operation::Sample { pos, .. } if pos == piston.pos)
                    }).count();
                });
                callbacks.extend(
                    entries
                        .into_iter()
                        .map(|entry| json!({"step": step_index, "entry": entry})),
                );
                let state: Vec<_> = positions
                    .iter()
                    .map(|&pos| block_state(&world, pos, origin(&fixture)))
                    .collect();
                if previous.as_ref() != Some(&state) {
                    states.push(json!({"tick": world.piston_state().logical_tick, "step": step_index, "blocks": state}));
                    previous = Some(state);
                }
            }
        }
        println!(
            "divider QC {}: {} callbacks, {} state changes",
            case["id"],
            callbacks.len(),
            states.len()
        );
        cases.push(json!({"id": case["id"], "callbacks": callbacks, "states": states}));
    }
    assert!(
        total_samples > 0,
        "protocol must exercise the reported piston"
    );
    let evidence = json!({"fixture": fixture["fixture"], "sha256": fixture["sha256"],
        "origin": origin(&fixture), "compiles": true, "piston": piston,
        "recognition": report.recognition[actor], "ports": report.ports.pistons[actor],
        "data": block_state(&world, data, origin(&fixture)), "wires": wires,
        "writers": writer_inventory, "initial": initial, "cases": cases});
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    serde_json::to_writer(std::io::BufWriter::new(file), &evidence).unwrap();
    println!("Divider QC evidence: {output}");
}
