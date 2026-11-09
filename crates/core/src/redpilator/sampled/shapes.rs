//! Sample local geometry once; replay shape changes without runtime world traversal.
use super::{
    Program, ScratchError, ScratchWorld,
    routing::{Axis, Local, compile_local},
};
use crate::{redpilator::TaskMonitor, world::World};
use mchprs_blocks::blocks::{Block, RedstoneWire, RedstoneWireSide};
use std::collections::HashMap;

#[derive(Clone)]
pub(super) struct ShapeStep {
    pub(super) node: usize,
    /// Empty for an observer; otherwise maps stored sides to their next state.
    pub(super) states: Local<usize>,
    pub(super) callbacks: Vec<usize>,
}

pub(super) fn pack(wire: RedstoneWire) -> u8 {
    (wire.north.get_id() | wire.south.get_id() << 2 | wire.east.get_id() << 4 | wire.west.get_id() << 6) as u8
}

pub(super) fn unpack(sides: u8, power: u8) -> RedstoneWire {
    RedstoneWire {
        north: RedstoneWireSide::from_id(u32::from(sides & 3)),
        south: RedstoneWireSide::from_id(u32::from(sides >> 2 & 3)),
        east: RedstoneWireSide::from_id(u32::from(sides >> 4 & 3)),
        west: RedstoneWireSide::from_id(u32::from(sides >> 6 & 3)),
        power,
    }
}

/// Compile shape callbacks caused by movable geometry.
///
/// Inputs: The mutable program, reusable scratch world and optional monitor.
///
/// Find token and piston cells, probe nearby wire shapes for each local token arrangement, share identical
/// side maps and record observer/wire callbacks in native order.
pub(super) fn sample(program: &mut Program, scratch: &mut ScratchWorld, monitor: Option<&TaskMonitor>) -> Result<(), ScratchError> {
    program.shape_programs.clear();
    program.shape_steps.clear();
    program.shape_maps.clear();
    let mut maps = HashMap::new();
    let components: HashMap<_, _> = program.positions.iter().enumerate().map(|(i, &p)| (p, i)).collect();
    let token_cells = program.tokens.iter().flat_map(|token| token.cells.iter().copied());
    let piston_cells = program.pistons.iter().flat_map(|piston| [piston.base, piston.home, piston.far]);
    // Shape sampling needs both movable occupancy and piston anchor cells.
    let mut cells: Vec<_> = token_cells.chain(piston_cells).collect();
    cells.sort_by_key(|p| (p.x, p.y, p.z));
    cells.dedup();
    for cell in cells {
        let mut steps = Vec::new();
        let mut neighbors = Vec::new();
        crate::redstone::piston::shape_notifications(cell, |pos, side| neighbors.push((pos, side)));
        for (pos, side) in neighbors {
            let Some(&node) = components.get(&pos) else {
                continue;
            };
            match program.initial[node] {
                Block::Observer { observer } if pos.offset(observer.facing.into()) == cell => {
                    steps.push(ShapeStep {
                        node,
                        states: Local { axes: vec![], ports: vec![] },
                        callbacks: vec![],
                    });
                }
                Block::RedstoneWire { .. } if side != mchprs_blocks::BlockFace::Top => {
                    // Only token axes within two blocks can change this wire's sampled shape.
                    let axes = program
                        .tokens
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            t.cells
                                .iter()
                                .any(|p| p.x.abs_diff(pos.x) + p.y.abs_diff(pos.y) + p.z.abs_diff(pos.z) <= 2)
                        })
                        .map(|(i, t)| Axis::Token(i, t.cells.len()))
                        .collect();
                    let states = compile_local(scratch, &program.tokens, axes, pos, monitor, |world| {
                        let raw = crate::redstone::wire::get_raw_sides_from(RedstoneWire::default(), pos, |at| world.get_block(at));
                        // Stored dot/cross choices affect the result even with identical geometry.
                        Ok(std::array::from_fn(|sides| {
                            let sides = sides as u8;
                            if (0..4).any(|i| sides >> (i * 2) & 3 == 3) {
                                return sides;
                            }
                            pack(crate::redstone::wire::on_neighbor_changed_from(unpack(sides, 0), raw, side))
                        }))
                    })?;
                    let states = Local {
                        axes: states.axes,
                        ports: states
                            .ports
                            .into_iter()
                            .map(|state| {
                                state.map(|map| {
                                    *maps.entry(map).or_insert_with(|| {
                                        let index = program.shape_maps.len();
                                        program.shape_maps.push(map);
                                        index
                                    })
                                })
                            })
                            .collect(),
                    };
                    let mut callbacks = Vec::new();
                    crate::redstone::wire_update_notifications(pos, |at, direction, indirect| {
                        if let Some(&target) = components.get(&at) {
                            if let Block::Observer { observer } = program.initial[target]
                                && (indirect || observer.facing != direction.into())
                            {
                                return;
                            }
                            callbacks.push(target);
                        }
                    });
                    steps.push(ShapeStep { node, states, callbacks });
                }
                _ => {}
            }
        }
        let start = program.shape_steps.len();
        program.shape_steps.extend(steps);
        program.shape_programs.insert(cell, start..program.shape_steps.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;
    use mchprs_blocks::{BlockFace, BlockPos};

    #[test]
    fn sampled_maps_share_equal_arrangements_and_preserve_invalid_overlap() {
        let mut world = super::super::tests::host();
        let wire = BlockPos::new(15, 15, 15);
        let north = wire.offset(BlockFace::North);
        let upper_wire = north.offset(BlockFace::Top);
        let cap = wire.offset(BlockFace::Top);
        for pos in [wire.offset(BlockFace::Bottom), north] {
            world.set_block(pos, Block::Stone {});
        }
        for pos in [wire, upper_wire] {
            world.set_block(
                pos,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
        }
        let mut program = Program::new(
            vec![wire, upper_wire],
            vec![world.get_block(wire), world.get_block(upper_wire)],
            vec![],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        program.tokens = vec![
            super::super::pistons::PistonToken {
                block: Block::Stone {},
                position: None,
                cells: vec![cap],
            };
            2
        ];
        let mut scratch = ScratchWorld::from_world(
            &world,
            (BlockPos::new(9, 9, 9), BlockPos::new(22, 22, 22)),
            super::super::ScratchEntry {
                scheduled: vec![],
                piston: world.piston_state().clone(),
            },
        )
        .unwrap();
        sample(&mut program, &mut scratch, None).unwrap();
        let step = program.shape_programs[&cap]
            .clone()
            .find(|&index| program.shape_steps[index].node == 0)
            .unwrap();
        let states = &program.shape_steps[step].states;
        assert_eq!(states.axes, vec![Axis::Token(0, 1), Axis::Token(1, 1)]);
        assert_ne!(states.ports[0], states.ports[1]);
        assert_eq!(states.ports[1], states.ports[2]);
        assert_eq!(states.ports[3], None);
        let maps = program.shape_maps.clone();
        assert_eq!(maps.iter().collect::<std::collections::HashSet<_>>().len(), maps.len());
        let mut malformed = program.clone();
        malformed.shape_steps[step].states.ports[0] = Some(maps.len());
        assert_eq!(malformed.validate().unwrap_err().reason, "invalid shape map reference");
        program.shape_maps.push([255; 256]);
        sample(&mut program, &mut scratch, None).unwrap();
        assert_eq!(program.shape_maps, maps);
    }
}
