//! Active TEST-family episodes: the native interpreter against rich-graph
//! execution, compared at every host boundary through the reset projection.
use super::fuzz_tests::{Operation, graph_operations_since, native_operations};
use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::{BackendKind, Compiler, CompilerOptions, GraphExecutionCounts};
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::storage::Chunk;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::{Lever, LeverFace, RotateAmt};
use mchprs_world::{AdvancePhase, PistonAction};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const SOURCE: &str = "test_data/piston-research/test-prefix-20261008";
const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const FAMILY: [&str; 13] = [
    "TEST_BUD_1",
    "TEST_BUD_2",
    "TEST_CODER_1",
    "TEST_DEC",
    "TEST_DEC2",
    "TEST_INSTANT_3",
    "TEST_OR1",
    "TEST_OR2",
    "TEST_OR_3",
    "TEST_PISTION",
    "TEST_PISTION_2",
    "TEST_XOR",
    "TEST_XOR2",
];
const PULSE_WIDTHS: [usize; 6] = [1, 2, 3, 4, 6, 8];
const SETTLE: usize = 12;

fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(SOURCE)
}

fn json(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(source().join(name)).unwrap()).unwrap()
}

fn local(value: &Value) -> BlockPos {
    BlockPos::new(
        value["x"].as_i64().unwrap() as i32,
        value["y"].as_i64().unwrap() as i32,
        value["z"].as_i64().unwrap() as i32,
    )
}

/// One pinned fixture with its controls and observation points in world
/// coordinates for a given horizontal rotation.
struct Circuit {
    name: &'static str,
    sha256: String,
    rotation: Option<RotateAmt>,
    /// Saved fixture cells with controls already placed, unrotated, local.
    cells: Vec<(BlockPos, Block, Option<BlockEntity>)>,
    size: BlockPos,
    controls: Vec<BlockPos>,
    added_controls: usize,
    /// Support blocks supplied under bottom-layer dust or attachments.
    floor: usize,
    outputs: Vec<BlockPos>,
}

fn load_circuit(name: &'static str) -> Circuit {
    let manifest = json("download-manifest.json");
    let file = format!("{name}.schem");
    let entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == file.as_str())
        .expect("fixture belongs to the pinned manifest");
    let bytes = std::fs::read(source().join(&file)).unwrap();
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(entry["bytes"].as_u64().unwrap() as usize, bytes.len());
    assert_eq!(entry["sha256"], sha256.as_str(), "{name} hash");
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let size = BlockPos::new(
        clipboard.size_x as i32,
        clipboard.size_y as i32,
        clipboard.size_z as i32,
    );
    let mut world = empty_world();
    paste_clipboard(
        &mut world,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );

    // Controls and observation points recorded by the earlier TEST research.
    let summary = json("output-summary.json");
    let record = summary["schematics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["name"] == file.as_str())
        .expect("fixture has recorded observations");
    let added: Vec<_> = record["added_controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(local)
        .collect();
    for &position in &added {
        // The added lever drives the saved torch support exactly as the
        // levered fixtures do: a wall lever on the support's north face.
        let support = ORIGIN + position.offset(BlockFace::South);
        assert!(
            world.get_block(support).is_solid(),
            "{name}: added control at {position:?} needs a solid support"
        );
        world.set_block(
            ORIGIN + position,
            Block::Lever {
                lever: Lever::new(LeverFace::Wall, BlockDirection::North, false),
            },
        );
    }
    // The saved selections omit the ground: give bottom-layer dust and
    // attachments the stone support they had in the source world.
    let mut floor = 0;
    for z in 0..size.z {
        for x in 0..size.x {
            let position = ORIGIN + BlockPos::new(x, 0, z);
            let block = world.get_block(position);
            let below = position.offset(BlockFace::Bottom);
            if crate::interaction::attachment_support(block, position)
                .is_some_and(|(support, _)| support == below)
                && world.get_block(below) == Block::Air
            {
                world.set_block(below, Block::Stone {});
                floor += 1;
            }
        }
    }
    let low = BlockPos::new(0, -1, -1);
    let high = size - BlockPos::new(1, 1, 1);
    let mut cells = Vec::new();
    let mut controls = Vec::new();
    for y in low.y..=high.y {
        for z in low.z..=high.z {
            for x in low.x..=high.x {
                let position = BlockPos::new(x, y, z);
                let block = world.get_block(ORIGIN + position);
                if matches!(block, Block::Lever { .. }) {
                    controls.push(position);
                }
                if block != Block::Air {
                    cells.push((
                        position,
                        block,
                        world.get_block_entity(ORIGIN + position).cloned(),
                    ));
                }
            }
        }
    }
    controls.sort_by_key(|position| (position.x, position.y, position.z));
    Circuit {
        name,
        sha256,
        rotation: None,
        cells,
        size,
        controls,
        added_controls: added.len(),
        floor,
        outputs: record["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(local)
            .collect(),
    }
}

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}

impl Circuit {
    fn rotated(&self, rotation: RotateAmt) -> Circuit {
        Circuit {
            name: self.name,
            sha256: self.sha256.clone(),
            rotation: Some(rotation),
            cells: self.cells.clone(),
            size: self.size,
            controls: self.controls.clone(),
            added_controls: self.added_controls,
            floor: self.floor,
            outputs: self.outputs.clone(),
        }
    }

    /// World coordinates of a local cell under this circuit's rotation,
    /// turning about the local origin column. Rotation keeps world-up.
    fn place(&self, local: BlockPos) -> BlockPos {
        let rotated = match self.rotation {
            None => local,
            Some(RotateAmt::Rotate90) => BlockPos::new(-local.z, local.y, local.x),
            Some(RotateAmt::Rotate180) => BlockPos::new(-local.x, local.y, -local.z),
            Some(RotateAmt::Rotate270) => BlockPos::new(local.z, local.y, -local.x),
        };
        ORIGIN + rotated
    }

    fn label(&self) -> String {
        match self.rotation {
            None => self.name.to_owned(),
            Some(rotation) => format!("{}@{rotation:?}", self.name),
        }
    }

    fn build(&self) -> PlotWorld {
        let mut world = empty_world();
        for (position, block, entity) in &self.cells {
            let mut block = *block;
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
        let corners = [
            BlockPos::new(0, -1, -1),
            BlockPos::new(self.size.x - 1, self.size.y - 1, self.size.z - 1),
            BlockPos::new(0, self.size.y - 1, self.size.z - 1),
            BlockPos::new(self.size.x - 1, -1, -1),
        ]
        .map(|corner| self.place(corner));
        let low = corners.iter().copied().reduce(BlockPos::min).unwrap();
        let high = corners.iter().copied().reduce(BlockPos::max).unwrap();
        (low - BlockPos::new(1, 1, 1), high + BlockPos::new(1, 1, 1))
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

    fn control(&self, index: usize) -> BlockPos {
        self.place(self.controls[index])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Toggle(usize),
    Tick,
}

struct Episode {
    id: String,
    steps: Vec<Step>,
}

fn ticks(count: usize) -> impl Iterator<Item = Step> {
    (0..count).map(|_| Step::Tick)
}

/// Reused TEST research protocols (lever masks and one-control pulses) plus
/// reversed actor order, same-tick cancellation, rearm, and long holds.
fn episodes(controls: usize) -> Vec<Episode> {
    let mut episodes = Vec::new();
    for mask in 0..1usize << controls {
        let steps = (0..controls)
            .filter(|index| mask & (1 << index) != 0)
            .map(Step::Toggle)
            .chain(ticks(SETTLE))
            .collect();
        episodes.push(Episode {
            id: format!("mask_{mask}"),
            steps,
        });
    }
    if controls == 2 {
        episodes.push(Episode {
            id: "mask_3_reversed_order".into(),
            steps: [Step::Toggle(1), Step::Toggle(0)]
                .into_iter()
                .chain(ticks(SETTLE))
                .collect(),
        });
        episodes.push(Episode {
            id: "staggered_both".into(),
            steps: std::iter::once(Step::Toggle(0))
                .chain(ticks(1))
                .chain([Step::Toggle(1)])
                .chain(ticks(3))
                .chain([Step::Toggle(0)])
                .chain(ticks(1))
                .chain([Step::Toggle(1)])
                .chain(ticks(SETTLE))
                .collect(),
        });
    }
    for control in 0..controls {
        for width in PULSE_WIDTHS {
            episodes.push(Episode {
                id: format!("lever_{control}_pulse_{width}"),
                steps: ticks(4)
                    .chain([Step::Toggle(control)])
                    .chain(ticks(width))
                    .chain([Step::Toggle(control)])
                    .chain(ticks(SETTLE))
                    .collect(),
            });
        }
        episodes.push(Episode {
            id: format!("lever_{control}_same_tick_cancel"),
            steps: [Step::Toggle(control), Step::Toggle(control)]
                .into_iter()
                .chain(ticks(SETTLE))
                .collect(),
        });
        episodes.push(Episode {
            id: format!("lever_{control}_rearm"),
            steps: [Step::Toggle(control)]
                .into_iter()
                .chain(ticks(2))
                .chain([Step::Toggle(control)])
                .chain(ticks(SETTLE))
                .chain([Step::Toggle(control)])
                .chain(ticks(2))
                .chain([Step::Toggle(control)])
                .chain(ticks(SETTLE))
                .collect(),
        });
        episodes.push(Episode {
            id: format!("lever_{control}_held"),
            steps: [Step::Toggle(control)]
                .into_iter()
                .chain(ticks(20))
                .chain([Step::Toggle(control)])
                .chain(ticks(20))
                .collect(),
        });
    }
    episodes
}

/// Interpreter state read by the next operation inside the fixture region,
/// with raw motion identities replaced by their order.
#[derive(Debug, PartialEq)]
struct Future {
    tick: u64,
    phase: AdvancePhase,
    cells: Vec<(BlockPos, Block)>,
    entities: Vec<(BlockPos, String)>,
    ticks: Vec<TickEntry>,
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
            .filter_map(|&position| {
                world
                    .get_block_entity(position)
                    .map(|entity| (position, format!("{entity:?}")))
            })
            .collect(),
        ticks: world.scheduler().iter_entries().collect(),
        events: state.events.iter().copied().collect(),
        motions: state
            .motions
            .iter()
            .map(|motion| {
                (
                    motion.pos,
                    motion.progress,
                    motion.previous_progress,
                    motion.last_tick,
                )
            })
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
    if expected.cells != actual.cells {
        let mismatch: Vec<_> = expected
            .cells
            .iter()
            .filter(|cell| !actual.cells.contains(cell))
            .chain(
                actual
                    .cells
                    .iter()
                    .filter(|cell| !expected.cells.contains(cell)),
            )
            .collect();
        return Some(format!("cells: {mismatch:?}"));
    }
    differs!(entities);
    differs!(ticks);
    differs!(events);
    differs!(motions);
    None
}

fn native_step(world: &mut PlotWorld, circuit: &Circuit, step: Step) {
    match step {
        Step::Toggle(index) => {
            let position = circuit.control(index);
            let Block::Lever { mut lever } = world.get_block(position) else {
                panic!("{}: control {index} missing", circuit.label());
            };
            lever.powered = !lever.powered;
            world.set_block(position, Block::Lever { lever });
            redstone::update_surrounding_blocks(world, position);
            let support = match lever.face {
                LeverFace::Floor => BlockFace::Bottom,
                LeverFace::Ceiling => BlockFace::Top,
                LeverFace::Wall => lever.facing.opposite().block_face(),
            };
            redstone::update_surrounding_blocks(world, position.offset(support));
        }
        Step::Tick => world.tick_interpreted(),
    }
}

/// Write the graph's interpreter-equivalent future into a scratch world.
fn project(backend: &GraphBackend, scratch: &mut PlotWorld) {
    scratch.clear_scheduled_ticks();
    scratch.piston_state_mut().motions.clear();
    backend.project_future(scratch);
}

#[derive(Default, Clone, Copy)]
struct Coverage {
    boundaries: usize,
    pending_transition_boundaries: usize,
    pending_tick_boundaries: usize,
    output_changes: usize,
    accepted_extensions: usize,
    accepted_retractions: usize,
    cancelled_attempts: usize,
}

/// Lockstep native and graph replay. Each boundary compares ordered
/// callbacks, samples, requests, acceptance, and the projected future.
fn replay(
    circuit: &Circuit,
    episode: &Episode,
) -> Result<(Coverage, GraphExecutionCounts), String> {
    let positions = circuit.positions();
    let outputs: Vec<_> = circuit.outputs.iter().map(|&o| circuit.place(o)).collect();
    let mut native = circuit.build();
    let mut scratch = circuit.build();
    let ticks = native.scheduler().iter_entries().collect();
    let mut backend = GraphBackend::compile(&native, circuit.bounds(), ticks, Arc::default())
        .map_err(|error| format!("compile: {error}"))?;
    backend.set_test_recording(true);
    let mut coverage = Coverage::default();
    let mut last_outputs: Vec<_> = outputs.iter().map(|&o| native.get_block(o)).collect();
    for (index, &step) in episode.steps.iter().enumerate() {
        let start = backend.observed_operations.len();
        let entries = capture_at(&positions, || native_step(&mut native, circuit, step));
        match step {
            Step::Toggle(control) => backend.use_block(circuit.control(control)),
            Step::Tick => backend.tick(),
        }
        .map_err(|error| format!("step {index}: {error}"))?;
        let expected_ops = native_operations(&entries, &positions);
        let actual_ops = graph_operations_since(&backend, start, &positions);
        if expected_ops != actual_ops {
            let first = expected_ops
                .iter()
                .zip(&actual_ops)
                .position(|(a, b)| a != b)
                .unwrap_or(expected_ops.len().min(actual_ops.len()));
            let window = |ops: &[Operation]| {
                ops[first.saturating_sub(8)..(first + 8).min(ops.len())].to_vec()
            };
            return Err(format!(
                "step {index} {step:?}: operation {first}: native {:?}; graph {:?}",
                window(&expected_ops),
                window(&actual_ops),
            ));
        }
        for operation in &actual_ops {
            match operation {
                Operation::Accepted {
                    action: PistonAction::Extend,
                    ..
                } => coverage.accepted_extensions += 1,
                Operation::Accepted { .. } => coverage.accepted_retractions += 1,
                Operation::AcceptanceAttempt { .. } => coverage.cancelled_attempts += 1,
                _ => {}
            }
        }
        project(&backend, &mut scratch);
        let expected = future(&native, &positions);
        let actual = future(&scratch, &positions);
        if let Some(difference) = first_difference(&expected, &actual) {
            return Err(format!("step {index} {step:?}: {difference}"));
        }
        coverage.boundaries += 1;
        coverage.pending_transition_boundaries += usize::from(!expected.motions.is_empty());
        coverage.pending_tick_boundaries += usize::from(!expected.ticks.is_empty());
        let current: Vec<_> = outputs.iter().map(|&o| native.get_block(o)).collect();
        coverage.output_changes += current
            .iter()
            .zip(&last_outputs)
            .filter(|(a, b)| a != b)
            .count();
        last_outputs = current;
    }
    coverage.cancelled_attempts -= coverage.accepted_extensions + coverage.accepted_retractions;
    Ok((coverage, backend.execution_counts()))
}

/// Public activation: compile, run a prefix, reset during whatever is
/// pending, let the interpreter continue, recompile once stationary, run
/// again, reset, and compare with an uninterrupted native run.
fn stop_and_restart(circuit: &Circuit, episode: &Episode, cut: usize) -> Result<bool, String> {
    let positions = circuit.positions();
    let mut native = circuit.build();
    let mut world = circuit.build();
    let compile = |compiler: &mut Compiler, world: &mut PlotWorld| -> Result<(), String> {
        let ticks = world.scheduler().iter_entries().collect();
        let corners = world.get_corners();
        compiler
            .compile(
                world,
                corners,
                CompilerOptions::default(),
                ticks,
                Arc::default(),
            )
            .map_err(|error| format!("compile: {error}"))?;
        if compiler.backend_kind() != Some(BackendKind::Graph) {
            return Err(format!("selected {:?}", compiler.backend_kind()));
        }
        world.clear_scheduled_ticks();
        Ok(())
    };
    let mut compiler = Compiler::default();
    compile(&mut compiler, &mut world)?;
    let corners = world.get_corners();
    let mut index = 0;
    while index < cut {
        native_step(&mut native, circuit, episode.steps[index]);
        match episode.steps[index] {
            Step::Toggle(control) => compiler.on_use_block(circuit.control(control)),
            Step::Tick => compiler.tick(),
        }
        compiler.flush(&mut world);
        index += 1;
    }
    compiler.reset(&mut world, corners);
    let pending = !world.piston_state().motions.is_empty();
    let compare = |native: &PlotWorld, world: &PlotWorld, at: &str| {
        first_difference(&future(native, &positions), &future(world, &positions))
            .map(|difference| format!("{at}: {difference}"))
    };
    if let Some(error) = compare(&native, &world, &format!("reset at {cut}")) {
        return Err(error);
    }
    // The interpreter owns the circuit until it is stationary again.
    while index < episode.steps.len()
        && (!world.piston_state().motions.is_empty() || !world.piston_state().events.is_empty())
    {
        native_step(&mut native, circuit, episode.steps[index]);
        native_step(&mut world, circuit, episode.steps[index]);
        index += 1;
    }
    if index < episode.steps.len() {
        compile(&mut compiler, &mut world)?;
        while index < episode.steps.len() {
            native_step(&mut native, circuit, episode.steps[index]);
            match episode.steps[index] {
                Step::Toggle(control) => compiler.on_use_block(circuit.control(control)),
                Step::Tick => compiler.tick(),
            }
            index += 1;
        }
        compiler.reset(&mut world, corners);
    }
    if let Some(error) = compare(&native, &world, "after restart") {
        return Err(error);
    }
    Ok(pending)
}

struct FamilyRow {
    label: String,
    sha256: String,
    stimulus: String,
    episodes: usize,
    coverage: Coverage,
    counts: GraphExecutionCounts,
    waveform: String,
    continuation: String,
}

fn run_circuit(circuit: &Circuit, continuation_cuts: bool) -> Result<FamilyRow, String> {
    let episodes = episodes(circuit.controls.len());
    let mut coverage = Coverage::default();
    let mut counts = GraphExecutionCounts::default();
    for episode in &episodes {
        let (episode_coverage, episode_counts) = replay(circuit, episode)
            .map_err(|error| format!("{} {}: {error}", circuit.label(), episode.id))?;
        coverage.boundaries += episode_coverage.boundaries;
        coverage.pending_transition_boundaries += episode_coverage.pending_transition_boundaries;
        coverage.pending_tick_boundaries += episode_coverage.pending_tick_boundaries;
        coverage.output_changes += episode_coverage.output_changes;
        coverage.accepted_extensions += episode_coverage.accepted_extensions;
        coverage.accepted_retractions += episode_coverage.accepted_retractions;
        coverage.cancelled_attempts += episode_coverage.cancelled_attempts;
        counts.ticks += episode_counts.ticks;
        counts.scheduled_ticks += episode_counts.scheduled_ticks;
        counts.callbacks += episode_counts.callbacks;
        counts.wire_walks += episode_counts.wire_walks;
        counts.piston_samples += episode_counts.piston_samples;
        counts.piston_actions += episode_counts.piston_actions;
        counts.deferred_publications += episode_counts.deferred_publications;
    }

    // Output waveform of the two-tick pulse on control 0, native and graph agree.
    let pulse = episodes
        .iter()
        .find(|episode| episode.id == "lever_0_pulse_2")
        .unwrap();
    let mut world = circuit.build();
    let outputs: Vec<_> = circuit.outputs.iter().map(|&o| circuit.place(o)).collect();
    let mut waveform = String::new();
    for &step in &pulse.steps {
        native_step(&mut world, circuit, step);
        if step == Step::Tick {
            let powered = outputs
                .iter()
                .filter(|&&o| output_on(world.get_block(o)))
                .count();
            waveform.push(char::from_digit(powered as u32, 10).unwrap_or('+'));
        }
    }

    let mut checked = 0;
    let mut pending = 0;
    if continuation_cuts {
        for episode in episodes.iter().filter(|e| {
            e.id.ends_with("pulse_2") || e.id.ends_with("rearm") || e.id.starts_with("mask")
        }) {
            for cut in 0..=episode.steps.len() {
                pending +=
                    usize::from(stop_and_restart(circuit, episode, cut).map_err(|error| {
                        format!("{} {} cut {cut}: {error}", circuit.label(), episode.id)
                    })?);
                checked += 1;
            }
        }
    }
    let floor = if circuit.floor > 0 {
        format!("; {} floor supports added", circuit.floor)
    } else {
        String::new()
    };
    let stimulus = if circuit.added_controls > 0 {
        format!(
            "{} added wall levers on torch supports {:?}",
            circuit.added_controls, circuit.controls
        )
    } else {
        format!(
            "{} saved levers {:?}",
            circuit.controls.len(),
            circuit.controls
        )
    } + &floor;
    Ok(FamilyRow {
        label: circuit.label(),
        sha256: circuit.sha256.clone(),
        stimulus,
        episodes: episodes.len(),
        coverage,
        counts,
        waveform,
        continuation: format!("{checked} stop/restart cuts, {pending} during pending transitions"),
    })
}

/// Visible state of an observation point.
fn output_on(block: Block) -> bool {
    match block {
        Block::RedstoneRepeater { repeater } => repeater.powered,
        Block::RedstoneComparator { comparator } => comparator.powered,
        Block::RedstoneLamp { lit } => lit,
        Block::RedstoneWire { wire } => wire.power > 0,
        _ => block.copper_bulb_state().is_some_and(|(lit, _)| lit),
    }
}

fn print_row(row: &FamilyRow) {
    let c = &row.coverage;
    let n = &row.counts;
    eprintln!(
        "| {} | {} | {} | graph | {} episodes, {} boundaries | ticks={} scheduled={} callbacks={} wire_walks={} samples={} accepted={} (ext={} ret={}) cancelled={} deferred={} | output changes={}, pulse_2 waveform {} | pending-transition boundaries={}, pending-tick boundaries={}; {} |",
        row.label,
        &row.sha256[..16],
        row.stimulus,
        row.episodes,
        c.boundaries,
        n.ticks,
        n.scheduled_ticks,
        n.callbacks,
        n.wire_walks,
        n.piston_samples,
        n.piston_actions,
        c.accepted_extensions,
        c.accepted_retractions,
        c.cancelled_attempts,
        n.deferred_publications,
        c.output_changes,
        row.waveform,
        c.pending_transition_boundaries,
        c.pending_tick_boundaries,
        row.continuation,
    );
}

fn assert_meaningful(row: &FamilyRow) {
    let c = &row.coverage;
    let n = &row.counts;
    assert!(
        n.piston_actions > 0 && n.piston_samples > 0,
        "{}: no piston work",
        row.label
    );
    assert!(
        n.callbacks > 0 && n.ticks > 0,
        "{}: no graph callbacks",
        row.label
    );
    assert!(
        c.accepted_extensions > 0 && c.accepted_retractions > 0,
        "{}",
        row.label
    );
    assert!(
        c.pending_transition_boundaries > 0,
        "{}: no pending transition",
        row.label
    );
    assert!(c.output_changes > 0, "{}: outputs never changed", row.label);
}

#[test]
fn test_family_matches_native_under_active_controls() {
    let mut failures = Vec::new();
    for name in FAMILY {
        let circuit = load_circuit(name);
        assert!(!circuit.controls.is_empty(), "{name} has no control");
        match run_circuit(&circuit, true) {
            Ok(row) => {
                print_row(&row);
                assert_meaningful(&row);
            }
            Err(error) => {
                eprintln!("FAIL {error}");
                failures.push(error);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures; first: {}",
        failures.len(),
        failures[0]
    );
}

#[test]
fn test_family_rotations_match_native() {
    let mut failures = Vec::new();
    for name in FAMILY {
        let circuit = load_circuit(name);
        for rotation in [
            RotateAmt::Rotate90,
            RotateAmt::Rotate180,
            RotateAmt::Rotate270,
        ] {
            match run_circuit(&circuit.rotated(rotation), false) {
                Ok(row) => {
                    print_row(&row);
                    assert_meaningful(&row);
                }
                Err(error) => {
                    eprintln!("FAIL {error}");
                    failures.push(error);
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures; first: {}",
        failures.len(),
        failures[0]
    );
}

#[test]
fn piston_beside_already_unsupported_dust_is_rejected_with_its_position() {
    let base = BlockPos::new(8, 24, 8);
    let stone = base.offset(BlockFace::Bottom).offset(BlockFace::Bottom);
    let dust = stone.offset(BlockFace::Bottom);
    let build = |floor: bool| {
        let mut world = empty_world();
        let piston = mchprs_blocks::blocks::RedstonePiston {
            facing: mchprs_blocks::BlockFacing::Down,
            sticky: true,
            extended: true,
        };
        world.set_block(base, Block::Piston { piston });
        world.set_block(
            base.offset(BlockFace::Bottom),
            Block::PistonHead {
                head: piston.into(),
            },
        );
        world.set_block(stone, Block::Stone {});
        world.set_block(
            dust,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        if floor {
            world.set_block(dust.offset(BlockFace::Bottom), Block::Stone {});
        }
        world
    };
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 63, 15));
    let error = GraphBackend::compile(&build(false), bounds, vec![], Arc::default())
        .err()
        .expect("floating dust beside a moving resource must be rejected");
    eprintln!("witness: {error}");
    assert!(matches!(
        error,
        GraphBackendError::Geometry(geometry::GeometryError::Rejected(
            geometry::GeometryIssue::SupportLoss { dependent, .. }
        )) if dependent == dust
    ));
    // The stone itself is an admitted inert resource once the dust is supported.
    assert!(GraphBackend::compile(&build(true), bounds, vec![], Arc::default()).is_ok());
}
