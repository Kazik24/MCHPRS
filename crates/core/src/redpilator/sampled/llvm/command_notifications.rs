//! Ordered command notifications, with live solidity guards for far comparators.
use super::{Program, ScratchError, overflow};
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};

fn id(value: usize) -> Result<u32, ScratchError> {
    u32::try_from(value).map_err(|_| overflow())
}

pub(super) struct CommandNotifications {
    pub(super) rows: Vec<u32>,
    pub(super) targets: Vec<u32>,
}

struct NotificationTarget {
    component: usize,
    solidity_guard: Option<usize>,
    fallback_is_solid: bool,
}

impl NotificationTarget {
    fn encode(self) -> Result<[u32; 3], ScratchError> {
        let component = id(self.component)?;
        let solidity_guard = self.solidity_guard.map(id).transpose()?.unwrap_or(u32::MAX);
        // Bit 8 is the native callback's fallback-solidity flag.
        let flags = u32::from(self.fallback_is_solid) << 8;
        Ok([component, solidity_guard, flags])
    }
}

fn captured_block_at(program: &Program, pos: BlockPos) -> Block {
    if let Some(&component) = program.component_indices.get(&pos) {
        return program.initial[component];
    }
    program.geometry_indices.get(&pos).copied().unwrap_or(Block::Air)
}

/// Encode native callbacks caused by command-block output changes.
///
/// Inputs: The program's component positions, initial blocks and notification identities.
///
/// Follow the interpreter's surrounding-update rules, preserve observer/piston-head routes and comparator-
/// through-solid callbacks, then flatten rows and checked recipient records.
pub(super) fn encode(program: &Program) -> Result<CommandNotifications, ScratchError> {
    let mut rows = Vec::with_capacity(program.initial.len() * 2);
    let mut targets = Vec::new();
    for (node, &block) in program.initial.iter().enumerate() {
        let start = targets.len();
        if block.is_command_block() {
            let pos = program.positions[node];
            crate::redstone::surrounding_notifications(pos, |target, dir, diagonal| {
                let Some(&component) = program.component_indices.get(&target) else {
                    return;
                };
                let block = program.initial[component];
                let skips_diagonal = matches!(block, Block::Observer { .. } | Block::Piston { .. });
                let is_source_bulb = target == pos && block.is_copper_bulb();
                if diagonal && (skips_diagonal || is_source_bulb) {
                    return;
                }
                if matches!(block, Block::Observer { observer } if observer.facing != dir.into()) {
                    return;
                }
                targets.push(NotificationTarget {
                    component,
                    solidity_guard: None,
                    fallback_is_solid: true,
                });
            });
            for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
                let adjacent = pos.offset(face);
                let Some(&component) = program.component_indices.get(&adjacent.offset(face)) else {
                    continue;
                };
                if !matches!(program.initial[component], Block::RedstoneComparator { .. }) {
                    continue;
                }
                let fallback = captured_block_at(program, adjacent);
                targets.push(NotificationTarget {
                    component,
                    solidity_guard: program.projection_indices.get(&adjacent).copied(),
                    fallback_is_solid: fallback.is_solid(),
                });
            }
        }
        rows.extend([id(start)?, id(targets.len() - start)?]);
    }
    let mut encoded_targets = Vec::with_capacity(targets.len() * 3);
    for target in targets {
        encoded_targets.extend(target.encode()?);
    }
    Ok(CommandNotifications {
        rows,
        targets: encoded_targets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[test]
    fn notification_guards_encode_absence_flags_and_checked_ids() {
        let unconditional = NotificationTarget {
            component: 7,
            solidity_guard: None,
            fallback_is_solid: true,
        };
        assert_eq!(unconditional.encode().unwrap(), [7, u32::MAX, 0x100]);
        let guarded = NotificationTarget {
            component: 7,
            solidity_guard: Some(3),
            fallback_is_solid: false,
        };
        assert_eq!(guarded.encode().unwrap(), [7, 3, 0]);
        if let Some(too_large) = (u32::MAX as usize).checked_add(1) {
            let invalid = NotificationTarget {
                component: too_large,
                solidity_guard: None,
                fallback_is_solid: true,
            };
            assert!(invalid.encode().is_err());
            let invalid_guard = NotificationTarget {
                component: 7,
                solidity_guard: Some(too_large),
                fallback_is_solid: true,
            };
            assert!(invalid_guard.encode().is_err());
        }
    }

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
            let CommandNotifications { rows, targets } = encode(&program).unwrap();
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
