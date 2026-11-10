use super::{Operation, program::ElectricalLink};
use super::{
    Program,
    pistons::{PistonActor, PistonToken},
};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpilator::{Compiler, CompilerOptions, TaskMonitor};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{
    BlockFacing, BlockPos,
    blocks::{Block, RedstonePiston},
};
use mchprs_world::{TickEntry, TickPriority};
use std::sync::Arc;

fn lamp_program() -> Program {
    let positions = (0..3).map(|x| BlockPos::new(x, 10, 10)).collect::<Vec<_>>();
    let initial = vec![Block::RedstoneLamp { lit: true }; positions.len()];
    Program::new(positions, initial, vec![], vec![[vec![], vec![]]; 3], vec![], vec![], vec![]).unwrap()
}

#[test]
fn compact_optional_indexes_preserve_component_zero() {
    let runtime = lamp_program().start();
    let cell = runtime.program.component_cells[0];
    assert_eq!(runtime.program.cell_components[cell].unwrap().get(), 1);
    assert_eq!(runtime.program.cell_components[cell].map(|index| index.get() - 1), Some(0));
    assert!(runtime.program.repeater_by_component.iter().all(Option::is_none));
    assert_eq!(
        std::mem::size_of_val(&runtime.program.cell_components[cell]),
        std::mem::size_of::<usize>()
    );
    assert_eq!(std::mem::size_of_val(&runtime.updated_in_phase[0]), std::mem::size_of::<u64>());
}

#[test]
fn projection_token_rows_preserve_order_when_inline_capacity_is_exceeded() {
    let mut program = lamp_program();
    let cell = BlockPos::new(5, 10, 10);
    program.tokens = (0..3)
        .map(|_| PistonToken {
            block: Block::Stone {},
            position: None,
            cells: vec![cell],
        })
        .collect();
    let runtime = program.start();
    let row = &runtime.program.cell_tokens[runtime.program.projection_indices[&cell]];
    assert!(row.spilled());
    assert_eq!(row.as_slice(), &[0, 1, 2]);
}

#[test]
fn initial_phase_does_not_suppress_the_first_update() {
    let mut runtime = lamp_program().start();
    runtime.update_once(0);
    runtime.update_once(0);
    assert_eq!(runtime.deliveries, 1);
    assert_ne!(runtime.updated_in_phase[0], 0);
}

#[test]
fn phase_stamps_suppress_only_the_same_component_in_the_same_phase() {
    let mut runtime = lamp_program().start();
    runtime.phase = 41;

    runtime.update_once(2);
    runtime.update_once(2);
    assert_eq!(runtime.updated_in_phase, [0, 0, 41]);
    assert_eq!(runtime.deliveries, 1);

    runtime.begin_phase();
    runtime.update_once(2);
    assert_eq!(runtime.phase, 42);
    assert_eq!(runtime.updated_in_phase, [0, 0, 42]);
    assert_eq!(runtime.deliveries, 2);
}

#[test]
fn phase_stamp_wrap_clears_stale_marks_and_restarts_at_one() {
    let mut runtime = lamp_program().start();
    runtime.phase = u64::MAX;
    runtime.updated_in_phase = [u64::MAX, 7, 0].into();

    runtime.begin_phase();

    assert_eq!(runtime.phase, 1);
    assert_eq!(runtime.updated_in_phase, [0, 0, 0]);
    runtime.update_once(2);
    assert_eq!(runtime.updated_in_phase, [0, 0, 1]);
    assert_eq!(runtime.deliveries, 1);
}

#[test]
fn piston_bindings_keep_home_far_and_component_ids_attached_to_their_candidates() {
    let unrelated_a = BlockPos::new(4, 10, 10);
    let left_base = BlockPos::new(10, 10, 10);
    let right_base = BlockPos::new(14, 10, 10);
    let shared_home_base = BlockPos::new(12, 10, 10);
    let far_home_base = BlockPos::new(13, 10, 10);
    let unrelated_b = BlockPos::new(50, 10, 10);
    let positions = vec![unrelated_a, left_base, right_base, shared_home_base, far_home_base, unrelated_b];
    let piston = |facing| Block::Piston {
        piston: RedstonePiston {
            facing,
            sticky: true,
            extended: false,
        },
    };
    let mut program = Program::new(
        positions.clone(),
        vec![
            Block::RedstoneLamp { lit: false },
            piston(BlockFacing::East),
            piston(BlockFacing::West),
            piston(BlockFacing::West),
            piston(BlockFacing::West),
            Block::RedstoneLamp { lit: false },
        ],
        vec![],
        vec![[vec![], vec![]]; positions.len()],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    let actor = |id, base, facing, home, far| PistonActor {
        id,
        base,
        facing,
        sticky: true,
        home,
        far,
        extended: false,
        head_present: false,
        group: 0,
    };
    let left_home = BlockPos::new(11, 10, 10);
    let shared_far = BlockPos::new(12, 10, 10);
    let right_home = BlockPos::new(13, 10, 10);
    program.pistons = vec![
        actor(0, left_base, BlockFacing::East, left_home, shared_far),
        actor(1, right_base, BlockFacing::West, right_home, shared_far),
        actor(2, shared_home_base, BlockFacing::West, left_home, BlockPos::new(10, 10, 10)),
        actor(3, far_home_base, BlockFacing::West, shared_far, left_home),
    ];
    program.tokens = vec![
        PistonToken {
            block: Block::Stone {},
            position: Some(left_home),
            cells: vec![left_home],
        },
        PistonToken {
            block: Block::Stone {},
            position: Some(shared_far),
            cells: vec![shared_far],
        },
        PistonToken {
            block: Block::Stone {},
            position: Some(right_home),
            cells: vec![right_home],
        },
        PistonToken {
            block: Block::Stone {},
            position: None,
            cells: vec![BlockPos::new(30, 10, 10)],
        },
    ];

    let runtime = program.start();
    let compiled = &runtime.program;
    let left_component = positions.iter().position(|&pos| pos == left_base).unwrap();
    assert_eq!(compiled.piston_by_component[left_component], std::num::NonZeroUsize::new(1));
    assert_eq!(compiled.piston_by_component[0], None);
    assert_eq!(compiled.piston_by_component[5], None);
    assert_eq!(compiled.piston_bindings[0].component, left_component);
    assert_eq!(compiled.piston_bindings[0].mate, Some(1));
    assert_eq!(compiled.cell_tokens[compiled.piston_bindings[0].home_cell].as_slice(), &[0]);
    assert_eq!(compiled.cell_tokens[compiled.piston_bindings[0].far_cell].as_slice(), &[1]);
    assert_eq!(compiled.piston_bindings[0].home_heads, [2]);
    assert_eq!(compiled.piston_bindings[0].far_heads, [3]);
    assert!(compiled.projection_positions.contains(&shared_far));
}

#[test]
fn imported_tick_ids_preserve_duplicates_stale_types_passive_positions_and_ring_order() {
    let observer_pos = BlockPos::new(10, 10, 10);
    let lamp_pos = BlockPos::new(11, 10, 10);
    let passive_pos = BlockPos::new(12, 10, 10);
    let observer = Block::from_name("observer").unwrap();
    let lamp = Block::RedstoneLamp { lit: true };
    let positions = vec![observer_pos, lamp_pos];
    let program = Program::new(
        positions.clone(),
        vec![observer, lamp],
        vec![],
        vec![[vec![], vec![]]; positions.len()],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    let lamp_type = lamp.registry_id();
    let stale_type = Block::Stone {}.registry_id();
    let entry = |pos, block_type, ticks_left, tick_priority| TickEntry {
        pos,
        block_type,
        ticks_left,
        tick_priority,
    };
    let imported = vec![
        entry(lamp_pos, Some(lamp_type), 3, TickPriority::High),
        entry(observer_pos, None, 0, TickPriority::Normal),
        entry(passive_pos, Some(stale_type), 1, TickPriority::Higher),
        entry(observer_pos, Some(stale_type), 1, TickPriority::Highest),
        entry(lamp_pos, Some(lamp_type), 1, TickPriority::High),
    ];
    let mut runtime = program.start();
    runtime.set_pending(imported).unwrap();

    assert_eq!(
        runtime.pending_entries(),
        vec![
            entry(observer_pos, None, 0, TickPriority::Normal),
            entry(observer_pos, Some(stale_type), 1, TickPriority::Highest),
            entry(passive_pos, Some(stale_type), 1, TickPriority::Higher),
            entry(lamp_pos, Some(lamp_type), 1, TickPriority::High),
            entry(lamp_pos, Some(lamp_type), 3, TickPriority::High),
        ],
        "import/export keeps ring offset, priority FIFO, block type, and duplicates",
    );
    assert_eq!(runtime.pending_counts, [0, 2]);

    runtime.tick().unwrap();

    assert_eq!(runtime.blocks[0], observer, "stale and untyped entries do not dispatch");
    assert_eq!(runtime.blocks[1], Block::RedstoneLamp { lit: false });
    assert_eq!(runtime.pending_counts, [0, 1], "one duplicate remains indexed");
    assert_eq!(
        runtime.pending_entries(),
        vec![entry(lamp_pos, Some(lamp_type), 2, TickPriority::High)],
        "one ring slot elapsed and one duplicate was consumed",
    );

    runtime.tick().unwrap();
    assert_eq!(runtime.pending_counts, [0, 1]);
    assert_eq!(runtime.pending_entries(), vec![entry(lamp_pos, Some(lamp_type), 1, TickPriority::High)]);

    runtime.tick().unwrap();
    assert_eq!(runtime.pending_counts, [0, 0], "membership clears only after the last duplicate drains");
    assert!(runtime.pending_entries().is_empty());
}

#[test]
fn failed_runtime_reset_keeps_compiler_active_and_host_unchanged() {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, crate::world::scheduler::TickScheduler::default());
    let pos = BlockPos::new(20, 20, 20);
    let lamp = Block::RedstoneLamp { lit: false };
    world.set_block(pos, lamp);
    world.set_block_entity(pos, mchprs_blocks::block_entities::BlockEntity::Comparator { output_strength: 6 });
    let pending = TickEntry {
        pos,
        block_type: Some(lamp.registry_id()),
        ticks_left: 2,
        tick_priority: TickPriority::Normal,
    };
    world.restore_scheduled_tick(pending);
    world.piston_state_mut().logical_tick = 17;
    world.piston_state_mut().next_identity = 9;
    let bounds = (pos, pos);

    let mut runtime = lamp_program().start();
    runtime.failure = Some(super::ScratchError {
        position: Some(pos),
        reason: "test runtime failure",
    });
    let mut compiler = super::super::Compiler::default();
    compiler.runtime = Some(runtime);
    let before_block = world.get_block(pos);
    let before_entity = format!("{:?}", world.get_block_entity(pos));
    let before_ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let before_identity = world.piston_state().next_identity;
    let before_logical_tick = world.piston_state().logical_tick;

    let error = compiler.try_reset(&mut world, bounds).unwrap_err();

    assert!(compiler.is_active(), "failed full publication must retain runtime ownership");
    assert!(error.to_string().contains("test runtime failure"));
    assert_eq!(world.get_block(pos), before_block);
    assert_eq!(format!("{:?}", world.get_block_entity(pos)), before_entity);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before_ticks);
    assert_eq!(world.piston_state().next_identity, before_identity);
    assert_eq!(world.piston_state().logical_tick, before_logical_tick);
}

#[test]
fn invalid_imported_tick_is_rejected_without_changing_native_ownership_state() {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, crate::world::scheduler::TickScheduler::default());
    let pos = BlockPos::new(20, 20, 20);
    let lamp = Block::RedstoneLamp { lit: false };
    world.set_block(pos, lamp);
    world.restore_scheduled_tick(TickEntry {
        pos,
        block_type: Some(lamp.registry_id()),
        ticks_left: 2,
        tick_priority: TickPriority::Normal,
    });
    world.piston_state_mut().logical_tick = 17;
    world.piston_state_mut().next_identity = 9;

    let before_block = world.get_block(pos);
    let before_ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let before_piston = format!("{:?}", world.piston_state());
    let invalid = TickEntry {
        pos,
        block_type: Some(lamp.registry_id()),
        ticks_left: crate::world::scheduler::NUM_QUEUES as u32,
        tick_priority: TickPriority::Normal,
    };
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            (pos, pos),
            CompilerOptions::default(),
            vec![invalid],
            Arc::new(TaskMonitor::default()),
        )
        .unwrap_err();

    assert!(!compiler.is_active());
    assert!(error.to_string().contains("invalid scheduled delay or priority"));
    assert_eq!(world.get_block(pos), before_block);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before_ticks);
    assert_eq!(format!("{:?}", world.piston_state()), before_piston);
}

#[test]
fn sealed_program_validation_rejects_bad_output_ranges_and_flip_links() {
    use super::pistons::PistonToken;
    use super::program::{ArrangementSource, InputArrangement};

    let make_program = || {
        let source = BlockPos::new(10, 10, 10);
        let target = BlockPos::new(11, 10, 10);
        let mut program = Program::new(
            vec![source, target],
            vec![Block::RedstoneBlock, Block::RedstoneLamp { lit: false }],
            vec![],
            vec![[vec![], vec![]]; 2],
            vec![ElectricalLink {
                source: 0,
                target: 1,
                attenuation: 0,
            }],
            vec![],
            vec![],
        )
        .unwrap();
        program.tokens.push(PistonToken {
            block: Block::Stone {},
            position: None,
            cells: vec![BlockPos::new(10, 11, 10)],
        });
        program.input_arrangements.push(InputArrangement {
            target: 1,
            side: false,
            silent: false,
            tokens: vec![0],
            token_states: vec![],
            conditional_sources: vec![ArrangementSource::Component(0)],
            inputs: vec![vec![], vec![(ArrangementSource::Component(0), 0)]],
        });
        program.compile_movement_flips().unwrap();
        program
    };

    let mut bad_output = make_program();
    bad_output.set_output_program(1, false, vec![Operation::UpdateLamp(1)]);
    bad_output.validate().unwrap();
    bad_output.output_programs[1][0] = 0..usize::MAX;
    assert_eq!(bad_output.validate().unwrap_err().reason, "invalid output program range");

    let mut bad_flip = make_program();
    bad_flip.validate().unwrap();
    bad_flip.movement_flips[0].link = u32::MAX;
    assert_eq!(bad_flip.validate().unwrap_err().reason, "invalid movement flip link");

    let mut bad_side = make_program();
    bad_side.movement_flips[0].side = !bad_side.movement_flips[0].side;
    assert_eq!(bad_side.validate().unwrap_err().reason, "movement flip does not match its link");

    let mut bad_strength = make_program();
    bad_strength.movement_flips[0].token_contribution = 15;
    assert_eq!(bad_strength.validate().unwrap_err().reason, "invalid movement flip token contribution");

    let mut bad_plan = make_program();
    bad_plan.movement_transitions[0].delta += 1;
    assert_eq!(bad_plan.validate().unwrap_err().reason, "invalid movement plan transition");
}
