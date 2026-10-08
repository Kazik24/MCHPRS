//! Native evidence for notification routes outside the fixed-writer sampler.
use super::*;
use crate::redpiler::instant::sampling;
use crate::redstone::instant_piston_tests::capture_at;
use crate::redstone::piston::trace::Operation;
use mchprs_blocks::blocks::{Lever, LeverFace};
use mchprs_blocks::BlockDirection;

fn extended_sticky(world: &mut PlotWorld, pos: BlockPos, payload: Block) {
    let piston = RedstonePiston {
        facing: BlockFacing::Down,
        sticky: true,
        extended: true,
    };
    world.set_block(pos, Block::Piston { piston });
    world.set_block(
        pos.offset(BlockFace::Bottom),
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(pos + BlockPos::new(0, -2, 0), payload);
}

#[test]
fn sticky_base_change_rechecks_an_existing_bud_head_without_powering_its_base() {
    let mut world = empty();
    let cell = BASE;
    let head = cell.offset(BlockFace::Bottom);
    let source = head.offset(BlockFace::West);
    let payload = Block::from_name("gray_wool").unwrap();
    for (pos, payload) in [(cell, payload), (source, Block::RedstoneBlock)] {
        extended_sticky(&mut world, pos, payload);
    }
    let control = source.offset(BlockFace::South);
    world.set_block(control.offset(BlockFace::Bottom), Block::Stone {});
    let lever = Lever::new(LeverFace::Floor, BlockDirection::North, true);
    world.set_block(control, Block::Lever { lever });
    let data = cell + BlockPos::new(0, 2, 0);
    world.set_block(data, Block::RedstoneBlock);
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == cell).unwrap();
    assert!(report.ports.pistons[actor].updates.iter().any(|update| {
        update.source == source
            && update.kind == crate::redpiler::analysis::ports::UpdateKind::PistonBaseChange
            && update.requires_extended
    }));
    for assume_instant in [false, true] {
        let mut compiler = Compiler::default();
        let result = compiler.compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                assume_instant,
                ..Default::default()
            },
            vec![],
            Default::default(),
        );
        if assume_instant {
            result.unwrap();
            assert!(compiler.is_active());
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains(&format!("{source:?}")), "{error}");
            assert!(
                error.contains("no verified observer reset or payload-following response"),
                "{error}"
            );
            assert!(!compiler.is_active());
        }
    }
    world.set_block(data, Block::Air);
    let held = trace::capture(|| {
        for _ in 0..4 {
            world.tick_interpreted();
        }
    });
    assert!(!held
        .iter()
        .any(|entry| { matches!(entry.operation, Operation::Sample { pos, .. } if pos == cell) }));
    assert!(matches!(world.get_block(cell), Block::Piston { piston } if piston.extended));

    let callbacks = capture_at(&[cell, head, source], || {
        world.set_block(
            control,
            Block::Lever {
                lever: Lever {
                    powered: false,
                    ..lever
                },
            },
        );
        crate::redstone::update_surrounding_blocks(&mut world, control);
        crate::redstone::update_surrounding_blocks(&mut world, control.offset(BlockFace::Bottom));
        for _ in 0..4 {
            world.tick_interpreted();
        }
    });
    assert!(callbacks.iter().any(|entry| {
        entry["kind"] == "callback"
            && entry["data"]["pos"] == json!(head)
            && entry["data"]["dir"] == json!(BlockFace::West)
            && entry["data"]["block"] == "piston_head"
    }));
    assert!(matches!(world.get_block(cell), Block::Piston { piston } if !piston.extended));
    assert_eq!(world.get_block(head), payload);
    assert_eq!(world.get_block(cell + BlockPos::new(0, -2, 0)), Block::Air);
    let absent_head = trace::capture(|| crate::redstone::piston::notify(&mut world, source));
    assert!(!absent_head
        .iter()
        .any(|entry| { matches!(entry.operation, Operation::Sample { pos, .. } if pos == cell) }));
}

#[test]
fn bud_head_request_rechecks_restored_power_before_commit() {
    let mut world = empty();
    let cell = BASE;
    let head = cell.offset(BlockFace::Bottom);
    let payload = Block::from_name("gray_wool").unwrap();
    extended_sticky(&mut world, cell, payload);
    let data = cell + BlockPos::new(0, 2, 0);
    assert!(!crate::redstone::piston::should_piston_extend(
        &world,
        BlockFacing::Down,
        cell,
    ));
    let callbacks = capture_at(&[cell, head], || {
        crate::redstone::update(
            world.get_block(head),
            &mut world,
            head,
            Some(BlockFace::West),
        );
        // A request records the desired action; execution must recheck power.
        world.set_block(data, Block::RedstoneBlock);
        world.tick_interpreted();
    });
    let requested = callbacks
        .iter()
        .position(|entry| {
            entry["kind"] == "event_enqueue"
                && entry["data"]["pos"] == json!(cell)
                && entry["data"]["action"] == "Retract"
        })
        .unwrap();
    let validated = callbacks
        .iter()
        .position(|entry| entry["kind"] == "event_execute" && entry["data"]["pos"] == json!(cell))
        .unwrap();
    assert!(requested < validated);
    assert!(!callbacks
        .iter()
        .any(|entry| { entry["kind"] == "event_applied" && entry["data"]["pos"] == json!(cell) }));
    assert!(matches!(world.get_block(cell), Block::Piston { piston } if piston.extended));
    assert!(matches!(world.get_block(head), Block::PistonHead { .. }));
    assert_eq!(world.get_block(cell + BlockPos::new(0, -2, 0)), payload);
    assert!(world.piston_state().events.is_empty());
    assert!(world.piston_state().motions.is_empty());
}

#[test]
#[ignore = "hash-checked PM1 route inventory and native trace; explicit new output required"]
fn pm1_missing_sampling_route_inventory_and_native_trace() {
    let output = root().join(std::env::var("MCHPRS_PM1_ROUTE_OUTPUT").unwrap());
    assert!(
        !output.exists(),
        "route diagnostics must preserve existing evidence"
    );
    let cpu = cpus::Cpu {
        schematic: "piston-research/pm1-compilation-1-20261008-fresh/PM1_FIXED_COMPILATION_1.schem",
        sha256: "cf5ef6b5e62defbc02dc3b201b9bb29766feb310f6abfcad2941486312e0bd7c",
        start: BlockPos::new(156, 67, 69),
        stop: Some(BlockPos::new(156, 64, 69)),
        ..cpus::CPUS[0]
    };
    let mut world = cpus::load_cpu(cpu);
    let cell = cpu.origin + BlockPos::new(9, 51, 0);
    let head = cell.offset(BlockFace::Bottom);
    let source = head.offset(BlockFace::West);
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
    let actor = report.pistons.iter().position(|p| p.pos == cell).unwrap();
    let source_actor = report.pistons.iter().position(|p| p.pos == source).unwrap();
    let classifier = sampling::recognize(
        &world,
        &report,
        &monitor,
        None,
        &sampling::reset_candidates(&world, &report),
    )
    .map(|c| json!({"memory": c.memory.len(), "events": c.events.len()}));
    let source_group = report
        .payload_groups
        .iter()
        .find(|group| group.members.contains(&source_actor))
        .unwrap();
    let mut topology = crate::redpiler::analysis::topology::Topology::new(
        &world,
        report.bounds,
        &monitor,
        AnalysisLimits::for_budget(8).max_dependency_steps,
        report
            .payload_groups
            .iter()
            .enumerate()
            .flat_map(|(group, descriptor)| descriptor.positions.iter().map(move |&p| (p, group)))
            .collect(),
    );
    let wire_inputs: Vec<_> = report.ports.pistons[actor]
        .updates
        .iter()
        .filter(|update| matches!(world.get_block(update.source), Block::RedstoneWire { .. }))
        .map(|update| {
            json!({"local": update.source - cpu.origin,
                "inputs": topology.wire_inputs(update.source).map_err(|error| error.to_string())})
        })
        .collect();
    let mut positions = vec![cell, head, source];
    for piston in [cell, source] {
        positions.extend((1..=3).map(|dy| piston + BlockPos::new(0, dy, 0)));
        positions.extend((1..=2).map(|dy| piston + BlockPos::new(0, -dy, 0)));
    }
    positions.extend(report.ports.pistons[actor].updates.iter().map(|u| u.source));
    positions.extend(
        report.recognition[actor]
            .inputs
            .wires
            .iter()
            .chain(&report.recognition[source_actor].inputs.wires)
            .copied(),
    );
    for &member in &source_group.members {
        let piston = &report.pistons[member];
        positions.extend([piston.pos, piston.head, piston.payload]);
    }
    positions.sort_by_key(|p| (p.y, p.z, p.x));
    positions.dedup();
    let initial: Vec<_> = positions
        .iter()
        .map(|&p| block_state(&world, p, cpu.origin))
        .collect();
    let mut operations = Vec::new();
    let mut samples = Vec::new();
    let mut poses = vec![json!({"tick": 0, "state": block_state(&world, cell, cpu.origin)})];
    let mut previous = world.get_block(cell);
    cpus::click_cpu(&mut world, cpu, cpu.start);
    for tick in 1..=500 {
        let callbacks = capture_at(&positions, || {
            for (sequence, entry) in trace::capture(|| world.tick_interpreted())
                .into_iter()
                .enumerate()
            {
                let pos = match entry.operation {
                    Operation::Sample { pos, .. } => pos,
                    Operation::Applied(event) => event.pos,
                };
                if positions.contains(&pos) {
                    samples.push(json!({"game_tick": tick, "sequence": sequence, "entry": entry}));
                }
            }
        });
        operations.extend(
            callbacks
                .into_iter()
                .map(|entry| json!({"game_tick": tick, "entry": entry})),
        );
        let current = world.get_block(cell);
        if current != previous {
            poses.push(json!({"tick": tick, "state": block_state(&world, cell, cpu.origin)}));
            previous = current;
        }
    }
    let mut probe_world = cpus::load_cpu(cpu);
    let data = cell + BlockPos::new(0, 2, 0);
    let control = source + BlockPos::new(-1, 1, 0);
    let mut perturbation = Vec::new();
    let mut snapshot = |label: &str, world: &PlotWorld, callbacks: Vec<Value>| {
        perturbation.push(
            json!({"label": label, "tick": world.piston_state().logical_tick,
            "target": block_state(world, cell, cpu.origin),
            "source": block_state(world, source, cpu.origin),
            "observer": block_state(world, source.offset(BlockFace::Top), cpu.origin),
            "callbacks": callbacks}),
        );
    };
    snapshot("saved_state", &probe_world, Vec::new());
    let Block::RedstoneWire { mut wire } = probe_world.get_block(data) else {
        panic!("missing data wire at {data:?}");
    };
    wire.power = 0;
    probe_world.set_block(data, Block::RedstoneWire { wire });
    let callbacks = capture_at(&positions, || probe_world.tick_interpreted());
    snapshot(
        "raw_data_change_without_notification",
        &probe_world,
        callbacks,
    );
    let Block::RedstoneWire { mut wire } = probe_world.get_block(control) else {
        panic!("missing source control wire at {control:?}");
    };
    wire.power = 0;
    probe_world.set_block(control, Block::RedstoneWire { wire });
    let callbacks = capture_at(&positions, || {
        crate::redstone::update(
            probe_world.get_block(source),
            &mut probe_world,
            source,
            None,
        );
    });
    snapshot(
        "raw_control_change_then_source_recheck",
        &probe_world,
        callbacks,
    );
    for _ in 0..16 {
        let callbacks = capture_at(&positions, || probe_world.tick_interpreted());
        snapshot("native_source_episode", &probe_world, callbacks);
    }
    let result = json!({
        "fixture": cpu.schematic, "sha256": cpu.sha256, "origin": cpu.origin,
        "target_local": cell - cpu.origin, "source_local": source - cpu.origin,
        "classifier": classifier, "initial": initial,
        "target": {"descriptor": report.pistons[actor], "recognition": report.recognition[actor], "ports": report.ports.pistons[actor]},
        "source": {"descriptor": report.pistons[source_actor], "recognition": report.recognition[source_actor], "ports": report.ports.pistons[source_actor]},
        "source_payload_group": source_group, "notification_wire_inputs": wire_inputs,
        "native_ticks": 500, "operations": operations, "samples": samples, "poses": poses,
        "controlled_perturbation": {"is_original_protocol": false,
            "data_wire_local": data - cpu.origin, "source_control_wire_local": control - cpu.origin,
            "steps": perturbation},
    });
    std::fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!("PM1 missing route evidence: {}", output.display());
}
