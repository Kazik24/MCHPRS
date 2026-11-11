use super::*;
use crate::redpilator::sampled::{routing::Local, shapes::ShapeStep};

#[test]
fn relaxed_wire_shape_filter_preserves_observer_order_and_explicit_empty_ranges() {
    let positions = vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10), BlockPos::new(12, 10, 10)];
    let mut program = Program::new(
        positions.clone(),
        vec![
            Block::from_name("observer").unwrap(),
            Block::from_name("redstone_wire").unwrap(),
            Block::from_name("observer").unwrap(),
        ],
        vec![],
        vec![[vec![], vec![]]; 3],
        vec![],
        vec![],
        vec![],
    )
    .unwrap();
    program.shape_maps.push(std::array::from_fn(|sides| sides as u8));
    program.shape_steps = vec![
        ShapeStep {
            node: 0,
            states: Local { axes: vec![], ports: vec![] },
            callbacks: vec![],
        },
        ShapeStep {
            node: 1,
            states: Local {
                axes: vec![],
                ports: vec![Some(0)],
            },
            callbacks: vec![0, 2],
        },
        ShapeStep {
            node: 2,
            states: Local { axes: vec![], ports: vec![] },
            callbacks: vec![],
        },
    ];
    program.shape_programs.insert(positions[0], 0..3);
    program.shape_programs.insert(positions[1], 1..2);
    program.shape_programs.insert(positions[2], 0..0);
    program.prepare_projection();
    assert!(!program.relaxed_mode);
    assert_eq!(program.shape_steps.len(), 3);
    program.enable_relaxed_wire_shapes().unwrap();
    assert!(program.relaxed_mode);
    assert_eq!(program.shape_steps.iter().map(|step| step.node).collect::<Vec<_>>(), [0, 2]);
    assert!(program.shape_maps.is_empty());
    assert_eq!(program.shape_programs[&positions[0]], 0..2);
    assert_eq!(program.shape_programs[&positions[1]], 1..1);
    assert_eq!(program.shape_programs[&positions[2]], 0..0);
    assert_eq!(program.cell_shape_steps[program.component_cells[1]], Some(1..1));
    program.enable_relaxed_wire_shapes().unwrap();
    assert_eq!(program.shape_steps.iter().map(|step| step.node).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(program.cell_shape_steps[program.component_cells[1]], Some(1..1));
}
