use crate::plot::PlotWorld;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::world::storage::Chunk;
use mchprs_blocks::BlockPos;
use sha2::{Digest, Sha256};

pub(crate) fn piston_reset_fixture() -> (PlotWorld, (BlockPos, BlockPos)) {
    const ORIGIN: BlockPos = BlockPos::new(8, 12, 8);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/test-prefix-20261008/TEST_PISTION_2.schem");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "567f61a69ff36e693916ea6069241ba2b56882f384b022fc1d88dd710b4985df"
    );
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..3).flat_map(|x| (0..3).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(
        &mut world,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let bounds = (
        ORIGIN - BlockPos::new(2, 2, 2),
        ORIGIN + BlockPos::new(clipboard.size_x as i32 + 1, clipboard.size_y as i32 + 1, clipboard.size_z as i32 + 1),
    );
    (world, bounds)
}
