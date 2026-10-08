//! Large fixtures use the interpreter as their timing oracle, including broken arithmetic.
use super::*;
use rustc_hash::FxHashSet;

fn digest(world: &PlotWorld, watched: &[BlockPos]) -> String {
    let mut hash = Sha256::new();
    for &pos in watched {
        hash.update(world.get_block_raw(pos).to_le_bytes());
        if let Some(entity) = world.get_block_entity(pos) {
            hash.update(serde_json::to_vec(entity).unwrap());
        }
    }
    hash.update(serde_json::to_vec(world.piston_state()).unwrap());
    hash.update(serde_json::to_vec(&world.command_output().collect::<Vec<_>>()).unwrap());
    format!("{:x}", hash.finalize())
}

fn compare(name: &str, cases: &[Value]) {
    let fixture = manifest(name);
    for case in cases {
        let (mut native, _) = load(&fixture);
        native.set_random_tick_speed(0);
        native.disable_command_output_limits_for_replay();
        let mut watched = FxHashSet::default();
        let bounds = native.get_corners();
        crate::world::for_each_block_optimized(&native, bounds.0, bounds.1, |pos| {
            let block = native.get_block(pos);
            if block != Block::Air {
                watched.insert(pos);
            }
        });
        // Include both payload positions even when one is air in the saved pose.
        for piston in analyze_world(&native).pistons {
            watched.extend([piston.head, piston.payload]);
        }
        let mut watched: Vec<_> = watched.into_iter().collect();
        watched.sort_by_key(|pos| (pos.x, pos.y, pos.z));
        let mut expected = Vec::new();
        let mut operations = Vec::new();
        for step in case["steps"].as_array().unwrap() {
            if let Some(count) = step["advance"].as_u64() {
                for _ in 0..count {
                    let entries = trace::capture(|| native.tick_interpreted());
                    expected.push((digest(&native, &watched), entries));
                    operations.push(None);
                }
            } else if step["op"] == "lever" {
                let entries = trace::capture(|| action(&mut native, &fixture, step));
                expected.push((digest(&native, &watched), entries));
                operations.push(Some(step.clone()));
            } else {
                assert_eq!(step["diagnose"], true);
            }
        }
        for (optimize, assume_instant) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (mut world, _) = load(&fixture);
            world.set_random_tick_speed(0);
            world.disable_command_output_limits_for_replay();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        optimize,
                        assume_instant,
                        ..Default::default()
                    },
                    world.scheduler().iter_entries().collect(),
                    Default::default(),
                )
                .unwrap();
            world.native_scheduler().clear();
            for (index, (operation, (state, entries))) in
                operations.iter().zip(&expected).enumerate()
            {
                let actual = trace::capture(|| {
                    if let Some(operation) = operation {
                        let pos = local_pos(&operation["pos"]) - BASE + origin(&fixture);
                        let Block::Lever { lever } = world.get_block(pos) else {
                            panic!("missing fixture control")
                        };
                        if lever.powered != operation["powered"].as_bool().unwrap() {
                            compiler.on_use_block(pos);
                        }
                    } else {
                        compiler.tick_with_world(&mut world);
                    }
                    compiler.flush(&mut world);
                });
                assert_eq!(
                    &actual, entries,
                    "{name} {} operation={index} O={optimize} A={assume_instant} piston trace",
                    case["id"]
                );
                assert_eq!(
                    &digest(&world, &watched),
                    state,
                    "{name} {} operation={index} O={optimize} A={assume_instant} physical state",
                    case["id"]
                );
            }
            compiler.reset(&mut world, bounds);
            assert_eq!(
                digest(&world, &watched),
                digest(&native, &watched),
                "{name} {} handoff",
                case["id"]
            );
            assert_eq!(
                json!(world.scheduler().iter_entries().collect::<Vec<_>>()),
                json!(native.scheduler().iter_entries().collect::<Vec<_>>()),
                "{name} {} pending handoff",
                case["id"]
            );
            println!(
                "Native/Redpiler {name} {}: {} operations O={optimize} A={assume_instant}",
                case["id"],
                operations.len()
            );
        }
    }
}

#[test]
fn production_divider_preserves_all_declared_native_episodes() {
    compare(
        "fpu_divider",
        manifest("fpu_divider")["cases"].as_array().unwrap(),
    );
}

#[test]
fn production_fpu_preserves_native_inputs_opcodes_trigger_and_outputs() {
    let fixture = manifest("fpu_legal");
    let mut steps = vec![json!({"advance":24})];
    let trigger = &fixture["ports"]["trigger"];
    for opcode in 0..8 {
        steps.push(json!({"op":"lever", "pos":trigger, "powered":false}));
        for key in ["light_blue_operand_inputs", "red_operand_inputs"] {
            for (bit, pos) in fixture["ports"][key].as_array().unwrap().iter().enumerate() {
                steps.push(json!({"op":"lever", "pos":pos, "powered":(bit + opcode) % 3 == 0}));
            }
        }
        for (bit, pos) in fixture["ports"]["opcode_levers_by_sign_weight_4_2_1"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            steps.push(json!({"op":"lever", "pos":pos, "powered":opcode & (4 >> bit) != 0}));
        }
        steps.push(json!({"advance":48}));
        steps.push(json!({"op":"lever", "pos":trigger, "powered":true}));
        steps.push(json!({"advance":48}));
    }
    compare(
        "fpu_legal",
        &[json!({"id":"all-opcodes-native-timing", "steps":steps})],
    );
}
