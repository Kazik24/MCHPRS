mod instant_piston;
pub mod storage;
pub use instant_piston::InstantPistonCache;
pub(crate) use instant_piston::PowerRoute;
pub(crate) mod wire_cache;
pub use wire_cache::Neighbor as WireNeighbor;

use crate::messages;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockPos};
pub use mchprs_world::PistonAction;
use mchprs_world::TickPriority;
use storage::Chunk;

pub trait World {
    /// Stateful assemblies handle their own geometry and attachment changes.
    fn dispatch_neighbor_shape_update(&mut self, _pos: BlockPos, _direction: BlockFace) -> bool {
        false
    }
    /// A compiled consumer channel replaces physical electrical input, not analog overrides.
    fn resolved_redstone_input(&self, _pos: BlockPos, _side: bool) -> Option<u8> {
        None
    }

    /// Return true when a compiled owner has delivered or suppressed this callback.
    fn dispatch_redstone_update(
        &mut self,
        _pos: BlockPos,
        _dir: Option<mchprs_blocks::BlockFace>,
        _source: Option<BlockPos>,
    ) -> bool {
        false
    }

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

    /// Native motion restoration retains its certificate until completion is checked.
    fn delete_piston_entity(&mut self, pos: BlockPos) {
        self.delete_block_entity(pos);
    }

    /// Returns a reference to the block entity at `pos` if it exists.
    /// Returns None if there is no block entity at `pos`.
    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity>;
    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity>;
    fn set_piston_progress(&mut self, pos: BlockPos, progress: f32) {
        if let Some(BlockEntity::MovingPiston(entity)) = self.get_block_entity_mut(pos) {
            entity.set_progress(progress);
        }
    }
    fn piston_state(&self) -> &mchprs_world::PistonState;
    fn piston_state_mut(&mut self) -> &mut mchprs_world::PistonState;

    fn piston_motion_index(&self, pos: BlockPos, identity: Option<u64>) -> Option<usize> {
        self.piston_state()
            .motions
            .iter()
            .position(|m| m.pos == pos && identity.is_none_or(|id| id == m.identity))
    }

    fn advance_piston_motion(&mut self, index: usize) -> (bool, f32) {
        let state = self.piston_state_mut();
        state.motions[index].advance(state.logical_tick)
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

    /// Read a live state through an address returned by this world's wire cache.
    fn get_wire_block_raw(&self, location: WireNeighbor) -> u32 {
        self.get_block_raw(location.pos)
    }

    /// Opt-in worlds must revoke affected certificates on arbitrary block/entity edits.
    fn instant_piston_cache(&self) -> Option<&InstantPistonCache> {
        None
    }

    fn instant_piston_cache_mut(&mut self) -> Option<&mut InstantPistonCache> {
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
        Err(messages::COMMAND_BLOCK_EXECUTION_UNAVAILABLE.into())
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
    fn level_event_for_action(&mut self, _pos: BlockPos, _event: i32, _excluded: u128) {}
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

/// Executes the given function for each block excluding most air blocks
pub fn for_each_block_optimized<F, W: World>(
    world: &W,
    first_pos: BlockPos,
    second_pos: BlockPos,
    mut f: F,
) where
    F: FnMut(BlockPos),
{
    for (first, last) in section_bounds(first_pos, second_pos) {
        if section_has_blocks(world, first) {
            for_each_position(first, last, &mut f);
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
    for (first, last) in section_bounds(first_pos, second_pos) {
        if section_has_blocks(world, first) {
            for_each_position(first, last, |pos| f(world, pos));
        }
    }
}

/// Intersect the selection with each aligned section, including partial sections.
fn section_bounds(
    first_pos: BlockPos,
    second_pos: BlockPos,
) -> impl Iterator<Item = (BlockPos, BlockPos)> {
    let first = first_pos.min(second_pos);
    let last = first_pos.max(second_pos);
    (first.x.div_euclid(16)..=last.x.div_euclid(16)).flat_map(move |chunk_x| {
        (first.z.div_euclid(16)..=last.z.div_euclid(16)).flat_map(move |chunk_z| {
            (first.y.div_euclid(16)..=last.y.div_euclid(16)).map(move |section_y| {
                let origin = BlockPos::new(chunk_x * 16, section_y * 16, chunk_z * 16);
                (
                    first.max(origin),
                    last.min(origin + BlockPos::new(15, 15, 15)),
                )
            })
        })
    })
}

fn section_has_blocks(world: &impl World, first: BlockPos) -> bool {
    world
        .get_chunk(first.x.div_euclid(16), first.z.div_euclid(16))
        .expect("block iteration requires loaded chunks")
        .sections[first.y as usize / 16]
        .block_count()
        > 0
}

fn for_each_position(first: BlockPos, last: BlockPos, mut f: impl FnMut(BlockPos)) {
    for y in first.y..=last.y {
        for z in first.z..=last.z {
            for x in first.x..=last.x {
                f(BlockPos::new(x, y, z));
            }
        }
    }
}

#[cfg(test)]
mod tests;
