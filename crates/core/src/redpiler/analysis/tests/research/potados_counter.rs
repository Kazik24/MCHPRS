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

fn repeater_states(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Vec<(BlockPos, bool, bool)> {
    (bounds.0.y..=bounds.1.y)
        .flat_map(|y| {
            (bounds.0.z..=bounds.1.z)
                .flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z)))
        })
        .filter_map(|pos| match world.get_block(pos) {
            Block::RedstoneRepeater { repeater } => {
                Some((pos - BASE, repeater.powered, repeater.locked))
            }
            _ => None,
        })
        .collect()
}

fn ordinary_sources(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Vec<(BlockPos, u8)> {
    (bounds.0.y..=bounds.1.y)
        .flat_map(|y| {
            (bounds.0.z..=bounds.1.z)
                .flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z)))
        })
        .filter_map(|pos| {
            let block = world.get_block(pos);
            matches!(
                block,
                Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
            )
            .then_some((pos, crate::redstone::source_strength(block, world, pos)))
        })
        .collect()
}

fn scheduler_delta(
    native: &[mchprs_world::TickEntry],
    direct: &[mchprs_world::TickEntry],
    native_world: &PlotWorld,
    direct_world: &PlotWorld,
) -> (Vec<Value>, Vec<Value>) {
    let mut unmatched_direct = direct.to_vec();
    let mut native_only = Vec::new();
    for tick in native {
        if let Some(index) = unmatched_direct.iter().position(|other| other == tick) {
            unmatched_direct.remove(index);
        } else {
            native_only.push(json!({
                "pos": tick.pos - BASE,
                "ticks_left": tick.ticks_left,
                "priority": tick.tick_priority,
                "block": native_world.get_block(tick.pos).get_name(),
                "properties": native_world.get_block(tick.pos).properties(),
                "watched_pos": tick.pos + BlockPos::new(0, -1, 0),
                "watched_block": native_world.get_block(tick.pos + BlockPos::new(0, -1, 0)).get_name(),
                "watched_properties": native_world.get_block(tick.pos + BlockPos::new(0, -1, 0)).properties(),
                "direct_watched_block": direct_world.get_block(tick.pos + BlockPos::new(0, -1, 0)).get_name(),
                "direct_watched_properties": direct_world.get_block(tick.pos + BlockPos::new(0, -1, 0)).properties(),
            }));
        }
    }
    let direct_only = unmatched_direct
        .iter()
        .map(|tick| {
            json!({
                "pos": tick.pos - BASE,
                "ticks_left": tick.ticks_left,
                "priority": tick.tick_priority,
                "block": direct_world.get_block(tick.pos).get_name(),
                "properties": direct_world.get_block(tick.pos).properties(),
            })
        })
        .collect();
    (native_only, direct_only)
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

fn compiled_state(compiler: &Compiler) -> (Vec<(BlockPos, bool)>, Vec<(BlockPos, bool)>) {
    let mut responses = compiler.backend.as_ref().unwrap().activation_states();
    responses.sort_by_key(|(pos, _)| (pos.y, pos.z, pos.x));
    let mut memory: Vec<_> = compiler
        .backend
        .as_ref()
        .unwrap()
        .logical_stats()
        .into_iter()
        .flat_map(|(_, _, memory)| memory)
        .collect();
    memory.sort_by_key(|(pos, _)| (pos.y, pos.z, pos.x));
    (responses, memory)
}

fn interpreted_state(world: &PlotWorld, cells: &[(BlockPos, bool)]) -> Vec<(BlockPos, bool)> {
    cells
        .iter()
        .map(|&(pos, _)| {
            let Block::Piston { piston } = world.get_block(pos) else {
                panic!("logical memory at {pos:?} is not a piston")
            };
            (pos, !piston.extended)
        })
        .collect()
}

#[test]
#[ignore = "12-tick terminal activation and compile-handoff diagnostic"]
fn pc_counter_terminal_activation_handoff() {
    use crate::redstone::instant_piston_tests::capture_at;

    let fixture = json!({
        "fixture": "test_data/piston-research/pc-counter-measurement-points-20261010/PC_COUNTER_MESURMENT_POINTS.schem",
        "sha256": "0749f1b8d5e52e13f62323d42aa194b5d1440f48fcc679aaf687eaa67d5e078b",
        "dimensions": [24, 19, 76], "loader_offset": [23, 1, 0]
    });
    let terminal = BASE + BlockPos::new(18, 9, 68);
    let (mut native, bounds) = load(&fixture);
    let (mut world, _) = load(&fixture);
    let Block::Piston { piston } = native.get_block(terminal) else {
        panic!("terminal piston missing from revised grid");
    };
    let watched = [terminal, terminal.offset(piston.facing.into())];
    let initialize = |world: &mut PlotWorld| {
        let levers = lever_positions(world, bounds);
        assert_eq!(levers.len(), 23);
        for pos in levers {
            assert!(matches!(world.get_block(pos), Block::Lever { lever } if !lever.powered));
            lever_action(world, pos, true);
        }
        world.tick_interpreted();
    };
    let mut callbacks = Vec::new();
    let samples = trace::capture(|| {
        callbacks = capture_at(&watched, || initialize(&mut native));
    });
    initialize(&mut world);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                optimize: false,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    world.clear_scheduled_ticks();
    let terminal_samples = |entries: Vec<trace::Entry>| {
        entries
            .into_iter()
            .filter(|entry| match entry.operation {
                trace::Operation::Sample { pos, .. } => pos == terminal,
                trace::Operation::Applied(event) => event.pos == terminal,
            })
            .collect::<Vec<_>>()
    };
    let geometry = |compiler: &Compiler| {
        let (actor, retracted, activation, memory, phase, block) = compiler
            .backend
            .as_ref()
            .unwrap()
            .geometry_state(terminal)
            .unwrap();
        json!({"actor":actor,"response_retracted":retracted,
            "activation":activation,"memory":memory,"phase":phase,
            "block":block.get_name(),"properties":block.properties(),
            "extended":matches!(block, Block::Piston { piston } if piston.extended)})
    };
    eprintln!(
        "{}",
        json!({"stage":"initialization","origin":BASE,
        "native":block_state(&native,terminal,BASE),"direct":geometry(&compiler),
        "samples":terminal_samples(samples),"callbacks":callbacks,
        "pending_events":native.piston_state().events})
    );

    let probes = [
        ("adder_out", BlockPos::new(0, 10, 62)),
        ("adder_stage", BlockPos::new(3, 11, 65)),
    ];
    let mut first_pose_difference = None;
    for tick in 1..=12 {
        let samples = trace::capture(|| {
            callbacks = capture_at(&watched, || native.tick_interpreted());
        });
        compiler.tick_with_world(&mut world);
        compiler.flush(&mut world);
        let direct = geometry(&compiler);
        let native_pose =
            matches!(native.get_block(terminal), Block::Piston { piston } if piston.extended);
        let direct_pose = direct["extended"] == true;
        if native_pose != direct_pose && first_pose_difference.is_none() {
            first_pose_difference = Some(tick);
        }
        let inputs: Vec<_> = probes
            .iter()
            .map(|(name, local)| {
                let pos = BASE + *local;
                let Block::RedstoneRepeater { repeater } = native.get_block(pos) else {
                    unreachable!()
                };
                let face = repeater.facing.block_face();
                let source = pos.offset(face);
                let block = native.get_block(source);
                let mut native_input =
                    crate::redstone::get_redstone_power(block, &native, source, face);
                if native_input == 0 {
                    if let Block::RedstoneWire { wire } = block {
                        native_input = wire.power;
                    }
                }
                let (_, powered, _, strengths, _) =
                    compiler.backend.as_ref().unwrap().node_state(pos).unwrap();
                let direct_input = strengths.iter().rposition(|&count| count != 0).unwrap_or(0);
                json!({"probe":name,"native_input":native_input,"direct_input":direct_input,
                "native_powered":repeater.powered,"direct_powered":powered})
            })
            .collect();
        assert_eq!(
            inputs[1]["native_input"], inputs[1]["direct_input"],
            "adder_stage control at tick {tick}"
        );
        assert_eq!(
            inputs[1]["native_powered"], inputs[1]["direct_powered"],
            "adder_stage control at tick {tick}"
        );
        let deliveries: Vec<_> = compiler
            .backend.as_mut().unwrap().take_activation_trace().into_iter()
            .filter(|delivery| watched.contains(&delivery.recipient))
            .map(|delivery| json!({"source":delivery.source - BASE,
                "recipient":delivery.recipient - BASE,"actor":delivery.actor,
                "direction":delivery.direction,"requires_extended":delivery.requires_extended}))
            .collect();
        eprintln!(
            "{}",
            json!({"stage":"replay","tick":tick,
            "native":block_state(&native,terminal,BASE),"direct":direct,
            "samples":terminal_samples(samples),"callbacks":callbacks,
            "direct_activations":deliveries,
            "pending_events":native.piston_state().events,"inputs":inputs})
        );
    }
    eprintln!("first terminal pose difference: {first_pose_difference:?}");
}

#[test]
#[ignore = "six-tick electrical route and publication diagnostic"]
fn pc_counter_memory_input_publication_route() {
    use crate::redstone::instant_piston_tests::capture_at;

    let target = BASE + BlockPos::new(15, 9, 64);
    let wire = BASE + BlockPos::new(15, 10, 65);
    let source = BASE + BlockPos::new(16, 10, 66);
    let gate = BASE + BlockPos::new(18, 10, 65);
    let payload = BASE + BlockPos::new(16, 10, 65);
    let mut watched = vec![target, wire, source, gate, payload];
    let snapshot = |world: &PlotWorld| {
        let Block::RedstoneRepeater { repeater } = world.get_block(target) else {
            panic!("expected memory repeater");
        };
        let face = repeater.facing.block_face();
        let input = target.offset(face);
        let blocks: Vec<_> = [target, wire, source, gate, payload]
            .iter()
            .map(|&pos| block_state(world, pos, BASE))
            .collect();
        json!({"blocks":blocks,
            "main_input":crate::redstone::get_redstone_power(world.get_block(input),world,input,face),
            "source_strength":crate::redstone::source_strength(world.get_block(source),world,source)})
    };
    let (mut native, bounds) = load_counter();
    let initialization = capture_at(&watched, || {
        turn_on_all_levers(&mut native, bounds);
    });
    let report = analyze_world(&native);
    let group = report
        .payload_groups
        .iter()
        .find(|group| group.positions.contains(&payload))
        .unwrap();
    let members: Vec<_> = group
        .members
        .iter()
        .map(|&actor| report.pistons[actor].pos)
        .collect();
    let response_actor = report
        .pistons
        .iter()
        .position(|piston| piston.pos == BASE + BlockPos::new(16, 12, 65))
        .unwrap();
    let response_sources = report.recognition[response_actor].inputs.sources.clone();
    let mut tracked_actors = vec![response_actor];
    let mut upstream_groups = Vec::new();
    let mut cursor = 0;
    while cursor < tracked_actors.len() {
        let actor = tracked_actors[cursor];
        cursor += 1;
        for source in &report.recognition[actor].inputs.sources {
            let crate::redpiler::analysis::topology::SourceKind::MobilePayload { group } =
                source.kind
            else {
                continue;
            };
            upstream_groups.push(group);
            for &member in &report.payload_groups[group].members {
                if !tracked_actors.contains(&member) {
                    tracked_actors.push(member);
                }
            }
        }
    }
    upstream_groups.sort_unstable();
    upstream_groups.dedup();
    tracked_actors.sort_unstable();
    tracked_actors.dedup();
    for &actor in &tracked_actors {
        watched.extend([report.pistons[actor].pos, report.pistons[actor].head]);
        watched.extend(report.ports.pistons[actor].updates.iter().map(|update| update.source));
    }
    watched.sort_by_key(|pos| (pos.y, pos.z, pos.x));
    watched.dedup();
    let (mut world, _) = load_counter();
    turn_on_all_levers(&mut world, bounds);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                optimize: false,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    world.clear_scheduled_ticks();
    eprintln!(
        "{}",
        json!({"payload_group":group,"actors":group.members.iter().map(|&actor| {
        json!({"actor":actor,"pos":report.pistons[actor].pos-BASE,
            "sources":report.recognition[actor].inputs.sources,"updates":report.ports.pistons[actor].updates})
    }).collect::<Vec<_>>()})
    );
    eprintln!(
        "{}",
        json!({"tick":0,"origin":BASE,"native":snapshot(&native),
        "direct_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(target)),
        "direct_wire_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(wire)),
        "upstream_groups":upstream_groups.iter().map(|&group| &report.payload_groups[group]).collect::<Vec<_>>(),
        "response_actor":response_actor,
        "native_response":crate::redstone::piston::should_piston_extend(&native,
            report.pistons[response_actor].piston.facing,report.pistons[response_actor].pos),
        "direct_response":format!("{:?}",compiler.backend.as_ref().unwrap().geometry_state(report.pistons[response_actor].pos)),
        "response_sources":response_sources.iter().map(|source| {
            let pos=source.source;
            let native_block=native.get_block(pos);
            let direct_block=world.get_block(pos);
            json!({"pos":pos-BASE,"kind":source.kind,
                "notifies":crate::redpiler::instant::sampling::data_notifies(&native,pos,report.pistons[response_actor].pos),
                "native_block":native_block.get_name(),"native_properties":native_block.properties(),
                "native_strength":crate::redstone::source_strength(native_block,&native,pos),
                "direct_block":direct_block.get_name(),"direct_properties":direct_block.properties(),
                "direct_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(pos))})
        }).collect::<Vec<_>>(),
        "tracked_actors":tracked_actors.iter().map(|&actor| {
            let pos=report.pistons[actor].pos;
            json!({"actor":actor,"pos":pos-BASE,
                "native_power":crate::redstone::piston::should_piston_extend(&native,
                    report.pistons[actor].piston.facing,pos),
                "native":block_state(&native,pos,BASE),
                "direct":format!("{:?}",compiler.backend.as_ref().unwrap().geometry_state(pos)),
                "sources":report.recognition[actor].inputs.sources.iter().map(|source| {
                    let source_pos=source.source;
                    let block=native.get_block(source_pos);
                    json!({"pos":source_pos-BASE,"kind":source.kind,
                        "notifies":crate::redpiler::instant::sampling::data_notifies(&native,source_pos,pos),
                        "native_block":block.get_name(),"native_properties":block.properties(),
                        "native_strength":crate::redstone::source_strength(block,&native,source_pos),
                        "direct_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(source_pos))})
                }).collect::<Vec<_>>(),
                "updates":report.ports.pistons[actor].updates})
        }).collect::<Vec<_>>(),
        "direct_port":compiler.backend.as_ref().unwrap().output_states(target),
        "initialization_callbacks":initialization})
    );

    // Recompute a separate native copy; preserve the paired replay's pending work.
    let (mut recomputed, _) = load_counter();
    turn_on_all_levers(&mut recomputed, bounds);
    let before = snapshot(&recomputed);
    assert_eq!(
        before,
        snapshot(&native),
        "recompute probe must start from the same activated state"
    );
    let callbacks = capture_at(&watched, || {
        crate::redstone::update(recomputed.get_block(wire), &mut recomputed, wire, None);
    });
    eprintln!(
        "{}",
        json!({"probe":"native_wire_recompute_at_tick_zero",
        "before":before,"after":snapshot(&recomputed),"callbacks":callbacks})
    );

    for tick in 1..=6 {
        let callbacks = capture_at(&watched, || native.tick_interpreted());
        compiler.tick_with_world(&mut world);
        compiler.flush(&mut world);
        let direct_activations: Vec<_> = compiler
            .backend
            .as_mut()
            .unwrap()
            .take_activation_trace()
            .into_iter()
            .filter(|delivery| tracked_actors.contains(&delivery.actor))
            .map(|delivery| {
                json!({"source":delivery.source-BASE,"actor":delivery.actor,
                    "recipient":delivery.recipient-BASE,
                    "requires_extended":delivery.requires_extended})
            })
            .collect();
        eprintln!(
            "{}",
            json!({"tick":tick,"native":snapshot(&native),
            "upstream_groups":upstream_groups.iter().map(|&group| &report.payload_groups[group]).collect::<Vec<_>>(),
            "native_response":crate::redstone::piston::should_piston_extend(&native,
                report.pistons[response_actor].piston.facing,report.pistons[response_actor].pos),
            "direct_response":format!("{:?}",compiler.backend.as_ref().unwrap().geometry_state(report.pistons[response_actor].pos)),
            "response_sources":response_sources.iter().map(|source| {
                let pos=source.source;
                let native_block=native.get_block(pos);
                let direct_block=world.get_block(pos);
                json!({"pos":pos-BASE,"kind":source.kind,
                    "native_block":native_block.get_name(),"native_properties":native_block.properties(),
                    "native_strength":crate::redstone::source_strength(native_block,&native,pos),
                    "direct_block":direct_block.get_name(),"direct_properties":direct_block.properties(),
                    "direct_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(pos))})
            }).collect::<Vec<_>>(),
            "tracked_actors":tracked_actors.iter().map(|&actor| {
                let pos=report.pistons[actor].pos;
                json!({"actor":actor,"pos":pos-BASE,
                    "native_power":crate::redstone::piston::should_piston_extend(&native,
                        report.pistons[actor].piston.facing,pos),
                    "native":block_state(&native,pos,BASE),
                    "direct":format!("{:?}",compiler.backend.as_ref().unwrap().geometry_state(pos)),
                    "sources":report.recognition[actor].inputs.sources.iter().map(|source| {
                        let source_pos=source.source;
                        let block=native.get_block(source_pos);
                        json!({"pos":source_pos-BASE,"kind":source.kind,
                            "native_block":block.get_name(),"native_properties":block.properties(),
                            "native_strength":crate::redstone::source_strength(block,&native,source_pos),
                            "direct_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(source_pos))})
                    }).collect::<Vec<_>>()})
            }).collect::<Vec<_>>(),
            "direct_wire":block_state(&world,wire,BASE),
            "direct_gate":format!("{:?}",compiler.backend.as_ref().unwrap().geometry_state(gate)),
            "direct_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(target)),
            "direct_wire_node":format!("{:?}",compiler.backend.as_ref().unwrap().node_state(wire)),
            "direct_activations":direct_activations,
            "direct_port":compiler.backend.as_ref().unwrap().output_states(target),
            "members":members.iter().map(|&pos| json!({"native":block_state(&native,pos,BASE),
                "direct":format!("{:?}",compiler.backend.as_ref().unwrap().geometry_state(pos))})).collect::<Vec<_>>(),
            "callbacks":callbacks})
        );
    }
}

#[test]
#[ignore = "4,096-tick interpreted/compiled Potados counter equivalence"]
fn potados_pc_counter_counts_up_with_and_without_optimization() {
    use crate::redstone::instant_piston_tests::capture_at;

    let bytes = std::fs::read(root().join(FIXTURE)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7"
    );
    let (mut native, bounds) = load_counter();
    turn_on_all_levers(&mut native, bounds);
    let (mut world, _) = load_counter();
    turn_on_all_levers(&mut world, bounds);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                optimize: false,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    let diagnostic = crate::redpiler::analysis::analyze(
        &world,
        bounds,
        &world.scheduler().iter_entries().collect::<Vec<_>>(),
        &Default::default(),
        Default::default(),
    )
    .unwrap();
    let guard = BASE + BlockPos::new(12, 4, 63);
    if let Some(actor) = diagnostic.pistons.iter().position(|piston| piston.pos == guard) {
        eprintln!(
            "guard actor={actor} recognition={:?} updates={:?} source_notifications={:?} compiled={:?}",
            diagnostic.recognition[actor].inputs.sources,
            diagnostic.ports.pistons[actor].updates,
            diagnostic.recognition[actor].inputs.sources.iter().map(|source| (
                source.source,
                crate::redpiler::instant::sampling::data_notifies(&world, source.source, guard),
                world.get_block(source.source).properties(),
            )).collect::<Vec<_>>(),
            compiler.backend.as_ref().unwrap().geometry_state(guard),
        );
    }
    world.clear_scheduled_ticks();
    let (initial_responses, initial_memory) = compiled_state(&compiler);
    assert!(
        !initial_responses.is_empty(),
        "counter should have logical pistons"
    );
    assert_eq!(interpreted_state(&native, &initial_memory), initial_memory);
    let mut expected = Vec::new();
    let mut expected_repeaters = Vec::new();
    let mut compiled_repeaters = Vec::new();
    let mut repeater_divergence_reported = false;
    let mut scheduler_divergence_reported = false;
    let mut expected_compiled_state = Vec::new();
    let mut peaks = Vec::new();
    let mut peak = 0;
    let mut initial_native_ticks: Vec<_> = native.scheduler().iter_entries().collect();
    let mut initial_direct_ticks = compiler.backend.as_ref().unwrap().scheduled_ticks();
    let tick_key = |tick: &mchprs_world::TickEntry| {
        (
            tick.pos.y,
            tick.pos.z,
            tick.pos.x,
            tick.ticks_left,
            tick.tick_priority,
            tick.block_type,
        )
    };
    initial_native_ticks.sort_by_key(tick_key);
    initial_direct_ticks.sort_by_key(tick_key);
    let (native_only, direct_only) = scheduler_delta(
        &initial_native_ticks,
        &initial_direct_ticks,
        &native,
        &world,
    );
    if !native_only.is_empty() || !direct_only.is_empty() {
        eprintln!(
            "compile-start scheduler native-only={native_only:?} direct-only={direct_only:?}"
        );
    }
    for tick in 0..4096 {
        let mut guard_callbacks = Vec::new();
        let guard_head = guard + BlockPos::new(0, -1, 0);
        let native_trace = trace::capture(|| {
            guard_callbacks = capture_at(&[guard, guard_head], || native.tick_interpreted());
        });
        compiler.tick_with_world(&mut world);
        compiler.flush(&mut world);
        let current_bus = bus(&native);
        let compiled_bus = bus(&world);
        let current_repeaters = repeater_states(&native, bounds);
        let actual_repeaters = repeater_states(&world, bounds);
        let deliveries = compiler.backend.as_mut().unwrap().take_activation_trace();
        let mut native_ticks: Vec<_> = native.scheduler().iter_entries().collect();
        let mut direct_ticks = compiler.backend.as_ref().unwrap().scheduled_ticks();
        let sort_ticks = |ticks: &mut Vec<mchprs_world::TickEntry>| {
            ticks.sort_by_key(|tick| {
                (
                    tick.pos.y,
                    tick.pos.z,
                    tick.pos.x,
                    tick.ticks_left,
                    tick.tick_priority,
                    tick.block_type,
                )
            });
        };
        sort_ticks(&mut native_ticks);
        sort_ticks(&mut direct_ticks);
        if tick < 8 {
            let pos = BASE + BlockPos::new(15, 9, 4);
            let Block::RedstoneRepeater { repeater } = native.get_block(pos) else {
                unreachable!()
            };
            let input_pos = pos.offset(repeater.facing.block_face());
            let input = native.get_block(input_pos);
            let mut input_power = crate::redstone::get_redstone_power(
                input, &native, input_pos, repeater.facing.block_face(),
            );
            if input_power == 0 {
                if let Block::RedstoneWire { wire } = input {
                    input_power = wire.power;
                }
            }
            eprintln!(
                "boundary tick={} native_repeater={:?} native_input_pos={:?} native_input={} {:?} power={} direct_node={:?} port={:?}",
                tick + 1, repeater, input_pos - BASE, input.get_name(), input.properties(), input_power,
                compiler.backend.as_ref().unwrap().node_state(pos),
                compiler.backend.as_ref().unwrap().output_states(pos),
            );
            let watch = BASE + BlockPos::new(15, 9, 64);
            let Block::RedstoneRepeater { repeater: watch_repeater } = native.get_block(watch) else { unreachable!() };
            let watch_face = watch_repeater.facing.block_face();
            let watch_input_pos = watch.offset(watch_face);
            let watch_input = native.get_block(watch_input_pos);
            let watch_power = crate::redstone::get_redstone_power(watch_input, &native, watch_input_pos, watch_face);
            eprintln!("watch tick={} native={:?} input_pos={:?} input={} {:?} power={} direct={:?} port={:?} native_ticks={:?} direct_ticks={:?}",
                tick + 1, watch_repeater, watch_input_pos - BASE, watch_input.get_name(), watch_input.properties(), watch_power,
                compiler.backend.as_ref().unwrap().node_state(watch),
                compiler.backend.as_ref().unwrap().output_states(watch),
                native.scheduler().iter_entries().filter(|entry| entry.pos == watch).map(|entry| (entry.ticks_left, entry.tick_priority)).collect::<Vec<_>>(),
                compiler.backend.as_ref().unwrap().scheduled_ticks().into_iter().filter(|entry| entry.pos == watch).map(|entry| (entry.ticks_left, entry.tick_priority)).collect::<Vec<_>>());
            let watch_source = BASE + BlockPos::new(16, 10, 66);
            let native_source = native.get_block(watch_source);
            eprintln!("watch_source tick={} pos={:?} native={} {:?} strength={} direct={} {:?} node={:?}", tick + 1,
                watch_source - BASE, native_source.get_name(), native_source.properties(),
                crate::redstone::source_strength(native_source, &native, watch_source),
                world.get_block(watch_source).get_name(), world.get_block(watch_source).properties(),
                compiler.backend.as_ref().unwrap().node_state(watch_source));
            if tick == 0 {
                let nearby: Vec<_> = [watch_input_pos, watch_source]
                    .into_iter()
                    .flat_map(|center| [
                        BlockPos::new(1, 0, 0), BlockPos::new(-1, 0, 0),
                        BlockPos::new(0, 1, 0), BlockPos::new(0, -1, 0),
                        BlockPos::new(0, 0, 1), BlockPos::new(0, 0, -1),
                    ].into_iter().map(move |offset| center + offset))
                    .collect();
                let snapshot: Vec<_> = nearby.iter().copied()
                    .map(|pos| (pos - BASE, native.get_block(pos).get_name(), native.get_block(pos).properties()))
                    .collect();
                eprintln!("watch_output_port={:?} snapshot={snapshot:?}",
                    diagnostic.ports.outputs.iter().find(|output| output.consumer == watch));
                eprintln!("direct_watch_nearby={:?}", nearby.iter().map(|&pos| {
                    let block = world.get_block(pos);
                    (pos - BASE, block.get_name(), block.properties(),
                        crate::redstone::get_redstone_power(block, &world, pos, watch_face),
                        compiler.backend.as_ref().unwrap().node_state(pos),
                        compiler.backend.as_ref().unwrap().output_states(pos))
                }).collect::<Vec<_>>());
            }
            let gate = BASE + BlockPos::new(18, 10, 5);
            let Block::Piston { piston } = native.get_block(gate) else { unreachable!() };
            eprintln!("native_should_extend={}", crate::redstone::piston::should_piston_extend(&native, piston.facing, gate));
            let gate_blocks: Vec<_> = [
                BlockPos::new(18, 10, 5),
                BlockPos::new(17, 10, 5),
                BlockPos::new(16, 10, 5),
                BlockPos::new(15, 10, 5),
                BlockPos::new(16, 10, 6),
                BlockPos::new(21, 10, 5),
                BlockPos::new(18, 9, 7),
                BlockPos::new(18, 9, 6),
                BlockPos::new(18, 9, 5),
                BlockPos::new(18, 9, 67),
                BlockPos::new(18, 10, 68),
                BlockPos::new(17, 12, 67),
            ].into_iter().map(|local| {
                let block = native.get_block(BASE + local);
                (local, block.get_name(), block.properties())
            }).collect();
            eprintln!("gate tick={} native={:?} compiled={:?}", tick + 1, gate_blocks,
                compiler.backend.as_ref().unwrap().geometry_state(gate));
            eprintln!("lower gate tick={} compiled={:?} source={:?}", tick + 1,
                compiler.backend.as_ref().unwrap().geometry_state(BASE + BlockPos::new(18, 9, 7)),
                compiler.backend.as_ref().unwrap().node_state(BASE + BlockPos::new(21, 10, 5)));
            let terminal = BASE + BlockPos::new(18, 9, 67);
            let terminal_block = native.get_block(terminal);
            eprintln!("terminal tick={} native_desired={:?} compiled={:?} sources={:?}", tick + 1,
                match terminal_block { Block::Piston { piston } => Some(crate::redstone::piston::should_piston_extend(&native, piston.facing, terminal)), _ => None },
                compiler.backend.as_ref().unwrap().geometry_state(terminal),
                [BlockPos::new(18, 10, 68), BlockPos::new(17, 12, 67)].map(|local| {
                    let source = BASE + local;
                    (local, crate::redstone::source_strength(native.get_block(source), &native, source), compiler.backend.as_ref().unwrap().node_state(source))
                }));
        }
        if native_ticks != direct_ticks && !scheduler_divergence_reported {
            scheduler_divergence_reported = true;
            let (native_only, direct_only) =
                scheduler_delta(&native_ticks, &direct_ticks, &native, &world);
            let native_sources: std::collections::HashMap<_, _> = ordinary_sources(&native, bounds)
                .into_iter()
                .map(|(pos, strength)| (pos, strength))
                .collect();
            let source_differences: Vec<_> = compiler
                .backend
                .as_ref()
                .unwrap()
                .ordinary_sources()
                .into_iter()
                .filter_map(|(pos, strength)| {
                    (native_sources.get(&pos) != Some(&strength)).then_some((
                        pos - BASE,
                        native_sources.get(&pos).copied(),
                        strength,
                    ))
                })
                .take(20)
                .collect();
            let wire_differences: Vec<_> = (0..16)
                .filter_map(|bit| {
                    let pos = BASE + BlockPos::new(14, 6, 3 + bit * 4);
                    match (native.get_block(pos), world.get_block(pos)) {
                        (
                            Block::RedstoneWire { wire: expected },
                            Block::RedstoneWire { wire: actual },
                        ) if expected.power != actual.power => {
                            Some((pos - BASE, expected.power, actual.power))
                        }
                        _ => None,
                    }
                })
                .collect();
            let applied_events: Vec<_> = native_trace
                .iter()
                .filter_map(|entry| match &entry.operation {
                    trace::Operation::Applied(event) => Some(format!(
                        "tick={} phase={:?} event={event:?}",
                        entry.tick, entry.phase
                    )),
                    _ => None,
                })
                .take(8)
                .collect();
            let watched_base = BASE + BlockPos::new(16, 7, 65);
            let direct_base_activation = compiler
                .backend
                .as_ref()
                .unwrap()
                .activation_states()
                .into_iter()
                .find(|(pos, _)| *pos == watched_base);
            let direct_geometry = compiler
                .backend
                .as_ref()
                .unwrap()
                .geometry_state(watched_base);
            let direct_observer = compiler
                .backend
                .as_ref()
                .unwrap()
                .observer_state(BASE + BlockPos::new(16, 8, 5));
            let current_responses = compiled_state(&compiler).0;
            let response_changes: Vec<_> = initial_responses
                .iter()
                .filter_map(|&(pos, initial)| {
                    let current =
                        current_responses
                            .iter()
                            .find_map(|&(current_pos, current)| {
                                (current_pos == pos).then_some(current)
                            })?;
                    (initial != current).then_some((
                        pos - BASE,
                        initial,
                        current,
                        native.get_block(pos).get_name(),
                        native.get_block(pos).properties(),
                    ))
                })
                .collect();
            eprintln!(
                "first scheduler divergence tick={} native-only={:?} direct-only={:?} source_differences={:?} activation_wire_differences={:?} response_changes={:?} native_trace_len={} native_applied={:?} direct_activations={:?} watched_base_activation={:?} direct_geometry={:?} direct_observer={:?} logical_stats={:?}",
                tick + 1,
                native_only.into_iter().take(8).collect::<Vec<_>>(),
                direct_only.into_iter().take(8).collect::<Vec<_>>(),
                source_differences,
                wire_differences,
                response_changes,
                native_trace.len(),
                applied_events,
                deliveries.iter().take(16).map(|event| (event.source - BASE, event.actor, event.recipient - BASE, event.direction)).collect::<Vec<_>>(),
                direct_base_activation,
                direct_geometry,
                direct_observer,
                compiler.backend.as_ref().unwrap().logical_stats(),
            );
        }
        if actual_repeaters != current_repeaters && !repeater_divergence_reported {
            repeater_divergence_reported = true;
            let differences: Vec<_> = actual_repeaters
                .iter()
                .zip(&current_repeaters)
                .filter(|(actual, expected)| actual != expected)
                .take(8)
                .map(|(actual, expected)| (actual, expected))
                .collect();
            let native_events: Vec<_> = native_trace
                .iter()
                .take(20)
                .map(|entry| (entry.phase, format!("{:?}", entry.operation)))
                .take(20)
                .collect();
            let pos = BASE + differences[0].0 .0;
            let direct_node = compiler.backend.as_ref().unwrap().node_state(pos);
            let direct_incoming = compiler.backend.as_ref().unwrap().incoming_states(pos);
            let direct_outputs = compiler.backend.as_ref().unwrap().output_states(pos);
            let neighbors = [
                BlockPos::new(1, 0, 0),
                BlockPos::new(-1, 0, 0),
                BlockPos::new(0, 1, 0),
                BlockPos::new(0, -1, 0),
                BlockPos::new(0, 0, 1),
                BlockPos::new(0, 0, -1),
            ];
            let around: Vec<_> = neighbors
                .into_iter()
                .map(|offset| {
                    let neighbor = pos + offset;
                    (
                        neighbor - BASE,
                        native.get_block(neighbor).get_name(),
                        native.get_block(neighbor).properties(),
                        world.get_block(neighbor).get_name(),
                        world.get_block(neighbor).properties(),
                    )
                })
                .collect();
            let nearby_ticks = |ticks: &[mchprs_world::TickEntry]| {
                ticks
                    .iter()
                    .filter(|tick| {
                        tick.pos == pos || neighbors.iter().any(|offset| tick.pos == pos + *offset)
                    })
                    .map(|tick| (tick.pos - BASE, tick.ticks_left, tick.tick_priority))
                    .collect::<Vec<_>>()
            };
            eprintln!(
                "first repeater divergence tick={} differences={:?} node_state={:?} incoming={:?} output_terms={:?} around={:?} pending native={:?} direct={:?} piston trace={:?} instant callbacks={:?} guard_native={:?} guard_direct={:?} guard_activations={:?} guard_callbacks={:?}",
                tick + 1,
                differences,
                direct_node,
                direct_incoming,
                direct_outputs,
                around,
                nearby_ticks(&native_ticks),
                nearby_ticks(&direct_ticks),
                native_events,
                deliveries.iter().take(20).map(|event| (event.source - BASE, event.actor, event.recipient - BASE, event.direction)).collect::<Vec<_>>(),
                native_trace.iter().filter(|entry| match entry.operation {
                    trace::Operation::Sample { pos, .. } => pos == guard,
                    trace::Operation::Applied(event) => event.pos == guard,
                }).map(|entry| (entry.phase, format!("{:?}", entry.operation))).collect::<Vec<_>>(),
                compiler.backend.as_ref().unwrap().geometry_state(guard),
                deliveries.iter().filter(|event| event.actor == 46).map(|event| (event.source - BASE, event.recipient - BASE, event.direction)).collect::<Vec<_>>(),
                guard_callbacks,
            );
        }
        let current_memory = interpreted_state(&native, &initial_memory);
        assert_eq!(compiled_bus, current_bus, "unoptimized tick={}", tick + 1);
        assert_eq!(
            compiled_state(&compiler).1,
            current_memory,
            "unoptimized memory tick={}",
            tick + 1
        );
        let value = !u16::from_str_radix(&current_bus, 2).unwrap();
        if value == 0 && peak != 0 {
            peaks.push(peak);
            peak = 0;
        }
        peak = peak.max(value);
        expected.push(current_bus);
        expected_repeaters.push(current_repeaters);
        compiled_repeaters.push(actual_repeaters);
        expected_compiled_state.push(compiled_state(&compiler));
    }
    if peak != 0 {
        peaks.push(peak);
    }
    assert_eq!(peaks, (1..=682).collect::<Vec<u16>>());
    assert_eq!(
        !u16::from_str_radix(expected.last().unwrap(), 2).unwrap(),
        682
    );
    let (mut world, bounds) = load_counter();
    turn_on_all_levers(&mut world, bounds);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                optimize: true,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    world.clear_scheduled_ticks();
    assert_eq!(
        compiled_state(&compiler),
        (initial_responses, initial_memory)
    );
    for (tick, ((expected_bus, expected_repeater_state), expected_state)) in expected
        .iter()
        .zip(&expected_repeaters)
        .zip(&expected_compiled_state)
        .enumerate()
    {
        compiler.tick_with_world(&mut world);
        compiler.flush(&mut world);
        assert_eq!(bus(&world), *expected_bus, "optimized tick={}", tick + 1);
        assert_eq!(
            repeater_states(&world, bounds),
            *expected_repeater_state,
            "optimized repeater state tick={}",
            tick + 1
        );
        assert_eq!(
            compiled_state(&compiler),
            *expected_state,
            "optimized tick={}",
            tick + 1
        );
    }
}
