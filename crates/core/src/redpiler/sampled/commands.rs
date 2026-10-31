//! Run the interpreter's command-block lifecycle against compiled state.
use super::{Runtime, executor::ScheduledComponentTick};
use crate::redstone::command_block::CommandBlockHost;
use mchprs_blocks::{BlockFace, BlockPos, block_entities::CommandBlockEntity, blocks::Block};
use mchprs_world::TickPriority;

impl CommandBlockHost for Runtime {
    fn block(&self, pos: BlockPos) -> Block {
        self.project(pos).unwrap_or(Block::Air)
    }

    fn entity(&self, pos: BlockPos) -> Option<&CommandBlockEntity> {
        self.command_blocks.get(&pos).map(Box::as_ref)
    }

    fn entity_mut(&mut self, pos: BlockPos) -> Option<&mut CommandBlockEntity> {
        self.command_blocks.get_mut(&pos).map(Box::as_mut)
    }

    fn create_entity(&mut self, pos: BlockPos) {
        self.command_blocks.entry(pos).or_default();
    }

    fn powered(&self, pos: BlockPos) -> bool {
        self.program
            .component_indices
            .get(&pos)
            .is_some_and(|&component| self.has_input(component))
    }

    fn pending(&mut self, pos: BlockPos) -> bool {
        self.program
            .component_indices
            .get(&pos)
            .is_some_and(|&component| self.pending_counts[component] != 0)
    }

    fn schedule(&mut self, pos: BlockPos) {
        let component = self.program.component_indices[&pos];
        self.schedule_half_tick(
            ScheduledComponentTick {
                component,
                block_type: Some(self.blocks[component].registry_id()),
            },
            1,
            TickPriority::Normal,
        );
    }

    fn logical_tick(&self) -> u64 {
        Runtime::logical_tick(self)
    }

    fn execute(&mut self, pos: BlockPos, command: &str, source: &str) -> Result<(), String> {
        let state = self
            .command_output_state
            .as_mut()
            .ok_or(crate::messages::COMMAND_BLOCK_EXECUTION_UNAVAILABLE)?;
        let message = state.execute(command, source)?;
        self.command_emissions.push(crate::world::CommandBlockEmission {
            pos,
            tick: self.logical_tick(),
            message,
        });
        Ok(())
    }

    fn notify(&mut self, pos: BlockPos) {
        crate::redstone::surrounding_notifications(pos, |target, dir, diagonal| {
            let Some(&component) = self.program.component_indices.get(&target) else {
                return;
            };
            let block = self.blocks[component];
            if diagonal && (matches!(block, Block::Observer { .. } | Block::Piston { .. }) || target == pos && block.is_copper_bulb()) {
                return;
            }
            match block {
                Block::Observer { observer } if observer.facing != dir.into() => {}
                Block::RedstoneWire { .. } => {
                    self.refresh_wire_power(component, self.input_strength(component));
                    if let Some(watch) = self.program.watched_by_component[component] {
                        self.check_watched_wire(watch);
                    }
                }
                _ => self.update(component),
            }
        });
        for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
            let adjacent = pos.offset(face);
            if self.block(adjacent).is_solid() {
                if let Some(&component) = self.program.component_indices.get(&adjacent.offset(face)) {
                    if matches!(self.blocks[component], Block::RedstoneComparator { .. }) {
                        self.update_comparator(component);
                    }
                }
            }
        }
    }
}

impl Runtime {
    pub(crate) fn publish_commands(&mut self, world: &mut impl crate::world::World, io_only: bool) {
        if !io_only {
            for (&pos, entity) in &self.command_blocks {
                if let Some(mchprs_blocks::block_entities::BlockEntity::CommandBlock(stored)) = world.get_block_entity_mut(pos) {
                    if stored != entity {
                        stored.clone_from(entity);
                    }
                } else {
                    world.set_block_entity(pos, mchprs_blocks::block_entities::BlockEntity::CommandBlock(entity.clone()));
                }
            }
        }
        if let Some(state) = &self.command_output_state {
            world.publish_command_block_output(state, &mut self.command_emissions);
            self.command_output_state = world.command_block_output_state();
        }
    }
}
