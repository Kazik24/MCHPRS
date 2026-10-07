//! Viewer-only block displays. They never enter the world's entity/block storage.
use super::{Marker, Plot};
use crate::config::CONFIG;
use crate::messages;
use crate::player::{allocate_entity_id, PacketSender, PlayerPos};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockColorVariant;
use mchprs_network::packets::clientbound::{
    CDestroyEntities, CEntityMetadata, CEntityMetadataEntry, CSpawnEntity, ClientBoundPacket,
};
use mchprs_network::packets::PacketEncoderExt;
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn varint(value: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.write_varint(value);
    bytes
}

fn vector(value: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..3 {
        bytes.write_float(value);
    }
    bytes
}

pub(super) fn metadata(entity_id: i32, kind: u8) -> CEntityMetadata {
    // Minecraft 1.21.5 block_display keys/types, from mc_data/1.21.5.
    let entry = |index, metadata_type, value| CEntityMetadataEntry {
        index,
        metadata_type,
        value,
    };
    CEntityMetadata {
        entity_id,
        metadata: vec![
            entry(0, 0, vec![0x40]), // Glowing.
            entry(5, 8, vec![1]),    // No gravity.
            // Center the enlarged glass around the block to avoid overlapping faces.
            entry(11, 33, vector(-0.005)),
            entry(12, 33, vector(1.01)),
            entry(16, 1, varint((15 << 4) | (15 << 20))), // Full block/sky brightness.
            entry(17, 3, 2.0f32.to_be_bytes().to_vec()),
            entry(
                22,
                1,
                varint(match kind {
                    0 => 0x39ff14,
                    1 => 0xff2d2d,
                    4 => 0xffffff,
                    _ => 0xffe23d,
                }),
            ),
            entry(
                23,
                14,
                varint(
                    Block::StainedGlass {
                        color: match kind {
                            0 => BlockColorVariant::Lime,
                            1 => BlockColorVariant::Red,
                            4 => BlockColorVariant::White,
                            _ => BlockColorVariant::Yellow,
                        },
                    }
                    .get_id() as i32,
                ),
            ),
        ],
    }
}

impl Plot {
    pub(super) fn clear_git_inspection(&mut self, player: usize) {
        if let Some((_, entity_id, _)) = self.git.inspections.remove(&self.players[player].uuid) {
            self.players[player].send_packet(
                &CDestroyEntities {
                    entity_ids: vec![entity_id],
                }
                .encode(),
            );
        }
    }

    pub(super) fn show_git_inspection(&mut self, player: usize, pos: mchprs_blocks::BlockPos) {
        self.clear_git_inspection(player);
        let viewer = &self.players[player];
        let entity_id = spawn_marker(viewer, Marker { pos, kind: 4 });
        self.git.inspections.insert(
            viewer.uuid,
            (pos, entity_id, Instant::now() + Duration::from_secs(3)),
        );
    }

    pub(in crate::plot) fn unload_git_chunk(&mut self, player: usize, chunk_x: i32, chunk_z: i32) {
        if self
            .git
            .inspections
            .get(&self.players[player].uuid)
            .is_some_and(|(pos, _, _)| pos.x >> 4 == chunk_x && pos.z >> 4 == chunk_z)
        {
            self.clear_git_inspection(player);
        }
        let viewer = &self.players[player];
        let Some(session) = self.git.sessions.get_mut(&viewer.uuid) else {
            return;
        };
        let positions: Vec<_> = session
            .markers
            .keys()
            .filter(|p| p.x >> 4 == chunk_x && p.z >> 4 == chunk_z)
            .copied()
            .collect();
        let ids: Vec<_> = positions
            .into_iter()
            .filter_map(|p| session.markers.remove(&p).map(|v| v.0))
            .collect();
        if !ids.is_empty() {
            viewer.send_packet(&CDestroyEntities { entity_ids: ids }.encode());
            session.last_pos = None;
        }
    }

    pub(super) fn replace_git_markers(
        &mut self,
        player: usize,
        markers: Vec<Marker>,
        center: PlayerPos,
    ) {
        let viewer = &self.players[player];
        let Some(session) = self
            .git
            .sessions
            .get_mut(&viewer.uuid)
            .filter(|s| s.enabled)
        else {
            return;
        };
        let wanted: HashMap<_, _> = markers
            .into_iter()
            .filter(|m| {
                Self::get_chunk_distance(
                    m.pos.x >> 4,
                    m.pos.z >> 4,
                    viewer.last_chunk_x,
                    viewer.last_chunk_z,
                ) <= CONFIG.view_distance.max(0) as u32
            })
            .map(|m| (m.pos, m.kind))
            .collect();
        let removed: Vec<_> = session
            .markers
            .keys()
            .filter(|p| !wanted.contains_key(p))
            .copied()
            .take(32)
            .collect();
        let ids = removed
            .iter()
            .filter_map(|pos| session.markers.remove(pos).map(|v| v.0))
            .collect();
        if !removed.is_empty() {
            viewer.send_packet(&CDestroyEntities { entity_ids: ids }.encode());
        }
        for (&pos, &kind) in wanted
            .iter()
            .filter(|(p, _)| !session.markers.contains_key(p))
            .take(32 - removed.len())
            .collect::<Vec<_>>()
        {
            let entity_id = spawn_marker(viewer, Marker { pos, kind });
            session.markers.insert(pos, (entity_id, kind));
        }
        // A stationary player still gets subsequent batches until the overlay is complete.
        let complete = session.markers.len() == wanted.len()
            && session.markers.keys().all(|p| wanted.contains_key(p));
        if complete && session.last_pos.is_none() {
            let total: u64 = session.diff.counts.iter().sum();
            viewer.send_color_message(
                crate::chat::ColorCode::Gray,
                messages::git_glow_status(session.markers.len(), total),
            );
        }
        session.last_pos = complete.then_some(center);
        session.next_update = Instant::now() + Duration::from_secs(1);
    }
}

fn spawn_marker(viewer: &impl PacketSender, marker: Marker) -> i32 {
    let entity_id = allocate_entity_id() as i32;
    viewer.send_packet(
        &CSpawnEntity {
            entity_id,
            object_uuid: rand::random(),
            entity_type: 15,
            x: marker.pos.x as f64,
            y: marker.pos.y as f64,
            z: marker.pos.z as f64,
            pitch: 0.0,
            yaw: 0.0,
            data: 0,
            velocity_x: 0,
            velocity_y: 0,
            velocity_z: 0,
        }
        .encode(),
    );
    viewer.send_packet(&metadata(entity_id, marker.kind).encode());
    entity_id
}
