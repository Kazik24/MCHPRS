//! Basic Java 1.21.5 block sounds, derived from the official block registry.
use crate::player::PlayerPos;
use crate::world::World;
use mchprs_blocks::{blocks::Block, BlockPos};
use mchprs_network::packets::clientbound::{CSoundEffect, ClientBoundPacket};
use mchprs_network::packets::PacketEncoder;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct RegistrySound {
    id: i32,
    name: String,
}
static IDS: Lazy<HashMap<String, i32>> = Lazy::new(|| {
    serde_json::from_str::<Vec<RegistrySound>>(include_str!("../../../mc_data/1.21.5/sounds.json"))
        .expect("Invalid checked-in sound registry")
        .into_iter()
        // minecraft-data uses the holder's wire ID; CSoundEffect adds that offset.
        .map(|sound| (sound.name, sound.id - 1))
        .collect()
});
#[derive(Deserialize)]
struct BlockSound {
    name: String,
    place: String,
    volume: f32,
    pitch: f32,
}
static BLOCKS: Lazy<HashMap<String, BlockSound>> = Lazy::new(|| {
    serde_json::from_str::<Vec<BlockSound>>(include_str!(
        "../../../mc_data/1.21.5/block_sounds.json"
    ))
    .expect("Invalid checked-in block sounds")
    .into_iter()
    .map(|profile| (profile.name.clone(), profile))
    .collect()
});

pub fn event_id(name: &str) -> i32 {
    *IDS.get(name)
        .expect("Sound missing from the pinned 1.21.5 registry")
}

pub fn play(
    world: &mut impl World,
    pos: BlockPos,
    name: &str,
    volume: f32,
    pitch: f32,
    excluded: Option<u128>,
) {
    world.play_sound_for_action(pos, event_id(name), 4, volume, pitch, excluded);
}
pub fn placed(world: &mut impl World, pos: BlockPos, block: Block, player: u128) {
    if block.get_id() == 0 {
        return;
    }
    if let Some(profile) = BLOCKS.get(block.get_name()) {
        play(
            world,
            pos,
            &profile.place,
            (profile.volume + 1.0) / 2.0,
            profile.pitch * 0.8,
            Some(player),
        );
    }
}
/// Only genuine control transitions make sounds; powered buttons are idempotent.
pub fn control_used(world: &mut impl World, pos: BlockPos, block: Block, player: u128) {
    match block {
        Block::Lever { lever } => play(
            world,
            pos,
            "block.lever.click",
            0.3,
            if lever.powered { 0.5 } else { 0.6 },
            Some(player),
        ),
        Block::StoneButton { button } if !button.powered => play(
            world,
            pos,
            "block.stone_button.click_on",
            1.0,
            1.0,
            Some(player),
        ),
        Block::RedstoneComparator { comparator } => play(
            world,
            pos,
            "block.comparator.click",
            0.3,
            if comparator.mode == mchprs_blocks::blocks::ComparatorMode::Compare {
                0.55
            } else {
                0.5
            },
            Some(player),
        ),
        _ => {}
    }
}

pub(crate) struct Emission {
    pub pos: BlockPos,
    pub excluded: Option<u128>,
    sound_id: i32,
    category: i32,
    volume: f32,
    pitch: f32,
    seed: i64,
}
impl Emission {
    pub fn new(
        pos: BlockPos,
        sound_id: i32,
        category: i32,
        volume: f32,
        pitch: f32,
        excluded: Option<u128>,
    ) -> Option<Self> {
        (pos.x
            .checked_mul(8)
            .and_then(|x| x.checked_add(4))
            .is_some()
            && pos
                .y
                .checked_mul(8)
                .and_then(|y| y.checked_add(4))
                .is_some()
            && pos
                .z
                .checked_mul(8)
                .and_then(|z| z.checked_add(4))
                .is_some()
            && sound_id >= 0
            && sound_id < i32::MAX
            && (0..=9).contains(&category)
            && volume.is_finite()
            && volume > 0.0
            && pitch.is_finite()
            && pitch > 0.0)
            .then(|| Self {
                pos,
                excluded,
                sound_id,
                category,
                volume,
                pitch,
                seed: rand::random(),
            })
    }
    pub fn audible(&self, uuid: u128, player: PlayerPos) -> bool {
        if self.excluded == Some(uuid) {
            return false;
        }
        let radius = 16.0 * f64::from(self.volume.max(1.0));
        let distance = (player.x - f64::from(self.pos.x) - 0.5).powi(2)
            + (player.y - f64::from(self.pos.y) - 0.5).powi(2)
            + (player.z - f64::from(self.pos.z) - 0.5).powi(2);
        distance < radius * radius
    }
    pub fn packet(&self) -> PacketEncoder {
        CSoundEffect {
            sound_id: self.sound_id,
            sound_category: self.category,
            x: self.pos.x * 8 + 4,
            y: self.pos.y * 8 + 4,
            z: self.pos.z * 8 + 4,
            volume: self.volume,
            pitch: self.pitch,
            seed: self.seed,
        }
        .encode()
    }
}
