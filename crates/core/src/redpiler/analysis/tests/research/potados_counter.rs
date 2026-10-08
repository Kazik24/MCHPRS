//! Opt-in compile and native timing probe for TEST_POTADOS_PC_COUNTER.schem.
use super::*;
use crate::redstone::piston::trace;
use mchprs_blocks::blocks::Block;
use sha2::{Digest, Sha256};
use std::time::Instant;

const FIXTURE: &str =
    "test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem";
const CONTROL_LABELS: [(&str, BlockPos); 7] = [
    ("CIN", BlockPos::new(6, 9, 73)),
    ("CH 3 FRW", BlockPos::new(11, 13, 65)),
    ("READ CH 3", BlockPos::new(14, 13, 67)),
    ("WRITE", BlockPos::new(17, 12, 69)),
    ("READ CH R2", BlockPos::new(14, 7, 69)),
    ("READ CH R1", BlockPos::new(16, 8, 70)),
    ("WRITE BUS", BlockPos::new(18, 10, 70)),
];

fn load_counter() -> (PlotWorld, (BlockPos, BlockPos)) {
    let bytes = std::fs::read(root().join(FIXTURE)).unwrap();
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
    (world, (first, last))
}

fn bus(world: &PlotWorld) -> String {
    (0..16)
        .map(|bit| {
            let pos = BASE + BlockPos::new(13, 2, 3 + bit * 4);
            match world.get_block(pos) {
                Block::RedstoneRepeater { repeater } => {
                    if repeater.powered {
                        '1'
                    } else {
                        '0'
                    }
                }
                other => panic!(
                    "output bit {bit} at {pos:?} is {}, expected repeater",
                    other.get_name()
                ),
            }
        })
        .collect()
}

fn lever_positions(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Vec<BlockPos> {
    let mut found = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                if matches!(world.get_block(pos), Block::Lever { .. }) {
                    found.push(pos);
                }
            }
        }
    }
    found
}

fn turn_on_all_levers(world: &mut PlotWorld, bounds: (BlockPos, BlockPos)) -> Value {
    let positions = lever_positions(world, bounds);
    assert_eq!(positions.len(), 23, "all 23 schematic levers are expected");
    let mut actions = Vec::new();
    for pos in positions {
        let Block::Lever { lever } = world.get_block(pos) else {
            unreachable!()
        };
        assert!(!lever.powered, "lever at {:?} starts off", pos - BASE);
        let action = trace::capture(|| lever_action(world, pos, true));
        let local = pos - BASE;
        let label = CONTROL_LABELS
            .iter()
            .find_map(|(name, control)| (*control == local).then_some(*name))
            .unwrap_or("input lever");
        actions.push(json!({"label":label,"pos":local,"action":compact_trace(&action)}));
    }
    let activation_tick = trace::capture(|| world.tick_interpreted());
    json!({"count":actions.len(),"all_left_on":true,"actions":actions,
        "activation_tick":compact_trace(&activation_tick),"bus_after_activation_tick":bus(world),
        "pending_ticks":world.scheduler().iter_entries().count(),
        "piston_events":world.piston_state().events.len(),"motions":world.piston_state().motions.len()})
}

fn sampling_candidate_diagnostic(world: &PlotWorld) -> Value {
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let report = crate::redpiler::analysis::analyze(
        world,
        world.get_corners(),
        &ticks,
        &crate::redpiler::TaskMonitor::default(),
        crate::redpiler::analysis::AnalysisLimits::default(),
    )
    .unwrap();
    let pos = BASE + BlockPos::new(6, 15, 4);
    let actor = report
        .pistons
        .iter()
        .position(|piston| piston.pos == pos)
        .unwrap();
    let piston = &report.pistons[actor];
    let recognition = &report.recognition[actor];
    let reset_observers = crate::redpiler::instant::sampling::reset_candidates(world, &report);
    let group = report
        .payload_groups
        .iter()
        .find(|group| group.members.contains(&actor))
        .unwrap();
    let nearby_blocks: Vec<_> = (-2..=2)
        .flat_map(|dy| {
            (-2..=2).flat_map(move |dz| (-2..=2).map(move |dx| BlockPos::new(dx, dy, dz)))
        })
        .filter_map(|offset| {
            let nearby = pos + offset;
            let block = world.get_block(nearby);
            (block != Block::Air).then(|| {
                json!({"pos":nearby-BASE,"state":block.get_name(),"properties":block.properties()})
            })
        })
        .collect();
    let sources: Vec<_> = recognition
        .inputs
        .sources
        .iter()
        .map(|source| {
            json!({"source":source.source-BASE,"kind":source.kind,"route":source.route,
                "notifies_piston_base":crate::redpiler::instant::sampling::data_notifies(world,source.source,pos)})
        })
        .collect();
    let reset_actor = reset_observers.iter().any(|&observer_pos| {
        matches!(world.get_block(observer_pos), Block::Observer { observer }
            if observer_pos.offset(observer.facing.into()) == pos)
    });
    json!({"pos":pos-BASE,"state":world.get_block(pos).properties(),
        "piston":{"facing":format!("{:?}",piston.piston.facing),
            "sticky":piston.piston.sticky,"extended":piston.piston.extended},"powered":piston.powered,
        "native_should_extend":crate::redstone::piston::should_piston_extend(world,piston.piston.facing,pos),
        "reset_seeds":piston.reset_seeds,"recognized_reset":recognition.is_matched(),
        "recognition_failures":recognition.failures,"input_sources":sources,
        "input_wires":recognition.inputs.wires.iter().map(|wire|*wire-BASE).collect::<Vec<_>>(),
        "update_ports":report.ports.pistons[actor].updates,
        "reset_observer_candidate":reset_actor,
        "payload_group":{"members":group.members,"positions":group.positions.iter().map(|p|*p-BASE).collect::<Vec<_>>()},
        "nearby_blocks":nearby_blocks,
        "sampling_error":crate::redpiler::instant::sampling::recognize(world,&report,
            &crate::redpiler::TaskMonitor::default(),None,&reset_observers).err()})
}

fn piston_summary(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Value {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                if let Block::Piston { piston } = world.get_block(BlockPos::new(x, y, z)) {
                    *counts
                        .entry(format!("{:?}:{}", piston.facing, piston.extended))
                        .or_default() += 1;
                }
            }
        }
    }
    json!(counts)
}

fn compact_trace(entries: &[trace::Entry]) -> Value {
    let mut samples = 0;
    let mut applied = 0;
    let mut first = Vec::new();
    for entry in entries {
        let event = match entry.operation {
            trace::Operation::Sample { pos, .. } => {
                samples += 1;
                json!({"kind":"sample","tick":entry.tick,"phase":entry.phase,"pos":pos-BASE})
            }
            trace::Operation::Applied(event) => {
                applied += 1;
                json!({"kind":"applied","tick":entry.tick,"phase":entry.phase,
                    "pos":event.pos-BASE,"action":event.action,"sticky":event.sticky})
            }
        };
        if first.len() < 40 {
            first.push(event);
        }
    }
    json!({"sample_count":samples,"applied_count":applied,"first_operations":first})
}

#[test]
#[ignore = "opt-in Potados PC counter protocol and compile matrix"]
fn potados_pc_counter_compile_and_native_protocol() {
    let output = std::path::PathBuf::from(std::env::var("MCHPRS_POTADOS_COUNTER_OUTPUT").unwrap());
    assert!(!output.exists(), "choose a new output path");
    let bytes = std::fs::read(root().join(FIXTURE)).unwrap();
    let schematic_hash = format!("{:x}", Sha256::digest(&bytes));
    let (initial, bounds) = load_counter();
    let initial_levers = lever_positions(&initial, bounds);
    assert_eq!(initial_levers.len(), 23);
    assert!(initial_levers
        .iter()
        .all(|&pos| matches!(initial.get_block(pos),
        Block::Lever { lever } if !lever.powered)));
    let mut compilation = Vec::new();
    for budget_multiplier in [1, 8] {
        for optimize in [false, true] {
            for assume_instant in [false, true] {
                let (mut world, bounds) = load_counter();
                turn_on_all_levers(&mut world, bounds);
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
                compilation.push(json!({"budget_multiplier":budget_multiplier,"optimize":optimize,
                    "assume_instant":assume_instant,"elapsed_seconds":start.elapsed().as_secs_f64(),
                    "result":result.map(|_| "compiled".to_owned()).unwrap_or_else(|e|e.to_string())}));
            }
        }
    }

    let (mut world, _) = load_counter();
    let idle = json!({"bus":bus(&world),"piston_summary":piston_summary(&world,bounds),
        "pending_ticks":world.scheduler().iter_entries().count(),"piston_events":world.piston_state().events.len(),
        "motions":world.piston_state().motions.len()});

    let all_levers_on = turn_on_all_levers(&mut world, bounds);
    let candidate_diagnostic = sampling_candidate_diagnostic(&world);

    let mut transitions = Vec::new();
    let mut last_bus = bus(&world);
    let candidate_pos = BASE + BlockPos::new(6, 15, 4);
    let mut last_candidate_pose = match world.get_block(candidate_pos) {
        Block::Piston { piston } => Some(piston.extended),
        _ => None,
    };
    let mut candidate_pose_changes = Vec::new();
    let mut total = json!({"sample_count":0u64,"applied_count":0u64});
    let mut first_timing = Vec::new();
    let run_started = Instant::now();
    for step in 1..=4096 {
        let entries = trace::capture(|| world.tick_interpreted());
        let mut sample_count = 0;
        let mut applied_count = 0;
        for entry in &entries {
            match entry.operation {
                trace::Operation::Sample { .. } => sample_count += 1,
                trace::Operation::Applied(_) => applied_count += 1,
            }
            if first_timing.len() < 200 {
                if let Some(event) = compact_trace(std::slice::from_ref(entry))["first_operations"]
                    .as_array()
                    .and_then(|a| a.first())
                {
                    first_timing.push(event.clone());
                }
            }
        }
        total["sample_count"] = json!(total["sample_count"].as_u64().unwrap() + sample_count);
        total["applied_count"] = json!(total["applied_count"].as_u64().unwrap() + applied_count);
        let current_bus = bus(&world);
        let current_candidate_pose = match world.get_block(candidate_pos) {
            Block::Piston { piston } => Some(piston.extended),
            _ => None,
        };
        if current_candidate_pose != last_candidate_pose {
            candidate_pose_changes
                .push(json!({"game_tick_after_activation":step,"pose":current_candidate_pose}));
            last_candidate_pose = current_candidate_pose;
        }
        if current_bus != last_bus {
            transitions.push(json!({"game_tick_after_activation":step,"bus":current_bus}));
            last_bus = current_bus;
        }
    }

    let tightly_spaced_seconds = run_started.elapsed().as_secs_f64();
    let (mut settled_world, settled_bounds) = load_counter();
    let settled_activation = turn_on_all_levers(&mut settled_world, settled_bounds);
    for _ in 1..24 {
        settled_world.tick_interpreted();
    }
    let bus_after_24_ticks = bus(&settled_world);
    let mut settled_bus = bus(&settled_world);
    let mut settled_transitions = Vec::new();
    let settled_started = Instant::now();
    for step in 1..=4096 {
        settled_world.tick_interpreted();
        let current = bus(&settled_world);
        if current != settled_bus {
            settled_transitions.push(json!({"game_tick_after_controls":step,"bus":current}));
            settled_bus = current;
        }
    }
    let settled_seconds = settled_started.elapsed().as_secs_f64();

    let report = json!({"fixture":FIXTURE,"sha256":schematic_hash,"dimensions":[24,19,75],
        "all_levers_on_at_tick_zero":all_levers_on,
        "candidate_diagnostic":candidate_diagnostic,
        "output_bus":{"physical_order":"z=3,7,...,63; repeater power at x=13,y=2","initial_saved_state":idle["bus"],
            "after_all_levers_on":all_levers_on["bus_after_activation_tick"],"transitions":transitions},
        "saved_state":idle,"compile_matrix":compilation,"run_ticks_after_activation":4096,
        "tick_zero_run_seconds":tightly_spaced_seconds,"run_operations_after_activation":total,
        "first_timing_operations":first_timing,"candidate_pose_changes":candidate_pose_changes,
        "settled_protocol":{"ticks_after_activation":24,"activation":settled_activation,
            "bus_after_24_ticks":bus_after_24_ticks,
            "run_ticks_after_activation":4096,"run_seconds":settled_seconds,
            "bus_transitions":settled_transitions,"final_bus":settled_bus,
            "pending_ticks":settled_world.scheduler().iter_entries().count(),
            "piston_events":settled_world.piston_state().events.len(),
            "motions":settled_world.piston_state().motions.len()},
        "final":{"bus":bus(&world),"piston_summary":piston_summary(&world,bounds),
            "pending_ticks":world.scheduler().iter_entries().count(),"piston_events":world.piston_state().events.len(),
            "motions":world.piston_state().motions.len()}});
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

#[test]
#[ignore = "4,096-tick native/compiled Potados counter comparison across eight compile options"]
fn potados_pc_counter_preserves_native_count_waveform() {
    let bytes = std::fs::read(root().join(FIXTURE)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7"
    );
    let (mut native, bounds) = load_counter();
    native.set_random_tick_speed(0);
    turn_on_all_levers(&mut native, bounds);
    let mut expected = Vec::new();
    let mut peaks = Vec::new();
    let mut peak = 0;
    for _ in 0..4096 {
        native.tick_interpreted();
        let value = !u16::from_str_radix(&bus(&native), 2).unwrap();
        if value == 0 && peak != 0 {
            peaks.push(peak);
            peak = 0;
        }
        peak = peak.max(value);
        expected.push(bus(&native));
    }
    if peak != 0 {
        peaks.push(peak);
    }
    assert_eq!(peaks, (1..=682).collect::<Vec<u16>>());
    assert_eq!(
        !u16::from_str_radix(expected.last().unwrap(), 2).unwrap(),
        682
    );
    let candidate = BASE + BlockPos::new(6, 15, 4);
    for budget_multiplier in [1, 8] {
        for optimize in [false, true] {
            for assume_instant in [false, true] {
                let (mut world, bounds) = load_counter();
                world.set_random_tick_speed(0);
                turn_on_all_levers(&mut world, bounds);
                let initial = block_state(&world, candidate, BASE);
                let mut compiler = Compiler::default();
                let start = Instant::now();
                compiler
                    .compile(
                        &world,
                        world.get_corners(),
                        CompilerOptions {
                            budget_multiplier,
                            optimize,
                            assume_instant,
                            ..Default::default()
                        },
                        world.scheduler().iter_entries().collect(),
                        Default::default(),
                    )
                    .unwrap();
                assert!(compiler.stats().unwrap().regions.static_pistons >= 1);
                world.native_scheduler().clear();
                for (tick, expected) in expected.iter().enumerate() {
                    compiler.tick_with_world(&mut world);
                    compiler.flush(&mut world);
                    assert_eq!(
                        &bus(&world),
                        expected,
                        "tick={} O={optimize} A={assume_instant} budget={budget_multiplier}",
                        tick + 1
                    );
                    assert_eq!(
                        block_state(&world, candidate, BASE),
                        initial,
                        "static piston tick={}",
                        tick + 1
                    );
                }
                let full_bounds = world.get_corners();
                compiler.reset(&mut world, full_bounds);
                assert_eq!(bus(&world), bus(&native));
                eprintln!("Potados: O={optimize} A={assume_instant} budget={budget_multiplier}; 4096 native-equivalent ticks in {:.2}s",start.elapsed().as_secs_f64());
            }
        }
    }
}
