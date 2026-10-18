//! Fixture-derived input frames and native accepted-action census.
use super::*;
use crate::redstone::piston::trace::{self, Entry as NativeEntry, Operation as NativeOperation};
use mchprs_world::{AdvancePhase, PistonAction};
use std::collections::{BTreeMap, BTreeSet};

/// A physical input change keeps fixture provenance and the executed setter order.
#[derive(Clone, Debug)]
struct Setter {
    port: String,           // The manifest-named input changed by this physical lever action.
    position: BlockPos,     // The physical lever selected by the fixture.
    old_powered: bool,      // The lever level immediately before this setter.
    new_powered: bool,      // The lever level requested by this setter.
    source_action: usize,   // The zero-based action index in the pinned fixture case.
    execution_order: usize, // This setter's position among sequential data changes.
}

/// One fixture-derived frame preserves old data, preparation order, and activation.
#[derive(Clone, Debug)]
struct InputFrame {
    case_id: String,                   // The pinned arithmetic history sampled by this frame.
    old_data: Vec<(String, bool)>,     // Imported data-port levels before preparation.
    sampled_data: Vec<(String, bool)>, // Stable physical levels immediately before activation.
    preparation: Vec<Setter>,          // Sequential data setters; the test never treats them as atomic.
    activation: Setter,                // The distinct fixture event qualifying the sample.
    ready_ticks: usize,                // Real fixture wait ticks between data setters and activation.
    response_ticks: usize,             // Bounded post-activation observation ticks from the manifest.
}

/// Accepted piston events retain their original FIFO order within a native drain.
#[derive(Clone, Debug)]
struct ActionDrain {
    tick: u64,                // Interpreter logical tick recorded when the action was accepted.
    phase: AdvancePhase,      // Native phase of this FIFO drain, expected to be PistonEvents.
    events: Vec<PistonEvent>, // Accepted actions in their exact native order.
}

/// Native observations and accepted actions for one qualifying activation response.
#[derive(Clone, Debug, Default)]
struct NativeCensus {
    samples: usize,           // Piston samples during the response, including unchanged samples.
    drains: Vec<ActionDrain>, // Distinct actual tick/phase drains with at least one accepted action.
}

/// Accepted action kinds stay separate; no kind is assumed to mean compute or reset.
#[derive(Clone, Copy, Debug, Default)]
struct ActionCounts {
    extends: usize,     // Accepted Extend actions.
    retracts: usize,    // Accepted pull-capable Retract actions.
    early_drops: usize, // Accepted RetractWithoutPull actions.
}

/// Aggregates raw census records for a fixture without discarding actor identities.
#[derive(Debug, Default)]
struct CensusTotals {
    samples: usize,                                    // All native samples across the selected histories.
    accepted: ActionCounts,                            // Accepted actions by native action kind.
    drains: usize,                                     // Actual PistonEvents drains containing accepted actions.
    combined_drains: usize,                            // Per-actor drains containing both Extend and retraction.
    by_actor: BTreeMap<(i32, i32, i32), ActionCounts>, // Accepted action counts keyed by fixture actor position.
}

const DATA_ORDERS: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];

fn read_lever(world: &PlotWorld, position: BlockPos) -> bool {
    let Block::Lever { lever } = world.get_block(position) else {
        panic!("expected fixture lever at {position:?}");
    };
    lever.powered
}

fn position_key(position: BlockPos) -> (i32, i32, i32) {
    (position.x, position.y, position.z)
}

fn input_name(circuit: &Adder, position: BlockPos) -> String {
    let inputs = circuit.ports["inputs"].as_object().expect("fixture input port object");
    let matches: Vec<_> = inputs
        .keys()
        .filter(|name| name.as_str() != "trigger")
        .filter_map(|name| {
            let positions = circuit.port_positions("inputs", name);
            positions.iter().position(|&candidate| candidate == position).map(|index| {
                if positions.len() == 1 {
                    name.to_owned()
                } else {
                    format!("{name}[{index}]")
                }
            })
        })
        .collect();
    assert_eq!(matches.len(), 1, "fixture data lever {position:?} must map to one named input");
    matches.into_iter().next().unwrap()
}

fn frame_for_order(circuit: &Adder, case: &Value, order: &[usize]) -> InputFrame {
    let initial = circuit.build();
    let mut data = Vec::<(usize, String, BlockPos, bool)>::new();
    let mut activation = None;
    let mut ready_ticks = 0;
    for (source_action, action) in case["actions"].as_array().unwrap().iter().enumerate() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let Step::SetLever { position, powered } = step_for_action(circuit, action) else {
                    unreachable!()
                };
                if position == circuit.trigger() {
                    activation = Some((source_action, position, read_lever(&initial, position), powered));
                } else {
                    data.push((source_action, input_name(circuit, position), position, powered));
                }
            }
            "wait" | "wait_ready" => ready_ticks += action["ticks"].as_u64().unwrap() as usize,
            operation => panic!("unexpected adder input operation {operation}"),
        }
    }
    assert_eq!(order.len(), data.len(), "one order entry is required for every physical data setter");
    let mut sorted_order = order.to_vec();
    sorted_order.sort_unstable();
    assert_eq!(sorted_order, (0..data.len()).collect::<Vec<_>>(), "data setter order is a permutation");
    assert_eq!(
        data.iter().map(|(_, name, _, _)| name).collect::<BTreeSet<_>>().len(),
        data.len(),
        "current frames set each named input once"
    );
    assert_eq!(
        data.iter()
            .map(|(_, _, position, _)| position_key(*position))
            .collect::<BTreeSet<_>>()
            .len(),
        data.len(),
        "current frames use distinct data levers"
    );

    let old_data: BTreeMap<_, _> = data
        .iter()
        .map(|(_, port, position, _)| (port.clone(), read_lever(&initial, *position)))
        .collect();
    let mut sampled_data = old_data.clone();
    let preparation = order
        .iter()
        .enumerate()
        .map(|(execution_order, &source_index)| {
            let (source_action, port, position, new_powered) = &data[source_index];
            let old_powered = *old_data.get(port).expect("old level for named input");
            sampled_data.insert(port.clone(), *new_powered);
            Setter {
                port: port.clone(),
                position: *position,
                old_powered,
                new_powered: *new_powered,
                source_action: *source_action,
                execution_order,
            }
        })
        .collect();
    let (source_action, position, old_powered, new_powered) = activation.expect("fixture activation edge");
    let old_data = old_data.into_iter().collect();
    let sampled_data = sampled_data.into_iter().collect();
    InputFrame {
        case_id: case["id"].as_str().unwrap().to_owned(),
        old_data,
        sampled_data,
        preparation,
        activation: Setter {
            port: "trigger".to_owned(),
            position,
            old_powered,
            new_powered,
            source_action,
            execution_order: order.len(),
        },
        ready_ticks,
        response_ticks: case["ticks"].as_u64().unwrap() as usize,
    }
}

fn fixture_data_order(circuit: &Adder, case: &Value) -> Vec<usize> {
    let count = case["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| action["op"] == "lever" && circuit.place(local(&action["pos"])) != circuit.trigger())
        .count();
    (0..count).collect()
}

fn data_levels(circuit: &Adder, world: &PlotWorld) -> Vec<(String, bool)> {
    let mut levels = BTreeMap::new();
    for name in circuit.ports["inputs"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| name.as_str() != "trigger")
    {
        let positions = circuit.port_positions("inputs", name);
        for (index, position) in positions.iter().enumerate() {
            let port = if positions.len() == 1 {
                name.to_owned()
            } else {
                format!("{name}[{index}]")
            };
            levels.insert(port, read_lever(world, *position));
        }
    }
    levels.into_iter().collect()
}

fn record_trace(census: &mut NativeCensus, entries: Vec<NativeEntry>) {
    for entry in entries {
        match entry.operation {
            NativeOperation::Sample { .. } => census.samples += 1,
            NativeOperation::Applied(event) => {
                assert_eq!(
                    entry.phase,
                    AdvancePhase::PistonEvents,
                    "accepted events belong to the native action drain"
                );
                let index = census
                    .drains
                    .iter()
                    .position(|drain| drain.tick == entry.tick && drain.phase == entry.phase);
                if let Some(index) = index {
                    census.drains[index].events.push(event);
                } else {
                    census.drains.push(ActionDrain {
                        tick: entry.tick,
                        phase: entry.phase,
                        events: vec![event],
                    });
                }
            }
        }
    }
}

fn trace_step(world: &mut PlotWorld, step: Step) -> Vec<NativeEntry> {
    trace::capture(|| native_step(world, step))
}

fn record_action(counts: &mut ActionCounts, action: PistonAction) {
    match action {
        PistonAction::Extend => counts.extends += 1,
        PistonAction::Retract => counts.retracts += 1,
        PistonAction::RetractWithoutPull => counts.early_drops += 1,
    }
}

fn retractions(counts: ActionCounts) -> usize {
    counts.retracts + counts.early_drops
}

fn run_frame(circuit: &Adder, frame: &InputFrame, sum: u16, carry: u16) -> NativeCensus {
    let mut world = circuit.build();
    assert_eq!(data_levels(circuit, &world), frame.old_data, "{} imported data", frame.case_id);
    let mut source_actions = BTreeSet::new();
    for (execution_order, setter) in frame.preparation.iter().enumerate() {
        assert_eq!(setter.execution_order, execution_order, "{} data setter order", frame.case_id);
        assert!(
            source_actions.insert(setter.source_action),
            "{} source setter provenance is unique",
            frame.case_id
        );
        assert_eq!(
            read_lever(&world, setter.position),
            setter.old_powered,
            "{} {} prior level",
            frame.case_id,
            setter.port
        );
        native_step(
            &mut world,
            Step::SetLever {
                position: setter.position,
                powered: setter.new_powered,
            },
        );
    }
    assert_eq!(data_levels(circuit, &world), frame.sampled_data, "{} prepared data", frame.case_id);
    assert_eq!(frame.activation.port, "trigger");
    assert_eq!(frame.activation.execution_order, frame.preparation.len());
    assert!(
        source_actions.insert(frame.activation.source_action),
        "{} activation has distinct fixture provenance",
        frame.case_id
    );
    for _ in 0..frame.ready_ticks {
        native_step(&mut world, Step::Tick);
        assert_eq!(
            data_levels(circuit, &world),
            frame.sampled_data,
            "{} stable data through readiness",
            frame.case_id
        );
    }
    assert_eq!(read_lever(&world, frame.activation.position), frame.activation.old_powered);
    let mut wave = NativeCensus::default();
    record_trace(
        &mut wave,
        trace_step(
            &mut world,
            Step::SetLever {
                position: frame.activation.position,
                powered: frame.activation.new_powered,
            },
        ),
    );
    for tick in 1..=frame.response_ticks {
        record_trace(&mut wave, trace_step(&mut world, Step::Tick));
        assert_eq!(
            data_levels(circuit, &world),
            frame.sampled_data,
            "{} stable data through response",
            frame.case_id
        );
        assert_math_window(circuit, &world, sum, carry, tick);
    }
    for drain in &wave.drains {
        let mut by_actor = BTreeMap::<_, ActionCounts>::new();
        for event in &drain.events {
            record_action(by_actor.entry(position_key(event.pos)).or_default(), event.action);
        }
        for (actor, actions) in by_actor {
            assert!(
                actions.retracts <= 1,
                "{} actor {actor:?} accepted multiple retractions in one {:?} drain at tick {}: {actions:?}",
                frame.case_id,
                drain.phase,
                drain.tick
            );
        }
    }
    wave
}

fn totals(censuses: &[NativeCensus]) -> CensusTotals {
    let mut totals = CensusTotals::default();
    for census in censuses {
        totals.samples += census.samples;
        for drain in &census.drains {
            totals.drains += 1;
            let mut by_actor = BTreeMap::<_, ActionCounts>::new();
            for event in &drain.events {
                record_action(&mut totals.accepted, event.action);
                record_action(&mut by_actor.entry(position_key(event.pos)).or_default(), event.action);
            }
            for (actor, actions) in by_actor {
                let actor_total = totals.by_actor.entry(actor).or_default();
                actor_total.extends += actions.extends;
                actor_total.retracts += actions.retracts;
                actor_total.early_drops += actions.early_drops;
                if actions.extends > 0 && retractions(actions) > 0 {
                    totals.combined_drains += 1;
                }
            }
        }
    }
    totals
}

pub(super) fn arithmetic_cases(id: &str) -> Vec<Value> {
    let path = root().join(format!("test_data/instant-pistons-io/fixtures/{id}.json"));
    let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
        .cloned()
        .collect()
}

fn raw_drains(census: &NativeCensus, count: usize) -> Vec<(u64, AdvancePhase, &Vec<PistonEvent>)> {
    census
        .drains
        .iter()
        .take(count)
        .map(|drain| (drain.tick, drain.phase, &drain.events))
        .collect()
}

fn port_name_at(circuit: &Adder, group: &str, position: BlockPos) -> Vec<String> {
    circuit.ports[group]
        .as_object()
        .expect("declared port group")
        .keys()
        .filter_map(|name| {
            let positions = circuit.port_positions(group, name);
            positions.iter().position(|&candidate| candidate == position).map(|index| {
                if positions.len() == 1 {
                    name.to_owned()
                } else {
                    format!("{name}[{index}]")
                }
            })
        })
        .collect()
}

fn coordinate_count(value: &Value) -> usize {
    if value.as_array().is_some_and(|items| items.len() == 3 && items[0].is_number()) {
        1
    } else if let Some(items) = value.as_array() {
        items.iter().map(coordinate_count).sum()
    } else {
        0
    }
}

fn port_group_counts(group: &Value) -> Vec<(String, usize)> {
    group
        .as_object()
        .expect("declared logical port group")
        .iter()
        .map(|(name, value)| (name.clone(), coordinate_count(value)))
        .collect()
}

fn compact_observation(value: &Value) -> String {
    fn compact_cell(value: &Value) -> String {
        let name = value["name"].as_str().unwrap_or("?");
        let properties = value["properties"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, value)| format!("{name}={}", value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())))
            .collect::<Vec<_>>();
        let mut result = if properties.is_empty() {
            name.to_owned()
        } else {
            format!("{name}{{{}}}", properties.join(","))
        };
        if !value["entity"].is_null() {
            result.push_str(&format!("#entity={}", value["entity"]));
        }
        if !value["piston_power"].is_null() {
            result.push_str(&format!("#piston_power={}", value["piston_power"]));
        }
        result
    }

    if value.get("name").and_then(Value::as_str).is_some() {
        return compact_cell(value);
    }
    if let Some(items) = value.as_array() {
        let cells: Vec<_> = items.iter().map(compact_observation).collect();
        let mut runs = Vec::new();
        let mut index = 0;
        while index < cells.len() {
            let mut end = index + 1;
            while end < cells.len() && cells[end] == cells[index] {
                end += 1;
            }
            let count = end - index;
            runs.push(if count == 1 {
                cells[index].clone()
            } else {
                format!("{}×{count}", cells[index])
            });
            index = end;
        }
        return format!("[{}]", runs.join(","));
    }
    if let Some(fields) = value.as_object() {
        return format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(name, value)| format!("{name}:{}", compact_observation(value)))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    value.to_string()
}

fn run_pack_case(manifest: &Value, rotation: u32, case: &Value) -> (NativeCensus, BTreeSet<(i32, i32, i32)>, usize, bool, String) {
    use crate::redstone::instant_piston_tests as pack;

    let (mut world, watch, dimensions) = pack::load(manifest, rotation);
    let actor_positions: BTreeSet<_> = watch
        .iter()
        .copied()
        .filter(|&position| {
            matches!(world.get_block(position), Block::Piston { .. })
                || matches!(world.get_block_entity(position), Some(BlockEntity::MovingPiston(entity)) if entity.source)
        })
        .map(position_key)
        .collect();
    let moving_bases = watch
        .iter()
        .filter(|&&position| matches!(world.get_block_entity(position), Some(BlockEntity::MovingPiston(entity)) if entity.source))
        .count();
    let mut saw_inflight = moving_bases > 0 || !world.piston_state().motions.is_empty();
    let mut census = NativeCensus::default();
    let mut operation = 0;
    for (action_index, action) in case["actions"].as_array().unwrap().iter().enumerate() {
        match action["op"].as_str().unwrap() {
            "wait" | "wait_ready" => {
                let bound = action["ticks"].as_u64().unwrap();
                let mut stable = 0;
                let mut last = pack::physical(&world, &watch)["cells"].clone();
                for _ in 0..bound {
                    record_trace(
                        &mut census,
                        trace::capture(|| pack::step(&mut world, "game", action_index + 1, &mut operation, |_, _| {})),
                    );
                    saw_inflight |= !world.piston_state().motions.is_empty()
                        || watch
                            .iter()
                            .any(|&position| matches!(world.get_block(position), Block::MovingPiston { .. }));
                    let now = pack::physical(&world, &watch)["cells"].clone();
                    stable = if pack::quiet(&world) && now == last { stable + 1 } else { 0 };
                    last = now;
                    if action["op"] == "wait_ready" && stable >= 2 {
                        break;
                    }
                }
                if action["op"] == "wait_ready" {
                    assert!(stable >= 2, "{} {} r{rotation} failed bounded readiness", manifest["id"], case["id"]);
                }
            }
            _ => record_trace(&mut census, trace::capture(|| pack::mutate(&mut world, action, dimensions, rotation))),
        }
        saw_inflight |= !world.piston_state().motions.is_empty()
            || watch
                .iter()
                .any(|&position| matches!(world.get_block(position), Block::MovingPiston { .. }));
    }
    for _ in 0..case["ticks"].as_u64().unwrap() {
        record_trace(
            &mut census,
            trace::capture(|| pack::step(&mut world, "game", 0, &mut operation, |_, _| {})),
        );
        saw_inflight |= !world.piston_state().motions.is_empty()
            || watch
                .iter()
                .any(|&position| matches!(world.get_block(position), Block::MovingPiston { .. }));
    }
    let mut observed_manifest = manifest.clone();
    if let Some(observations) = case["observations"].as_object() {
        for (name, positions) in observations {
            observed_manifest["ports"]["observations"][name] = positions.clone();
        }
    }
    let final_observations = compact_observation(&pack::observe(&world, &observed_manifest, dimensions, rotation));
    (census, actor_positions, moving_bases, saw_inflight, final_observations)
}

fn assert_adder_fixture_ports() {
    let adder_1bit = boundary_contract_tests::load_mechanism("adder_1bit");
    let one_bit_inputs: BTreeSet<_> = adder_1bit.ports["inputs"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(one_bit_inputs, BTreeSet::from(["A", "B", "Cin", "trigger"]));
    for name in ["A", "B", "Cin", "trigger"] {
        assert_eq!(
            adder_1bit.port_positions("inputs", name).len(),
            1,
            "one-bit {name} is a scalar control/data port"
        );
    }
    for name in ["sum_payload", "sum_repeater", "carry_repeater"] {
        assert_eq!(
            adder_1bit.port_positions("observations", name).len(),
            1,
            "one-bit {name} is one observed site"
        );
    }
    assert_ne!(
        adder_1bit.port_positions("observations", "sum_payload"),
        adder_1bit.port_positions("observations", "sum_repeater"),
        "sum_payload and sum_repeater are two physical observations of the Sum result"
    );
    let one_bit_observations: BTreeSet<_> = adder_1bit.ports["observations"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(one_bit_observations, BTreeSet::from(["carry_repeater", "sum_payload", "sum_repeater"]));

    let adder_11bits = boundary_contract_tests::load_mechanism("adder_11bits");
    let eleven_bit_inputs: BTreeSet<_> = adder_11bits.ports["inputs"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(eleven_bit_inputs, BTreeSet::from(["A", "B", "trigger"]));
    for name in ["A", "B"] {
        assert_eq!(
            adder_11bits.port_positions("inputs", name).len(),
            11,
            "eleven-bit {name} is an 11-site input bus"
        );
    }
    assert_eq!(adder_11bits.port_positions("inputs", "trigger").len(), 1);
    for name in ["sum_payload", "sum_repeater"] {
        assert_eq!(
            adder_11bits.port_positions("observations", name).len(),
            11,
            "eleven-bit {name} observes the Sum bus"
        );
    }
    assert_ne!(
        adder_11bits.port_positions("observations", "sum_payload"),
        adder_11bits.port_positions("observations", "sum_repeater"),
        "the two 11-bit Sum groups are separate observation sites"
    );
    let eleven_bit_observations: BTreeSet<_> = adder_11bits.ports["observations"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(eleven_bit_observations, BTreeSet::from(["sum_payload", "sum_repeater"]));
}

#[test]
fn one_bit_fixture_censuses_all_48_physical_data_orders_before_using_single_use() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let cases = arithmetic_cases("adder_1bit");
    assert_eq!(cases.len(), 8, "all pinned one-bit arithmetic vectors are current");
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    assert_eq!(graph.pistons.len(), 13, "actor count comes from the current loaded fixture");
    let mechanisms = crate::redpiler::passes::utils::instant_mechanisms::discover_mechanisms(&graph, &TaskMonitor::default()).unwrap();
    let mechanism_actors: BTreeSet<_> = mechanisms
        .iter()
        .map(|item| position_key(graph.pistons[item.piston.index()].position))
        .collect();
    assert_eq!(mechanism_actors.len(), 13, "all current actors have distinct observer-reset provenance");

    let mut histories = Vec::with_capacity(48);
    let mut first_wave = None;
    let mut prepared_wave = None;
    for case in &cases {
        let expected = expectation(&circuit, case);
        let canonical = frame_for_order(&circuit, case, &DATA_ORDERS[0]);
        assert_eq!(canonical.preparation.len(), 3, "one-bit frame has A, B and Cin setters");
        assert_eq!(canonical.ready_ticks, 32, "fixture readiness is an explicit physical wait");
        assert!(
            canonical.activation.old_powered && !canonical.activation.new_powered,
            "activation is a distinct falling edge"
        );
        for (port, value) in ["A", "B", "Cin"].into_iter().zip(case["inputs"].as_array().unwrap()) {
            let level = canonical.sampled_data.iter().find(|(name, _)| name == port).unwrap().1;
            assert_eq!(
                !level,
                value.as_u64().unwrap() != 0,
                "physical decode for {port} in {}",
                canonical.case_id
            );
        }
        for order in DATA_ORDERS {
            let frame = frame_for_order(&circuit, case, &order);
            let census = run_frame(&circuit, &frame, expected.0, expected.1);
            if first_wave.is_none() && !census.drains.is_empty() {
                first_wave = Some((frame.case_id.clone(), order, census.clone()));
            }
            if frame.case_id == "prepared-0-1-1" && order == DATA_ORDERS[0] {
                prepared_wave = Some(census.clone());
            }
            histories.push(census);
        }
    }
    assert_eq!(histories.len(), 48, "eight vectors times six sequential preparation orders");
    let totals = totals(&histories);
    let actor_totals: Vec<_> = totals
        .by_actor
        .iter()
        .map(|(position, counts)| (*position, counts.extends, counts.retracts, counts.early_drops))
        .collect();
    println!(
        "adder_1bit census: actors=13, mechanisms={}, histories=48, samples={}, drains={}, Extend={}, Retract={}, RetractWithoutPull={}, mixed actor-drains={}, per-actor={actor_totals:?}",
        mechanisms.len(),
        totals.samples,
        totals.drains,
        totals.accepted.extends,
        totals.accepted.retracts,
        totals.accepted.early_drops,
        totals.combined_drains
    );
    let (case_id, order, first) = first_wave.expect("some pinned vector produces an accepted action");
    println!("adder_1bit first nonempty wave {case_id} order {order:?}: {:?}", raw_drains(&first, 4));
    let prepared_wave = prepared_wave.expect("the pinned prepared-0-1-1 wave was retained");
    let first_eight: Vec<_> = raw_drains(&prepared_wave, 8)
        .into_iter()
        .map(|(tick, phase, events)| {
            let ordered: Vec<_> = events.iter().map(|event| (event.pos, event.facing, event.action)).collect();
            (tick, phase, ordered)
        })
        .collect();
    println!("adder_1bit prepared-0-1-1 first eight accepted drains: {first_eight:?}");

    let counterexample = cases.iter().find(|case| case["id"] == "prepared-0-1-1").unwrap();
    let mut steps = case_steps(&circuit, counterexample, false, true);
    steps.extend(std::iter::repeat_n(Step::Tick, counterexample["ticks"].as_u64().unwrap() as usize));
    let (coverage, _) =
        replay(&circuit, &steps, None, None, None).unwrap_or_else(|error| panic!("prepared-0-1-1 native/Graph protocol comparison: {error}"));
    assert!(coverage.accepted_extensions + coverage.accepted_retractions > 0);
}

#[test]
fn eleven_bit_fixture_censuses_all_pinned_arithmetic_histories() {
    let circuit = load_adder("adder_11bits", ORIGIN, None);
    let cases = arithmetic_cases("adder_11bits");
    assert_eq!(cases.len(), 40, "current pinned eleven-bit arithmetic histories are covered");
    let (graph, _) = stable_region_tests::stable_graph("adder_11bits");
    assert_eq!(graph.pistons.len(), 142, "actor count comes from the current loaded fixture");
    let mechanisms = crate::redpiler::passes::utils::instant_mechanisms::discover_mechanisms(&graph, &TaskMonitor::default()).unwrap();
    let mechanism_actors: BTreeSet<_> = mechanisms
        .iter()
        .map(|item| position_key(graph.pistons[item.piston.index()].position))
        .collect();
    assert_eq!(mechanism_actors.len(), 142, "all current actors have distinct observer-reset provenance");

    let mut histories = Vec::with_capacity(cases.len());
    for case in &cases {
        let order = fixture_data_order(&circuit, case);
        assert_eq!(order.len(), 22, "eleven-bit fixture sets A and B; it has no Cin port");
        let frame = frame_for_order(&circuit, case, &order);
        assert_eq!(frame.preparation.len(), 22);
        assert_eq!(frame.ready_ticks, 32, "fixture readiness is an explicit physical wait");
        assert!(
            frame.activation.old_powered && !frame.activation.new_powered,
            "activation is a distinct falling edge"
        );
        let expected = expectation(&circuit, case);
        histories.push(run_frame(&circuit, &frame, expected.0, expected.1));
    }
    let totals = totals(&histories);
    let actor_totals: Vec<_> = totals
        .by_actor
        .iter()
        .map(|(position, counts)| (*position, counts.extends, counts.retracts, counts.early_drops))
        .collect();
    println!(
        "adder_11bits census: actors=142, mechanisms={}, histories={}, samples={}, drains={}, Extend={}, Retract={}, RetractWithoutPull={}, mixed actor-drains={}, per-actor={actor_totals:?}",
        mechanisms.len(),
        histories.len(),
        totals.samples,
        totals.drains,
        totals.accepted.extends,
        totals.accepted.retracts,
        totals.accepted.early_drops,
        totals.combined_drains
    );
    let (case, first) = cases
        .iter()
        .zip(&histories)
        .find(|(_, history)| !history.drains.is_empty())
        .expect("some pinned vector produces an accepted action");
    println!(
        "adder_11bits first nonempty wave {}: {:?}",
        case["id"].as_str().unwrap(),
        raw_drains(first, 2)
    );
}

#[test]
fn every_pinned_io_fixture_case_has_a_native_typed_census_and_resolved_ports() {
    const FIXTURES: [&str; 22] = [
        "adder_11bits",
        "adder_1bit",
        "and_1",
        "and_2",
        "and_3",
        "bud_instantmemorycellobserverupdate",
        "bud_instantpistonupdate",
        "bud_noninstantinputs",
        "bud_pistonupdate",
        "counter_basic",
        "instant_blocked",
        "instant_chain",
        "instant_down",
        "instant_down_torch_reset",
        "instant_observer",
        "instant_reset_redstone",
        "instant_reset_redstone_2",
        "instant_torch",
        "not_1",
        "or_1",
        "or_interpreter_illigal",
        "xor_simple",
    ];
    let fixture_directory = root().join("test_data/instant-pistons-io/fixtures");
    let available = std::fs::read_dir(&fixture_directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .count();
    assert_eq!(
        available,
        FIXTURES.len(),
        "update this explicit fixture list when the pinned corpus changes"
    );

    assert_adder_fixture_ports();

    for id in FIXTURES {
        let circuit = boundary_contract_tests::load_mechanism(id);
        let manifest_path = fixture_directory.join(format!("{id}.json"));
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
        let cases = manifest["cases"].as_array().expect("fixture cases");
        let initial = circuit.build();
        let inputs: Vec<_> = circuit.ports["inputs"]
            .as_object()
            .expect("logical fixture inputs")
            .keys()
            .map(|name| (name.clone(), circuit.port_positions("inputs", name).len()))
            .collect();
        let observations: Vec<_> = circuit.ports["observations"]
            .as_object()
            .expect("logical fixture observations")
            .keys()
            .map(|name| (name.clone(), circuit.port_positions("observations", name).len()))
            .collect();
        let mut input_actions = BTreeMap::<String, usize>::new();
        for case in cases {
            for action in case["actions"].as_array().unwrap() {
                if matches!(action["op"].as_str(), Some("lever" | "set_notified")) {
                    let position = circuit.place(local(&action["pos"]));
                    let matches = port_name_at(&circuit, "inputs", position);
                    assert_eq!(
                        matches.len(),
                        1,
                        "{id} {} action at {position:?} maps to one declared logical input",
                        case["id"]
                    );
                    *input_actions.entry(matches[0].clone()).or_default() += 1;
                }
            }
        }
        let memory = circuit.port_positions("observations", "memory").first().copied().map(|position| {
            let stored = initial.get_block(position);
            let is_piston_actor = matches!(stored, Block::Piston { .. });
            (position, format!("{stored:?}"), is_piston_actor)
        });

        for rotation in manifest["rotations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as u32)
        {
            let mut first_multi_retract: Option<(String, u64, (i32, i32, i32), ActionCounts)> = None;
            let mut multi_action_actor_drains = 0;
            let mut recurring_retract_actors = BTreeSet::new();
            let mut early_drop_actors = BTreeSet::new();
            let mut off_initial_actor_cases = Vec::new();
            let mut no_action_cases = Vec::new();
            let mut inflight_cases = Vec::new();
            let mut initial_moving_cases = Vec::new();
            let mut actor_count = None;
            let mut samples = 0;
            let mut drains = 0;
            let mut accepted = ActionCounts::default();
            for case in cases {
                let (census, actor_positions, moving_bases, saw_inflight, final_observations) = run_pack_case(&manifest, rotation, case);
                assert_eq!(
                    actor_count.replace(actor_positions.len()).unwrap_or(actor_positions.len()),
                    actor_positions.len(),
                    "{id} r{rotation} actor inventory is stable across cases"
                );
                let mut case_actor_counts = BTreeMap::<(i32, i32, i32), ActionCounts>::new();
                let mut off_initial_actor_positions = BTreeSet::new();
                let mut repeated_retraction = None;
                for drain in &census.drains {
                    assert_eq!(drain.phase, AdvancePhase::PistonEvents);
                    let mut drain_actor_counts = BTreeMap::<(i32, i32, i32), ActionCounts>::new();
                    for event in &drain.events {
                        let actor = position_key(event.pos);
                        if !actor_positions.contains(&actor) {
                            off_initial_actor_positions.insert(actor);
                        }
                        record_action(&mut drain_actor_counts.entry(actor).or_default(), event.action);
                        record_action(&mut case_actor_counts.entry(actor).or_default(), event.action);
                    }
                    for (actor, counts) in drain_actor_counts {
                        if counts.extends + retractions(counts) > 1 {
                            multi_action_actor_drains += 1;
                        }
                        if counts.retracts > 1 && first_multi_retract.is_none() {
                            first_multi_retract = Some((case["id"].as_str().unwrap().to_owned(), drain.tick, actor, counts));
                        }
                        if counts.retracts > 1 && repeated_retraction.is_none() {
                            repeated_retraction = Some((drain.tick, actor, counts));
                        }
                        if counts.early_drops > 0 {
                            early_drop_actors.insert(actor);
                        }
                    }
                }
                if moving_bases == 0 && off_initial_actor_positions.is_empty() {
                    assert!(
                        repeated_retraction.is_none(),
                        "{id} {} r{rotation}: repeated ordinary retraction in one stationary-entry/source-site drain: {repeated_retraction:?}",
                        case["id"]
                    );
                }
                if !off_initial_actor_positions.is_empty() {
                    off_initial_actor_cases.push((case["id"].as_str().unwrap().to_owned(), off_initial_actor_positions.clone()));
                }
                recurring_retract_actors.extend(
                    case_actor_counts
                        .iter()
                        .filter_map(|(actor, counts)| (counts.retracts > 1).then_some(*actor)),
                );
                let counts = totals(std::slice::from_ref(&census));
                let case_id = case["id"].as_str().unwrap_or("unnamed");
                let mut cohorts = Vec::new();
                if counts.accepted.extends + retractions(counts.accepted) == 0 {
                    no_action_cases.push(case_id.to_owned());
                    cohorts.push("no-accepted-actions");
                }
                if saw_inflight {
                    inflight_cases.push(case_id.to_owned());
                    cohorts.push("inflight-during-episode");
                }
                if moving_bases > 0 {
                    initial_moving_cases.push((case_id.to_owned(), moving_bases));
                    cohorts.push("initially-moving-base");
                }
                if counts.accepted.early_drops > 0 {
                    cohorts.push("early-drop");
                }
                if !off_initial_actor_positions.is_empty() {
                    cohorts.push("off-initial-actor");
                }
                println!(
                    "IO case fixture={id} rotation={rotation} case={case_id} controls={} samples={} Extend={} Retract={} drop={} drains={} ordered_drains={:?} moving_bases={moving_bases} cohorts={cohorts:?} off_initial_actors={off_initial_actor_positions:?} observations={final_observations}",
                    case["inputs"],
                    counts.samples,
                    counts.accepted.extends,
                    counts.accepted.retracts,
                    counts.accepted.early_drops,
                    census.drains.len(),
                    raw_drains(&census, census.drains.len()),
                );
                samples += counts.samples;
                drains += census.drains.len();
                accepted.extends += counts.accepted.extends;
                accepted.retracts += counts.accepted.retracts;
                accepted.early_drops += counts.accepted.early_drops;
            }
            println!(
                "IO rotation fixture={id} rotation={rotation} cases={} actors={} samples={samples} Extend={} Retract={} drop={} drains={drains} recurring_retract_actors={recurring_retract_actors:?} early_drop_actors={early_drop_actors:?} multi_action_actor_drains={multi_action_actor_drains} first_multi_retract={first_multi_retract:?} no_action_cases={no_action_cases:?} inflight_cases={inflight_cases:?} initially_moving_cases={initial_moving_cases:?} off_initial_actor_cases={off_initial_actor_cases:?}",
                cases.len(),
                actor_count.unwrap_or_default(),
                accepted.extends,
                accepted.retracts,
                accepted.early_drops,
            );
        }
        println!("IO fixture={id} declared_inputs={inputs:?} action_controls={input_actions:?} observations={observations:?} memory={memory:?}");
    }
}

#[test]
fn every_nonempty_core_pack_case_and_declared_rotation_keeps_native_drain_census() {
    use crate::redstone::instant_piston_tests as pack;

    let all_manifests = pack::all_manifests();
    assert_eq!(all_manifests.len(), 22, "the core pack contains 22 pinned fixture manifests");
    let empty_fixtures: Vec<_> = all_manifests
        .iter()
        .filter(|manifest| manifest["cases"].as_array().is_some_and(Vec::is_empty))
        .map(|manifest| manifest["id"].as_str().unwrap())
        .collect();
    assert_eq!(empty_fixtures.len(), 2, "sort fixtures without declared protocols remain inventoried");

    let mut first_single_retract_failure = None;
    for manifest in &all_manifests {
        let cases = manifest["cases"].as_array().unwrap();
        if cases.is_empty() {
            println!("core fixture={} cases=0 cohort=no-declared-protocol", manifest["id"]);
            continue;
        }
        let inputs = port_group_counts(&manifest["ports"]["inputs"]);
        let mut observation_counts: BTreeMap<_, _> = port_group_counts(&manifest["ports"]["observations"]).into_iter().collect();
        for case in cases {
            if let Some(overrides) = case["observations"].as_object() {
                for (name, positions) in overrides {
                    observation_counts.insert(name.clone(), coordinate_count(positions));
                }
            }
        }
        let observations: Vec<_> = observation_counts.into_iter().collect();
        for rotation in manifest["rotations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rotation| rotation.as_u64().unwrap() as u32)
        {
            let mut negative_cases = Vec::new();
            let mut inflight_cases = Vec::new();
            let mut initial_moving_cases = Vec::new();
            let mut off_initial_actor_cases = Vec::new();
            let mut drop_cases = Vec::new();
            let mut eligible_cases = 0;
            let mut rotation_failure = None;
            let mut actor_count = None;
            let mut samples = 0;
            let mut drains = 0;
            let mut accepted = ActionCounts::default();
            let mut accepted_actors = BTreeSet::new();
            let mut multi_action_actor_drains = 0;
            for case in cases {
                let (census, actor_positions, moving_bases, saw_inflight, final_observations) = run_pack_case(manifest, rotation, case);
                assert_eq!(
                    actor_count.replace(actor_positions.len()).unwrap_or(actor_positions.len()),
                    actor_positions.len(),
                    "{} r{rotation} actor inventory is stable across cases",
                    manifest["id"]
                );
                let case_totals = totals(std::slice::from_ref(&census));
                let case_id = case["id"].as_str().unwrap().to_owned();
                let no_accepted_actions = case_totals.accepted.extends + retractions(case_totals.accepted) == 0;
                if no_accepted_actions {
                    negative_cases.push(case_id.clone());
                }
                if saw_inflight {
                    inflight_cases.push(case_id.clone());
                }
                if moving_bases > 0 {
                    initial_moving_cases.push((case_id.clone(), moving_bases));
                }
                if case_totals.accepted.early_drops > 0 {
                    drop_cases.push((case_id.clone(), case_totals.accepted.early_drops));
                }
                let off_initial_actor_positions: BTreeSet<_> = census
                    .drains
                    .iter()
                    .flat_map(|drain| drain.events.iter().map(|event| position_key(event.pos)))
                    .filter(|actor| !actor_positions.contains(actor))
                    .collect();
                // Only initially stationary, fixed-site bases enter the same-drain acceptance lemma.
                let single_retract_eligible = moving_bases == 0 && off_initial_actor_positions.is_empty();
                eligible_cases += usize::from(single_retract_eligible);
                if !off_initial_actor_positions.is_empty() {
                    off_initial_actor_cases.push((case_id.clone(), off_initial_actor_positions.clone()));
                }
                let mut case_actor_totals = BTreeMap::<(i32, i32, i32), ActionCounts>::new();
                let mut case_multi_action_actor_drains = 0;
                for drain in &census.drains {
                    assert_eq!(drain.phase, AdvancePhase::PistonEvents);
                    let mut by_actor = BTreeMap::<(i32, i32, i32), ActionCounts>::new();
                    for event in &drain.events {
                        let actor = position_key(event.pos);
                        accepted_actors.insert(actor);
                        record_action(by_actor.entry(actor).or_default(), event.action);
                        record_action(case_actor_totals.entry(actor).or_default(), event.action);
                    }
                    let multi = by_actor.values().filter(|counts| counts.extends + retractions(**counts) > 1).count();
                    case_multi_action_actor_drains += multi;
                    multi_action_actor_drains += multi;
                    if single_retract_eligible
                        && let Some((actor, counts)) = by_actor.iter().find(|(_, counts)| counts.retracts > 1)
                        && rotation_failure.is_none()
                    {
                        rotation_failure = Some((case_id.clone(), drain.tick, *actor, *counts));
                    }
                }
                let stimuli: Vec<_> = case["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|action| action["op"].as_str().unwrap())
                    .collect();
                let repeated_retract_actors = case_actor_totals.values().filter(|counts| counts.retracts > 1).count();
                let mut cohorts = Vec::new();
                if no_accepted_actions {
                    cohorts.push("no-accepted-actions");
                }
                if saw_inflight {
                    cohorts.push("inflight-during-episode");
                }
                if moving_bases > 0 {
                    cohorts.push("initially-moving-base");
                }
                if !off_initial_actor_positions.is_empty() {
                    cohorts.push("off-initial-actor");
                }
                if case_totals.accepted.early_drops > 0 {
                    cohorts.push("early-drop");
                }
                if single_retract_eligible {
                    cohorts.push("stationary-fixed-site");
                }
                println!(
                    "core case fixture={} rotation={rotation} case={case_id} controls={} stimuli={stimuli:?} samples={} Extend={} Retract={} drop={} drains={} ordered_drains={:?} cohorts={cohorts:?} moving_bases={moving_bases} off_initial_actors={off_initial_actor_positions:?} repeated_retract_actors={repeated_retract_actors} multi_action_actor_drains={case_multi_action_actor_drains} observations={final_observations}",
                    manifest["id"],
                    case["inputs"],
                    case_totals.samples,
                    case_totals.accepted.extends,
                    case_totals.accepted.retracts,
                    case_totals.accepted.early_drops,
                    census.drains.len(),
                    raw_drains(&census, census.drains.len()),
                );
                samples += case_totals.samples;
                drains += census.drains.len();
                accepted.extends += case_totals.accepted.extends;
                accepted.retracts += case_totals.accepted.retracts;
                accepted.early_drops += case_totals.accepted.early_drops;
            }
            if first_single_retract_failure.is_none()
                && let Some(failure) = rotation_failure.clone()
            {
                first_single_retract_failure = Some((manifest["id"].as_str().unwrap().to_owned(), rotation, failure));
            }
            println!(
                "core rotation fixture={} rotation={rotation} cases={} actors={} active_actors={} samples={samples} Extend={} Retract={} drop={} drains={drains} eligible_stationary_fixed_site_cases={eligible_cases} no_action_cases={negative_cases:?} inflight_cases={inflight_cases:?} initially_moving_cases={initial_moving_cases:?} off_initial_actor_cases={off_initial_actor_cases:?} drop_cases={drop_cases:?} multi_action_actor_drains={multi_action_actor_drains} single_retract_failure={rotation_failure:?}",
                manifest["id"],
                cases.len(),
                actor_count.unwrap_or_default(),
                accepted_actors.len(),
                accepted.extends,
                accepted.retracts,
                accepted.early_drops,
            );
        }
        println!("core fixture={} declared_inputs={inputs:?} observations={observations:?}", manifest["id"]);
    }
    println!("core pack first stationary/fixed-site single-Retract-per-drain counterexample: {first_single_retract_failure:?}");
    assert!(
        first_single_retract_failure.is_none(),
        "stationary fixed-site fixture case violated one accepted Retract per actor per actual PistonEvents drain: {first_single_retract_failure:?}"
    );
}
