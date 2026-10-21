//! Full adder fixture replays against the interpreter and compact graph inventory.
use crate::redpiler::passes::Pass;
#[path = "boundary_contract.rs"]
mod boundary_contract_tests;
#[path = "builtin_reset.rs"]
mod builtin_reset_tests;
#[path = "execution_coverage.rs"]
mod execution_coverage_tests;
#[path = "fixed_point_adder.rs"]
mod fixed_point_tests;
#[path = "guarded_transport.rs"]
mod guarded_transport_tests;
#[path = "instant_boundaries.rs"]
mod instant_boundary_tests;
#[path = "instant_cells.rs"]
mod instant_cell_tests;
#[path = "instant_contract.rs"]
mod instant_contract_tests;
#[path = "instant_events.rs"]
mod instant_event_tests;
#[path = "instant_layers.rs"]
mod instant_layer_tests;
#[path = "instant_net_counterexample.rs"]
mod instant_net_counterexample;
#[path = "instant_proxy.rs"]
mod instant_proxy_tests;
#[path = "instant_recognition.rs"]
mod instant_recognition_tests;
#[path = "instant_transport.rs"]
mod instant_transport_tests;
#[path = "production_tiers.rs"]
mod production_tier_tests;
#[path = "reset_elision.rs"]
mod reset_elision_tests;
#[path = "stable_region.rs"]
mod stable_region_tests;
use super::fuzz_tests::{Operation, graph_operations_since, native_operations};
use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::GraphExecutionCounts;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::storage::Chunk;
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, LeverFace, RotateAmt};
use mchprs_world::{AdvancePhase, PistonAction, PistonEvent, TickEntry};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) fn counter_world() -> (PlotWorld, (BlockPos, BlockPos)) {
    let circuit = boundary_contract_tests::load_mechanism("counter_basic");
    (circuit.build(), circuit.bounds())
}

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const EXTRA: BlockPos = BlockPos::new(1, 1, 1);
const FAMILY: [&str; 2] = ["adder_1bit", "adder_11bits"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    )
}

fn local(value: &Value) -> BlockPos {
    BlockPos::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

struct Adder {
    id: &'static str,
    hash: String,
    size: BlockPos,
    anchor: BlockPos,
    rotation: Option<RotateAmt>,
    cells: Vec<(BlockPos, Block, Option<BlockEntity>)>,
    ports: Value,
}

fn load_adder(id: &'static str, anchor: BlockPos, rotation: Option<RotateAmt>) -> Adder {
    let directory = root().join("test_data/instant-pistons-io");
    let manifest: Value = serde_json::from_slice(&std::fs::read(directory.join("fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
    let filename = manifest["fixture"].as_str().unwrap();
    let bytes = std::fs::read(root().join(filename)).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(manifest["sha256"], hash);
    let pinned: Value = serde_json::from_slice(&std::fs::read(directory.join("download-manifest.json")).unwrap()).unwrap();
    let file = filename.rsplit('/').next().unwrap();
    let entry = pinned["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == file)
        .expect("the adder schematic is in the pinned IO manifest");
    assert_eq!(entry["sha256"], hash);
    assert_eq!(entry["bytes"].as_u64().unwrap() as usize, bytes.len());

    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let size = BlockPos::new(clipboard.size_x as i32, clipboard.size_y as i32, clipboard.size_z as i32);
    let mut imported = empty_world();
    paste_clipboard(
        &mut imported,
        &clipboard,
        anchor + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let mut cells = Vec::new();
    let mut controls = Vec::new();
    for y in 0..size.y {
        for z in 0..size.z {
            for x in 0..size.x {
                let position = BlockPos::new(x, y, z);
                let world_position = anchor + position;
                let block = imported.get_block(world_position);
                let entity = imported.get_block_entity(world_position).cloned();
                if matches!(block, Block::Lever { .. }) {
                    controls.push(position);
                }
                if block != Block::Air || entity.is_some() {
                    cells.push((position, block, entity));
                }
            }
        }
    }
    controls.sort_by_key(|p| (p.x, p.y, p.z));
    assert_eq!(controls.len(), if id == "adder_1bit" { 4 } else { 23 });
    assert_eq!(manifest["dimensions"], serde_json::json!([size.x, size.y, size.z]));
    Adder {
        id,
        hash,
        size,
        anchor,
        rotation,
        cells,
        ports: manifest["ports"].clone(),
    }
}

impl Adder {
    fn place(&self, position: BlockPos) -> BlockPos {
        let rotated = match self.rotation {
            None => position,
            Some(RotateAmt::Rotate90) => BlockPos::new(-position.z, position.y, position.x),
            Some(RotateAmt::Rotate180) => BlockPos::new(-position.x, position.y, -position.z),
            Some(RotateAmt::Rotate270) => BlockPos::new(position.z, position.y, -position.x),
        };
        self.anchor + rotated
    }

    fn build(&self) -> PlotWorld {
        let mut world = empty_world();
        for (position, original, entity) in &self.cells {
            let mut block = *original;
            if let Some(rotation) = self.rotation {
                block.rotate(rotation);
            }
            let position = self.place(*position);
            world.set_block(position, block);
            if let Some(entity) = entity {
                world.set_block_entity(position, entity.clone());
            }
        }
        world
    }

    fn bounds(&self) -> (BlockPos, BlockPos) {
        let high = self.size - BlockPos::new(1, 1, 1);
        let corners = [
            BlockPos::new(0, 0, 0),
            high,
            BlockPos::new(high.x, 0, high.z),
            BlockPos::new(0, high.y, high.z),
        ]
        .map(|p| self.place(p));
        (
            corners.iter().copied().reduce(BlockPos::min).unwrap() - EXTRA,
            corners.iter().copied().reduce(BlockPos::max).unwrap() + EXTRA,
        )
    }

    fn positions(&self) -> Vec<BlockPos> {
        let (low, high) = self.bounds();
        let mut positions = Vec::new();
        for y in low.y..=high.y {
            for z in low.z..=high.z {
                for x in low.x..=high.x {
                    positions.push(BlockPos::new(x, y, z));
                }
            }
        }
        positions
    }

    fn port_positions(&self, group: &str, name: &str) -> Vec<BlockPos> {
        fn flatten(value: &Value, out: &mut Vec<BlockPos>) {
            if value.as_array().is_some_and(|v| v.len() == 3 && v[0].is_number()) {
                out.push(local(value));
            } else if let Some(values) = value.as_array() {
                for value in values {
                    flatten(value, out);
                }
            }
        }
        let mut local_positions = Vec::new();
        flatten(&self.ports[group][name], &mut local_positions);
        local_positions.into_iter().map(|position| self.place(position)).collect()
    }

    fn trigger(&self) -> BlockPos {
        self.place(local(&self.ports["inputs"]["trigger"]))
    }
}

#[derive(Debug, Clone, Copy)]
enum Step {
    SetLever { position: BlockPos, powered: bool },
    Tick,
}

fn step_for_action(circuit: &Adder, action: &Value) -> Step {
    assert_eq!(action["op"], "lever");
    Step::SetLever {
        position: circuit.place(local(&action["pos"])),
        powered: action["powered"].as_bool().unwrap(),
    }
}

/// Preserves fixture order by default; reverse_data changes only the order of operand levers.
fn case_steps(circuit: &Adder, case: &Value, reverse_data: bool, trigger: bool) -> Vec<Step> {
    let mut data = Vec::new();
    let mut waits = Vec::new();
    let mut trigger_action = None;
    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" if circuit.place(local(&action["pos"])) == circuit.trigger() => {
                trigger_action = Some(step_for_action(circuit, action));
            }
            "lever" => data.push(step_for_action(circuit, action)),
            "wait" | "wait_ready" => {
                waits.extend(std::iter::repeat_n(Step::Tick, action["ticks"].as_u64().unwrap() as usize));
            }
            op => panic!("unexpected adder fixture action: {op}"),
        }
    }
    if reverse_data {
        data.reverse();
    }
    data.extend(waits);
    if trigger {
        data.extend(trigger_action);
    }
    data
}

fn native_step(world: &mut PlotWorld, step: Step) {
    match step {
        Step::Tick => world.tick_interpreted(),
        Step::SetLever { position, powered } => {
            let Block::Lever { mut lever } = world.get_block(position) else {
                panic!("adder control missing at {position:?}");
            };
            if lever.powered == powered {
                return;
            }
            lever.powered = powered;
            world.set_block(position, Block::Lever { lever });
            redstone::update_surrounding_blocks(world, position);
            let support = match lever.face {
                LeverFace::Ceiling => BlockFace::Top,
                LeverFace::Floor => BlockFace::Bottom,
                LeverFace::Wall => lever.facing.opposite().block_face(),
            };
            redstone::update_surrounding_blocks(world, position.offset(support));
        }
    }
}

fn graph_step(backend: &mut GraphBackend, step: Step, toggle: bool) -> Result<(), GraphExecutionError> {
    match step {
        Step::Tick => backend.tick(),
        Step::SetLever { position, .. } => {
            if toggle {
                backend.use_block(position)?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, PartialEq)]
struct Future {
    tick: u64,
    phase: AdvancePhase,
    cells: Vec<(BlockPos, Block)>,
    entities: Vec<(BlockPos, String)>,
    scheduled: Vec<TickEntry>,
    events: Vec<PistonEvent>,
    motions: Vec<(BlockPos, f32, f32, u64)>,
}

fn future(world: &PlotWorld, positions: &[BlockPos]) -> Future {
    let state = world.piston_state();
    Future {
        tick: state.logical_tick,
        phase: state.phase,
        cells: positions
            .iter()
            .map(|&position| (position, world.get_block(position)))
            .filter(|(_, block)| *block != Block::Air)
            .collect(),
        entities: positions
            .iter()
            .filter_map(|&position| world.get_block_entity(position).map(|entity| (position, format!("{entity:?}"))))
            .collect(),
        scheduled: world.scheduler().iter_entries().collect(),
        events: state.events.iter().copied().collect(),
        motions: state
            .motions
            .iter()
            .map(|motion| (motion.pos, motion.progress, motion.previous_progress, motion.last_tick))
            .collect(),
    }
}

fn first_difference(expected: &Future, actual: &Future) -> Option<String> {
    macro_rules! differs {
        ($field:ident) => {
            if expected.$field != actual.$field {
                return Some(format!(
                    "{}: native {:?}; graph {:?}",
                    stringify!($field),
                    expected.$field,
                    actual.$field
                ));
            }
        };
    }
    differs!(tick);
    differs!(phase);
    differs!(cells);
    differs!(entities);
    differs!(scheduled);
    differs!(events);
    differs!(motions);
    None
}

fn project(backend: &GraphBackend, scratch: &mut PlotWorld) {
    scratch.clear_scheduled_ticks();
    scratch.piston_state_mut().motions.clear();
    backend.project_future(scratch);
}

#[derive(Default)]
struct Coverage {
    boundaries: usize,
    operations: usize,
    callbacks: usize,
    samples: usize,
    acceptance_attempts: usize,
    accepted_extensions: usize,
    accepted_retractions: usize,
}

fn check_boundary(backend: &GraphBackend, native: &PlotWorld, scratch: &mut PlotWorld, positions: &[BlockPos], label: &str) -> Result<(), String> {
    project(backend, scratch);
    let expected = future(native, positions);
    let actual = future(scratch, positions);
    first_difference(&expected, &actual).map_or(Ok(()), |difference| Err(format!("{label}: {difference}")))
}

fn payload_bit(world: &PlotWorld, position: BlockPos) -> Option<u16> {
    match world.get_block(position) {
        Block::Air => Some(1),
        Block::RedstoneBlock => Some(0),
        Block::MovingPiston { .. } => None,
        block => panic!("unexpected adder payload output {block:?} at {position:?}"),
    }
}

fn repeater_bit(world: &PlotWorld, position: BlockPos) -> u16 {
    let Block::RedstoneRepeater { repeater } = world.get_block(position) else {
        panic!("adder repeater output missing at {position:?}");
    };
    u16::from(!repeater.powered)
}

fn assert_math_window(circuit: &Adder, world: &PlotWorld, sum: u16, carry: u16, tick: usize) {
    let payload = circuit.port_positions("observations", "sum_payload");
    if (1..=3).contains(&tick) {
        let bits = payload
            .iter()
            .enumerate()
            .try_fold(0, |value, (index, &position)| Some(value | (payload_bit(world, position)? << index)));
        assert_eq!(bits, Some(sum), "{} sum payload at tick {tick}", circuit.id);
    }
    if (3..=7).contains(&tick) {
        let bits = circuit
            .port_positions("observations", "sum_repeater")
            .iter()
            .enumerate()
            .fold(0, |value, (index, &position)| value | (repeater_bit(world, position) << index));
        assert_eq!(bits, sum, "{} sum consumer at tick {tick}", circuit.id);
    }
    if circuit.id == "adder_1bit" && (5..=9).contains(&tick) {
        let position = circuit.port_positions("observations", "carry_repeater")[0];
        assert_eq!(repeater_bit(world, position), carry, "carry at tick {tick}");
    }
}

fn compile_with_instant_pass(circuit: &Adder, world: &PlotWorld, ticks: Vec<TickEntry>) -> Result<GraphBackend, String> {
    let monitor = TaskMonitor::default();
    let mut prepared =
        passes::rich::prepare(world, circuit.bounds(), ticks, &monitor, AnalysisLimits::default()).map_err(|error| error.to_string())?;
    let pipeline = passes::PassPipeline::new(vec![
        passes::PassEntry {
            pass: Pass::rewrite(passes::simplify_instant_resets::SimplifyInstantResets),
            enabled: true,
        },
        passes::PassEntry {
            pass: Pass::rewrite(passes::specialize_instant_pistons::SpecializeInstantPistons),
            enabled: true,
        },
        passes::PassEntry {
            pass: Pass::rewrite(passes::compile_builtin_resets::CompileBuiltinResets),
            enabled: true,
        },
        passes::PassEntry {
            pass: Pass::build(passes::compile_instant_resets::CompileInstantResets),
            enabled: true,
        },
        passes::PassEntry {
            pass: Pass::build(passes::identify_instant_regions::IdentifyInstantRegions),
            enabled: true,
        },
        passes::PassEntry {
            pass: Pass::build(passes::compile_instant_proxies::CompileInstantProxies),
            enabled: true,
        },
    ]);
    // Isolate this rewrite: older O1 passes intentionally erase identity callbacks.
    passes::run_graph_pipeline(&mut prepared.graph, &Default::default(), &prepared.entry, &pipeline, &monitor).map_err(|error| error.to_string())?;
    GraphBackend::from_lowered(prepared.lower(&monitor).map_err(|error| error.to_string())?, &monitor).map_err(|error| error.to_string())
}

fn replay(
    circuit: &Adder,
    steps: &[Step],
    response_start: Option<usize>,
    expected: Option<(u16, u16)>,
    accepted_at: Option<usize>,
) -> Result<(Coverage, GraphExecutionCounts), String> {
    let positions = circuit.positions();
    let mut native = circuit.build();
    let mut scratch = circuit.build();
    let ticks = native.scheduler().iter_entries().collect();
    let mut backend = compile_with_instant_pass(circuit, &native, ticks).map_err(|error| format!("{} compile: {error}", circuit.id))?;
    backend.set_test_recording(true);
    let mut coverage = Coverage::default();
    check_boundary(&backend, &native, &mut scratch, &positions, "initial")?;

    for (index, &step) in steps.iter().enumerate() {
        let toggle = match step {
            Step::SetLever { position, powered } => {
                let Block::Lever { lever } = native.get_block(position) else {
                    panic!("native adder control missing at {position:?}");
                };
                lever.powered != powered
            }
            Step::Tick => false,
        };
        let start = backend.observed_operations.len();
        let entries = capture_at(&positions, || native_step(&mut native, step));
        graph_step(&mut backend, step, toggle).map_err(|error| format!("{} step {index}: {error:?}", circuit.id))?;
        let expected_ops = native_operations(&entries, &positions);
        let actual_ops = graph_operations_since(&backend, start, &positions);
        if expected_ops != actual_ops {
            let first = expected_ops
                .iter()
                .zip(&actual_ops)
                .position(|(native, graph)| native != graph)
                .unwrap_or(expected_ops.len().min(actual_ops.len()));
            let show = |operations: &[Operation]| operations[first.saturating_sub(8)..(first + 8).min(operations.len())].to_vec();
            return Err(format!(
                "{} step {index} {step:?}, operation {first}: native {:?}; graph {:?}",
                circuit.id,
                show(&expected_ops),
                show(&actual_ops)
            ));
        }
        if accepted_at == Some(index) && !actual_ops.iter().any(|operation| matches!(operation, Operation::Accepted { .. })) {
            return Err(format!(
                "{} step {index}: expected an accepted piston action before later inhibition",
                circuit.id
            ));
        }
        super::tests::assert_resource_invariants(&backend);
        coverage.operations += actual_ops.len();
        coverage.callbacks += actual_ops
            .iter()
            .filter(|operation| matches!(operation, Operation::Callback { .. }))
            .count();
        coverage.samples += actual_ops
            .iter()
            .filter(|operation| matches!(operation, Operation::Sample { .. }))
            .count();
        coverage.acceptance_attempts += actual_ops
            .iter()
            .filter(|operation| matches!(operation, Operation::AcceptanceAttempt { .. }))
            .count();
        for operation in &actual_ops {
            match operation {
                Operation::Accepted {
                    action: PistonAction::Extend,
                    ..
                } => coverage.accepted_extensions += 1,
                Operation::Accepted { .. } => coverage.accepted_retractions += 1,
                _ => {}
            }
        }
        check_boundary(&backend, &native, &mut scratch, &positions, &format!("step {index} {step:?}"))?;
        coverage.boundaries += 1;
        if let (Some(response_start), Some((sum, carry))) = (response_start, expected) {
            if index >= response_start {
                assert_math_window(circuit, &native, sum, carry, index - response_start + 1);
            }
        }
    }
    Ok((coverage, backend.execution_counts()))
}

fn expectation(circuit: &Adder, case: &Value) -> (u16, u16) {
    let values: Vec<_> = case["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_u64().unwrap() as u16)
        .collect();
    let total = values.iter().copied().sum::<u16>();
    let expected = (
        case["expectation"]["sum"].as_u64().unwrap() as u16,
        case["expectation"]["carry"].as_u64().unwrap() as u16,
    );
    let width = if circuit.id == "adder_1bit" { 1 } else { 11 };
    let mask = (1 << width) - 1;
    assert_eq!(expected, (total & mask, total >> width), "{} {} arithmetic", circuit.id, case["id"]);
    expected
}

fn assert_case_outputs(circuit: &Adder, case: &Value) -> Result<Coverage, String> {
    let mut steps = case_steps(circuit, case, false, true);
    let response_start = steps.len();
    steps.extend(std::iter::repeat_n(Step::Tick, case["ticks"].as_u64().unwrap() as usize));
    let (coverage, counts) = replay(circuit, &steps, Some(response_start), Some(expectation(circuit, case)), None)?;
    assert!(coverage.callbacks > 0, "{} {} callbacks", circuit.id, case["id"]);
    assert!(coverage.samples > 0, "{} {} samples", circuit.id, case["id"]);
    assert!(counts.ticks > 0 && counts.wire_walks > 0);
    Ok(coverage)
}

#[test]
fn one_bit_all_inputs_and_eleven_bit_manifest_vectors_match_every_graph_boundary() {
    let mut total = BTreeMap::new();
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let cases: Vec<_> = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
            .collect();
        assert_eq!(cases.len(), if id == "adder_1bit" { 8 } else { 40 });
        let mut coverage = Coverage::default();
        for &case in &cases {
            let case_coverage = assert_case_outputs(&circuit, case).unwrap_or_else(|error| panic!("{} {}: {error}", id, case["id"]));
            coverage.boundaries += case_coverage.boundaries;
            coverage.acceptance_attempts += case_coverage.acceptance_attempts;
            coverage.accepted_extensions += case_coverage.accepted_extensions;
            coverage.accepted_retractions += case_coverage.accepted_retractions;
        }
        assert!(coverage.acceptance_attempts > 0, "{id} acceptance attempts");
        assert!(coverage.accepted_extensions > 0, "{id} extensions");
        assert!(coverage.accepted_retractions > 0, "{id} retractions");
        total.insert(id, (cases.len(), coverage.boundaries));
    }
    assert_eq!(total["adder_1bit"].0, 8);
    assert_eq!(total["adder_11bits"].0, 40);
}

#[test]
fn adder_operands_accept_reversed_order_same_tick_cancel_hold_and_rearm() {
    for (id, case_id) in [("adder_1bit", "prepared-1-1-1"), ("adder_11bits", "prepared-1365-682-0")] {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let case = manifest["cases"].as_array().unwrap().iter().find(|case| case["id"] == case_id).unwrap();
        for (name, mut steps) in [
            ("reverse-order", case_steps(&circuit, case, true, true)),
            ("same-tick-cancel", case_steps(&circuit, case, false, false)),
            ("held-rearm", case_steps(&circuit, case, false, false)),
        ] {
            if name == "same-tick-cancel" {
                steps.extend([
                    Step::SetLever {
                        position: circuit.trigger(),
                        powered: false,
                    },
                    Step::SetLever {
                        position: circuit.trigger(),
                        powered: true,
                    },
                ]);
                steps.extend(std::iter::repeat_n(Step::Tick, 12));
            } else if name == "held-rearm" {
                steps.push(Step::SetLever {
                    position: circuit.trigger(),
                    powered: false,
                });
                steps.extend(std::iter::repeat_n(Step::Tick, 20));
                steps.push(Step::SetLever {
                    position: circuit.trigger(),
                    powered: true,
                });
                steps.extend(std::iter::repeat_n(Step::Tick, 8));
                steps.push(Step::SetLever {
                    position: circuit.trigger(),
                    powered: false,
                });
                steps.extend(std::iter::repeat_n(Step::Tick, 24));
            } else {
                steps.extend(std::iter::repeat_n(Step::Tick, 24));
            }
            let (coverage, counts) = replay(&circuit, &steps, None, None, None).unwrap_or_else(|error| panic!("{} {name}: {error}", circuit.id));
            assert!(coverage.operations > 0, "{} {name}", circuit.id);
            assert!(counts.callbacks > 0 && counts.piston_samples > 0);
        }
    }
}

#[test]
fn inhibition_after_acceptance_does_not_cancel_an_adder_action() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-1-0-0")
        .unwrap();
    let mut steps = case_steps(&circuit, case, false, true);
    steps.push(Step::Tick);
    let accepted_at = steps.len() - 1;
    steps.push(Step::SetLever {
        position: circuit.port_positions("inputs", "A")[0],
        powered: true,
    });
    steps.extend(std::iter::repeat_n(Step::Tick, 24));
    let (coverage, _) = replay(&circuit, &steps, None, None, Some(accepted_at)).unwrap_or_else(|error| panic!("late inhibition: {error}"));
    assert!(coverage.accepted_extensions > 0);
}

#[test]
fn translated_rotated_adders_preserve_world_up_qc_and_native_timing() {
    for (id, case_id, anchor, rotation) in [
        ("adder_1bit", "prepared-1-1-1", BlockPos::new(110, 30, 110), RotateAmt::Rotate90),
        ("adder_11bits", "prepared-2047-1-0", BlockPos::new(130, 30, 130), RotateAmt::Rotate270),
    ] {
        let circuit = load_adder(id, anchor, Some(rotation));
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let case = manifest["cases"].as_array().unwrap().iter().find(|case| case["id"] == case_id).unwrap();
        let mut steps = case_steps(&circuit, case, false, true);
        let response_start = steps.len();
        steps.extend(std::iter::repeat_n(Step::Tick, case["ticks"].as_u64().unwrap() as usize));
        replay(&circuit, &steps, Some(response_start), Some(expectation(&circuit, case)), None)
            .unwrap_or_else(|error| panic!("{id} {case_id} rotated: {error}"));
    }
}

fn reset_and_continue(circuit: &Adder, case: &Value, response_tick: usize, expected_active: bool) {
    let mut steps = case_steps(circuit, case, false, true);
    let response_start = steps.len();
    steps.extend(std::iter::repeat_n(Step::Tick, 24));
    let cut = response_start + response_tick;
    let positions = circuit.positions();
    let mut native = circuit.build();
    let mut handed_back = circuit.build();
    let ticks = native.scheduler().iter_entries().collect();
    let mut backend =
        GraphBackend::compile(&native, circuit.bounds(), ticks, Arc::default()).unwrap_or_else(|error| panic!("{} compile: {error}", circuit.id));
    backend.set_test_recording(true);
    let mut scratch = circuit.build();
    for (index, &step) in steps[..cut].iter().enumerate() {
        let toggle = match step {
            Step::SetLever { position, powered } => {
                let Block::Lever { lever } = native.get_block(position) else {
                    panic!("native adder control missing at {position:?}");
                };
                lever.powered != powered
            }
            Step::Tick => false,
        };
        let entries = capture_at(&positions, || native_step(&mut native, step));
        let start = backend.observed_operations.len();
        graph_step(&mut backend, step, toggle).unwrap();
        assert_eq!(
            native_operations(&entries, &positions),
            graph_operations_since(&backend, start, &positions),
            "{} prefix step {index}",
            circuit.id
        );
        check_boundary(&backend, &native, &mut scratch, &positions, "reset prefix").unwrap_or_else(|error| panic!("{error}"));
    }
    let pending = |world: &PlotWorld| {
        !world.piston_state().motions.is_empty() || !world.piston_state().events.is_empty() || !world.piston_state().movement_work.is_empty()
    };
    assert_eq!(pending(&native), expected_active, "{} cut {response_tick}", circuit.id);
    backend.reset(&mut handed_back);
    assert_eq!(
        first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
        None,
        "{} reset at response tick {response_tick}",
        circuit.id
    );
    for _ in 0..256 {
        if !pending(&native) && !pending(&handed_back) {
            break;
        }
        native.tick_interpreted();
        handed_back.tick_interpreted();
        assert_eq!(
            first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
            None,
            "{} interpreter continuation after reset",
            circuit.id
        );
    }
    if !pending(&native) && !pending(&handed_back) {
        let ticks = handed_back.scheduler().iter_entries().collect();
        let mut resumed = GraphBackend::compile(&handed_back, circuit.bounds(), ticks, Arc::default())
            .unwrap_or_else(|error| panic!("{} recompile: {error}", circuit.id));
        for tick in 0..4 {
            native.tick_interpreted();
            resumed.tick().unwrap();
            check_boundary(
                &resumed,
                &native,
                &mut handed_back,
                &positions,
                &format!("recompiled continuation tick {tick}"),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", circuit.id));
        }
    } else {
        println!(
            "{} reset at response tick {response_tick}: interpreter future remained active for 256 ticks",
            circuit.id
        );
    }
}

#[test]
fn graph_reset_at_early_and_late_adder_cuts_hands_back_the_same_future() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let active_case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-1-0-0")
        .unwrap();
    let late_case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    reset_and_continue(&circuit, active_case, 1, true);
    reset_and_continue(&circuit, late_case, 24, false);
}

fn structure(circuit: &Adder) -> String {
    let world = circuit.build();
    let ticks = world.scheduler().iter_entries().collect();
    let backend = GraphBackend::compile(&world, circuit.bounds(), ticks, Arc::default()).unwrap();
    let mut kinds = BTreeMap::<String, usize>::new();
    for node in backend.program.nodes() {
        let name = match &node.kind {
            CircuitNodeKind::Electrical(kind) => format!("electrical:{kind:?}"),
            CircuitNodeKind::Observer { .. } => "observer".into(),
            CircuitNodeKind::BuiltinReset { .. } => "builtin_reset".into(),
            CircuitNodeKind::Wire(_) => "wire".into(),
            CircuitNodeKind::Piston(_) => "piston".into(),
            CircuitNodeKind::Contact(_) => "contact".into(),
        };
        *kinds.entry(name).or_default() += 1;
    }
    let deliveries = backend.program.routes().iter().map(|route| route.deliveries.len()).sum::<usize>();
    let shared: Vec<_> = backend
        .program
        .resources()
        .iter()
        .filter(|resource| resource.contacts.len() > 1)
        .map(|resource| resource.contacts.len())
        .collect();
    let mut pistons_by_z = BTreeMap::<i32, usize>::new();
    for piston in backend.program.pistons() {
        let position = backend.program.nodes()[piston.node.index()].position.unwrap();
        *pistons_by_z.entry(position.z - circuit.anchor.z).or_default() += 1;
    }
    format!(
        "{} sha256={} nodes={} {:?}; profiles={} guards={} sites={} contacts={} pistons={} resources={} shared-resource arities={:?}; wires={} power-links={} routes={} ordered-deliveries={} pistons-per-z={:?}",
        circuit.id,
        circuit.hash,
        backend.program.nodes().len(),
        kinds,
        backend.program.profiles().len(),
        backend.program.guards().len(),
        backend.program.sites().len(),
        backend.program.contacts().len(),
        backend.program.pistons().len(),
        backend.program.resources().len(),
        shared,
        backend.program.wires().len(),
        backend.program.power_links().len(),
        backend.program.routes().len(),
        deliveries,
        pistons_by_z,
    )
}

#[test]
fn lowered_adder_inventory_is_reported_for_optimizer_research() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let report = structure(&circuit);
        println!("{report}");
        assert!(report.contains("piston"));
    }
}
