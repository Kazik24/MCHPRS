//! Private native-interpreter world used only while compiling event probes.
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld};
use crate::redpiler::TickScheduler;
use crate::redpiler::backend::ScheduledBlockTick;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_world::{AdvancePhase, PistonState, TickEntry};

pub(crate) struct ScratchEntry {
    pub(crate) scheduled: Vec<TickEntry>,
    pub(crate) piston: PistonState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScratchError {
    pub(crate) position: Option<BlockPos>,
    pub(crate) reason: &'static str,
}

pub(crate) struct ScratchWorld {
    world: PlotWorld,
    bounds: (BlockPos, BlockPos),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redstone;
    use crate::world::RedstoneUpdateObservation;
    use mchprs_blocks::{
        BlockDirection, BlockFace, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston, RedstoneWire, RedstoneWireSide},
    };

    const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(38, 26, 38), BlockPos::new(44, 34, 42));
    const LEVER: BlockPos = BlockPos::new(40, 30, 40);
    const WIRE: BlockPos = BlockPos::new(41, 30, 40);
    const PISTON: BlockPos = BlockPos::new(42, 30, 40);

    fn fixture() -> PlotWorld {
        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::<ScheduledBlockTick>::default());
        world.set_block(BlockPos::new(40, 29, 40), Block::Stone {});
        world.set_block(
            LEVER,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        world.set_block(
            WIRE,
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::None,
                    RedstoneWireSide::None,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    0,
                ),
            },
        );
        world.set_block(
            PISTON,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: false,
                    extended: false,
                },
            },
        );
        world
    }

    fn entry(world: &PlotWorld) -> ScratchEntry {
        ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(),
            piston: world.piston_state().clone(),
        }
    }

    fn drive(world: &mut PlotWorld) {
        world.set_block(
            LEVER,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        redstone::update_surrounding_blocks(world, LEVER);
        redstone::update_surrounding_blocks(world, WIRE);
        redstone::update_surrounding_blocks(world, BlockPos::new(40, 29, 40));
    }

    fn sample(world: &PlotWorld) -> (Vec<Block>, serde_json::Value, Vec<TickEntry>) {
        let blocks = [BlockPos::new(40, 29, 40), LEVER, WIRE, PISTON, PISTON.offset(BlockFace::East)]
            .into_iter()
            .map(|pos| world.get_block(pos))
            .collect();
        (
            blocks,
            serde_json::to_value(world.piston_state()).unwrap(),
            world.scheduler().iter_entries().collect(),
        )
    }

    #[test]
    fn scratch_copy_runs_interpreter_passively_and_is_independent() {
        let host = fixture();
        let host_before = sample(&host);
        let mut source_run = fixture();
        drive(&mut source_run);
        let mut traced = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        let mut untraced = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        traced.world_mut().observe_redstone_updates(true);
        drive(traced.world_mut());
        drive(untraced.world_mut());
        traced.finish_probe().unwrap();
        untraced.finish_probe().unwrap();
        assert_eq!(sample(traced.world()), sample(&source_run));
        assert_eq!(sample(traced.world()), sample(untraced.world()));
        let events: Vec<RedstoneUpdateObservation> = traced.world_mut().take_redstone_update_observations();
        assert!(
            events
                .iter()
                .any(|event| event.pos == PISTON && matches!(event.block, Block::Piston { piston } if !piston.extended) && event.direction.is_some()),
            "piston delivery absent: {events:?}"
        );
        assert_eq!(sample(&host), host_before);

        let other = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        traced.world_mut().set_block(LEVER, Block::Air);
        assert_ne!(traced.world().get_block(LEVER), other.world().get_block(LEVER));
        assert_eq!(sample(&host), host_before);
    }

    #[test]
    fn scratch_reports_unknown_boundary_and_pending_entry_work() {
        let host = fixture();
        let scratch = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        let outside = BlockPos::new(BOUNDS.1.x + 1, BOUNDS.1.y, BOUNDS.1.z);
        let _ = scratch.world().get_block(outside);
        assert_eq!(
            scratch.finish_probe().unwrap_err(),
            ScratchError {
                position: Some(outside),
                reason: "scratch probe read outside captured boundary"
            }
        );

        let mut pending = entry(&host);
        pending.piston.events.push_back(mchprs_world::PistonEvent {
            pos: PISTON,
            sticky: false,
            facing: BlockFace::East,
            action: mchprs_world::PistonAction::Extend,
        });
        let error = match ScratchWorld::from_world(&host, BOUNDS, pending) {
            Ok(_) => panic!("pending piston work was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ScratchError {
                position: Some(PISTON),
                reason: "scratch entry has queued piston work"
            }
        );
    }
}

impl ScratchWorld {
    pub(crate) fn from_world(source: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Self, ScratchError> {
        let (min, max) = bounds;
        if min.x < 0
            || min.y < 0
            || min.z < 0
            || max.x >= PLOT_BLOCK_WIDTH
            || max.y >= PLOT_BLOCK_HEIGHT
            || max.z >= PLOT_BLOCK_WIDTH
            || min.x > max.x
            || min.y > max.y
            || min.z > max.z
        {
            return Err(ScratchError {
                position: Some(min),
                reason: "invalid scratch capture bounds",
            });
        }
        if entry.piston.phase != AdvancePhase::BetweenTicks {
            return Err(ScratchError {
                position: None,
                reason: "scratch entry is mid-tick",
            });
        }
        if let Some(tick) = entry.scheduled.first() {
            return Err(ScratchError {
                position: Some(tick.pos),
                reason: "scratch entry has scheduled block work",
            });
        }
        if let Some(event) = entry.piston.events.front() {
            return Err(ScratchError {
                position: Some(event.pos),
                reason: "scratch entry has queued piston work",
            });
        }
        if let Some(motion) = entry.piston.motions.front() {
            return Err(ScratchError {
                position: Some(motion.pos),
                reason: "scratch entry has piston motion",
            });
        }
        if let Some((pos, _)) = entry.piston.movement_work.first() {
            return Err(ScratchError {
                position: Some(*pos),
                reason: "scratch entry has pending movement work",
            });
        }

        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::<ScheduledBlockTick>::default());
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let pos = BlockPos::new(x, y, z);
                    let raw = source.get_block_raw(pos);
                    if raw != Block::Air.get_id() {
                        world.set_block_raw(pos, raw);
                    }
                    if let Some(entity) = source.get_block_entity(pos) {
                        world.set_block_entity(pos, entity.clone());
                    }
                }
            }
        }
        world.piston_state_mut().clone_from(&entry.piston);
        world.configure_scratch(bounds);
        Ok(Self { world, bounds })
    }

    pub(crate) fn world(&self) -> &PlotWorld {
        &self.world
    }
    pub(crate) fn world_mut(&mut self) -> &mut PlotWorld {
        &mut self.world
    }
    pub(crate) fn bounds(&self) -> (BlockPos, BlockPos) {
        self.bounds
    }

    pub(crate) fn finish_probe(&self) -> Result<(), ScratchError> {
        if let Some(position) = self.world.take_scratch_boundary_read() {
            return Err(ScratchError {
                position: Some(position),
                reason: "scratch probe read outside captured boundary",
            });
        }
        Ok(())
    }
}
