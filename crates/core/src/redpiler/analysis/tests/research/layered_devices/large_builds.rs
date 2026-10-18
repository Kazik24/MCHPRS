//! Requested large-device checks: cold compiler coverage and independent native execution.
use super::*;

#[test]
#[ignore = "explicit large CPU preparation, layer census and 100 active native tick pairs"]
fn bubblesort_layers_and_active_history_match_native() {
    check_cpu(cpus::CPUS[2], 100);
}

#[test]
#[ignore = "explicit mixed PM1 preparation, layer census, boot and early instructions"]
fn pm1_layers_preserve_mixed_devices_and_native_boot_history() {
    check_cpu(cpus::CPUS[0], 512);
}

#[test]
#[ignore = "explicit fresh mixed PM1 save, layer census, boot and early instructions"]
fn pm1_fresh_layers_and_native_boot_history_match() {
    // The documented fresh save translates the old control by (-31,+32,-3).
    check_cpu(
        cpus::Cpu {
            name: "pm1_fresh_compilation",
            schematic: "piston-research/pm1-compilation-1-20261008-fresh/PM1_FIXED_COMPILATION_1.schem",
            sha256: "cf5ef6b5e62defbc02dc3b201b9bb29766feb310f6abfcad2941486312e0bd7c",
            start: BlockPos::new(148, 59, 61),
            stop: Some(BlockPos::new(148, 56, 61)),
            origin: BlockPos::new(8, 8, 8),
            active_ticks: 512,
        },
        512,
    );
}

#[test]
#[ignore = "explicit FPU layer census and stable-input activation/reset comparisons"]
fn fpu_layers_and_stable_input_histories_match_native() {
    let mut fixture = manifest("fpu_legal");
    let trigger = fixture["ports"]["trigger"].clone();
    let mut prepared = vec![lever(&trigger, false), json!({"advance":64})];
    for (bank, inverted) in [("light_blue_operand_inputs", false), ("red_operand_inputs", true)] {
        for (index, position) in fixture["ports"][bank].as_array().unwrap().iter().enumerate() {
            prepared.push(lever(position, (index % 2 == 0) ^ inverted));
        }
    }
    for (index, position) in fixture["ports"]["opcode_levers_by_sign_weight_4_2_1"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        prepared.push(lever(position, index != 1));
    }
    prepared.extend([
        json!({"advance":64}),
        lever(&trigger, true),
        json!({"advance":64}),
        lever(&trigger, false),
        json!({"advance":64}),
    ]);
    // The author identifies trigger OFF as the preparation level. No numeric bit order is assumed.
    fixture["cases"] = json!([
        {"id":"saved-idle", "steps":[{"advance":32}]},
        {"id":"saved-trigger-reset", "steps":[lever(&trigger,false), {"advance":64}, lever(&trigger,true), {"advance":64}, lever(&trigger,false), {"advance":64}]},
        {"id":"stable-alternating-operands-opcode-101", "steps":prepared}
    ]);
    check_manifest_device("fpu", &fixture, 8);
}

fn lever(position: &Value, powered: bool) -> Value {
    json!({"op":"lever", "pos":position, "powered":powered})
}

fn check_cpu(cpu: cpus::Cpu, active_ticks: u32) {
    let (mut native, bounds, normalization) = load_cpu(cpu);
    let (mut projected, other_bounds, other_normalization) = load_cpu(cpu);
    assert_eq!(bounds, other_bounds);
    assert_eq!(normalization, other_normalization);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let prepared = prepare_artifact(&projected, bounds, 8).unwrap_or_else(|error| {
        save_report(
            cpu.name,
            &json!({"fixture":cpu.schematic,"status":"preparation_rejected","error":error,
            "normalization":normalization,"runtime_replacement":false}),
        );
        panic!("{}: layer preparation rejected: {error}", cpu.name);
    });
    let mut report = layer_report(cpu.name, &prepared.graph);
    report["source"] = json!({"fixture":cpu.schematic,"sha256":cpu.sha256,"normalization":normalization});
    report["correctness"] = json!({"status":"pending","active_ticks_requested":active_ticks});
    save_report(cpu.name, &report);
    let lowered = prepared.lower(&monitor).unwrap();
    let mut backend = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    backend.set_test_recording(false);
    assert_eq!(backend.execution_coverage().instant_kernel_nodes, 0);
    assert_eq!(backend.execution_coverage().native_nodes, 0);
    project(&backend, &mut projected);
    pc_counter_contract::compare_checkpoint(&native, &projected, bounds, cpu.name);
    cpus::click_cpu(&mut native, cpu, cpu.start);
    backend.use_block(cpu.origin + cpu.start).unwrap();
    let mut checkpoints = Vec::new();
    for tick in 1..=active_ticks {
        native.tick_interpreted();
        backend
            .tick_with_world(&mut projected)
            .unwrap_or_else(|error| panic!("{} active tick {tick}: {error}", cpu.name));
        project(&backend, &mut projected);
        assert_future_work(&native, &projected, &format!("{} active tick {tick}", cpu.name));
        assert_eq!(
            command_output(&native),
            command_output(&projected),
            "{} command output at tick {tick}",
            cpu.name
        );
        if [10, 100, 300, active_ticks].contains(&tick) {
            pc_counter_contract::compare_checkpoint(&native, &projected, bounds, &format!("{} active tick {tick}", cpu.name));
            checkpoints.push(tick);
        }
    }
    let counts = format!("{:?}", backend.execution_counts());
    check_handoff(&mut native, &mut backend, &mut projected, bounds, cpu.name);
    assert_eq!(
        command_output(&native),
        command_output(&projected),
        "{} command output after handoff",
        cpu.name
    );
    report["correctness"] = json!({"status":"passed","graph_native_ticks":active_ticks,"full_state_checkpoints":checkpoints,
        "ordered_pending_work_each_tick":true,"command_output_each_tick":true,"handoff_ticks":16,
        "complete_sort_verified":false,"work_counts":counts});
    save_report(cpu.name, &report);
    println!("{}: {active_ticks} active native tick pairs and 16 handoff ticks passed", cpu.name);
}

fn load_cpu(cpu: cpus::Cpu) -> (PlotWorld, (BlockPos, BlockPos), Value) {
    // Reuse the frozen CPU loader, including BubbleSort's load/reset preparation.
    let mut world = cpus::load_cpu(cpu);
    let bytes = std::fs::read(root().join("test_data").join(cpu.schematic)).unwrap();
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1),
    );
    let normalization = if cpu.name == "cpu_bubblesort" {
        let position = cpu.origin + BlockPos::new(186, 2, 184);
        assert!(matches!(world.get_block(position), Block::WallSign { .. }));
        assert!(world.set_block(position, Block::Air));
        json!({"removed_wall_sign":position,"scope":"identical in-memory copies only"})
    } else {
        Value::Null
    };
    (world, bounds, normalization)
}

fn command_output(world: &PlotWorld) -> Vec<String> {
    world.command_output().map(str::to_owned).collect()
}
