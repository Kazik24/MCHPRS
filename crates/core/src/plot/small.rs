use super::Plot;
use crate::messages;
use crate::player::{PacketSender, Player, SmallModel};
use crate::world::World;
use mchprs_network::packets::clientbound::*;

pub(super) fn spawn_model(viewer: &impl PacketSender, player: &Player) {
    let Some(model) = &player.small_model else {
        return;
    };
    viewer.send_packet(
        &CSpawnEntity {
            entity_id: model.entity_id as i32,
            object_uuid: model.uuid,
            entity_type: mchprs_network::generated::OCELOT_ENTITY,
            x: player.pos.x,
            y: player.pos.y,
            z: player.pos.z,
            pitch: player.pitch,
            yaw: player.yaw,
            data: 0,
            velocity_x: 0,
            velocity_y: 0,
            velocity_z: 0,
        }
        .encode(),
    );
    viewer.send_packet(
        &CEntityMetadata {
            entity_id: model.entity_id as i32,
            metadata: vec![CEntityMetadataEntry {
                index: 5,
                metadata_type: 8,
                value: vec![1], // The visual model follows the player, without gravity.
            }],
        }
        .encode(),
    );
}

impl Plot {
    pub(super) fn set_small(&mut self, player: usize, enabled: bool) -> bool {
        let current = &self.players[player];
        if current.small_model.is_some() == enabled {
            current.send_system_message(if enabled {
                messages::SMALL_ENABLED
            } else {
                messages::SMALL_DISABLED
            });
            return true;
        }
        if !enabled
            && !super::compass::body_clear(current.pos, 1.0, &|pos| {
                if !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z) {
                    None
                } else {
                    Some(self.world.get_block(pos))
                }
            })
        {
            current.send_error_message(messages::SMALL_NO_ROOM);
            return false;
        }
        if let Some(model) = self.players[player].small_model.take() {
            self.destroy_entity(model.entity_id);
        }
        self.players[player].small_model = enabled.then(SmallModel::new);
        let current = &self.players[player];
        let scale = CEntityScale {
            entity_id: current.entity_id as i32,
            scale: current.scale(),
        }
        .encode();
        let metadata = current.entity_metadata_packet(false);
        let own_metadata = current.entity_metadata_packet(true);
        let equipment = current.entity_equipment_packet();
        for (index, viewer) in self.players.iter().enumerate() {
            viewer.send_packet(&scale);
            viewer.send_packet(if index == player {
                &own_metadata
            } else {
                &metadata
            });
            // A proxy in the owner's camera would intercept their block clicks.
            if index != player {
                viewer.send_packet(&equipment);
                spawn_model(viewer, current);
            }
        }
        current.send_system_message(if enabled {
            messages::SMALL_ENABLED
        } else {
            messages::SMALL_DISABLED
        });
        true
    }

    pub(super) fn sync_small_models(&mut self) {
        for owner in 0..self.players.len() {
            let player = &mut self.players[owner];
            let Some(model) = &mut player.small_model else {
                continue;
            };
            let pose = [
                player.pos.x,
                player.pos.y,
                player.pos.z,
                f64::from(player.yaw),
                f64::from(player.pitch),
            ];
            if model.last_pose == Some(pose) {
                continue;
            }
            model.last_pose = Some(pose);
            let position = CEntityTeleport {
                entity_id: model.entity_id as i32,
                x: player.pos.x,
                y: player.pos.y,
                z: player.pos.z,
                yaw: player.yaw,
                pitch: player.pitch,
                on_ground: player.on_ground,
            }
            .encode();
            let head = CEntityHeadLook {
                entity_id: model.entity_id as i32,
                yaw: player.yaw,
            }
            .encode();
            for (index, viewer) in self.players.iter().enumerate() {
                if index == owner {
                    continue;
                }
                viewer.send_packet(&position);
                viewer.send_packet(&head);
            }
        }
    }
}

#[cfg(test)]
mod tests;
