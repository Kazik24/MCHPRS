//! Project runtime blocks and piston parts for publication into the host world.
use super::{Runtime, ScratchError};
use mchprs_blocks::{
    block_entities::MovingPistonEntity,
    blocks::{Block, RedstonePistonHead},
};

pub(in crate::redpilator) type ExportKey = (usize, u64, bool, mchprs_blocks::BlockPos);

pub(in crate::redpilator) struct MovingPart {
    /// stable key identifying the piston/token part across host publications.
    pub(in crate::redpilator) key: ExportKey,
    /// world position where this moving part is published.
    pub(in crate::redpilator) pos: mchprs_blocks::BlockPos,
    /// currently rendered at the moving-part position.
    pub(in crate::redpilator) block: mchprs_blocks::blocks::Block,
    /// block-entity payload for the moving part.
    pub(in crate::redpilator) entity: mchprs_blocks::block_entities::MovingPistonEntity,
    /// normalized interpolation progress of the moving part.
    pub(in crate::redpilator) progress: f32,
}

impl Runtime {
    pub(crate) fn project(&self, position: mchprs_blocks::BlockPos) -> Option<Block> {
        if let Some(&cell) = self.program.projection_indices.get(&position) {
            self.project_cell(cell)
        } else {
            Some(self.program.geometry_indices.get(&position).copied().unwrap_or(Block::Air))
        }
    }

    pub(super) fn token_at(&self, cell: usize) -> Option<usize> {
        self.program.cell_tokens[cell]
            .iter()
            .copied()
            .find(|&token| self.token_positions[token] == Some(self.program.projection_positions[cell]))
    }

    /// Resolve the stationary block currently occupying a projected cell.
    ///
    /// Inputs: The cell index and this runtime's active motions, token positions and piston flags.
    ///
    /// Suppress cells owned by motion, prefer a resident token or piston head, clear vacated payload
    /// homes, and otherwise return the component or static geometry.
    pub(super) fn project_cell(&self, cell: usize) -> Option<Block> {
        let position = self.program.projection_positions[cell];
        if self.cell_motion[cell].is_some() {
            return None;
        }
        let tokens = &self.program.cell_tokens[cell];
        if let Some(index) = self.token_at(cell) {
            return Some(self.program.tokens[index].block);
        }
        for &index in &self.program.cell_pistons[cell] {
            let piston = &self.program.pistons[index];
            if position == piston.base && self.piston_base_moving[index] {
                return None;
            }
            if position == piston.home {
                if self.piston_head_moving[index] {
                    return None;
                }
                if self.piston_head_present[index] {
                    return Some(Block::PistonHead {
                        head: mchprs_blocks::blocks::RedstonePistonHead {
                            facing: piston.facing,
                            sticky: piston.sticky,
                            short: false,
                        },
                    });
                }
            }
        }
        if tokens.iter().any(|&index| self.program.tokens[index].position == Some(position)) {
            return Some(Block::Air);
        }
        Some(
            self.program.cell_components[cell]
                .map(|index| index.get() - 1)
                .map_or(self.program.cell_geometry[cell], |component| self.blocks[component]),
        )
    }

    pub(crate) fn projected_blocks(&self) -> Vec<(mchprs_blocks::BlockPos, Block)> {
        self.program
            .projection_positions
            .iter()
            .enumerate()
            .filter_map(|(cell, &pos)| self.project_cell(cell).map(|block| (pos, block)))
            .collect()
    }
    /// Iterate dirty block states at the requested publication level.
    ///
    /// Inputs: The full-publication flag and this runtime's dirty-cell, component and comparator indexes.
    ///
    /// Resolve each dirty cell through geometry projection or the redstone component view, skip excluded
    /// piston geometry, and attach any comparator output value.
    pub(in crate::redpilator) fn publication_changes(
        &self,
        full: bool,
    ) -> impl Iterator<Item = (mchprs_blocks::BlockPos, mchprs_blocks::blocks::Block, Option<u8>)> + '_ {
        self.dirty_cells.iter().filter_map(move |&cell| {
            let pos = self.program.projection_positions[cell];
            let block = if full {
                self.project_cell(cell)?
            } else {
                let component = self.program.cell_components[cell].map(|index| index.get() - 1)?;
                if self.program.piston_by_component[component].is_some() {
                    return None;
                }
                self.blocks[component]
            };
            let comparator = self.program.cell_components[cell]
                .map(|index| index.get() - 1)
                .filter(|&component| self.program.comparator_by_component[component].is_some())
                .map(|component| self.comparator_outputs[component]);
            Some((pos, block, comparator))
        })
    }

    pub(in crate::redpilator) fn invalidate_publication(&mut self) {
        self.dirty_cells.clear();
        self.dirty_cells.extend(0..self.dirty.len());
        self.dirty.fill(true);
        self.needs_full_publication = true;
    }

    pub(in crate::redpilator) fn prepare_publication(&mut self, io_only: bool) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if !io_only && self.needs_full_publication {
            self.dirty_cells.clear();
            self.dirty_cells.extend(0..self.dirty.len());
            self.dirty.fill(true);
        }
        Ok(())
    }

    pub(in crate::redpilator) fn mark_published(&mut self, io_only: bool) {
        for &cell in &self.dirty_cells {
            self.dirty[cell] = false;
        }
        self.dirty_cells.clear();
        self.needs_full_publication = io_only;
    }

    pub(in crate::redpilator) fn comparator_outputs(&self) -> Vec<(mchprs_blocks::BlockPos, u8)> {
        self.program
            .comparators
            .iter()
            .map(|record| (self.program.positions[record.component], self.comparator_outputs[record.component]))
            .collect()
    }

    #[cfg(test)]
    pub(in crate::redpilator) fn moving_parts(&self) -> Result<Vec<MovingPart>, ScratchError> {
        let mut parts = Vec::new();
        self.write_moving_parts(&mut parts, &mut std::collections::HashSet::new())?;
        Ok(parts)
    }

    pub(in crate::redpilator) fn motion_part_capacity(&self) -> usize {
        self.motions.capacity() * 2
    }

    /// Export active piston animations as host-world block parts.
    ///
    /// Inputs: Reusable output buffers plus this runtime's motion slots, token payloads and piston bindings.
    ///
    /// Build the carried block and piston-head/base parts with stable keys and progress values, then
    /// reject overlapping exported positions.
    pub(in crate::redpilator) fn write_moving_parts(
        &self,
        parts: &mut Vec<MovingPart>,
        positions: &mut std::collections::HashSet<mchprs_blocks::BlockPos>,
    ) -> Result<(), ScratchError> {
        parts.clear();
        positions.clear();
        for &slot in &self.active_motions {
            let motion = &self.motions[slot];
            if motion.piston_done && motion.token_done {
                continue;
            }
            let piston = &self.program.pistons[motion.piston];
            let remaining = motion.landing.saturating_sub(self.motion_time);
            let progress = if remaining >= 2 { 0.5 } else { 1.0 };
            let mut add = |pos: mchprs_blocks::BlockPos, block: Block, source: bool| {
                let mut entity = MovingPistonEntity {
                    facing: piston.facing.into(),
                    progress: 0,
                    block_state: block.get_id(),
                    extending: motion.extending,
                    source,
                };
                entity.set_progress(progress);
                parts.push(MovingPart {
                    key: (motion.piston, motion.landing, motion.extending, pos),
                    pos,
                    block: Block::MovingPiston {
                        moving: mchprs_blocks::blocks::RedstoneMovingPiston {
                            facing: piston.facing,
                            sticky: piston.sticky,
                        },
                    },
                    entity,
                    progress,
                });
            };
            if motion.extending {
                if !motion.token_done {
                    if let Some(token) = motion.token {
                        add(
                            self.program.projection_positions[motion.destination],
                            self.program.tokens[token].block,
                            false,
                        );
                    }
                }
                if !motion.piston_done {
                    add(
                        piston.home,
                        Block::PistonHead {
                            head: RedstonePistonHead {
                                facing: piston.facing,
                                sticky: piston.sticky,
                                short: false,
                            },
                        },
                        true,
                    );
                }
            } else {
                if !motion.piston_done {
                    let component = self.program.piston_bindings[motion.piston].component;
                    let Block::Piston { piston: mut base } = self.blocks[component] else {
                        return Err(ScratchError {
                            position: Some(piston.base),
                            reason: "piston base missing during motion export",
                        });
                    };
                    base.extended = false;
                    add(piston.base, Block::Piston { piston: base }, true);
                }
                if !motion.token_done {
                    if let Some(token) = motion.token {
                        add(
                            self.program.projection_positions[motion.destination],
                            self.program.tokens[token].block,
                            false,
                        );
                    }
                }
            }
        }
        if parts.iter().any(|part| !positions.insert(part.pos)) {
            return Err(ScratchError {
                position: None,
                reason: "overlapping piston motion parts cannot be exported",
            });
        }
        Ok(())
    }
}
