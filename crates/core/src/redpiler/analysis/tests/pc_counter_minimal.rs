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
fn reduced_pc_counter_variants_and_notification_gated_counter_compile() {
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
            compile_result(&world, assume_instant, optimize).unwrap();
        }
    }
}

#[test]
fn qc_data_changes_hold_response_and_read_callbacks_keep_their_order() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let mut world = load();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    Vec::new(),
                    Default::default(),
                )
                .unwrap();
            let initial = compiler.backend.as_ref().unwrap().activation_states();
            assert_eq!(initial.len(), 2);
            assert!(compiler
                .backend
                .as_ref()
                .unwrap()
                .logical_stats()
                .iter()
                .all(|(_, _, memory)| memory.is_empty()));
            let upper_input = BASE + BlockPos::new(6, 7, 6);
            let readb = BASE + BlockPos::new(0, 4, 7);
            let reada = BASE + BlockPos::new(2, 3, 8);
            assert!(matches!(world.get_block(upper_input), Block::Lever { .. }));
            assert!(matches!(world.get_block(readb), Block::Lever { .. }));
            assert!(matches!(world.get_block(reada), Block::Lever { .. }));
            compiler.on_use_block(upper_input);
            for _ in 0..20 {
                compiler.tick_with_world(&mut world);
            }
            assert_eq!(
                compiler.backend.as_ref().unwrap().activation_states(),
                initial
            );
            assert!(compiler
                .backend
                .as_mut()
                .unwrap()
                .take_activation_trace()
                .is_empty());
            compiler.on_use_block(readb);
            for _ in 0..20 {
                compiler.tick_with_world(&mut world);
            }
            let events = compiler.backend.as_mut().unwrap().take_activation_trace();
            let directions: Vec<_> = events
                .iter()
                .filter(|event| {
                    event.source == BASE + BlockPos::new(0, 4, 2)
                        && event.recipient == BASE + BlockPos::new(1, 3, 2)
                })
                .map(|event| event.direction)
                .collect();
            assert!(directions.len() >= 3, "READB delivered {directions:?}");
            assert_eq!(
                &directions[..3],
                &[None, Some(BlockFace::West), Some(BlockFace::Top)]
            );
            compiler.on_use_block(reada);
            for _ in 0..20 {
                compiler.tick_with_world(&mut world);
            }
            let events = compiler.backend.as_mut().unwrap().take_activation_trace();
            let actual: Vec<_> = events
                .iter()
                .filter(|event| event.source == BASE + BlockPos::new(2, 3, 3))
                .map(|event| (event.recipient - BASE, event.direction))
                .collect();
            let first = BlockPos::new(1, 3, 2);
            let second = BlockPos::new(1, 3, 4);
            assert!(actual.len() >= 6, "READA delivered {actual:?}");
            assert_eq!(
                &actual[..6],
                &[
                    (second, None),
                    (first, None),
                    (first, Some(BlockFace::East)),
                    (second, Some(BlockFace::East)),
                    (first, Some(BlockFace::South)),
                    (second, Some(BlockFace::North))
                ]
            );
        }
    }
}
