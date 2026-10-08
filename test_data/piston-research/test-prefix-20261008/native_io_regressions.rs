//! Behavioral coverage for the separately versioned lever/consumer fixtures.
use super::*;

fn io_pack() -> PathBuf {
    root().join("test_data/instant-pistons-io")
}
fn io_fixture(id: &str) -> Value {
    manifests_at(&io_pack())
        .into_iter()
        .find(|m| m["id"] == id)
        .unwrap()
}
fn run(id: &str, name: &str, rotation: u32, capture: bool) -> Value {
    let m = io_fixture(id);
    episode(&m, &named_case(&m, name), rotation, "game", capture)
}
fn extended(v: &Value) -> bool {
    assert!(matches!(
        v["name"].as_str(),
        Some("piston" | "sticky_piston")
    ));
    v["properties"]["extended"] == "true"
}
fn repeater_bits(v: &Value) -> u16 {
    if let Some(bank) = v.as_array() {
        return bank
            .iter()
            .enumerate()
            .fold(0, |n, (i, b)| n | (repeater_bits(b) << i));
    }
    assert_eq!(v["name"], "repeater");
    (v["properties"]["powered"] == "false") as u16
}
fn memory_bits(bank: &Value) -> Option<u16> {
    bank.as_array()
        .unwrap()
        .iter()
        .enumerate()
        .try_fold(0, |n, (i, b)| {
            if b["name"] == "moving_piston" {
                None
            } else {
                Some(n | ((!extended(b)) as u16) << i)
            }
        })
}

#[test]
fn redpiler_preserves_all_small_io_episodes_and_rotations() {
    use crate::redpiler::{Compiler, CompilerOptions};
    let mut failures = Vec::new();
    let mut episodes = 0;
    for manifest in manifests_at(&io_pack()) {
        for case in manifest["cases"].as_array().unwrap() {
            let mut manifest = manifest.clone();
            if let Some(extra) = case["observations"].as_object() {
                for (name, positions) in extra {
                    manifest["ports"]["observations"][name] = positions.clone();
                }
            }
            for rotation in [0, 90, 180, 270] {
                for (optimize, assume_instant) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    let label = format!(
                        "{} {} r={rotation} O={optimize} A={assume_instant}",
                        manifest["id"], case["id"]
                    );
                    let (mut native, watch, dimensions) = load(&manifest, rotation);
                    let (mut world, _, _) = load(&manifest, rotation);
                    native.set_random_tick_speed(0);
                    world.set_random_tick_speed(0);
                    let options = || CompilerOptions {
                        optimize,
                        assume_instant,
                        ..Default::default()
                    };
                    let bounds = world.get_corners();
                    let mut compiler = Compiler::default();
                    let mut admitted = true;
                    for operation in case["actions"].as_array().unwrap() {
                        if operation["op"] == "wait" || operation["op"] == "wait_ready" {
                            let mut stable = 0;
                            let mut previous = physical(&native, &watch)["cells"].clone();
                            for _ in 0..operation["ticks"].as_u64().unwrap() {
                                if compiler.is_active() {
                                    let expected = capture_at(&watch, || native.tick_interpreted());
                                    let actual = capture_at(&watch, || {
                                        compiler.tick_with_world(&mut world);
                                        compiler.flush(&mut world);
                                    });
                                    assert_eq!(actual, expected, "{label} setup callbacks");
                                } else {
                                    native.tick_interpreted();
                                    world.tick_interpreted();
                                }
                                let now = physical(&native, &watch)["cells"].clone();
                                assert_eq!(
                                    physical(&world, &watch)["cells"],
                                    now,
                                    "{label} setup cells"
                                );
                                stable = if quiet(&native) && now == previous {
                                    stable + 1
                                } else {
                                    0
                                };
                                previous = now;
                                if operation["op"] == "wait_ready" && stable >= 2 {
                                    break;
                                }
                            }
                            if operation["op"] == "wait_ready" {
                                assert!(stable >= 2, "{label} native readiness");
                            }
                        } else if operation["op"] == "lever" {
                            if !compiler.is_active() {
                                if let Err(error) = compiler.compile(
                                    &world,
                                    bounds,
                                    options(),
                                    world.scheduler().iter_entries().collect(),
                                    Default::default(),
                                ) {
                                    failures.push(format!("{label}: {error}"));
                                    admitted = false;
                                    break;
                                }
                                world.native_scheduler().clear();
                            }
                            let pos = transform(triple(&operation["pos"]), dimensions, rotation);
                            let expected = capture_at(&watch, || {
                                mutate(&mut native, operation, dimensions, rotation)
                            });
                            let actual = capture_at(&watch, || {
                                let Block::Lever { lever } = world.get_block(pos) else {
                                    panic!("{label}: missing lever")
                                };
                                if lever.powered != operation["powered"].as_bool().unwrap() {
                                    compiler.on_use_block(pos);
                                }
                                compiler.flush(&mut world);
                            });
                            assert_eq!(actual, expected, "{label} lever callbacks");
                        } else {
                            if compiler.is_active() {
                                compiler.reset(&mut world, bounds);
                            }
                            mutate(&mut native, operation, dimensions, rotation);
                            mutate(&mut world, operation, dimensions, rotation);
                        }
                    }
                    if !admitted {
                        continue;
                    }
                    if !compiler.is_active() {
                        if let Err(error) = compiler.compile(
                            &world,
                            bounds,
                            options(),
                            world.scheduler().iter_entries().collect(),
                            Default::default(),
                        ) {
                            failures.push(format!("{label}: {error}"));
                            continue;
                        }
                        world.native_scheduler().clear();
                    }
                    for tick in 1..=case["ticks"].as_u64().unwrap() {
                        let expected = capture_at(&watch, || native.tick_interpreted());
                        let actual = capture_at(&watch, || {
                            compiler.tick_with_world(&mut world);
                            compiler.flush(&mut world);
                        });
                        assert_eq!(actual, expected, "{label} tick={tick} callbacks");
                        assert_eq!(
                            physical(&world, &watch)["cells"],
                            physical(&native, &watch)["cells"],
                            "{label} tick={tick} cells"
                        );
                        assert_eq!(
                            serde_json::to_value(world.piston_state()).unwrap(),
                            serde_json::to_value(native.piston_state()).unwrap(),
                            "{label} tick={tick} work"
                        );
                    }
                    compiler.reset(&mut world, bounds);
                    assert_eq!(
                        physical(&world, &watch),
                        physical(&native, &watch),
                        "{label} handoff"
                    );
                    episodes += 1;
                }
            }
        }
        println!(
            "Small traces {}: {episodes} admitted episodes, {} admission failures so far",
            manifest["id"],
            failures.len()
        );
    }
    println!("Native/Redpiler small trace episodes: {episodes}");
    assert!(
        failures.is_empty(),
        "{} small trace admission failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn strict_io_imports_remain_quiescent_and_preserve_saved_state() {
    for m in manifests_at(&io_pack()) {
        let (mut w, watch, d) = load(&m, 0);
        assert!(quiet(&w));
        let before = physical(&w, &watch)["cells"].clone();
        for _ in 0..24 {
            w.tick_interpreted();
        }
        assert_eq!(
            before,
            physical(&w, &watch)["cells"],
            "{} saved idle",
            m["id"]
        );
        assert!(quiet(&w));
        assert!(!observe(&w, &m, d, 0).as_object().unwrap().is_empty());
    }
}

#[test]
fn independent_bud_data_changes_hold_state_until_a_qualifying_update() {
    for id in ["bud_noninstantinputs", "bud_pistonupdate"] {
        for r in [0, 90, 180, 270] {
            for name in ["data-only", "update-before-data"] {
                let t = run(id, name, r, false);
                let p = &t["projections"][24];
                assert!(extended(&p["memory"]), "{id} {name} rotation {r}");
                assert_eq!(p["memory"]["piston_power"], false);
                assert_eq!(repeater_bits(&p["repeater"]), 0);
            }
            let t = run(id, "sample-1", r, true);
            assert_eq!(t["projections"][2]["memory"]["name"], "moving_piston");
            assert!(!extended(&t["projections"][4]["memory"]));
            assert_eq!(repeater_bits(&t["projections"][5]["repeater"]), 0);
            assert_eq!(repeater_bits(&t["projections"][6]["repeater"]), 1);
            let m = io_fixture(id);
            let pos = transform(
                triple(&m["ports"]["observations"]["memory"]),
                (
                    m["dimensions"][0].as_u64().unwrap() as u32,
                    9,
                    m["dimensions"][2].as_u64().unwrap() as u32,
                ),
                r,
            );
            assert!(applied(&t)
                .iter()
                .any(|e| e["data"]["pos"] == json!(pos) && e["data"]["action"] == "Retract"));
        }
    }
}

#[test]
fn bud_storage_survives_restored_power_and_resamples_on_the_next_update() {
    for id in ["bud_noninstantinputs", "bud_pistonupdate"] {
        let t = run(id, "store-hold-resample", 0, true);
        // After restoring data and demonstrating quiescence, power is present
        // while the mechanism still stores the previously sampled one.
        let hold = t["samples"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["label"] == "after-action" && s["action"] == 6)
            .unwrap();
        assert!(!extended(&hold["observations"]["memory"]));
        assert_eq!(hold["observations"]["memory"]["piston_power"], true);
        assert_eq!(repeater_bits(&hold["observations"]["repeater"]), 1);
        assert!(extended(&t["projections"][24]["memory"]));
        assert_eq!(repeater_bits(&t["projections"][8]["repeater"]), 0);
        assert!(t["readiness_checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["quiet"] == true && r["stable_boundaries"] == 2));
    }
}

#[test]
fn observer_update_samples_instant_produced_data_after_its_wave() {
    let t = run("bud_instantmemorycellobserverupdate", "sample-1", 0, true);
    assert_eq!(power(&t["projections"][1]["QC_data_wire"]), 0);
    assert_eq!(power(&t["projections"][2]["update_wire"]), 0);
    assert!(extended(&t["projections"][2]["memory"]));
    assert!(power(&t["projections"][3]["update_wire"]) > 0);
    assert_eq!(t["projections"][3]["memory"]["name"], "moving_piston");
    assert!(!extended(&t["projections"][5]["memory"]));
    assert_eq!(repeater_bits(&t["projections"][7]["repeater"]), 1);
    for p in t["projections"].as_array().unwrap().iter().skip(5) {
        assert!(!extended(&p["memory"]));
    }
}

#[test]
fn lever_chain_propagates_through_nested_callbacks_in_one_tick() {
    let m = io_fixture("instant_chain");
    let t = episode(&m, &named_case(&m, "activate"), 0, "pico", true);
    let entries: Vec<_> = t["samples"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["callbacks"].as_array().unwrap())
        .collect();
    let first = entries
        .iter()
        .position(|e| {
            e["kind"] == "event_execute" && e["data"]["pos"] == json!(BlockPos::new(40, 31, 45))
        })
        .unwrap();
    let request = entries
        .iter()
        .position(|e| {
            e["kind"] == "event_enqueue" && e["data"]["pos"] == json!(BlockPos::new(40, 31, 48))
        })
        .unwrap();
    let second = entries
        .iter()
        .position(|e| {
            e["kind"] == "event_execute" && e["data"]["pos"] == json!(BlockPos::new(40, 31, 48))
        })
        .unwrap();
    assert!(first < request && request < second);
    assert_eq!(entries[first]["tick"], 2);
    assert_eq!(entries[second]["tick"], 2);
    assert_eq!(entries[request]["operation"], entries[first]["operation"]);
    assert_ne!(entries[second]["operation"], entries[first]["operation"]);
    assert_eq!(entries[request - 1]["kind"], "callback");
    assert_eq!(entries[request - 1]["data"]["piston_power"], false);
}

#[test]
fn output_stage_rejects_pending_retraction_after_inhibition_becomes_effective() {
    // The triggering piston may retract first; acceptance of the downstream
    // event, rather than movement-completion order, is the relevant boundary.
    let t = run("not_1", "events-11-21", 0, true);
    let entries: Vec<_> = t["samples"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["callbacks"].as_array().unwrap())
        .collect();
    let trigger = json!(BlockPos::new(40, 32, 50));
    let inhibit = json!(BlockPos::new(42, 32, 50));
    let output = json!(BlockPos::new(41, 32, 45));
    let locate = |kind: &str, pos: &Value| {
        entries
            .iter()
            .position(|e| e["kind"] == kind && e["tick"] == 2 && e["data"]["pos"] == *pos)
            .unwrap()
    };
    let t_exec = locate("event_execute", &trigger);
    let o_enqueue = locate("event_enqueue", &output);
    let i_exec = locate("event_execute", &inhibit);
    let o_exec = locate("event_execute", &output);
    assert!(t_exec < o_enqueue && o_enqueue < i_exec && i_exec < o_exec);
    assert!(entries[i_exec..o_exec]
        .iter()
        .any(|e| e["kind"] == "callback"
            && e["data"]["pos"] == output
            && e["data"]["piston_power"] == true));
    assert!(!entries
        .iter()
        .any(|e| e["kind"] == "event_applied" && e["tick"] == 2 && e["data"]["pos"] == output));
    assert_eq!(repeater_bits(&t["projections"][4]["repeater"]), 0);
}

#[test]
fn new_or_output_stage_preserves_shared_payload_and_illegal_reset_starvation() {
    for id in ["or_1", "or_interpreter_illigal"] {
        let m = io_fixture(id);
        let y = if id == "or_1" { 1 } else { 2 };
        for name in [
            "events-10-12",
            "events-01-12",
            "events-11-12",
            "events-11-21",
        ] {
            let t = run(id, name, 0, true);
            let indices: Vec<_> = [[0, y, 7], [1, y, 7], [0, y, 8]]
                .into_iter()
                .map(|p| {
                    t["watch_absolute"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .position(|v| *v == json!(BlockPos::new(40 + p[0], 30 + p[1], 40 + p[2])))
                        .unwrap()
                })
                .collect();
            let mut cells = std::collections::BTreeMap::new();
            for s in t["samples"].as_array().unwrap() {
                for c in s["changes"].as_array().unwrap() {
                    cells.insert(c[0].as_u64().unwrap() as usize, c[1].clone());
                }
                let payloads = indices
                    .iter()
                    .filter(|i| {
                        cells[i]["name"] == "redstone_block"
                            || cells[i]["entity"]["MovingPiston"]["block_state"]
                                == Block::RedstoneBlock {}.get_id()
                    })
                    .count();
                assert_eq!(
                    payloads, 1,
                    "{id} {name} tick{} operation{}",
                    s["tick"], s["operation"]
                );
            }
            assert_eq!(repeater_bits(&t["projections"][4]["repeater"]), 1);
            if id == "or_interpreter_illigal" && name.starts_with("events-11") {
                let (mut w, _, d) = load(&m, 0);
                for op in named_case(&m, name)["actions"].as_array().unwrap() {
                    mutate(&mut w, op, d, 0);
                }
                for _ in 0..6 {
                    w.tick_interpreted();
                }
                let restored = [[0,2,9],[2,2,7]].into_iter().filter(|p|
                    matches!(w.get_block(BlockPos::new(40+p[0],30+p[1],40+p[2])),Block::Piston{piston} if piston.extended)).count();
                assert_eq!(
                    restored, 1,
                    "one owner takes the reset supply from the other participant"
                );
            }
        }
    }
}

#[test]
fn gate_consumers_have_a_delayed_projection_and_preserve_history_and_reset_effects() {
    for id in ["or_1", "and_1", "and_2", "not_1", "xor_simple"] {
        let m = io_fixture(id);
        for c in m["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["id"].as_str().unwrap().starts_with("events-"))
        {
            let a = c["inputs"][0] == 1;
            let b = c["inputs"][1] == 1;
            let expected = match id {
                "or_1" => a || b,
                "and_1" | "and_2" => a && b,
                "not_1" => b && !a,
                "xor_simple" => a ^ b,
                _ => unreachable!(),
            };
            let t = episode(&m, c, 0, "game", false);
            assert_eq!(
                repeater_bits(&t["projections"][4]["repeater"]),
                expected as u16,
                "{id} {}",
                c["id"]
            );
            assert_eq!(
                t["projections"][8]["lamp"]["properties"]["lit"],
                if expected { "false" } else { "true" }
            );
        }
    }
    assert_eq!(
        repeater_bits(&run("and_3", "events-11-12", 0, false)["projections"][4]["repeater"]),
        0
    );
    assert_eq!(
        repeater_bits(&run("and_3", "events-11-21", 0, false)["projections"][4]["repeater"]),
        1
    );
    let reset = run("xor_simple", "events-11-12", 0, false);
    assert_eq!(repeater_bits(&reset["projections"][4]["repeater"]), 0);
    assert_eq!(repeater_bits(&reset["projections"][8]["repeater"]), 1);
    let other = run("xor_simple", "events-11-21", 0, false);
    assert!(other["projections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| repeater_bits(&p["repeater"]) == 0));
}

#[test]
fn lever_torch_delay_and_held_levels_do_not_create_additional_root_computations() {
    for id in [
        "instant_observer",
        "instant_torch",
        "instant_reset_redstone",
        "instant_reset_redstone_2",
        "instant_down",
        "instant_down_torch_reset",
        "instant_chain",
    ] {
        let m = io_fixture(id);
        let mut c = named_case(&m, "activate");
        c["ticks"] = json!(36);
        let active = episode(&m, &c, 0, "game", true);
        assert!(power(&active["projections"][1]["raw_output"]) > 0);
        assert_eq!(power(&active["projections"][2]["raw_output"]), 0);
        assert_eq!(repeater_bits(&active["projections"][5]["repeater"]), 0);
        assert_eq!(repeater_bits(&active["projections"][6]["repeater"]), 1);
        let held = run(id, "held-active", 0, false);
        assert_eq!(
            held["projections"],
            json!(&active["projections"].as_array().unwrap()[12..]),
            "{id} held level"
        );
    }
    let blocked = run("instant_blocked", "activate", 0, true);
    assert!(applied(&blocked).is_empty());
    for p in blocked["projections"].as_array().unwrap() {
        assert!(extended(&p["base"]));
    }
    for id in ["instant_torch", "instant_down_torch_reset"] {
        let t = run(id, "reuse-after-quiescence", 0, true);
        assert!(t["readiness_checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["quiet"] == true));
        assert!(
            applied(&t)
                .iter()
                .filter(|e| e["data"]["action"] == "Retract")
                .count()
                >= 2
        );
    }
}

#[test]
fn lever_prepared_adders_match_math_at_payload_and_consumer_windows() {
    for id in ["adder_1bit", "adder_11bits"] {
        let m = io_fixture(id);
        for c in m["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["expectation"].is_object())
        {
            let t = episode(&m, c, 0, "game", false);
            let expected = c["expectation"]["sum"].as_u64().unwrap() as u16;
            for i in 1..=3 {
                let payload = &t["projections"][i]["sum_payload"];
                let actual = if payload.is_array() {
                    payload
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                        .try_fold(0, |n, (i, b)| bit(b).map(|b| n | (b << i)))
                } else {
                    bit(payload)
                };
                assert_eq!(
                    actual,
                    Some(expected),
                    "{id} {} payload boundary {i}",
                    c["id"]
                );
            }
            for i in 3..=7 {
                assert_eq!(
                    repeater_bits(&t["projections"][i]["sum_repeater"]),
                    expected,
                    "{id} {} sum consumer {i}",
                    c["id"]
                );
            }
            if id == "adder_1bit" {
                for i in 5..=9 {
                    assert_eq!(
                        repeater_bits(&t["projections"][i]["carry_repeater"]),
                        c["expectation"]["carry"].as_u64().unwrap() as u16
                    );
                }
            }
        }
    }
}

#[test]
fn counter_memory_and_repeater_bank_have_distinct_valid_count_windows() {
    let t = run("counter_basic", "release", 0, false);
    for n in 1..=16usize {
        assert_eq!(
            memory_bits(&t["projections"][6 * n]["memory"]),
            Some(n as u16)
        );
        for i in 6 * n + 5..=(6 * n + 8).min(102) {
            assert_eq!(
                repeater_bits(&t["projections"][i]["repeater"]),
                n as u16,
                "count {n} consumer boundary {i}"
            );
        }
    }
    assert_eq!(repeater_bits(&t["projections"][22]["repeater"]), 1);
    assert_eq!(repeater_bits(&t["projections"][23]["repeater"]), 3);
}

#[test]
fn io_episodes_agree_at_completed_boundaries_under_game_nano_and_pico_inspection() {
    for m in manifests_at(&io_pack()) {
        for c in m["cases"].as_array().unwrap() {
            for r in m["rotations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_u64().unwrap() as u32)
            {
                let game = episode(&m, c, r, "game", false);
                for mode in ["nano", "pico"] {
                    let fine = episode(&m, c, r, mode, false);
                    assert_eq!(
                        game["completed_boundaries"], fine["completed_boundaries"],
                        "{} {} {r} {mode}",
                        m["id"], c["id"]
                    );
                    assert_eq!(game["projections"], fine["projections"]);
                }
            }
        }
    }
}
