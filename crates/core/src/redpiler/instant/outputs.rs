//! Compiled electrical ports for consumers of conditional geometry.
use super::boolean::{BooleanArena, Expr, GeometryPart, Variable};
use crate::redpiler::analysis::ports::ConsumerInput;
use mchprs_blocks::BlockPos;

#[derive(Debug, Clone)]
pub(crate) struct PowerTerm {
    pub guard: Expr,
    /// None denotes a redstone block, whose strength is always fifteen.
    pub source: Option<BlockPos>,
    pub attenuation: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct OutputPort {
    pub consumer: BlockPos,
    pub input: ConsumerInput,
    pub terms: Vec<PowerTerm>,
    pub initial_strength: u8,
}

impl OutputPort {
    pub fn entry_strength(&self, arena: &BooleanArena, read: impl Fn(BlockPos) -> u8) -> u8 {
        self.terms
            .iter()
            .filter_map(|term| {
                let enabled = arena.evaluate(term.guard, |variable| match variable {
                    Variable::Geometry { part, .. } => {
                        matches!(part, GeometryPart::FarPayload | GeometryPart::Head)
                    }
                    _ => unreachable!("output guards depend only on compiled geometry"),
                });
                enabled.then(|| {
                    term.source
                        .map_or(15, &read)
                        .saturating_sub(term.attenuation)
                })
            })
            .max()
            .unwrap_or(0)
    }
}

/// Deliberately exclude entities, analog overrides and blocks with their own
/// update behavior. These payloads have fixed material properties.
pub(crate) fn supported_payload(block: mchprs_blocks::blocks::Block) -> bool {
    use mchprs_blocks::blocks::Block;
    block == Block::RedstoneBlock
        || (block.is_solid()
            && block.is_cube()
            && !block.is_transparent()
            && !block.has_block_entity()
            && !crate::redstone::has_neighbor_update(block)
            && !crate::redstone::comparator::has_override(block)
            // Unknown states (including powered targets) use fallback geometry.
            && !matches!(block, Block::Unknown { .. }))
}
