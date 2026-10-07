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
            entry(17, 3, 2.0f32.to_be_bytes().to_vec()),
            entry(
                22,
                1,
                varint(match kind {
                    0 => 0x55ff55,
                    1 => 0xff5555,
                    _ => 0xffff55,
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
    pub(in crate::plot) fn unload_git_chunk(&mut self, player: usize, chunk_x: i32, chunk_z: i32) {
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
            let entity_id = allocate_entity_id() as i32;
            viewer.send_packet(
                &CSpawnEntity {
                    entity_id,
                    object_uuid: rand::random(),
                    entity_type: 15,
                    x: pos.x as f64,
                    y: pos.y as f64,
                    z: pos.z as f64,
                    pitch: 0.0,
                    yaw: 0.0,
                    data: 0,
                    velocity_x: 0,
                    velocity_y: 0,
                    velocity_z: 0,
                }
                .encode(),
            );
            viewer.send_packet(&metadata(entity_id, kind).encode());
            session.markers.insert(pos, (entity_id, kind));
        }
        // A stationary player still gets subsequent batches until the overlay is complete.
        let complete = session.markers.len() == wanted.len()
            && session.markers.keys().all(|p| wanted.contains_key(p));
        if complete && session.last_pos.is_none() {
            let total: u64 = session.diff.counts.iter().sum();
            viewer.send_system_message(&messages::git_glow_status(session.markers.len(), total));
        }
        session.last_pos = complete.then_some(center);
        session.next_update = Instant::now() + Duration::from_secs(1);
    }
}
