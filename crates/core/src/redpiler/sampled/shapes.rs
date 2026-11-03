//! Sample local geometry once; replay shape changes without runtime world traversal.
use super::{
    Program, ScratchError, ScratchWorld,
    routing::{Axis, Local, compile_local},
};
use crate::{redpiler::TaskMonitor, world::World};
use mchprs_blocks::blocks::{Block, RedstoneWire, RedstoneWireSide};
use std::collections::HashMap;

#[derive(Clone)]
pub(super) struct ShapeStep {
    pub(super) node: usize,
    /// Empty for an observer; otherwise maps stored sides to their next state.
    pub(super) states: Local<[u8; 256]>,
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

pub(super) fn sample(program: &mut Program, scratch: &mut ScratchWorld, monitor: Option<&TaskMonitor>) -> Result<(), ScratchError> {
    program.shape_programs.clear();
    program.shape_steps.clear();
    let components: HashMap<_, _> = program.positions.iter().enumerate().map(|(i, &p)| (p, i)).collect();
    let mut cells: Vec<_> = program
        .tokens
        .iter()
        .flat_map(|t| t.cells.iter().copied())
        .chain(program.pistons.iter().flat_map(|p| [p.base, p.home, p.far]))
        .collect();
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
