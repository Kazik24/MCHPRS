use super::Plot;
use crate::messages;
use crate::player::PacketSender;
use crate::world::World;
use mchprs_blocks::{BlockPos, block_entities::BlockEntity, blocks::Block};
use mchprs_network::packets::serverbound::SUpdateCommandBlock;

#[cfg(test)]
mod tests;

impl Plot {
    pub(super) fn handle_update_command_block(&mut self, packet: SUpdateCommandBlock, player: usize) {
        if self.git_checkout_locked() {
            return;
        }
        let pos = BlockPos::from_packed(packet.pos);
        let data = &mut self.players[player];
        if !matches!(data.gamemode, crate::player::Gamemode::Creative)
            || !data.has_permission("commands.commandblock.edit")
            || !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !super::picking::within_reach(data.eye_position(), pos)
        {
            return;
        }
        if !data.can_build_action("commandblock", self.owner, (self.world.x, self.world.z)) {
            data.send_no_permission_message();
            return;
        }
        let old = self.world.get_block(pos);
        if !old.is_command_block() {
            return;
        }
        if let Err(error) = self.reset_redpiler() {
            self.players[player].send_error_message(&error.to_string());
            return;
        }
        let name = match packet.mode {
            0 => "chain_command_block",
            1 => "repeating_command_block",
            _ => "command_block",
        };
        let mut block = Block::from_name(name).unwrap();
        block.set_properties(std::collections::HashMap::from([
            ("facing", old.property("facing").unwrap_or("north")),
            ("conditional", if packet.flags & 2 != 0 { "true" } else { "false" }),
        ]));
        let mut entity = match self.world.get_block_entity(pos) {
            Some(BlockEntity::CommandBlock(entity)) => (**entity).clone(),
            _ => Default::default(),
        };
        entity.command = packet.command;
        entity.track_output = packet.flags & 1 != 0;
        entity.automatic = packet.flags & 4 != 0;
        entity.success_count = 0;
        entity.last_output = None;
        entity.last_execution = -1;
        self.world.set_block(pos, block);
        self.world.flush_block_changes();
        self.world.set_block_entity(pos, BlockEntity::CommandBlock(Box::new(entity)));
        crate::redstone::command_block::update(&mut self.world, pos);
        self.players[player].send_system_message(messages::COMMAND_BLOCK_UPDATED);
    }

}
