use super::*;

#[test]
fn rotated_divider_reproduces_the_reported_qc_coordinates() {
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
    let error = sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &[base].into_iter().collect(),
        &[data].into_iter().collect(),
    )
    .unwrap_err();
    assert!(
        error.contains(&format!("interface at {base:?}:")),
        "{error}"
    );
    assert!(error.contains(&format!("data source {data:?}")), "{error}");
    println!("{error}");
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
fn fixed_divider_rejects_unrepresented_feedback_sampling_transactionally() {
    let fixture = manifest("fpu_divider");
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (world, bounds) = load(&fixture);
            let before = compilation_fingerprint(&world, bounds);
            let mut compiler = Compiler::default();
            let error = compiler
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
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("unsupported internally driven QC sampling interface"),
                "{error}"
            );
            assert!(error.contains("data source"), "{error}");
            assert!(!compiler.is_active());
            assert!(compiler.current_flags().is_none());
            assert_eq!(compilation_fingerprint(&world, bounds), before);
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
    let error = Compiler::default()
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            vec![],
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    println!("{error}");
    assert!(error.contains("unsupported internally driven QC sampling interface"));
    let actor = report
        .pistons
        .iter()
        .position(|p| error.contains(&format!("interface at {:?}:", p.pos)))
        .unwrap();
    let piston = &report.pistons[actor];
    let data = report.recognition[actor]
        .inputs
        .sources
        .iter()
        .find(|s| error.contains(&format!("data source {:?} does not", s.source)))
        .unwrap()
        .source;
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
        "origin": origin(&fixture), "error": error, "piston": piston,
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
