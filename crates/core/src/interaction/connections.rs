use crate::redstone;
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::HashMap;

// One nibble per vanilla state. The bits follow the horizontal BlockFace IDs.
const FACES: &[u8; mchprs_blocks::generated::STATE_PROPERTIES.len().div_ceil(2)] = include_bytes!("../../../../mc_data/1.21.5/connection_faces.bin");

pub(crate) const SIDES: [(BlockFace, &str); 4] = [
    (BlockFace::North, "north"),
    (BlockFace::South, "south"),
    (BlockFace::West, "west"),
    (BlockFace::East, "east"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Pane,
    WoodenFence,
    NetherBrickFence,
}

fn family(block: Block) -> Option<Family> {
    if !matches!(block, Block::Unknown { .. }) {
        return None;
    }
    Some(match block.get_name() {
        "iron_bars" => Family::Pane,
        "nether_brick_fence" => Family::NetherBrickFence,
        name if name.ends_with("glass_pane") => Family::Pane,
        name if name.ends_with("_fence") => Family::WoodenFence,
        _ => return None,
    })
}

pub(crate) fn has_connections(block: Block) -> bool {
    family(block).is_some()
}

fn connection(own_family: Family, neighbor: Block, side: BlockFace) -> &'static str {
    let state = neighbor.get_id() as usize;
    let mask = FACES.get(state / 2).copied().unwrap_or(0) >> (state % 2 * 4);
    let sturdy = mask & (1 << (side.opposite().get_id() - 2)) != 0;
    let connects = sturdy
        || family(neighbor) == Some(own_family)
        || match own_family {
            Family::Pane => neighbor.get_name().ends_with("_wall"),
            Family::WoodenFence | Family::NetherBrickFence => {
                neighbor.get_name().ends_with("_fence_gate") && neighbor.facing().is_some_and(|facing| facing != side && facing != side.opposite())
            }
        };
    if connects { "true" } else { "false" }
}

pub(crate) fn state_at(mut block: Block, world: &impl World, pos: BlockPos) -> Block {
    if let Some(family) = family(block) {
        let properties: HashMap<_, _> = SIDES
            .into_iter()
            .map(|(face, key)| (key, connection(family, world.get_block(pos.offset(face)), face)))
            .filter(|&(key, value)| block.property(key) != Some(value))
            .collect();
        if !properties.is_empty() {
            block.set_properties(properties);
        }
    }
    block
}

pub(super) fn update_side(mut block: Block, world: &mut impl World, pos: BlockPos, side: BlockFace) {
    if let Some(family) = family(block)
        && let Some((_, key)) = SIDES.into_iter().find(|&(face, _)| face == side)
    {
        let value = connection(family, world.get_block(pos.offset(side)), side);
        if block.property(key) != Some(value) {
            block.set_properties(HashMap::from([(key, value)]));
            write_state(block, world, pos);
        }
    }
}

pub(crate) fn write_state(block: Block, world: &mut impl World, pos: BlockPos) {
    if world.set_block(pos, block) {
        for face in BlockFace::values() {
            let adjacent = pos.offset(face);
            let block = world.get_block(adjacent);
            if matches!(block, Block::Observer { .. }) {
                redstone::update(block, world, adjacent, Some(face.opposite()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::{destroy, place_in_world};
    use crate::plot::PlotWorld;
    use crate::world::storage::Chunk;

    #[test]
    fn pane_connections_follow_placement_and_breaking_without_losing_waterlogging() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let pos = BlockPos::new(8, 22, 8);
        let east = pos.offset(BlockFace::East);
        let west = pos.offset(BlockFace::West);
        let mut pane = Block::from_name("blue_stained_glass_pane").unwrap();
        pane.set_properties(HashMap::from([("waterlogged", "true")]));
        let bars = Block::from_name("iron_bars").unwrap();
        let planks = Block::from_name("pale_oak_planks").unwrap();

        place_in_world(planks, &mut world, east, &None);
        place_in_world(pane, &mut world, pos, &None);
        place_in_world(bars, &mut world, west, &None);
        assert_eq!(world.get_block(pos).property("east"), Some("true"));
        assert_eq!(world.get_block(pos).property("west"), Some("true"));
        assert_eq!(world.get_block(west).property("east"), Some("true"));

        destroy(planks, &mut world, east);
        let slab = Block::from_name("oak_slab").unwrap();
        place_in_world(slab, &mut world, east, &None);
        let pane = world.get_block(pos);
        assert_eq!(pane.property("east"), Some("false"));
        assert_eq!(pane.property("west"), Some("true"));
        assert_eq!(pane.waterlogged(), Some(true));
    }

    #[test]
    fn fence_connections_distinguish_wood_nether_brick_and_gate_direction() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let pos = BlockPos::new(8, 22, 8);
        let east = pos.offset(BlockFace::East);
        let fence = Block::from_name("pale_oak_fence").unwrap();
        let other_wood = Block::from_name("oak_fence").unwrap();
        let nether_brick = Block::from_name("nether_brick_fence").unwrap();
        let mut gate = Block::from_name("oak_fence_gate").unwrap();
        gate.set_properties(HashMap::from([("facing", "north"), ("open", "true")]));

        place_in_world(other_wood, &mut world, pos.offset(BlockFace::North), &None);
        place_in_world(nether_brick, &mut world, pos.offset(BlockFace::South), &None);
        place_in_world(gate, &mut world, east, &None);
        place_in_world(fence, &mut world, pos, &None);
        assert_eq!(world.get_block(pos).property("north"), Some("true"));
        assert_eq!(world.get_block(pos).property("south"), Some("false"));
        assert_eq!(world.get_block(pos).property("east"), Some("true"));

        gate.set_properties(HashMap::from([("facing", "east")]));
        place_in_world(gate, &mut world, east, &None);
        assert_eq!(world.get_block(pos).property("east"), Some("false"));
    }
}
