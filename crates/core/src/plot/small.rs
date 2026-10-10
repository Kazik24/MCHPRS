use super::Plot;
use crate::messages;
use crate::player::{PacketSender, Player, SmallAnimal, SmallModel};
use crate::world::World;
use mchprs_network::packets::clientbound::*;
use mchprs_network::packets::PacketEncoder;

fn fox_mouth_equipment(player: &Player, entity_id: u32) -> PacketEncoder {
    CEntityEquipment {
        entity_id: entity_id as i32,
        equipment: vec![CEntityEquipmentEquipment {
            slot: 0,
            item: player.inventory[player.selected_slot as usize + 36]
                .as_ref()
                .map(crate::container::slot_data),
        }],
    }
    .encode()
}

pub(super) fn spawn_model(viewer: &impl PacketSender, player: &Player) {
    let Some(model) = &player.small_model else {
        return;
    };
    viewer.send_packet(
        &CSpawnEntity {
            entity_id: model.entity_id as i32,
            object_uuid: model.uuid,
            entity_type: player.small_animal.entity_type(),
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
    let mut metadata = vec![CEntityMetadataEntry {
        index: 5,
        metadata_type: 8,
        value: vec![1], // The visual model follows the player, without gravity.
    }];
    if player.small_animal == SmallAnimal::BabyOcelot {
        metadata.push(CEntityMetadataEntry {
            index: 16,
            metadata_type: 8,
            value: vec![1],
        });
    }
    viewer.send_packet(
        &CEntityMetadata {
            entity_id: model.entity_id as i32,
            metadata,
        }
        .encode(),
    );
    if player.small_animal == SmallAnimal::Fox {
        viewer.send_packet(&fox_mouth_equipment(player, model.entity_id));
    }
}

impl Plot {
    pub(super) fn set_small_animal(&mut self, player: usize, animal: SmallAnimal) {
        if self.players[player].small_animal == animal {
            self.players[player].send_system_message(&messages::small_animal(animal.name()));
            return;
        }
        let old_scale = self.players[player].scale();
        self.players[player].small_animal = animal;
        if let Some(model) = self.players[player].small_model.take() {
            self.destroy_entity(model.entity_id);
            self.players[player].small_model = Some(SmallModel::new());
            let scale = (old_scale != self.players[player].scale()).then(|| {
                CEntityScale {
                    entity_id: self.players[player].entity_id as i32,
                    scale: self.players[player].scale(),
                }
                .encode()
            });
            for (index, viewer) in self.players.iter().enumerate() {
                if let Some(scale) = &scale {
                    viewer.send_packet(scale);
                }
                if index != player {
                    spawn_model(viewer, &self.players[player]);
                }
            }
        }
        self.players[player].send_system_message(&messages::small_animal(animal.name()));
    }

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
            let (model_id, position, head, mouth_changed) = {
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
                let moved = model.last_pose != Some(pose);
                if moved {
                    model.last_pose = Some(pose);
                }
                let mouth_changed = if player.small_animal == SmallAnimal::Fox {
                    let item = player.inventory[player.selected_slot as usize + 36].as_ref();
                    let signature = item
                        .map(crate::container::item_components)
                        .unwrap_or_default();
                    if model.last_mouth_item.as_ref() == Some(&signature) {
                        false
                    } else {
                        model.last_mouth_item = Some(signature);
                        true
                    }
                } else {
                    false
                };
                let position = moved.then(|| {
                    CEntityTeleport {
                        entity_id: model.entity_id as i32,
                        x: player.pos.x,
                        y: player.pos.y,
                        z: player.pos.z,
                        yaw: player.yaw,
                        pitch: player.pitch,
                        on_ground: player.on_ground,
                    }
                    .encode()
                });
                let head = moved.then(|| {
                    CEntityHeadLook {
                        entity_id: model.entity_id as i32,
                        yaw: player.yaw,
                    }
                    .encode()
                });
                (model.entity_id, position, head, mouth_changed)
            };
            if position.is_none() && !mouth_changed {
                continue;
            }
            let mouth = mouth_changed.then(|| fox_mouth_equipment(&self.players[owner], model_id));
            for (index, viewer) in self.players.iter().enumerate() {
                if index == owner {
                    continue;
                }
                if let Some(position) = &position {
                    viewer.send_packet(position);
                }
                if let Some(head) = &head {
                    viewer.send_packet(head);
                }
                if let Some(mouth) = &mouth {
                    viewer.send_packet(mouth);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
