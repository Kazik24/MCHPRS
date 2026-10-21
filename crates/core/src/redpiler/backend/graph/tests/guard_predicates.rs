//! Compiled guard predicates retain the caller's captured-site context.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::program_graph::{Guard, GuardPredicatePlans, OperandView, SiteTest, StateTest};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstonePiston};

#[test]
fn compiled_guards_preserve_full_single_site_and_missing_capture_contexts() {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(
        BlockPos::new(5, 20, 5),
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
        },
    );
    for x in [6, 7] {
        let pos = BlockPos::new(x, 20, 5);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(
        BlockPos::new(12, 20, 12),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::East.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    let monitor = TaskMonitor::default();
    let mut graph = passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &monitor,
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph;

    let (site_index, site) = graph
        .sites
        .iter()
        .enumerate()
        .find_map(|(index, site)| match site.source {
            SiteSource::Constant(profile) if graph.profiles[profile.index()].solid => Some((index, SiteId::new(index as u32))),
            _ => None,
        })
        .expect("fixture has a captured solid support site");
    let piston = PistonId::new(0);
    let extended = GuardId::try_from_index(graph.guards.len()).unwrap();
    graph.guards.push(Guard::State(StateTest::PistonExtended(piston)));
    let captured_solid = GuardId::try_from_index(graph.guards.len()).unwrap();
    graph.guards.push(Guard::Site {
        site,
        view: OperandView::Captured,
        test: SiteTest::Solid,
    });
    let not_extended = GuardId::try_from_index(graph.guards.len()).unwrap();
    graph.guards.push(Guard::Not(extended));
    let compound = GuardId::try_from_index(graph.guards.len()).unwrap();
    graph.guards.push(Guard::All(vec![not_extended, captured_solid].into_boxed_slice()));
    let negated_capture = GuardId::try_from_index(graph.guards.len()).unwrap();
    graph.guards.push(Guard::Not(captured_solid));
    graph.plans.guard_predicates = GuardPredicatePlans::compile(&graph, &monitor).unwrap();

    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    let observed = backend.observe_site(site).unwrap();
    let mut captures = vec![None; backend.program.sites().len()];
    captures[site_index] = Some(observed.clone());

    assert!(backend.guard_matches(compound, Some(&captures)));
    assert!(backend.guard_matches_for_site(compound, site, &observed));
    assert!(!backend.guard_matches(compound, None));
    assert!(!backend.guard_matches(compound, Some(&vec![None; captures.len()])));
    assert!(!backend.guard_matches(negated_capture, Some(&captures)));
    assert!(
        backend.guard_matches(negated_capture, None),
        "a missing captured site remains false before negation"
    );
    backend.state.pistons[piston.index()].extended = true;
    assert!(!backend.guard_matches(compound, Some(&captures)));
}
