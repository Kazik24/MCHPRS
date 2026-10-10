use super::{PLOT_BLOCK_HEIGHT, PLOT_SECTIONS, PlotWorld};
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use mchprs_network::packets::clientbound::{CUpdateLight, ClientBoundPacket};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct BulbLight {
    sources: FxHashMap<BlockPos, u8>,
    dirty: bool,
}

impl PlotWorld {
    pub(super) fn rebuild_bulb_light(&mut self) {
        self.bulb_light.sources = self
            .chunks
            .iter()
            .flat_map(|chunk| chunk.copper_bulbs())
            .filter_map(|(pos, block)| {
                let strength = block.copper_bulb_light();
                (strength > 0).then_some((pos, strength))
            })
            .collect();
        self.bulb_light.dirty = true;
        self.flush_bulb_light();
    }

    pub(super) fn track_bulb_light(&mut self, pos: BlockPos, old: Block, new: Block) {
        let before = old.copper_bulb_light();
        let after = new.copper_bulb_light();
        if before != after {
            if after == 0 {
                self.bulb_light.sources.remove(&pos);
            } else {
                self.bulb_light.sources.insert(pos, after);
            }
            self.bulb_light.dirty = true;
        } else if !self.bulb_light.sources.is_empty() && old.light_filter() != new.light_filter() {
            self.bulb_light.dirty = true;
        }
    }

    pub(super) fn flush_bulb_light(&mut self) {
        if !std::mem::take(&mut self.bulb_light.dirty) {
            return;
        }
        // ponytail: rebuild at visual flush; use incremental removal/addition if large bulb displays make this a bottleneck.
        // ponytail: registry attenuation omits partial-block face occlusion; add voxel faces when the lighting engine supports them.
        let mut levels = self.bulb_light.sources.clone();
        let mut pending: VecDeque<_> = levels.keys().copied().collect();
        while let Some(pos) = pending.pop_front() {
            let strength = levels[&pos];
            if strength <= 1 {
                continue;
            }
            for face in BlockFace::values() {
                let next = pos.offset(face);
                if !(0..PLOT_BLOCK_HEIGHT).contains(&next.y) || !self.contains_position(next) {
                    continue;
                }
                let value = strength.saturating_sub(self.get_block(next).light_filter().max(1));
                if value > levels.get(&next).copied().unwrap_or(0) {
                    levels.insert(next, value);
                    pending.push_back(next);
                }
            }
        }
        let mut arrays: Vec<[Option<Vec<u8>>; PLOT_SECTIONS]> = (0..self.chunks.len())
            .map(|_| std::array::from_fn(|_| None))
            .collect();
        for (pos, value) in levels {
            let Some(chunk) = self.get_chunk_index_for_block(pos.x, pos.z) else {
                continue;
            };
            let section = arrays[chunk][pos.y as usize / 16].get_or_insert_with(|| vec![0; 2048]);
            let index =
                ((pos.y as usize & 15) << 8) | ((pos.z as usize & 15) << 4) | (pos.x as usize & 15);
            section[index / 2] |= value << ((index & 1) * 4);
        }
        for (chunk, light) in self.chunks.iter_mut().zip(arrays) {
            if chunk.set_block_light(light) && !self.packet_senders.is_empty() {
                let packet = CUpdateLight {
                    chunk_x: chunk.x,
                    chunk_z: chunk.z,
                    block_light: chunk.light_arrays(),
                }
                .encode();
                for sender in &self.packet_senders {
                    sender.send_packet(&packet);
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn bulb_light_at(&self, pos: BlockPos) -> u8 {
        let chunk = &self.chunks[self.get_chunk_index_for_block(pos.x, pos.z).unwrap()];
        let Some(section) = &chunk.block_light[pos.y as usize / 16] else {
            return 0;
        };
        let index =
            ((pos.y as usize & 15) << 8) | ((pos.z as usize & 15) << 4) | (pos.x as usize & 15);
        (section[index / 2] >> ((index & 1) * 4)) & 15
    }
}
