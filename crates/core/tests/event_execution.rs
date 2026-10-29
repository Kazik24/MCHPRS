use mchprs_blocks::{
    BlockDirection, BlockFacing, BlockPos,
    blocks::{Block, Lever, LeverFace, RedstonePiston, RedstoneWire, RedstoneWireSide},
};
use mchprs_core::plot::PlotWorld;
use mchprs_core::redpiler::{BackendKind, Compiler, CompilerOptions};
use mchprs_core::world::{World, storage::Chunk};
use mchprs_world::{TickEntry, TickPriority};

struct RefuseBlockWrite<'a> {
    world: &'a mut PlotWorld,
    position: Option<BlockPos>,
    reject_after: Option<usize>,
    successes: usize,
    rejected_after: Option<usize>,
    persistent: bool,
}

impl World for RefuseBlockWrite<'_> {
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        self.world.get_block_raw(pos)
    }
    fn set_block_raw(&mut self, pos: BlockPos, block: u32) -> bool {
        if self.position == Some(pos) || (self.reject_after == Some(self.successes) && (self.persistent || self.rejected_after.is_none())) {
            self.rejected_after = Some(self.successes);
            false
        } else {
            let changed = self.world.set_block_raw(pos, block);
            if changed {
                self.successes += 1;
            }
            changed
        }
    }
    fn delete_block_entity(&mut self, pos: BlockPos) {
        self.world.delete_block_entity(pos);
    }
    fn get_block_entity(&self, pos: BlockPos) -> Option<&mchprs_blocks::block_entities::BlockEntity> {
        self.world.get_block_entity(pos)
    }
    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut mchprs_blocks::block_entities::BlockEntity> {
        self.world.get_block_entity_mut(pos)
    }
    fn set_block_entity(&mut self, pos: BlockPos, entity: mchprs_blocks::block_entities::BlockEntity) {
        self.world.set_block_entity(pos, entity);
    }
    fn piston_state(&self) -> &mchprs_world::PistonState {
        self.world.piston_state()
    }
    fn piston_state_mut(&mut self) -> &mut mchprs_world::PistonState {
        self.world.piston_state_mut()
    }
    fn get_chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        self.world.get_chunk(x, z)
    }
    fn get_chunk_mut(&mut self, x: i32, z: i32) -> Option<&mut Chunk> {
        self.world.get_chunk_mut(x, z)
    }
    fn schedule_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.world.schedule_tick(pos, delay, priority);
    }
    fn schedule_half_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.world.schedule_half_tick(pos, delay, priority);
    }
    fn pending_tick_at(&mut self, pos: BlockPos) -> bool {
        self.world.pending_tick_at(pos)
    }
    fn block_action(&mut self, pos: BlockPos, action: mchprs_core::world::BlockAction) {
        self.world.block_action(pos, action);
    }
    fn play_sound(&mut self, pos: BlockPos, id: i32, category: i32, volume: f32, pitch: f32) {
        self.world.play_sound(pos, id, category, volume, pitch);
    }
}

const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
const LEVER: BlockPos = BlockPos::new(3, 20, 5);
const WIRE: BlockPos = BlockPos::new(4, 20, 5);
const PISTON: BlockPos = BlockPos::new(5, 20, 5);
const NEAR: BlockPos = BlockPos::new(6, 20, 5);
const FAR: BlockPos = BlockPos::new(7, 20, 5);
const LAMP: BlockPos = BlockPos::new(8, 20, 5);
const OUTSIDE: BlockPos = BlockPos::new(4, 32, 5);

fn fixture() -> PlotWorld {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(BlockPos::new(3, 19, 5), Block::Stone {});
    world.set_block(BlockPos::new(4, 19, 5), Block::Stone {});
    world.set_block(
        LEVER,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::East, false),
        },
    );
    world.set_block(
        WIRE,
        Block::RedstoneWire {
            wire: RedstoneWire {
                east: RedstoneWireSide::Side,
                west: RedstoneWireSide::Side,
                ..Default::default()
            },
        },
    );
    world.set_block(
        PISTON,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(NEAR, Block::RedstoneBlock);
    world.set_block(LAMP, Block::RedstoneLamp { lit: false });
    world.set_block(OUTSIDE, Block::Stone {});
    world.restore_scheduled_tick(TickEntry {
        pos: OUTSIDE,
        ticks_left: 3,
        tick_priority: TickPriority::Normal,
        block_type: None,
    });
    world
}

fn event_options() -> CompilerOptions {
    CompilerOptions {
        experimental_event: true,
        ..Default::default()
    }
}

#[test]
fn public_compiler_runs_event_fixture_and_restores_on_stop() {
    let mut world = fixture();
    let outside_ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), outside_ticks.clone(), Default::default())
        .unwrap();
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Event));
    assert!(compiler.graph_execution_counts().is_none());
    let initial_link_visits = compiler.event_execution_counts().unwrap().link_visits;
    for powered in [true, false, true, false, true] {
        compiler.begin_computation_cycle().unwrap();
        compiler.try_on_use_block(LEVER).unwrap();
        for _ in 0..8 {
            compiler.tick().unwrap();
        }
        compiler.try_flush(&mut world).unwrap();
        assert!(matches!(world.get_block(LAMP), Block::RedstoneLamp { lit } if lit == powered));
        assert!(matches!(world.get_block(PISTON), Block::Piston { piston } if piston.extended == powered));
        assert_eq!(world.get_block(if powered { FAR } else { NEAR }), Block::RedstoneBlock);
    }
    let work = compiler.event_execution_counts().unwrap();
    eprintln!(
        "event fixture movements={} link_visits={} physical_settles={}",
        work.resource_movements,
        work.link_visits - initial_link_visits,
        work.physical_wire_settles
    );
    assert_eq!(work.resource_movements, 5);
    assert!(work.link_visits > initial_link_visits, "input changes executed compiled links");
    assert_eq!(work.physical_wire_settles, 0);
    compiler.try_reset(&mut world, BOUNDS).unwrap();
    assert!(!compiler.is_active());
    assert!(matches!(world.get_block(LEVER), Block::Lever { lever } if !lever.powered));
    assert!(matches!(world.get_block(LAMP), Block::RedstoneLamp { lit: false }));
    assert!(matches!(world.get_block(PISTON), Block::Piston { piston } if !piston.extended));
    assert_eq!(world.get_block(NEAR), Block::RedstoneBlock);
    assert_eq!(world.get_block(FAR), Block::Air);
    assert_eq!(world.get_block(OUTSIDE), Block::Stone {});
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), outside_ticks);
}

#[test]
fn public_compiler_rejects_observer_without_world_mutation() {
    let mut world = fixture();
    let observer = BlockPos::new(2, 20, 7);
    world.set_block(observer, Block::from_name("observer").unwrap());
    let positions = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP, OUTSIDE, observer];
    let before = positions.map(|position| world.get_block(position));
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("unsupported component or observer"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!(positions.map(|position| world.get_block(position)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn public_compiler_rejects_owned_pending_tick_without_consuming_work() {
    let mut world = fixture();
    world.restore_scheduled_tick(TickEntry {
        pos: LEVER,
        ticks_left: 2,
        tick_priority: TickPriority::Normal,
        block_type: None,
    });
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let before = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position));
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("scheduled component tick"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    assert_eq!([LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position)), before);
}

#[test]
fn public_compiler_rejects_quasi_powered_piston_without_mutation() {
    let mut world = fixture();
    let source = BlockPos::new(5, 22, 5);
    world.set_block(source, Block::RedstoneBlock);
    let positions = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP, OUTSIDE, source];
    let before = positions.map(|position| world.get_block(position));
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("quasi"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!(positions.map(|position| world.get_block(position)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn public_compiler_rejects_crossing_without_mutation() {
    let world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let before = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position));
    let mut compiler = Compiler::default();
    let bounds = (BlockPos::new(2, 19, 4), BlockPos::new(8, 21, 6));
    let error = compiler
        .compile(&world, bounds, event_options(), ticks.clone(), Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("crosses selection"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!([LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn public_compiler_rejects_shape_dependent_dust_without_mutation() {
    let mut world = fixture();
    world.set_block(
        WIRE,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    let before = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position));
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("shape-dependent dust"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!([LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    let owned_ticks: Vec<_> = ticks
        .iter()
        .filter(|tick| {
            let p = tick.pos;
            p.x >= BOUNDS.0.x && p.x <= BOUNDS.1.x && p.y >= BOUNDS.0.y && p.y <= BOUNDS.1.y && p.z >= BOUNDS.0.z && p.z <= BOUNDS.1.z
        })
        .cloned()
        .collect();
    compiler
        .compile(&world, BOUNDS, CompilerOptions::default(), owned_ticks, Default::default())
        .unwrap();
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn public_compiler_stop_during_motion_restores_entry() {
    let mut world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap();
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    compiler.tick().unwrap();
    assert_eq!(compiler.event_execution_counts().unwrap().resource_movements, 1);
    compiler.try_flush(&mut world).unwrap();
    compiler.try_reset(&mut world, BOUNDS).unwrap();
    assert!(!compiler.is_active());
    assert_eq!(world.get_block(NEAR), Block::RedstoneBlock);
    assert_eq!(world.get_block(FAR), Block::Air);
    assert!(matches!(world.get_block(PISTON), Block::Piston { piston } if !piston.extended));
    assert_eq!(world.get_block(OUTSIDE), Block::Stone {});
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn failed_stop_keeps_event_backend_and_snapshot_for_retry() {
    let mut world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap();
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    for _ in 0..8 {
        compiler.tick().unwrap();
    }
    compiler.try_flush(&mut world).unwrap();
    let error = compiler
        .try_reset(
            &mut RefuseBlockWrite {
                world: &mut world,
                position: Some(LEVER),
                reject_after: None,
                successes: 0,
                rejected_after: None,
                persistent: false,
            },
            BOUNDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("refused restoration"));
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Event));
    assert!(compiler.tick().unwrap_err().to_string().contains("restoration must be retried"));
    compiler.try_reset(&mut world, BOUNDS).unwrap();
    assert!(!compiler.is_active());
    assert!(matches!(world.get_block(LEVER), Block::Lever { lever } if !lever.powered));
    assert_eq!(world.get_block(NEAR), Block::RedstoneBlock);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn failed_projection_rolls_back_world_writes_and_can_retry() {
    let mut world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let positions = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP, OUTSIDE];
    let before = positions.map(|position| world.get_block(position));
    let region: Vec<_> = (0..=15)
        .flat_map(|x| (0..=31).flat_map(move |y| (0..=15).map(move |z| BlockPos::new(x, y, z))))
        .collect();
    let region_before: Vec<_> = region.iter().map(|&position| world.get_block_raw(position)).collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap();
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    for _ in 0..8 {
        compiler.tick().unwrap();
    }
    let before_failure = compiler.event_execution_counts().unwrap();
    assert_eq!(before_failure.logical_tick, 8);
    assert_eq!(before_failure.resource_movements, 1);
    let mut failing_world = RefuseBlockWrite {
        world: &mut world,
        position: None,
        reject_after: Some(1),
        successes: 0,
        rejected_after: None,
        persistent: false,
    };
    let error = compiler.try_flush(&mut failing_world).unwrap_err();
    assert_eq!(failing_world.rejected_after, Some(1), "one earlier host write must succeed");
    assert!(failing_world.successes >= 2, "the earlier write must be restored by another host write");
    assert!(error.to_string().contains("refused projection block write"));
    assert_eq!(positions.map(|position| world.get_block(position)), before);
    assert_eq!(
        region.iter().map(|&position| world.get_block_raw(position)).collect::<Vec<_>>(),
        region_before
    );
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    assert_eq!(
        compiler.event_execution_counts().unwrap(),
        before_failure,
        "projection failure must not execute work"
    );
    compiler.try_flush(&mut world).unwrap();
    assert_eq!(
        compiler.event_execution_counts().unwrap(),
        before_failure,
        "projection retry must not execute work"
    );
    assert!(matches!(world.get_block(LAMP), Block::RedstoneLamp { lit: true }));
    compiler.try_reset(&mut world, BOUNDS).unwrap();
}

#[test]
fn pending_piston_motion_rejects_cycle_and_input_reversal() {
    let mut world = fixture();
    let mut native = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap();
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    let queued = compiler.event_execution_counts().unwrap();
    assert!(compiler.begin_computation_cycle().unwrap_err().to_string().contains("movement"));
    assert!(compiler.try_on_use_block(LEVER).unwrap_err().to_string().contains("movement"));
    assert_eq!(compiler.event_execution_counts().unwrap(), queued);
    let Block::Lever { mut lever } = native.get_block(LEVER) else {
        panic!("fixture lever missing")
    };
    lever.powered = true;
    native.set_block(LEVER, Block::Lever { lever });
    mchprs_core::redstone::update_surrounding_blocks(&mut native, LEVER);
    mchprs_core::redstone::update_surrounding_blocks(&mut native, LEVER.offset(mchprs_blocks::BlockFace::Bottom));
    compiler.tick().unwrap();
    native.tick_interpreted();
    let before = compiler.event_execution_counts().unwrap();
    assert_eq!(before.resource_movements, 1);
    assert!(compiler.begin_computation_cycle().unwrap_err().to_string().contains("movement"));
    assert!(compiler.try_on_use_block(LEVER).unwrap_err().to_string().contains("movement"));
    assert_eq!(compiler.event_execution_counts().unwrap(), before);
    compiler.try_flush(&mut world).unwrap();
    assert!(matches!(world.get_block(LEVER), Block::Lever { lever } if lever.powered));
    for _ in 0..10 {
        compiler.tick().unwrap();
        native.tick_interpreted();
    }
    compiler.try_flush(&mut world).unwrap();
    assert_eq!(
        [PISTON, NEAR, FAR, LAMP].map(|pos| world.get_block(pos)),
        [PISTON, NEAR, FAR, LAMP].map(|pos| native.get_block(pos))
    );
    assert_eq!(world.get_block(FAR), Block::RedstoneBlock);
    assert!(matches!(world.get_block(LAMP), Block::RedstoneLamp { lit: true }));
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    let Block::Lever { mut lever } = native.get_block(LEVER) else {
        panic!("fixture lever missing")
    };
    lever.powered = false;
    native.set_block(LEVER, Block::Lever { lever });
    mchprs_core::redstone::update_surrounding_blocks(&mut native, LEVER);
    mchprs_core::redstone::update_surrounding_blocks(&mut native, LEVER.offset(mchprs_blocks::BlockFace::Bottom));
    for _ in 0..8 {
        compiler.tick().unwrap();
        native.tick_interpreted();
    }
    compiler.try_flush(&mut world).unwrap();
    assert_eq!(world.get_block(NEAR), Block::RedstoneBlock);
    assert_eq!(
        [PISTON, NEAR, FAR, LAMP].map(|pos| world.get_block(pos)),
        [PISTON, NEAR, FAR, LAMP].map(|pos| native.get_block(pos))
    );
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn failed_stop_after_a_write_rolls_back_the_entire_region() {
    let mut world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap();
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    for _ in 0..8 {
        compiler.tick().unwrap();
    }
    compiler.try_flush(&mut world).unwrap();
    let region: Vec<_> = (0..=15)
        .flat_map(|x| (0..=31).flat_map(move |y| (0..=15).map(move |z| BlockPos::new(x, y, z))))
        .collect();
    let before: Vec<_> = region.iter().map(|&pos| world.get_block_raw(pos)).collect();
    let mut failing = RefuseBlockWrite {
        world: &mut world,
        position: None,
        reject_after: Some(1),
        successes: 0,
        rejected_after: None,
        persistent: false,
    };
    let error = compiler.try_reset(&mut failing, BOUNDS).unwrap_err();
    assert_eq!(failing.rejected_after, Some(1));
    assert!(failing.successes >= 2, "rollback restored the earlier write");
    assert!(error.to_string().contains("refused restoration block write"));
    assert_eq!(region.iter().map(|&pos| world.get_block_raw(pos)).collect::<Vec<_>>(), before);
    assert_eq!(world.get_block(OUTSIDE), Block::Stone {});
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Event));
    assert!(compiler.tick().is_err());
    compiler.try_reset(&mut world, BOUNDS).unwrap();
    assert_eq!(world.get_block(NEAR), Block::RedstoneBlock);
}

#[test]
fn failed_stop_rollback_keeps_snapshot_for_retry() {
    let mut world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap();
    compiler.begin_computation_cycle().unwrap();
    compiler.try_on_use_block(LEVER).unwrap();
    for _ in 0..8 {
        compiler.tick().unwrap();
    }
    compiler.try_flush(&mut world).unwrap();
    let mut failing = RefuseBlockWrite {
        world: &mut world,
        position: None,
        reject_after: Some(1),
        successes: 0,
        rejected_after: None,
        persistent: true,
    };
    let error = compiler.try_reset(&mut failing, BOUNDS).unwrap_err();
    assert!(error.to_string().contains("restoration rollback"), "{error}");
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Event));
    assert!(compiler.tick().is_err());
    compiler.try_reset(&mut world, BOUNDS).unwrap();
    assert!(!compiler.is_active());
    assert_eq!(world.get_block(NEAR), Block::RedstoneBlock);
    assert_eq!(world.get_block(OUTSIDE), Block::Stone {});
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn event_selection_refuses_replacing_active_graph() {
    let world = fixture();
    let ticks: Vec<_> = world.scheduler().iter_entries().collect();
    let before = [LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position));
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, BOUNDS, CompilerOptions::default(), vec![], Default::default())
        .unwrap();
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
    let error = compiler
        .compile(&world, BOUNDS, event_options(), ticks.clone(), Default::default())
        .unwrap_err();
    assert!(matches!(error, mchprs_core::redpiler::CompileError::AlreadyActive));
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
    assert_eq!([LEVER, WIRE, PISTON, NEAR, FAR, LAMP].map(|position| world.get_block(position)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
}

#[test]
fn headless_session_declines_event_until_cycle_integration_is_ready() {
    use mchprs_core::headless::{Session, Snapshot};
    let mut session = Session::new(Snapshot {
        min: BlockPos::new(0, 0, 0),
        max: BlockPos::new(2, 2, 2),
        blocks: vec![],
        entities: vec![],
        ticks: vec![],
    })
    .unwrap();
    let error = session.compile(event_options()).unwrap_err();
    assert!(error.to_string().contains("computation-cycle boundaries"));
    assert_eq!(session.get(BlockPos::new(1, 1, 1)).unwrap(), Block::Air);
}
