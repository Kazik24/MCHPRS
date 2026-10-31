//! Native versus sampled Redpiler throughput on the frozen arithmetic fixtures.
use mchprs_blocks::{BlockFace, BlockPos, blocks::{Block, LeverFace}};
use mchprs_core::{
    plot::{
        PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    redstone,
    world::{World, storage::Chunk},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn pos(v: &Value) -> BlockPos {
    BlockPos::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}

fn load(id: &str) -> (Value, PlotWorld, (BlockPos, BlockPos)) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("test_data/instant-pistons-io/fixtures/{id}.json"))).unwrap()).unwrap();
    let schematic = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&schematic)),
        manifest["sha256"].as_str().unwrap(),
        "fixture hash"
    );
    let clipboard = load_schematic(std::io::Cursor::new(schematic)).unwrap();
    let inspection: Value = serde_json::from_slice(&std::fs::read(root.join(manifest["inspection"].as_str().unwrap())).unwrap()).unwrap();
    assert_eq!(inspection["sha256"], manifest["sha256"], "inspection hash");
    assert_eq!(
        [clipboard.size_x, clipboard.size_y, clipboard.size_z],
        [
            manifest["dimensions"][0].as_u64().unwrap() as u32,
            manifest["dimensions"][1].as_u64().unwrap() as u32,
            manifest["dimensions"][2].as_u64().unwrap() as u32
        ]
    );
    let origin = pos(&manifest["coordinates"]["origin"]);
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(&mut world, &clipboard, pos(&manifest["coordinates"]["paste_anchor"]), false);
    let bounds = if id == "counter_basic" {
        (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(27, 20, 38))
    } else {
        (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(27, 11, 50))
    };
    (manifest, world, bounds)
}

fn compiler(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Compiler {
    let mut c = Compiler::default();
    c.compile(
        world,
        bounds,
        CompilerOptions::default(),
        world.scheduler().iter_entries().collect(),
        Arc::new(TaskMonitor::default()),
    )
    .unwrap();
    c
}

fn lever(world: &mut PlotWorld, p: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(p) else {
        panic!("missing lever at {p:?}")
    };
    if lever.powered != powered {
        lever.powered = powered;
        world.set_block(p, Block::Lever { lever });
        redstone::update_surrounding_blocks(world, p);
        let support = match lever.face {
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        };
        redstone::update_surrounding_blocks(world, p.offset(support));
    }
}

fn counter_reference_check() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes = std::fs::read(root.join("test_data/instant-pistons-io/reference/counter-reference-timeline.csv")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "929a9c6a49aaf381780a3177bca97264a135019588cfe5061d08c85c8bec5f37"
    );
    let timeline = std::str::from_utf8(&bytes).unwrap();
    let (manifest, mut native, _) = load("counter_basic");
    let (_, mut published, bounds2) = load("counter_basic");
    let origin = pos(&manifest["coordinates"]["origin"]);
    let trigger = origin + pos(&manifest["ports"]["inputs"]["trigger"]);
    let mut compiled = compiler(&published, bounds2);
    lever(&mut native, trigger, true);
    compiled.try_on_use_block(trigger).unwrap();
    let mut native_reference_divergence = None;
    let mut compiled_reference_divergence = None;
    let mut native_compiled_divergence = None;
    for tick in 1..=102u64 {
        native.tick_interpreted();
        compiled.tick().unwrap();
        compiled.try_flush(&mut published).unwrap();
        let line = timeline.lines().find(|line| line.starts_with(&format!("release,{tick},"))).unwrap();
        let parts: Vec<_> = line.split('"').collect();
        let fields: Vec<_> = parts[0].split(',').collect();
        let expected_dust: Vec<u8> = serde_json::from_str(parts[1]).unwrap();
        let expected_repeaters = parts[2].split(',').nth(1).unwrap();
        let mut rows = Vec::new();
        for (label, world) in [("native", &native), ("Redpiler", &published)] {
            let memory: String = manifest["ports"]["observations"]["memory"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let at = origin + pos(p);
                    if world.piston_state().motions.iter().any(|motion| motion.pos == at) {
                        'M'
                    } else {
                        match world.get_block(at) {
                            Block::Piston { piston } => if piston.extended { '0' } else { '1' },
                            _ => '?',
                        }
                    }
                })
                .collect();
            let dust: Vec<_> = manifest["ports"]["observations"]["output_input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| match world.get_block(origin + pos(p)) {
                    Block::RedstoneWire { wire } => wire.power,
                    _ => 255,
                })
                .collect();
            let repeaters: String = manifest["ports"]["observations"]["repeater"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| match world.get_block(origin + pos(p)) {
                    Block::RedstoneRepeater { repeater } => {
                        if repeater.powered {
                            '0'
                        } else {
                            '1'
                        }
                    }
                    _ => '?',
                })
                .collect();
            assert_eq!(memory.len(), fields[2].len());
            for (bit, (actual, expected)) in memory.chars().zip(fields[2].chars()).enumerate() {
                if actual != expected {
                    let report = format!("{label} tick {tick} memory bit {bit}: {actual} vs reference {expected}");
                    if label == "native" { native_reference_divergence.get_or_insert(report); } else { compiled_reference_divergence.get_or_insert(report); }
                }
            }
            if dust != expected_dust {
                let report = format!("{label} tick {tick} dust: {dust:?} vs reference {expected_dust:?}");
                if label == "native" { native_reference_divergence.get_or_insert(report); } else { compiled_reference_divergence.get_or_insert(report); }
            }
            if repeaters != expected_repeaters {
                let report = format!("{label} tick {tick} repeaters: {repeaters} vs reference {expected_repeaters}");
                if label == "native" { native_reference_divergence.get_or_insert(report); } else { compiled_reference_divergence.get_or_insert(report); }
            }
            rows.push((memory, dust, repeaters));
        }
        if rows[0] != rows[1] && native_compiled_divergence.is_none() {
            native_compiled_divergence = Some(format!("tick {tick}: native={:?}; Redpiler={:?}", rows[0], rows[1]));
        }
    }
    println!("COUNTER_BASIC_DIAGNOSTICS {}", serde_json::json!({"native_vs_reference":native_reference_divergence,"redpiler_vs_reference":compiled_reference_divergence,"native_vs_redpiler":native_compiled_divergence}));
}

fn counter(output: &PathBuf, samples: usize, ticks: usize) {
    for sample in 0..samples {
        let (manifest, mut native, bounds) = load("counter_basic");
        let origin = pos(&manifest["coordinates"]["origin"]);
        let trigger = origin + pos(&manifest["ports"]["inputs"]["trigger"]);
        let compile_start = Instant::now();
        let mut compiled = compiler(&native, bounds);
        let compile_ns = compile_start.elapsed().as_nanos();
        lever(&mut native, trigger, true);
        compiled.try_on_use_block(trigger).unwrap();
        let start = Instant::now();
        for _ in 0..ticks {
            native.tick_interpreted();
        }
        let native_ns = start.elapsed().as_nanos();
        let (_, world2, bounds2) = load("counter_basic");
        let mut tick_only = compiler(&world2, bounds2);
        tick_only.try_on_use_block(trigger).unwrap();
        let start = Instant::now();
        for _ in 0..ticks {
            tick_only.tick().unwrap();
        }
        let compiled_ns = start.elapsed().as_nanos();
        let (_, mut world3, bounds3) = load("counter_basic");
        let mut full = compiler(&world3, bounds3);
        full.try_on_use_block(trigger).unwrap();
        let start = Instant::now();
        for _ in 0..ticks {
            full.tick().unwrap();
            full.try_flush(&mut world3).unwrap();
        }
        let full_ns = start.elapsed().as_nanos();
        assert!(tick_only.is_active() && full.is_active());
        let row = serde_json::json!({"circuit":"COUNTER_BASIC","sample":sample+1,"ticks":ticks,"compile_ns":compile_ns,"native_tick_ns":native_ns,"compiled_tick_ns":compiled_ns,"compiled_tick_flush_ns":full_ns,"native_tps":ticks as f64*1e9/native_ns as f64,"compiled_tick_tps":ticks as f64*1e9/compiled_ns as f64,"compiled_tick_flush_tps":ticks as f64*1e9/full_ns as f64,"scope":"headless, no clients/network"});
        println!("{}", row);
        std::fs::write(output.join(format!("counter-{sample}.json")), serde_json::to_vec_pretty(&row).unwrap()).unwrap();
    }
}

fn adder(output: &PathBuf, samples: usize, operations: usize) {
    // Inputs exercise full carry, 1023+1, zero and repeated re-arming on one compiled instance.
    let cases = [(2047u64, 1u64, 0u64), (1023, 1, 0), (0, 0, 0)];
    for sample in 0..samples {
        let (manifest, mut native, _) = load("adder_11bits");
        let (_, mut world, bounds) = load("adder_11bits");
        let origin = pos(&manifest["coordinates"]["origin"]);
        let compile_start = Instant::now();
        let mut compiled = compiler(&world, bounds);
        let compile_ns = compile_start.elapsed().as_nanos();
        let inputs = &manifest["ports"]["inputs"];
        let trigger = origin + pos(&inputs["trigger"]);
        let mut powered = std::collections::HashMap::new();
        for (name, ports) in inputs.as_object().unwrap() {
            if name == "trigger" {
                continue;
            }
            for p in ports.as_array().unwrap() {
                let p = origin + pos(p);
                let Block::Lever { lever } = world.get_block(p) else {
                    panic!("missing lever at {p:?}")
                };
                powered.insert(p, lever.powered);
            }
        }
        let Block::Lever { lever: trigger_lever } = world.get_block(trigger) else {
            panic!("missing trigger")
        };
        let mut trigger_powered = trigger_lever.powered;
        let mut native_trigger_powered = match native.get_block(trigger) {
            Block::Lever { lever } => lever.powered,
            _ => panic!("missing native trigger"),
        };
        let set = |compiled: &mut Compiler, state: &mut std::collections::HashMap<BlockPos, bool>, p: BlockPos, desired: bool| {
            if state[&p] != desired {
                compiled.try_on_use_block(p).unwrap();
                state.insert(p, desired);
            }
        };
        let mut active = 0usize;
        let mut ns = 0u128;
        let mut native_ns = 0u128;
        let overall_start = Instant::now();
        for i in 0..operations {
            let (a, b, cin) = cases[i % cases.len()];
            if !trigger_powered {
                compiled.try_on_use_block(trigger).unwrap();
                trigger_powered = true;
            }
            if !native_trigger_powered {
                lever(&mut native, trigger, true);
                native_trigger_powered = true;
            }
            if i > 0 {
                for _ in 0..32 {
                    compiled.tick().unwrap();
                    native.tick_interpreted();
                }
            }
            for (name, value) in [("A", a), ("B", b), ("Cin", cin)] {
                if let Some(ports) = inputs.get(name).and_then(Value::as_array) {
                    for (bit, p) in ports.iter().enumerate() {
                        let p = origin + pos(p);
                        let desired = value & (1 << bit) == 0;
                        set(&mut compiled, &mut powered, p, desired);
                        lever(&mut native, p, desired);
                    }
                }
            }
            for _ in 0..32 {
                compiled.tick().unwrap();
                native.tick_interpreted();
            }
            if trigger_powered {
                compiled.try_on_use_block(trigger).unwrap();
                trigger_powered = false;
            }
            if native_trigger_powered {
                lever(&mut native, trigger, false);
                native_trigger_powered = false;
            }
            let expected = (a + b + cin) & 2047;
            let expected_carry = (a + b + cin) >> 11;
            for tick in 1..=24 {
                let start_c = Instant::now();
                compiled.tick().unwrap();
                compiled.try_flush(&mut world).unwrap();
                ns += start_c.elapsed().as_nanos();
                let start_n = Instant::now();
                native.tick_interpreted();
                native_ns += start_n.elapsed().as_nanos();
                active += 1;
                if (1..=3).contains(&tick) || (3..=7).contains(&tick) || (5..=9).contains(&tick) {
                    for (port, window, value) in [
                        ("sum_payload", 1..=3, expected),
                        ("sum_repeater", 3..=7, expected),
                        ("carry_repeater", 5..=9, expected_carry),
                    ] {
                        if manifest["ports"]["observations"].get(port).is_none() {
                            continue;
                        }
                        if !window.contains(&tick) {
                            continue;
                        }
                        let actual = manifest["ports"]["observations"][port]
                            .as_array()
                            .unwrap()
                            .iter()
                            .enumerate()
                            .fold(0u64, |word, (bit, p)| {
                                let block = world.get_block(origin + pos(p));
                                let value = match (port, block) {
                                    ("sum_payload", Block::Air) => 1,
                                    ("sum_payload", Block::RedstoneBlock) => 0,
                                    ("sum_repeater", Block::RedstoneRepeater { repeater }) => u64::from(!repeater.powered),
                                    ("carry_repeater", Block::RedstoneRepeater { repeater }) => u64::from(!repeater.powered),
                                    _ => panic!("invalid {port} observation {block:?}"),
                                };
                                word | (value << bit)
                            });
                        let native_actual =
                            manifest["ports"]["observations"][port]
                                .as_array()
                                .unwrap()
                                .iter()
                                .enumerate()
                                .fold(0u64, |word, (bit, p)| {
                                    let block = native.get_block(origin + pos(p));
                                    let value = match (port, block) {
                                        ("sum_payload", Block::Air) => 1,
                                        ("sum_payload", Block::RedstoneBlock) => 0,
                                        ("sum_repeater", Block::RedstoneRepeater { repeater }) => u64::from(!repeater.powered),
                                        ("carry_repeater", Block::RedstoneRepeater { repeater }) => u64::from(!repeater.powered),
                                        _ => panic!("invalid native {port} observation {block:?}"),
                                    };
                                    word | (value << bit)
                                });
                        assert_eq!(actual, value, "Redpiler ADDER11BIT {} tick {tick} {port}", i % cases.len());
                        assert_eq!(
                            native_actual,
                            actual,
                            "native vs Redpiler ADDER11BIT {} tick {tick} {port}",
                            i % cases.len()
                        );
                    }
                }
            }
        }
        let end_to_end_ns = overall_start.elapsed().as_nanos();
        assert!(active >= 512, "need >=512 active ticks");
        assert!(compiled.is_active());
        let row = serde_json::json!({"circuit":"ADDER11BIT","sample":sample+1,"operations":operations,"active_ticks":active,"compile_ns":compile_ns,"end_to_end_ns":end_to_end_ns,"native_tick_ns":native_ns,"compiled_tick_flush_ns":ns,"native_tps":active as f64*1e9/native_ns as f64,"compiled_tick_flush_tps":active as f64*1e9/ns as f64,"case_cycle":["2047+1","1023+1","0+0"],"validation":"native and Redpiler sum_payload ticks 1-3; sum_repeater ticks 3-7"});
        println!("{}", row);
        std::fs::write(output.join(format!("adder-{sample}.json")), serde_json::to_vec_pretty(&row).unwrap()).unwrap();
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let output = PathBuf::from(
        args.next()
            .expect("usage: bench_arithmetic <output-dir> [samples] [ticks-per-counter-sample] [adder-operations]"),
    );
    let samples = args.next().map(|s| s.parse().unwrap()).unwrap_or(3);
    let ticks = args.next().map(|s| s.parse().unwrap()).unwrap_or(600);
    let operations = args.next().map(|s| s.parse().unwrap()).unwrap_or(24);
    assert!(samples > 0 && ticks >= 512 && operations >= 22);
    std::fs::create_dir_all(&output).unwrap();
    counter(&output, samples, ticks);
    counter_reference_check();
    adder(&output, samples, operations);
}
