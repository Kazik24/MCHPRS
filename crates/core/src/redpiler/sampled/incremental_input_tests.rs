use super::{Operation, Program, program::ElectricalLink};
use mchprs_blocks::{
    BlockPos,
    blocks::{Block, ComparatorMode, RedstoneComparator, RedstoneWire},
};

fn wire(power: u8) -> Block {
    Block::RedstoneWire {
        wire: RedstoneWire { power, ..Default::default() },
    }
}

#[test]
fn wire_strength_cache_tracks_competing_fixed_contributions() {
    let mut program = Program::new(
        (0..3).map(|x| BlockPos::new(x, 10, 10)).collect(),
        vec![wire(10), wire(8), wire(0)],
        vec![],
        vec![[vec![Operation::Contribution(0)], vec![Operation::Contribution(0)]],
             [vec![Operation::Contribution(1)], vec![Operation::Contribution(1)]],
             [vec![], vec![]]],
        vec![ElectricalLink { source: 0, target: 2, attenuation: 2 }, ElectricalLink { source: 1, target: 2, attenuation: 0 }],
        vec![], vec![],
    ).unwrap();
    program.compile_movement_flips().unwrap();
    let mut runtime = program.start();
    assert_eq!(runtime.wire_input_strength(2), 8);
    assert_eq!(runtime.wire_input_strength(2), 8);
    runtime.blocks[0] = wire(8);
    runtime.execute_output_program(0, true);
    assert_eq!(runtime.wire_input_cache[2], u8::MAX);
    assert_eq!(runtime.wire_input_strength(2), 8);
    runtime.blocks[1] = wire(0);
    runtime.execute_output_program(1, false);
    assert_eq!(runtime.wire_input_strength(2), 6);
    runtime.blocks[0] = wire(0);
    runtime.execute_output_program(0, false);
    assert_eq!(runtime.wire_input_strength(2), 0);
}

#[test]
fn wire_strength_cache_tracks_arrangement_changes() {
    use super::compiler::PistonToken;
    use super::program::{ArrangementSource, InputArrangement};

    let cell = BlockPos::new(10, 11, 10);
    let mut program = Program::new(
        vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10)],
        vec![Block::RedstoneBlock, wire(0)], vec![], vec![[vec![], vec![]]; 2], vec![], vec![], vec![],
    ).unwrap();
    program.tokens.push(PistonToken { block: Block::Stone {}, position: None, cells: vec![cell] });
    program.input_arrangements.push(InputArrangement {
        target: 1, side: false, silent: false, tokens: vec![0],
        conditional_sources: vec![ArrangementSource::Component(0)],
        inputs: vec![vec![], vec![(ArrangementSource::Component(0), 0)]],
    });
    program.compile_movement_flips().unwrap();
    let mut runtime = program.start();
    assert_eq!(runtime.wire_input_strength(1), 0);
    runtime.set_token_pos(0, Some(cell));
    assert_eq!(runtime.wire_input_strength(1), 15);
    runtime.set_token_pos(0, None);
    assert_eq!(runtime.wire_input_strength(1), 0);
}

#[test]
fn equal_strength_rear_sources_survive_revalidation_and_independent_removal() {
    use super::program::ComparatorRecord;

    let positions = vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10), BlockPos::new(12, 10, 10)];
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(mchprs_blocks::BlockDirection::East, ComparatorMode::Compare, false),
    };
    let links = vec![
        ElectricalLink {
            source: 0,
            target: 2,
            attenuation: 0,
        },
        ElectricalLink {
            source: 1,
            target: 2,
            attenuation: 0,
        },
    ];
    let mut output_programs = vec![[vec![], vec![]]; 3];
    for (source, link) in [(0, 0), (1, 1)] {
        let update = vec![Operation::Contribution(link), Operation::UpdateComparator(2)];
        output_programs[source] = [update.clone(), update];
    }
    let mut program = Program::new(
        positions.clone(),
        vec![wire(10), wire(8), comparator],
        vec![],
        output_programs,
        links,
        vec![],
        vec![],
    )
    .unwrap();
    program.comparators.push(ComparatorRecord {
        component: 2,
        stored: 0,
        rear_override: None,
        far_override: None,
        output_is_diode: false,
    });

    let mut runtime = program.start();
    assert_eq!(runtime.input_strength(2), 10);
    assert_eq!(runtime.input_levels[2][0].active, 2);

    runtime.blocks[0] = wire(8);
    runtime.begin_phase();
    runtime.execute_output_program(0, true);
    assert_eq!(runtime.contributions, [8, 8]);
    assert_eq!(runtime.input_strength(2), 8);
    assert_eq!(runtime.input_levels[2][0].active, 2);
    let pending = runtime.pending_entries();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].pos, positions[2]);
    assert_eq!(pending[0].ticks_left, 2);

    runtime.tick().unwrap();
    assert_eq!(runtime.comparator_outputs[2], 0);
    assert_eq!(runtime.pending_entries()[0].ticks_left, 1);
    runtime.tick().unwrap();
    assert_eq!(runtime.comparator_outputs[2], 8);
    assert!(matches!(runtime.blocks[2], Block::RedstoneComparator { comparator } if comparator.powered));

    runtime.blocks[0] = wire(0);
    runtime.begin_phase();
    runtime.execute_output_program(0, false);
    assert_eq!(runtime.input_strength(2), 8);
    assert_eq!(runtime.input_levels[2][0].active, 1);
    assert!(runtime.pending_entries().is_empty());

    runtime.blocks[1] = wire(0);
    runtime.begin_phase();
    runtime.execute_output_program(1, false);
    assert_eq!(runtime.input_strength(2), 0);
    assert_eq!(runtime.input_levels[2][0].active, 0);
    assert_eq!(runtime.contributions, [0, 0]);
    assert_eq!(runtime.pending_entries().len(), 1);
    assert_eq!(runtime.pending_entries()[0].ticks_left, 2);
    runtime.tick().unwrap();
    assert_eq!(runtime.comparator_outputs[2], 8);
    assert_eq!(runtime.pending_entries()[0].ticks_left, 1);
    runtime.tick().unwrap();
    assert_eq!(runtime.comparator_outputs[2], 0);
    assert!(matches!(runtime.blocks[2], Block::RedstoneComparator { comparator } if !comparator.powered));
}

#[test]
fn conditional_source_shadows_its_fixed_link_when_token_enters_and_leaves() {
    use super::compiler::PistonToken;
    use super::program::{ArrangementSource, InputArrangement};

    let source_pos = BlockPos::new(10, 10, 10);
    let target_pos = BlockPos::new(11, 10, 10);
    let token_cell = BlockPos::new(10, 11, 10);
    let mut program = Program::new(
        vec![source_pos, target_pos],
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
        cells: vec![token_cell],
    });
    program.input_arrangements.push(InputArrangement {
        target: 1,
        side: false,
        silent: false,
        tokens: vec![0],
        conditional_sources: vec![ArrangementSource::Component(0)],
        inputs: vec![vec![], vec![(ArrangementSource::Component(0), 0)]],
    });
    program.compile_movement_flips().unwrap();

    let mut runtime = program.start();
    assert_eq!(runtime.output(0), 15);
    assert_eq!(runtime.contributions, [15]);
    assert!(!runtime.program.effective_links[0]);
    assert_eq!(runtime.input_strength(1), 0);
    assert_eq!(runtime.input_levels[1][0].active, 0);

    runtime.set_token_pos(0, Some(token_cell));
    assert_eq!(runtime.input_strength(1), 15);
    assert_eq!(runtime.input_levels[1][0].active, 1);

    runtime.set_token_pos(0, None);
    assert_eq!(runtime.input_strength(1), 0);
    assert_eq!(runtime.input_levels[1][0].active, 0);
}
