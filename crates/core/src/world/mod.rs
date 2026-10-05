pub mod storage;
pub(crate) mod wire_cache;
pub use wire_cache::Neighbor as WireNeighbor;

use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::BlockPos;
pub use mchprs_world::PistonAction;
use mchprs_world::TickPriority;
use storage::Chunk;

pub trait World {
    /// Returns the block located at `pos`
    fn get_block(&self, pos: BlockPos) -> Block {
        Block::from_id(self.get_block_raw(pos))
    }

    /// Returns the block state id of the block at `pos`
    fn get_block_raw(&self, pos: BlockPos) -> u32;

    /// Sets the block at `pos`.
    /// This function may have side effects such as sending update block packets to the player.
    /// Returns true if the block was changed.
    fn set_block(&mut self, pos: BlockPos, block: Block) -> bool {
        let block_id = Block::get_id(block);
        self.set_block_raw(pos, block_id)
    }

    /// Sets a block in storage without any other side effects. Returns true if a block was changed.
    fn set_block_raw(&mut self, pos: BlockPos, block: u32) -> bool;

    /// Removes a block entity at `pos` if it exists.
    fn delete_block_entity(&mut self, pos: BlockPos);

    /// Returns a reference to the block entity at `pos` if it exists.
    /// Returns None if there is no block entity at `pos`.
    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity>;
    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity>;
    fn piston_state(&self) -> &mchprs_world::PistonState;
    fn piston_state_mut(&mut self) -> &mut mchprs_world::PistonState;

    fn piston_motion_index(&self, pos: BlockPos, identity: Option<u64>) -> Option<usize> {
        self.piston_state()
            .motions
            .iter()
            .position(|m| m.pos == pos && identity.is_none_or(|id| id == m.identity))
    }

    fn advance_piston_motion(&mut self, index: usize) -> (bool, f32) {
        let s = self.piston_state_mut();
        let m = &mut s.motions[index];
        m.last_tick = s.logical_tick;
        m.previous_progress = m.progress;
        let complete = m.progress >= 1.0;
        if !complete {
            m.progress = (m.progress + 0.5).min(1.0);
        }
        (complete, m.previous_progress)
    }

    fn remove_piston_motion(&mut self, index: usize) {
        self.piston_state_mut().motions.remove(index);
    }

    fn set_piston_carried_entity(&mut self, pos: BlockPos, entity: Option<Box<BlockEntity>>) {
        if let Some(i) = self.piston_motion_index(pos, None) {
            self.piston_state_mut().motions[i].carried_entity = entity;
        }
    }

    fn enqueue_piston_event(&mut self, event: mchprs_world::PistonEvent) {
        if !self.piston_state().events.contains(&event) {
            self.piston_state_mut().events.push_back(event);
        }
    }

    /// Compact section/local address, when this world supports indexed wire walks.
    fn wire_location(&self, _pos: BlockPos) -> Option<u32> {
        None
    }

    fn wire_neighborhood(&self, _pos: BlockPos) -> Option<std::sync::Arc<[WireNeighbor; 24]>> {
        None
    }

    /// Sets the block entity at `pos`, overwriting any other block entity that was there prior.
    fn set_block_entity(&mut self, pos: BlockPos, block_entity: BlockEntity);

    /// Returns an immutable reference to the chunk at `x` and `z` chunk coordinates.
    /// Returns None if the chunk does not exist in this world.
    fn get_chunk(&self, x: i32, z: i32) -> Option<&Chunk>;

    /// Returns a mutable reference to the chunk at `x` and `z` chunk coordinates.
    /// Returns None if the chunk does not exist in this world.
    fn get_chunk_mut(&mut self, x: i32, z: i32) -> Option<&mut Chunk>;

    /// Schedules a tick in the world with `delay` and `pritority`
    fn schedule_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority);

    /// Schedules a tick in the world with `delay` and `pritority`
    fn schedule_half_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority);

    /// Returns true if there is a tick entry with `pos`
    fn pending_tick_at(&mut self, pos: BlockPos) -> bool;

    fn is_cursed(&self) -> bool {
        false
    }

    fn block_action(&mut self, pos: BlockPos, action: BlockAction);

    /// Runs the supported command-block subset. Test worlds may leave it disabled.
    fn execute_command_block(&mut self, _command: &str, _source: &str) -> Result<(), String> {
        Err("Command block execution is unavailable in this world".into())
    }

    fn play_sound(
        &mut self,
        pos: BlockPos,
        sound_id: i32,
        sound_category: i32,
        volume: f32,
        pitch: f32,
    );
    /// Feedback for a successful player action, even while fast rendering.
    fn play_sound_for_action(
        &mut self,
        pos: BlockPos,
        sound_id: i32,
        category: i32,
        volume: f32,
        pitch: f32,
        _excluded: Option<u128>,
    ) {
        self.play_sound(pos, sound_id, category, volume, pitch);
    }
    fn container_opened(&mut self, _pos: BlockPos, _ty: ContainerType) {}
}

// https://wiki.vg/Block_Actions#Piston
pub enum BlockAction {
    Piston {
        action: PistonAction,
        piston: RedstonePiston,
    },
    BlockChange {
        pos: BlockPos,
        block_id: u32,
    },
}

// TODO: I have no idea how to deduplicate this in a sane way

/// Executes the given function for each block excluding most air blocks
pub fn for_each_block_optimized<F, W: World>(
    world: &W,
    first_pos: BlockPos,
    second_pos: BlockPos,
    mut f: F,
) where
    F: FnMut(BlockPos),
{
    let start_x = i32::min(first_pos.x, second_pos.x);
    let end_x = i32::max(first_pos.x, second_pos.x);

    let start_y = i32::min(first_pos.y, second_pos.y);
    let end_y = i32::max(first_pos.y, second_pos.y);

    let start_z = i32::min(first_pos.z, second_pos.z);
    let end_z = i32::max(first_pos.z, second_pos.z);

    // Iterate over chunks
    for chunk_start_x in (start_x..=end_x).step_by(16) {
        for chunk_start_z in (start_z..=end_z).step_by(16) {
            let chunk = world
                .get_chunk(chunk_start_x.div_euclid(16), chunk_start_z.div_euclid(16))
                .unwrap();
            for chunk_start_y in (start_y..=end_y).step_by(16) {
                // Check if the chunk even has non air blocks
                if chunk.sections[chunk_start_y as usize / 16].block_count() > 0 {
                    // Calculate the end position of the current chunk
                    let chunk_end_x = i32::min(chunk_start_x + 16 - 1, end_x);
                    let chunk_end_y = i32::min(chunk_start_y + 16 - 1, end_y);
                    let chunk_end_z = i32::min(chunk_start_z + 16 - 1, end_z);

                    // Iterate over each position within the current chunk
                    for y in chunk_start_y..=chunk_end_y {
                        for z in chunk_start_z..=chunk_end_z {
                            for x in chunk_start_x..=chunk_end_x {
                                let pos = BlockPos::new(x, y, z);
                                f(pos);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Executes the given function for each block excluding most air blocks
pub fn for_each_block_mut_optimized<F, W: World>(
    world: &mut W,
    first_pos: BlockPos,
    second_pos: BlockPos,
    mut f: F,
) where
    F: FnMut(&mut W, BlockPos),
{
    let start_x = i32::min(first_pos.x, second_pos.x);
    let end_x = i32::max(first_pos.x, second_pos.x);

    let start_y = i32::min(first_pos.y, second_pos.y);
    let end_y = i32::max(first_pos.y, second_pos.y);

    let start_z = i32::min(first_pos.z, second_pos.z);
    let end_z = i32::max(first_pos.z, second_pos.z);

    // Iterate over chunks
    for chunk_start_x in (start_x..=end_x).step_by(16) {
        for chunk_start_z in (start_z..=end_z).step_by(16) {
            for chunk_start_y in (start_y..=end_y).step_by(16) {
                // Check if the chunk even has non air blocks
                if world
                    .get_chunk(chunk_start_x.div_euclid(16), chunk_start_z.div_euclid(16))
                    .unwrap()
                    .sections[chunk_start_y as usize / 16]
                    .block_count()
                    > 0
                {
                    // Calculate the end position of the current chunk
                    let chunk_end_x = i32::min(chunk_start_x + 16 - 1, end_x);
                    let chunk_end_y = i32::min(chunk_start_y + 16 - 1, end_y);
                    let chunk_end_z = i32::min(chunk_start_z + 16 - 1, end_z);

                    // Iterate over each position within the current chunk
                    for y in chunk_start_y..=chunk_end_y {
                        for z in chunk_start_z..=chunk_end_z {
                            for x in chunk_start_x..=chunk_end_x {
                                let pos = BlockPos::new(x, y, z);
                                f(world, pos);
                            }
                        }
                    }
                }
            }
        }
    }
}
