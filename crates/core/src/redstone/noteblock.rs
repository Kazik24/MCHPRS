use mchprs_blocks::blocks::{Block, Instrument};
use mchprs_blocks::{BlockFace, BlockPos};

use crate::world::World;

const PITCHES_TABLE: [f32; 25] = {
    let mut pitches = [0.0; 25];
    let mut pitch = 0.5_f64;
    let mut note = 0;
    while note < pitches.len() {
        pitches[note] = pitch as f32;
        pitch *= 1.059_463_094_359_295_3; // 2^(1/12), one equal-tempered semitone.
        note += 1;
    }
    pitches
};

pub fn is_noteblock_unblocked(world: &impl World, pos: BlockPos) -> bool {
    matches!(world.get_block(pos.offset(BlockFace::Top)), Block::Air)
}

pub fn get_noteblock_instrument(world: &impl World, pos: BlockPos) -> Instrument {
    Instrument::from_block_below(world.get_block(pos.offset(BlockFace::Bottom)))
}

pub fn play_note(world: &mut impl World, pos: BlockPos, instrument: Instrument, note: u32) {
    let Some(&pitch) = PITCHES_TABLE.get(note as usize) else {
        return;
    };
    world.play_sound(
        pos,
        instrument.to_sound_id(),
        2, // RECORDS sound category
        3.0,
        pitch,
    );
}

/// The client waits for the server to play a manually tuned note.
pub fn play_note_for_action(
    world: &mut impl World,
    pos: BlockPos,
    instrument: Instrument,
    note: u32,
) {
    let Some(&pitch) = PITCHES_TABLE.get(note as usize) else {
        return;
    };
    world.play_sound_for_action(pos, instrument.to_sound_id(), 2, 3.0, pitch, None);
}

#[cfg(test)]
mod tests {
    use super::PITCHES_TABLE;

    #[test]
    fn pitches_follow_equal_temperament() {
        assert_eq!(PITCHES_TABLE[0], 0.5);
        assert_eq!(PITCHES_TABLE[12], 1.0);
        assert_eq!(PITCHES_TABLE[24], 2.0);
        for (note, &pitch) in PITCHES_TABLE.iter().enumerate() {
            let expected = 2.0_f32.powf((note as f32 - 12.0) / 12.0);
            assert!((pitch - expected).abs() <= f32::EPSILON * expected);
        }
    }
}
