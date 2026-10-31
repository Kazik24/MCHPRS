use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::plot::{PLOT_WIDTH, PlotWorld, worldedit::{load_schematic, paste_clipboard}};
use mchprs_core::world::{World, storage::Chunk};
use std::{fs::File, path::Path};

fn main() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/ZCore12B_FIX.schem");
    let mut clip = load_schematic(File::open(path).unwrap()).unwrap();
    println!("size {}x{}x{} offset {},{},{} entities {}", clip.size_x, clip.size_y, clip.size_z, clip.offset_x, clip.offset_y, clip.offset_z, clip.block_entities.len());
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let origin = BlockPos::new(8, 8, 8);
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(&mut world, &clip, origin, false);
    let mut buttons = Vec::new();
    let mut levers = Vec::new();
    for y in 0..clip.size_y as i32 {
        for z in 0..clip.size_z as i32 {
            for x in 0..clip.size_x as i32 {
                let p = origin + BlockPos::new(x,y,z);
                match world.get_block(p) {
                    Block::StoneButton { .. } => buttons.push(BlockPos::new(x,y,z)),
                    Block::Lever { .. } => levers.push(BlockPos::new(x,y,z)),
                    _ => {}
                }
            }
        }
    }
    println!("buttons {buttons:?}\nlevers {levers:?}");
    let mut lamp_counts = Vec::new();
    for x in 0..clip.size_x as i32 {
        for y in 0..clip.size_y as i32 {
            for z in 0..clip.size_z as i32 {
                if matches!(world.get_block(origin + BlockPos::new(x,y,z)), Block::RedstoneLamp { .. }) {
                    lamp_counts.push((x,y,z));
                }
            }
        }
    }
    for axis in 0..3 {
        let mut counts = std::collections::BTreeMap::<i32,usize>::new();
        for p in &lamp_counts { let c = [p.0,p.1,p.2][axis]; *counts.entry(c).or_default() += 1; }
        let largest = counts.iter().max_by_key(|(_, n)| *n).map(|(c,n)| (*c,*n));
        println!("lamp planes axis {axis}: {largest:?}; distinct planes {}", counts.len());
    }
}
