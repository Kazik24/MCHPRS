//! Ordered command notifications, with live solidity guards for far comparators.
use super::{Program, ScratchError, overflow};
use mchprs_blocks::{BlockFace, blocks::Block};

fn id(value: usize) -> Result<u32, ScratchError> {
    u32::try_from(value).map_err(|_| overflow())
}

pub(super) fn encode(program: &Program) -> Result<(Vec<u32>, Vec<u32>), ScratchError> {
    let mut rows = Vec::with_capacity(program.initial.len() * 2);
    let mut targets = Vec::new();
    for (node, &block) in program.initial.iter().enumerate() {
        let start = targets.len() / 3;
        if block.is_command_block() {
            let pos = program.positions[node];
            crate::redstone::surrounding_notifications(pos, |target, dir, diagonal| {
                let Some(&component) = program.component_indices.get(&target) else {
                    return;
                };
                let block = program.initial[component];
                if diagonal && (matches!(block, Block::Observer { .. } | Block::Piston { .. }) || target == pos && block.is_copper_bulb()) {
                    return;
                }
                if matches!(block, Block::Observer { observer } if observer.facing != dir.into()) {
                    return;
                }
                targets.push(component);
                targets.push(usize::MAX);
                targets.push(0x100);
            });
            for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
                let adjacent = pos.offset(face);
                let Some(&component) = program.component_indices.get(&adjacent.offset(face)) else {
                    continue;
                };
                if !matches!(program.initial[component], Block::RedstoneComparator { .. }) {
                    continue;
                }
                let fallback = program
                    .component_indices
                    .get(&adjacent)
                    .map(|&node| program.initial[node])
                    .or_else(|| program.geometry_indices.get(&adjacent).copied())
                    .unwrap_or(Block::Air);
                targets.extend([
                    component,
                    program.projection_indices.get(&adjacent).copied().unwrap_or(usize::MAX),
                    if fallback.is_solid() { 0x100 } else { 0 },
                ]);
            }
        }
        rows.extend([id(start)?, id(targets.len() / 3 - start)?]);
    }
    let targets = targets
        .into_iter()
        .map(|value| if value == usize::MAX { Ok(u32::MAX) } else { id(value) })
        .collect::<Result<_, _>>()?;
    Ok((rows, targets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;
    use mchprs_blocks::BlockPos;

    #[test]
    fn far_comparator_guards_preserve_solid_and_transparent_geometry() {
        for support in [Block::Stone {}, Block::Glass {}] {
            let mut world = super::super::super::tests::host();
            let command = BlockPos::new(15, 15, 15);
            let adjacent = command.offset(BlockFace::East);
            let comparator = adjacent.offset(BlockFace::East);
            world.set_block(command, Block::from_name("command_block").unwrap());
            world.set_block(adjacent, support);
            world.set_block(comparator, Block::from_name("comparator").unwrap());
            world.set_block(comparator.offset(BlockFace::Bottom), Block::Stone {});
            let program = super::super::super::compile_program(
                &world,
                (command - BlockPos::new(4, 4, 4), command + BlockPos::new(4, 4, 4)),
                super::super::super::ScratchEntry {
                    scheduled: vec![],
                    piston: world.piston_state().clone(),
                },
                None,
            )
            .unwrap();
            let (rows, targets) = encode(&program).unwrap();
            let node = program.component_indices[&command];
            let start = rows[node * 2] as usize;
            let count = rows[node * 2 + 1] as usize;
            let guard = &targets[(start + count - 1) * 3..(start + count) * 3];
            assert_eq!(guard[0], program.component_indices[&comparator] as u32);
            assert_eq!(guard[1], program.projection_indices.get(&adjacent).map_or(u32::MAX, |&cell| cell as u32));
            assert_eq!(guard[2], if support.is_solid() { 0x100 } else { 0 });
        }
    }
}
