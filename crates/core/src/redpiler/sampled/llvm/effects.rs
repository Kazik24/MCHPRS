//! Ordered reactive dependencies and note playback metadata.
use super::{AUX_TABLES, Program, ScratchError, overflow};
use mchprs_blocks::{BlockFace, blocks::{Block, Instrument}};

fn id(n: usize) -> Result<u32, ScratchError> { u32::try_from(n).map_err(|_| overflow()) }
fn metadata(block: Block) -> u32 { (Instrument::from_block_below(block).get_id() << 1) | u32::from(block == Block::Air) | (u32::from(block.is_solid()) << 8) }

pub(super) fn encode(p: &Program, t: &mut [Vec<u32>; AUX_TABLES]) -> Result<(), ScratchError> {
    for node in 0..p.initial.len() {
        t[43].extend([u32::from(p.reactive[node]), id(t[44].len())?, id(p.reactive_by_source[node].len())?,
            id(t[48].len())?, id(p.reactive_neighbours[node].len())?]);
        for &wire in &p.reactive_by_source[node] { t[44].push(id(wire)?); }
        for &target in &p.reactive_neighbours[node] { t[48].push(id(target)?); }
    }
    for wire in &p.reactive_wires {
        t[45].extend([id(wire.wire)?, id(t[46].len()/2)?, id(wire.sources.len())?, id(t[47].len())?, id(wire.recipients.len())?]);
        for &(source, attenuation) in &wire.sources { t[46].extend([id(source)?, u32::from(attenuation)]); }
        for &target in &wire.recipients { t[47].push(id(target)?); }
    }
    for (node, &block) in p.initial.iter().enumerate() {
        if let Block::NoteBlock { instrument, note, .. } = block {
            for face in [BlockFace::Top, BlockFace::Bottom] {
                let pos = p.positions[node].offset(face);
                t[49].extend([p.projection_indices.get(&pos).copied().map(id).transpose()?.unwrap_or(u32::MAX),
                    metadata(p.geometry_indices.get(&pos).copied().unwrap_or(Block::Air))]);
            }
            t[49].extend([note, instrument.get_id()]);
        } else { t[49].extend([0; 6]); }
    }
    for (cell, &block) in p.cell_geometry.iter().enumerate() {
        t[50].push(metadata(p.cell_components[cell].map_or(block, |node| p.initial[node.get()-1])));
    }
    for token in &p.tokens { t[51].push(metadata(token.block)); }
    Ok(())
}
