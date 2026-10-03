use super::*;
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::RedstoneObserver;

#[test]
fn observer_power_direction_matches_conduction_and_piston_inputs() {
    for facing in BlockFace::values() {
        for powered in [false, true] {
            for side in BlockFace::values() {
                let chunks = (0..PLOT_WIDTH)
                    .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
                    .collect();
                let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
                let observer_pos = BlockPos::new(40, 30, 40);
                let observer = Block::Observer {
                    observer: RedstoneObserver {
                        facing: facing.into(),
                        powered,
                    },
                };
                let expected = if powered && facing == side { 15 } else { 0 };
                assert_eq!(
                    get_weak_power(observer, &world, observer_pos, side, false),
                    expected
                );
                assert_eq!(
                    get_strong_power(observer, &world, observer_pos, side, false),
                    expected,
                    "observer facing {facing:?}, queried side {side:?}, powered {powered}"
                );
                world.set_block(observer_pos, observer);
                let conductor_pos = observer_pos.offset(side.opposite());
                world.set_block(conductor_pos, Block::Stone {});
                assert_eq!(
                    get_redstone_power(Block::Stone {}, &world, conductor_pos, side),
                    expected,
                    "observer must conduct power only toward its output"
                );
                let piston_pos = conductor_pos.offset(side.opposite());
                assert_eq!(
                    piston::should_piston_extend(&world, side.opposite().into(), piston_pos),
                    expected > 0,
                    "off-axis observers must not power a piston through a conducting block"
                );
            }
        }
    }
}
