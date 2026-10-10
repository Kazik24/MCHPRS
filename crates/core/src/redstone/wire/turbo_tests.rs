use super::*;
use crate::world::{BlockAction, storage::Chunk};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_world::{PistonState, TickPriority};

#[test]
fn compact_node_ids_preserve_boundary_indices() {
    for index in [0, 4095, 4096, u32::MAX as usize] {
        assert_eq!(NodeId::new(index).index(), index);
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
#[should_panic(expected = "wire walk exceeds u32 node capacity")]
fn compact_node_ids_cannot_silently_wrap() {
    NodeId::new(u32::MAX as usize + 1);
}

/// An omitted callback must not even read the world: any observable work panics.
struct UntouchedWorld;
impl World for UntouchedWorld {
    fn get_block_raw(&self, _: BlockPos) -> u32 {
        panic!("world read")
    }
    fn set_block_raw(&mut self, _: BlockPos, _: u32) -> bool {
        panic!("world write")
    }
    fn delete_block_entity(&mut self, _: BlockPos) {
        panic!("entity deletion")
    }
    fn get_block_entity(&self, _: BlockPos) -> Option<&BlockEntity> {
        panic!("entity read")
    }
    fn get_block_entity_mut(&mut self, _: BlockPos) -> Option<&mut BlockEntity> {
        panic!("entity write")
    }
    fn piston_state(&self) -> &PistonState {
        panic!("piston read")
    }
    fn piston_state_mut(&mut self) -> &mut PistonState {
        panic!("piston write")
    }
    fn set_block_entity(&mut self, _: BlockPos, _: BlockEntity) {
        panic!("entity write")
    }
    fn get_chunk(&self, _: i32, _: i32) -> Option<&Chunk> {
        panic!("chunk read")
    }
    fn get_chunk_mut(&mut self, _: i32, _: i32) -> Option<&mut Chunk> {
        panic!("chunk write")
    }
    fn schedule_tick(&mut self, _: BlockPos, _: u32, _: TickPriority) {
        panic!("tick scheduled")
    }
    fn schedule_half_tick(&mut self, _: BlockPos, _: u32, _: TickPriority) {
        panic!("tick scheduled")
    }
    fn pending_tick_at(&mut self, _: BlockPos) -> bool {
        panic!("scheduler read")
    }
    fn block_action(&mut self, _: BlockPos, _: BlockAction) {
        panic!("block action")
    }
    fn is_cursed(&self) -> bool {
        panic!("world flag read")
    }
    fn execute_command_block(&mut self, _: &str, _: &str) -> Result<(), String> {
        panic!("command")
    }
    fn play_sound(&mut self, _: BlockPos, _: i32, _: i32, _: f32, _: f32) {
        panic!("sound")
    }
}

#[test]
fn omitted_wire_callbacks_are_inert_for_every_registry_state() {
    let mut world = UntouchedWorld;
    let pos = BlockPos::new(40, 30, 40);
    let mut omitted = 0;
    for id in 0..mchprs_blocks::generated::STATE_PROPERTIES.len() as u32 {
        let block = Block::from_id(id);
        if redstone::has_neighbor_update(block) {
            continue;
        }
        omitted += 1;
        redstone::update(block, &mut world, pos, None);
        for face in BlockFace::values() {
            redstone::update(block, &mut world, pos, Some(face));
        }
    }
    assert!(omitted > 0);
    for name in [
        "redstone_wire",
        "redstone_torch",
        "redstone_wall_torch",
        "repeater",
        "comparator",
        "redstone_lamp",
        "hopper",
        "iron_trapdoor",
        "piston",
        "sticky_piston",
        "piston_head",
        "observer",
        "note_block",
        "command_block",
        "chain_command_block",
        "repeating_command_block",
    ] {
        assert!(
            redstone::has_neighbor_update(Block::from_name(name).unwrap()),
            "active component omitted: {name}"
        );
    }
}

#[test]
fn reused_walk_scratch_observes_changed_blocks_and_other_worlds() {
    use crate::plot::{PLOT_WIDTH, PlotWorld};
    let make_world = || {
        let chunks = (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect();
        PlotWorld::from_chunks(0, 0, chunks, Default::default())
    };
    let root = BlockPos::new(8, 8, 8);
    let source = BlockPos::new(7, 8, 8);
    let mut world = make_world();
    for x in 8..=14 {
        world.set_block(BlockPos::new(x, 7, 8), Block::Stone {});
        world.set_block(
            BlockPos::new(x, 8, 8),
            Block::from_name("redstone_wire").unwrap(),
        );
    }
    for powered in [true, false, true, false] {
        world.set_block(
            source,
            if powered {
                Block::RedstoneBlock {}
            } else {
                Block::Air
            },
        );
        redstone::update(world.get_block(root), &mut world, root, None);
        for x in 8..=14 {
            assert_eq!(
                unwrap_wire(world.get_block(BlockPos::new(x, 8, 8))).power,
                if powered { 15 - (x - 8) as u8 } else { 0 }
            );
        }
        // The same coordinates in another world must never inherit the wire graph.
        let mut empty = make_world();
        RedstoneWireTurbo::update_surrounding_neighbors(&mut empty, root);
        assert_eq!(empty.get_block(root), Block::Air);
    }
}
