//! Viewer-only block displays. They never enter the world's entity/block storage.
use super::{Marker, Plot};
use crate::config::CONFIG;
use crate::messages;
use crate::player::{PacketSender, PlayerPos};
use crate::plot::preview;
#[cfg(test)]
pub(super) use crate::plot::preview::metadata;
use mchprs_network::packets::clientbound::{CDestroyEntities, ClientBoundPacket};
use std::collections::HashMap;
use std::time::{Duration, Instant};

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
        let entity_id = preview::spawn_marker(viewer, pos, 4);
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
        if preview::unload_markers(viewer, &mut session.markers, chunk_x, chunk_z) {
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
        // A stationary player still gets subsequent batches until the overlay is complete.
        let complete = preview::reconcile_markers(viewer, &mut session.markers, &wanted, 32);
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
