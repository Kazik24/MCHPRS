use super::program::PISTON_HEAD_NOTIFICATION;
use super::*;
use crate::plot::PlotWorld;
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos, blocks::RedstonePiston};

fn scratch_entry(world: &PlotWorld) -> ScratchEntry {
    ScratchEntry {
        scheduled: world.scheduler().iter_entries().collect(),
        piston: world.piston_state().clone(),
    }
}

fn extended_piston_runtime() -> (Runtime, usize) {
    let mut world = super::tests::host();
    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: true,
            },
        },
    );
    world.set_block(
        home,
        Block::PistonHead {
            head: mchprs_blocks::blocks::RedstonePistonHead {
                facing: BlockFacing::East,
                sticky: false,
                short: false,
            },
        },
    );

    let program = compile(&world, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), scratch_entry(&world)).unwrap();
    let runtime = program.start();
    let component = runtime.program.component_indices[&base];
    (runtime, component)
}

#[test]
fn live_stationary_head_forwards_without_generic_dedup_and_event_queue_deduplicates() {
    let (mut runtime, base) = extended_piston_runtime();
    let tagged = PISTON_HEAD_NOTIFICATION | base;

    // A direct callback already delivered in this phase must not suppress the
    // separate head-forwarded piston check.
    runtime.updated_in_phase[base] = runtime.phase;
    runtime.update_once(tagged);
    runtime.update_once(tagged);

    assert_eq!(runtime.piston_events.len(), 1);
    assert_eq!(runtime.piston_events.front(), Some(&(0, super::executor::PistonEvent::Retract)));
}

#[test]
fn absent_or_moving_head_does_not_forward_a_tagged_notification() {
    let (mut absent, base) = extended_piston_runtime();
    absent.piston_head_present[0] = false;
    absent.update_once(PISTON_HEAD_NOTIFICATION | base);
    assert!(absent.piston_events.is_empty(), "an absent head cannot forward");

    let (mut moving, base) = extended_piston_runtime();
    moving.piston_head_moving[0] = true;
    moving.update_once(PISTON_HEAD_NOTIFICATION | base);
    assert!(moving.piston_events.is_empty(), "a moving head cannot forward");
}

#[test]
fn direct_base_notification_still_updates_when_the_head_is_absent() {
    let (mut runtime, base) = extended_piston_runtime();
    runtime.piston_head_present[0] = false;

    runtime.update_once(base);

    assert_eq!(runtime.piston_events.len(), 1);
    assert_eq!(runtime.piston_events.front(), Some(&(0, super::executor::PistonEvent::Retract)));
}

#[test]
fn reactive_component_list_rejects_head_notification_tags() {
    let (runtime, base) = extended_piston_runtime();
    let mut program = (*runtime.program).clone();
    program.reactive_neighbours[base].push(PISTON_HEAD_NOTIFICATION | base);
    let error = program.validate().err().expect("reactive entries are plain component IDs");
    assert_eq!(error.reason, "invalid reactive notification reference");
}

fn one_wire_program(initial: Vec<Block>, watcher: usize) -> Result<Program, ScratchError> {
    let positions = (0..initial.len()).map(|x| BlockPos::new(x as i32 + 10, 15, 15)).collect();
    let output_programs = vec![[Vec::new(), Vec::new()]; initial.len()];
    Program::new(
        positions,
        initial,
        vec![],
        output_programs,
        vec![],
        vec![WatchedWire {
            wire: 0,
            watchers: vec![watcher],
        }],
        vec![],
    )
}

#[test]
fn tagged_watcher_ids_must_name_an_in_bounds_piston() {
    let wire = Block::RedstoneWire {
        wire: mchprs_blocks::blocks::RedstoneWire::default(),
    };
    let lamp = Block::RedstoneLamp { lit: false };

    assert!(one_wire_program(vec![wire, lamp], PISTON_HEAD_NOTIFICATION | 1).is_err());
    assert!(one_wire_program(vec![wire], PISTON_HEAD_NOTIFICATION | 1).is_err());
}

#[test]
fn tagged_source_notification_requires_a_sealed_piston_record() {
    let wire = Block::RedstoneWire {
        wire: mchprs_blocks::blocks::RedstoneWire::default(),
    };
    let piston = Block::Piston {
        piston: RedstonePiston {
            facing: BlockFacing::East,
            sticky: false,
            extended: true,
        },
    };
    let mut program = one_wire_program(vec![wire, piston], 1).unwrap();
    program.source_watchers[1].push(PISTON_HEAD_NOTIFICATION | 1);

    let error = program.validate().err().expect("a tagged watcher without a piston actor is orphaned");
    assert_eq!(error.reason, "piston-head notification has no piston record");
}
