//! Saved PC counter admission fixture; coordinates are selection-local.
use super::*;

const FIXTURE: &str =
    "test_data/piston-research/pc-counter-minimal-20261009/PC_COUNTER_MINIMAL_FAIL.schem";

fn load() -> PlotWorld {
    let bytes = std::fs::read(root().join(FIXTURE)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "2b2df49a3bd2f9bf7049126f751a67e9d251a3ecd4e2234e867634d51887ea8b"
    );
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    world
}

fn compile_result(world: &PlotWorld, assume_instant: bool, optimize: bool) -> Result<(), String> {
    Compiler::default()
        .compile(
            world,
            world.get_corners(),
            CompilerOptions {
                assume_instant,
                optimize,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .map_err(|error| error.to_string())
}

#[test]
fn reduced_pc_counter_variants_compile_and_full_counter_stays_rejected() {
    for removed in [
        [[4, 7, 5], [3, 7, 5], [4, 8, 5]],
        [[0, 4, 5], [0, 4, 4], [0, 5, 5]],
    ] {
        let mut world = load();
        for [x, y, z] in removed {
            world.set_block(BASE + BlockPos::new(x, y, z), Block::Air);
        }
        for assume_instant in [false, true] {
            for optimize in [false, true] {
                compile_result(&world, assume_instant, optimize).unwrap();
            }
        }
    }

    let world = load();
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            assert!(
                compile_result(&world, assume_instant, optimize).is_err(),
                "full PC counter unexpectedly admitted with assume_instant={assume_instant}, optimize={optimize}"
            );
        }
    }
}
