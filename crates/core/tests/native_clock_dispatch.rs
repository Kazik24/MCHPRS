#![cfg(feature = "native-clock-test-support")]

use mchprs_blocks::BlockPos;
use mchprs_core::plot::worldedit::{load_schematic, paste_clipboard};
use mchprs_core::plot::{PLOT_WIDTH, PlotWorld};
use mchprs_core::redpiler::native_clock_test_support;
use mchprs_core::world::storage::Chunk;

// The harness manually installs a native clock; it does not test compiler admission.
#[test]
fn accepted_native_clock_retraction_runs_frame_in_production_library() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/COUNTER_BASIC.schem");
    let clipboard = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let origin = BlockPos::new(40, 30, 40);
    let bounds = (
        origin,
        origin + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1),
    );
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(
        &mut world,
        &clipboard,
        origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let (before, rejected, accepted) = native_clock_test_support(&world, bounds);
    assert_eq!(rejected, before);
    assert_eq!(accepted, before + 1);
}
