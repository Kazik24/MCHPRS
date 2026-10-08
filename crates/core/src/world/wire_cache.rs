use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockDirection, BlockPos};
use std::sync::{Arc, OnceLock};

/// State relevant to structural routing certificates, independent of current
/// output strength. Piston geometry and manually selected dust sides stay exact.
pub(crate) fn routing_state(mut block: Block) -> u32 {
    match &mut block {
        Block::RedstoneWire { wire } => wire.power = 0,
        Block::RedstoneRepeater { repeater } => repeater.powered = false,
        Block::RedstoneComparator { comparator } => comparator.powered = false,
        Block::Observer { observer } => observer.powered = false,
        Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => *lit = false,
        Block::Lever { lever } => lever.powered = false,
        Block::StoneButton { button } => button.powered = false,
        Block::StonePressurePlate { powered } => *powered = false,
        Block::RedstoneLamp { lit } => *lit = false,
        Block::NoteBlock { powered, .. } => *powered = false,
        _ => {}
    }
    if matches!(block, Block::Unknown { .. }) {
        block = block
            .with_copper_bulb_state(false, false)
            .or_else(|| block.with_pressure_plate_power(false))
            .unwrap_or(block);
    }
    block.get_id()
}

#[derive(Clone, Copy, Debug)]
pub struct Neighbor {
    pub pos: BlockPos,
    pub cell: Option<u32>,
}

pub(crate) fn positions(pos: BlockPos) -> [BlockPos; 24] {
    let BlockPos { x, y, z } = pos;
    [
        BlockPos::new(x - 1, y, z),
        BlockPos::new(x + 1, y, z),
        BlockPos::new(x, y - 1, z),
        BlockPos::new(x, y + 1, z),
        BlockPos::new(x, y, z - 1),
        BlockPos::new(x, y, z + 1),
        BlockPos::new(x - 2, y, z),
        BlockPos::new(x - 1, y - 1, z),
        BlockPos::new(x - 1, y + 1, z),
        BlockPos::new(x - 1, y, z - 1),
        BlockPos::new(x - 1, y, z + 1),
        BlockPos::new(x + 2, y, z),
        BlockPos::new(x + 1, y - 1, z),
        BlockPos::new(x + 1, y + 1, z),
        BlockPos::new(x + 1, y, z - 1),
        BlockPos::new(x + 1, y, z + 1),
        BlockPos::new(x, y - 2, z),
        BlockPos::new(x, y - 1, z - 1),
        BlockPos::new(x, y - 1, z + 1),
        BlockPos::new(x, y + 2, z),
        BlockPos::new(x, y + 1, z - 1),
        BlockPos::new(x, y + 1, z + 1),
        BlockPos::new(x, y, z - 2),
        BlockPos::new(x, y, z + 2),
    ]
}

type Neighborhood = Arc<[Neighbor; 24]>;
type Section = Box<[Option<Neighborhood>; 4096]>;

/// Immutable canonical addresses, never live states or oriented traversal data.
#[derive(Default)]
pub(crate) struct Topology {
    sections: Vec<Option<Section>>,
    entries: usize,
}

impl Topology {
    pub fn clear(&mut self) {
        self.sections.clear();
        self.entries = 0;
    }

    pub fn neighborhood(
        &mut self,
        cell: u32,
        pos: BlockPos,
        location: impl Fn(BlockPos) -> Option<u32>,
    ) -> Neighborhood {
        let section = (cell >> 12) as usize;
        let local = (cell & 4095) as usize;
        if let Some(cached) = self
            .sections
            .get(section)
            .and_then(|s| s.as_ref())
            .and_then(|s| s[local].as_ref())
        {
            return cached.clone();
        }
        let value = Arc::new(positions(pos).map(|pos| Neighbor {
            pos,
            cell: location(pos),
        }));
        // Bound persistent retention; saturation only disables further admissions.
        if self.entries < 1_048_576 {
            if self.sections.len() <= section {
                self.sections.resize_with(section + 1, || None);
            }
            let section = self.sections[section]
                .get_or_insert_with(|| Box::new(std::array::from_fn(|_| None)));
            section[local] = Some(value.clone());
            self.entries += 1;
        }
        value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Facts {
    pub solid: bool,
    pub transparent: bool,
    pub updates: bool,
    pub connections: u8,
}

fn uncached_facts(block: Block) -> Facts {
    let connections = [
        BlockDirection::North,
        BlockDirection::South,
        BlockDirection::East,
        BlockDirection::West,
    ]
    .into_iter()
    .enumerate()
    .fold(0, |mask, (i, side)| {
        mask | (u8::from(crate::redstone::wire::can_connect_to_uncached(block, side)) << i)
    });
    Facts {
        solid: block.is_solid(),
        transparent: block.is_transparent(),
        updates: crate::redstone::has_neighbor_update(block),
        connections,
    }
}

pub(crate) fn facts(block: Block) -> Facts {
    static FACTS: OnceLock<Box<[Facts]>> = OnceLock::new();
    FACTS
        .get_or_init(|| {
            (0..mchprs_blocks::generated::STATE_PROPERTIES.len() as u32)
                .map(|id| uncached_facts(Block::from_id(id)))
                .collect()
        })
        .get(block.get_id() as usize)
        .copied()
        .unwrap_or_else(|| uncached_facts(block))
}

pub(crate) fn connects(block: Block, side: BlockDirection) -> bool {
    let bit = match side {
        BlockDirection::North => 0,
        BlockDirection::South => 1,
        BlockDirection::East => 2,
        BlockDirection::West => 3,
    };
    facts(block).connections & (1 << bit) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cached_registry_fact_matches_original_block_behavior() {
        for id in 0..mchprs_blocks::generated::STATE_PROPERTIES.len() as u32 {
            let block = Block::from_id(id);
            assert_eq!(facts(block), uncached_facts(block), "state {id}");
        }
        assert_eq!(
            facts(Block::Unknown { id: u32::MAX }),
            uncached_facts(Block::Unknown { id: u32::MAX })
        );
    }

    #[test]
    fn canonical_geometry_reuses_entries_and_clear_drops_old_namespace() {
        let mut topology = Topology::default();
        let pos = BlockPos::new(-1, 16, 255);
        let a = topology.neighborhood(4095, pos, |_| None);
        let b = topology.neighborhood(4095, pos, |_| None);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.map(|n| n.pos), positions(pos));
        topology.clear();
        let c = topology.neighborhood(4095, BlockPos::new(255, 0, -1), |_| None);
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(c.map(|n| n.pos), positions(BlockPos::new(255, 0, -1)));
    }
}
