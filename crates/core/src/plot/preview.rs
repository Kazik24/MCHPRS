//! Viewer-only block displays shared by Git and interactive tools.
use crate::player::{allocate_entity_id, PacketSender};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockColorVariant, BlockPos};
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

pub(in crate::plot) fn metadata(entity_id: i32, kind: u8) -> CEntityMetadata {
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

pub(in crate::plot) fn spawn_marker(viewer: &impl PacketSender, pos: BlockPos, kind: u8) -> i32 {
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
    entity_id
}

/// Reconcile against the latest desired overlay; callers own cadence and generation identity.
pub(in crate::plot) fn reconcile_markers(
    viewer: &impl PacketSender,
    markers: &mut HashMap<BlockPos, (i32, u8)>,
    wanted: &HashMap<BlockPos, u8>,
    max_changes: usize,
) -> bool {
    let started = Instant::now();
    let removed: Vec<_> = markers
        .keys()
        .filter(|pos| !wanted.contains_key(pos))
        .copied()
        .take(max_changes)
        .collect();
    let mut changes = removed.len();
    if !removed.is_empty() {
        let entity_ids = removed
            .into_iter()
            .filter_map(|pos| markers.remove(&pos).map(|marker| marker.0))
            .collect();
        viewer.send_packet(&CDestroyEntities { entity_ids }.encode());
    }
    for (&pos, &kind) in wanted {
        if changes >= max_changes
            || (changes > 0 && started.elapsed() >= Duration::from_micros(500))
        {
            break;
        }
        match markers.get_mut(&pos) {
            Some((entity_id, old_kind)) if *old_kind != kind => {
                viewer.send_packet(&metadata(*entity_id, kind).encode());
                *old_kind = kind;
                changes += 1;
            }
            None => {
                markers.insert(pos, (spawn_marker(viewer, pos, kind), kind));
                changes += 1;
            }
            _ => {}
        }
    }
    markers.len() == wanted.len()
        && markers
            .iter()
            .all(|(pos, &(_, kind))| wanted.get(pos) == Some(&kind))
}

pub(in crate::plot) fn clear_markers(
    viewer: &impl PacketSender,
    markers: &mut HashMap<BlockPos, (i32, u8)>,
) {
    if markers.is_empty() {
        return;
    }
    let entity_ids = markers.drain().map(|(_, marker)| marker.0).collect();
    viewer.send_packet(&CDestroyEntities { entity_ids }.encode());
}

/// Returns whether the overlay needs rebuilding after a client chunk unload.
pub(in crate::plot) fn unload_markers(
    viewer: &impl PacketSender,
    markers: &mut HashMap<BlockPos, (i32, u8)>,
    chunk_x: i32,
    chunk_z: i32,
) -> bool {
    let positions: Vec<_> = markers
        .keys()
        .filter(|pos| pos.x >> 4 == chunk_x && pos.z >> 4 == chunk_z)
        .copied()
        .collect();
    if positions.is_empty() {
        return false;
    }
    let entity_ids = positions
        .into_iter()
        .filter_map(|pos| markers.remove(&pos).map(|marker| marker.0))
        .collect();
    viewer.send_packet(&CDestroyEntities { entity_ids }.encode());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_network::packets::{PacketDecoderExt, PacketEncoder};
    use std::cell::RefCell;
    use std::io::Cursor;

    #[derive(Default)]
    struct Viewer(RefCell<Vec<PacketEncoder>>);
    impl PacketSender for Viewer {
        fn send_packet(&self, packet: &PacketEncoder) {
            self.0.borrow_mut().push(packet.clone());
        }
    }

    #[test]
    fn reconciliation_obeys_budget_retains_ids_and_updates_color() {
        let viewer = Viewer::default();
        let mut markers = HashMap::new();
        let a = BlockPos::new(1, 64, 1);
        let b = BlockPos::new(17, 64, 1);
        let mut wanted = HashMap::from([(a, 0), (b, 0)]);
        assert!(!reconcile_markers(&viewer, &mut markers, &wanted, 1));
        assert_eq!(markers.len(), 1);
        assert_eq!(viewer.0.borrow().len(), 2);
        assert!(reconcile_markers(&viewer, &mut markers, &wanted, 1));
        let entity_id = markers[&a].0;
        viewer.0.borrow_mut().clear();
        wanted.insert(a, 1);
        assert!(!reconcile_markers(&viewer, &mut markers, &wanted, 0));
        assert!(viewer.0.borrow().is_empty());
        assert!(reconcile_markers(&viewer, &mut markers, &wanted, 1));
        assert_eq!(markers[&a], (entity_id, 1));
        let packets = viewer.0.borrow();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].packet_id, 0x5c);
        assert_eq!(
            Cursor::new(&packets[0].buffer).read_varint().unwrap(),
            entity_id
        );
        drop(packets);
        viewer.0.borrow_mut().clear();
        assert!(reconcile_markers(&viewer, &mut markers, &wanted, 1));
        assert!(viewer.0.borrow().is_empty());
        assert!(unload_markers(&viewer, &mut markers, 1, 0));
        assert!(markers.contains_key(&a));
        assert!(!markers.contains_key(&b));
        assert!(!unload_markers(&viewer, &mut markers, 1, 0));
        clear_markers(&viewer, &mut markers);
        assert!(markers.is_empty());
        assert!(viewer
            .0
            .borrow()
            .iter()
            .all(|packet| packet.packet_id == 0x46));

        // A newer target replaces unfinished work rather than finishing stale additions.
        assert!(!reconcile_markers(&viewer, &mut markers, &wanted, 1));
        let c = BlockPos::new(33, 64, 1);
        wanted = HashMap::from([(c, 2)]);
        assert!(!reconcile_markers(&viewer, &mut markers, &wanted, 1));
        assert!(markers.is_empty());
        assert!(reconcile_markers(&viewer, &mut markers, &wanted, 1));
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[&c].1, 2);
    }
}
