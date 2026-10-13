//! Public activation and reset continuation against uninterrupted native runs.
use super::*;
use crate::redpiler::{BackendKind, Compiler, CompilerOptions, GraphExecutionCounts};
use mchprs_world::{AdvancePhase, TickEntry};

/// Interpreter state read by the next native operation. Raw motion identities
/// are dropped; their order is retained.
#[derive(Debug, PartialEq)]
struct InterpreterFuture {
    tick: u64,
    phase: AdvancePhase,
    cells: Vec<(BlockPos, Block)>,
    entities: Vec<(BlockPos, String)>,
    ticks: Vec<TickEntry>,
    events: Vec<PistonEvent>,
    motions: Vec<(BlockPos, f32, f32, u64)>,
}

fn future(world: &PlotWorld) -> InterpreterFuture {
    let mut cells = Vec::new();
    let mut entities = Vec::new();
    let low = ORIGIN - BlockPos::new(4, 4, 4);
    let high = ORIGIN + BlockPos::new(4, 4, 4);
    for y in low.y..=high.y {
        for z in low.z..=high.z {
            for x in low.x..=high.x {
                let position = BlockPos::new(x, y, z);
                let block = world.get_block(position);
                if block != Block::Air {
                    cells.push((position, block));
                }
                if let Some(entity) = world.get_block_entity(position) {
                    entities.push((position, format!("{entity:?}")));
                }
            }
        }
    }
    let state = world.piston_state();
    InterpreterFuture {
        tick: state.logical_tick,
        phase: state.phase,
        cells,
        entities,
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

/// A host operation available while Redpiler is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Toggle(BlockPos),
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Display {
    Full,
    IoOnly,
    Suppressed,
}

/// Host-legal protocols: player lever use and whole ticks only.
fn host_protocols() -> Vec<Protocol> {
    let normal = Input::Normal;
    let quasi = Input::Quasi;
    let use_input = Delivery::NormalUse;
    let mut protocols: Vec<_> = protocols()
        .into_iter()
        .filter(|protocol| {
            protocol.actions.iter().all(|action| match action {
                Action::SetControl { delivery, .. } => *delivery == use_input,
                Action::Notify { .. } => false,
                Action::Tick { .. } => true,
            })
        })
        .collect();
    protocols.push(Protocol {
        id: "toggle_before_acceptance",
        actions: vec![
            set(normal, true, use_input),
            set(normal, false, use_input),
            tick(4),
        ],
    });
    protocols.push(Protocol {
        id: "quasi_pulse",
        actions: vec![
            set(quasi, true, use_input),
            tick(1),
            set(quasi, false, use_input),
            tick(5),
        ],
    });
    protocols
}

fn steps(fixture: &Fixture, protocol: &Protocol) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut powered = [false, false];
    for action in &protocol.actions {
        match *action {
            Action::SetControl {
                input, powered: on, ..
            } => {
                let (index, position) = match input {
                    Input::Normal => (0, fixture.normal),
                    Input::Quasi => (1, fixture.quasi),
                };
                if powered[index] != on {
                    powered[index] = on;
                    steps.push(Step::Toggle(position));
                }
            }
            Action::Tick { count } => steps.extend((0..count).map(|_| Step::Tick)),
            Action::Notify { .. } => unreachable!("host protocols cannot notify directly"),
        }
    }
    steps
}

fn native_step(world: &mut PlotWorld, step: Step) {
    match step {
        Step::Toggle(position) => {
            let Block::Lever { lever } = world.get_block(position) else {
                panic!("lever missing at {position:?}");
            };
            update_native_lever(world, position, !lever.powered, false);
        }
        Step::Tick => world.tick_interpreted(),
    }
}

fn graph_step(compiler: &mut Compiler, world: &mut PlotWorld, step: Step, display: Display) {
    match step {
        Step::Toggle(position) => compiler.on_use_block(position),
        Step::Tick => compiler.tick().unwrap(),
    }
    if display != Display::Suppressed {
        compiler.flush(world);
    }
}

/// A stationary observer watches the Near cell and drives a lamp, so pending
/// transitions overlap ordinary scheduled observer and lamp ticks.
fn with_observer(mut fixture: Fixture) -> Fixture {
    let side = match fixture.template.facing {
        BlockFacing::Up | BlockFacing::Down => BlockFace::North,
        _ => BlockFace::Top,
    };
    let observer = fixture.near.offset(side);
    fixture.blocks.push((
        observer,
        Block::Observer {
            observer: RedstoneObserver {
                facing: side.opposite().into(),
                powered: false,
            },
        },
    ));
    fixture
        .blocks
        .push((observer.offset(side), Block::RedstoneLamp { lit: false }));
    fixture
}

struct Continuation {
    counts: GraphExecutionCounts,
    pending_motions: usize,
    pending_ticks: usize,
    queued_requests: usize,
}

/// Run `steps[..cut]` through the public compiler, reset, and continue
/// natively, comparing with an uninterrupted native run at every boundary.
fn continue_after_reset(
    fixture: &Fixture,
    steps: &[Step],
    cut: usize,
    display: Display,
) -> Result<Continuation, String> {
    let mut expected = load_fixture(fixture);
    let mut world = load_fixture(fixture);
    let options = CompilerOptions {
        io_only: display == Display::IoOnly,
        ..Default::default()
    };
    let ticks = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, options, ticks, Arc::default())
        .map_err(|error| format!("compile: {error}"))?;
    if compiler.backend_kind() != Some(BackendKind::Graph) {
        return Err(format!("selected {:?}", compiler.backend_kind()));
    }
    world.clear_scheduled_ticks();
    for &step in &steps[..cut] {
        native_step(&mut expected, step);
        graph_step(&mut compiler, &mut world, step, display);
    }
    let counts = compiler.graph_execution_counts().unwrap();
    compiler.reset(&mut world, BOUNDS);
    let at_cut = future(&expected);
    let continuation = Continuation {
        counts,
        pending_motions: at_cut.motions.len(),
        pending_ticks: at_cut.ticks.len(),
        queued_requests: at_cut.events.len(),
    };
    for boundary in cut..=steps.len() {
        let (native, resumed) = (future(&expected), future(&world));
        if native != resumed {
            return Err(format!(
                "cut {cut}, boundary {boundary}: expected {native:?}; resumed {resumed:?}"
            ));
        }
        if let Some(&step) = steps.get(boundary) {
            native_step(&mut expected, step);
            native_step(&mut world, step);
        }
    }
    Ok(continuation)
}

#[derive(Default)]
struct CampaignCoverage {
    runs: usize,
    pending_motion_cuts: usize,
    pending_tick_cuts: usize,
    queued_request_cuts: usize,
    piston_actions: u64,
    callbacks: u64,
}

fn run_campaign(fixtures: &[Fixture], display: Display) -> (CampaignCoverage, Vec<String>) {
    let mut coverage = CampaignCoverage::default();
    let mut failures = Vec::new();
    for fixture in fixtures {
        for protocol in host_protocols() {
            let steps = steps(fixture, &protocol);
            for cut in 0..=steps.len() {
                match continue_after_reset(fixture, &steps, cut, display) {
                    Ok(result) => {
                        coverage.runs += 1;
                        coverage.pending_motion_cuts += usize::from(result.pending_motions > 0);
                        coverage.pending_tick_cuts += usize::from(result.pending_ticks > 0);
                        coverage.queued_request_cuts += usize::from(result.queued_requests > 0);
                        coverage.piston_actions += result.counts.piston_actions;
                        coverage.callbacks += result.counts.callbacks;
                    }
                    Err(error) => {
                        failures.push(format!("{:?}/{}: {error}", fixture.template, protocol.id))
                    }
                }
            }
        }
    }
    (coverage, failures)
}

fn report(name: &str, coverage: &CampaignCoverage, failures: &[String]) {
    eprintln!(
        "{name}: runs={}, failures={}, cuts with pending motions={}, with pending ticks={}, with queued requests={}, graph piston actions={}, graph callbacks={}",
        coverage.runs,
        failures.len(),
        coverage.pending_motion_cuts,
        coverage.pending_tick_cuts,
        coverage.queued_request_cuts,
        coverage.piston_actions,
        coverage.callbacks,
    );
    assert!(failures.is_empty(), "first: {}", failures[0]);
    assert!(coverage.pending_motion_cuts > 0 && coverage.piston_actions > 0);
}

#[test]
fn reset_at_every_host_boundary_continues_like_uninterrupted_native_execution() {
    let fixtures: Vec<_> = templates().into_iter().map(fixture).collect();
    let (coverage, failures) = run_campaign(&fixtures, Display::Full);
    report("continuation", &coverage, &failures);
}

#[test]
fn reset_preserves_ordinary_scheduled_work_beside_pending_transitions() {
    let fixtures: Vec<_> = templates()
        .into_iter()
        .map(|template| with_observer(fixture(template)))
        .collect();
    let (coverage, failures) = run_campaign(&fixtures, Display::Full);
    report("observer continuation", &coverage, &failures);
    assert!(coverage.pending_tick_cuts > 0);
}

#[test]
fn suppressed_display_does_not_change_execution_or_continuation() {
    let fixtures: Vec<_> = templates()
        .into_iter()
        .filter(|template| {
            template.facing == BlockFacing::East || template.facing == BlockFacing::Up
        })
        .map(|template| with_observer(fixture(template)))
        .collect();
    for display in [Display::IoOnly, Display::Suppressed] {
        let (coverage, failures) = run_campaign(&fixtures, display);
        report(&format!("{display:?} continuation"), &coverage, &failures);
    }
    // Identical counts with and without display writes.
    for fixture in &fixtures {
        for protocol in host_protocols() {
            let steps = steps(fixture, &protocol);
            let counts = [Display::Full, Display::IoOnly, Display::Suppressed].map(|display| {
                continue_after_reset(fixture, &steps, steps.len(), display)
                    .unwrap()
                    .counts
            });
            assert_eq!(
                counts[0], counts[1],
                "{:?}/{}",
                fixture.template, protocol.id
            );
            assert_eq!(
                counts[0], counts[2],
                "{:?}/{}",
                fixture.template, protocol.id
            );
        }
    }
}

#[test]
fn pending_extension_projects_motions_from_logical_state_only() {
    let fixture = fixture(Template {
        form: ActorForm::StickyRedstone,
        facing: BlockFacing::East,
        extended: false,
    });
    let mut expected = load_fixture(&fixture);
    let mut world = load_fixture(&fixture);
    let mut backend = GraphBackend::compile(&world, BOUNDS, vec![], Arc::default()).unwrap();
    native_step(&mut expected, Step::Toggle(fixture.normal));
    backend.use_block(fixture.normal).unwrap();
    native_step(&mut expected, Step::Tick);
    backend.tick().unwrap();

    let (_, piston) = graph_piston(&backend, fixture.base);
    let resource = backend.state.resources[0];
    let event = backend.deferred_pistons[0];
    eprintln!(
        "runtime: tick={} piston={piston:?} resource={resource:?} deferred={event:?}",
        backend.clock.logical_tick
    );
    assert_eq!(piston.availability, PistonAvailability::HeadPending);
    assert!(!resource.enabled);
    assert_eq!(event.due_tick, backend.clock.logical_tick + 2);

    backend.reset(&mut world);
    let projected = future(&world);
    eprintln!(
        "projection: motions={:?} entities={:?}",
        projected.motions, projected.entities
    );
    assert_eq!(projected, future(&expected));
    for _ in 0..4 {
        native_step(&mut expected, Step::Tick);
        native_step(&mut world, Step::Tick);
        assert_eq!(future(&world), future(&expected));
    }
}
