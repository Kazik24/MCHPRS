//! Entry state for an explicitly isolated, selected set of plot positions.
use super::*;

pub(super) struct RegionSnapshot {
    blocks: Vec<(BlockPos, u32, Option<BlockEntity>)>,
    positions: HashSet<BlockPos>,
    ticks: Vec<TickEntry>,
}

impl RegionSnapshot {
    /// Caller must prove the positions include every mutable cell and cannot interact with outside work.
    pub(super) fn capture(world: &PlotWorld, positions: HashSet<BlockPos>) -> Result<Self, &'static str> {
        if world.piston_state.phase != AdvancePhase::BetweenTicks
            || world.piston_state.events.iter().any(|event| positions.contains(&event.pos))
            || !world.piston_state.motions.is_empty()
        {
            return Err("selected region has pending piston work");
        }
        let mut blocks = Vec::with_capacity(positions.len());
        for &pos in &positions {
            if !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y) || world.get_chunk_index_for_block(pos.x, pos.z).is_none() {
                return Err("selected region extends outside the plot");
            }
            let block = world.get_block_raw(pos);
            let entity = world.get_block_entity(pos).cloned();
            if matches!(Block::from_id(block), Block::MovingPiston { .. }) || matches!(entity, Some(BlockEntity::MovingPiston(_))) {
                return Err("selected region has a moving piston");
            }
            blocks.push((pos, block, entity));
        }
        let ticks = world.scheduler().iter_entries().filter(|tick| positions.contains(&tick.pos)).collect();
        Ok(Self { blocks, positions, ticks })
    }

    pub(super) fn restore(&self, world: &mut PlotWorld) -> Result<(), &'static str> {
        // shortcut: any moving piston blocks reset until motion ownership can be proven.
        if world.piston_state.phase != AdvancePhase::BetweenTicks || !world.piston_state.motions.is_empty() {
            return Err("region reset requires a tick boundary without moving pistons");
        }
        let outside_ticks = world.scheduler().iter_entries().filter(|tick| !self.positions.contains(&tick.pos));
        let ticks = outside_ticks.chain(self.ticks.iter().cloned()).collect();
        world.piston_state.events.retain(|event| !self.positions.contains(&event.pos));
        for (pos, block, entity) in &self.blocks {
            world.delete_block_entity(*pos);
            world.set_block_raw(*pos, *block);
            if let Some(entity) = entity {
                world.set_block_entity(*pos, entity.clone());
            }
        }
        world.to_be_ticked = ticks;
        world.tick_index.invalidate();
        world.piston_index.get_mut().invalidate();
        world.wire_topology.get_mut().clear();
        world.instant_pistons.clear();
        Ok(())
    }

    /// Stop the graph before restoring the selected entry state. Retain this snapshot on failure.
    pub(super) fn reset_compiler(&self, world: &mut PlotWorld, compiler: &mut Compiler) -> Result<(), &'static str> {
        if world.piston_state.phase != AdvancePhase::BetweenTicks || !world.piston_state.motions.is_empty() {
            return Err("region reset requires a tick boundary without moving pistons");
        }
        compiler.reset_selected(world, &self.positions)?;
        self.restore(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::client_sync_tests::fixture;

    #[test]
    fn restore_preserves_outside_work_and_allows_a_following_edit() {
        let (mut plot, _peer) = fixture(false);
        let selected = BlockPos::new(33, 20, 32);
        let outside = BlockPos::new(35, 20, 32);
        plot.world.set_block(selected, Block::RedstoneLamp { lit: true });
        plot.world.set_block(outside, Block::RedstoneLamp { lit: true });
        plot.world.schedule_tick(selected, 2, TickPriority::Normal);
        plot.world.restore_scheduled_tick(TickEntry {
            pos: selected,
            ticks_left: 4,
            tick_priority: TickPriority::Normal,
            block_type: None,
        });
        let selected_ticks: Vec<_> = plot.world.scheduler().iter_entries().collect();
        let snapshot = RegionSnapshot::capture(&plot.world, HashSet::from([selected])).unwrap();
        plot.world.set_block(selected, Block::Air);
        plot.world.set_block(outside, Block::Stone {});
        plot.world.clear_scheduled_ticks();
        plot.world.schedule_tick(outside, 3, TickPriority::High);
        plot.world.restore_scheduled_tick(TickEntry {
            pos: outside,
            ticks_left: 6,
            tick_priority: TickPriority::High,
            block_type: None,
        });
        let outside_ticks: Vec<_> = plot.world.scheduler().iter_entries().collect();
        let outside_event = mchprs_world::PistonEvent {
            pos: outside,
            sticky: false,
            facing: BlockFace::North,
            action: mchprs_world::PistonAction::Extend,
        };
        plot.world.piston_state.events.push_back(outside_event);

        snapshot.restore(&mut plot.world).unwrap();
        assert_eq!(plot.world.get_block(selected), Block::RedstoneLamp { lit: true });
        assert_eq!(plot.world.get_block(outside), Block::Stone {});
        let ticks: Vec<_> = plot.world.scheduler().iter_entries().collect();
        assert_eq!(
            ticks.iter().filter(|tick| tick.pos == outside).cloned().collect::<Vec<_>>(),
            outside_ticks
        );
        assert_eq!(
            ticks.iter().filter(|tick| tick.pos == selected).cloned().collect::<Vec<_>>(),
            selected_ticks
        );
        assert_eq!(plot.world.piston_state.events.front(), Some(&outside_event));
        plot.world.set_block(selected, Block::Stone {});
        assert_eq!(plot.world.get_block(selected), Block::Stone {});
    }

    #[test]
    fn restore_rejects_moving_pistons_before_changing_anything() {
        let (mut plot, _peer) = fixture(false);
        let selected = BlockPos::new(33, 20, 32);
        let outside = BlockPos::new(35, 20, 32);
        let snapshot = RegionSnapshot::capture(&plot.world, HashSet::from([selected])).unwrap();
        plot.world.set_block(selected, Block::Stone {});
        plot.world.schedule_tick(outside, 3, TickPriority::High);
        let ticks = plot.world.scheduler().iter_entries().collect::<Vec<_>>();
        plot.world.piston_state.motions.push_back(PistonMotion {
            pos: outside,
            identity: 1,
            progress: 0.0,
            previous_progress: 0.0,
            last_tick: 0,
            carried_entity: None,
        });

        assert!(snapshot.restore(&mut plot.world).is_err());
        assert_eq!(plot.world.get_block(selected), Block::Stone {});
        assert_eq!(plot.world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
        assert_eq!(plot.world.piston_state.motions.len(), 1);
    }

    #[test]
    fn graph_selected_reset_hands_off_outside_work_before_restoring_region() {
        let (mut plot, _peer) = fixture(false);
        let selected = BlockPos::new(33, 20, 32);
        let outside = BlockPos::new(35, 20, 32);
        plot.world.set_block(selected, Block::RedstoneLamp { lit: true });
        plot.world.set_block(outside, Block::RedstoneLamp { lit: true });
        plot.world.schedule_tick(selected, 2, TickPriority::Normal);
        plot.world.schedule_tick(outside, 3, TickPriority::High);
        let positions = HashSet::from([selected]);
        let snapshot = RegionSnapshot::capture(&plot.world, positions.clone()).unwrap();
        let ticks = plot.world.scheduler().iter_entries().collect();
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &plot.world,
                (BlockPos::new(32, 20, 32), outside),
                Default::default(),
                ticks,
                Default::default(),
            )
            .unwrap();
        plot.world.clear_scheduled_ticks();
        plot.world.set_block(selected, Block::Air);

        plot.world.piston_state.motions.push_back(PistonMotion {
            pos: outside,
            identity: 1,
            progress: 0.0,
            previous_progress: 0.0,
            last_tick: 0,
            carried_entity: None,
        });
        assert!(snapshot.reset_compiler(&mut plot.world, &mut compiler).is_err());
        assert!(compiler.is_active());
        assert_eq!(plot.world.get_block(selected), Block::Air);
        assert!(plot.world.scheduler().iter_entries().next().is_none());
        plot.world.piston_state.motions.clear();

        snapshot.reset_compiler(&mut plot.world, &mut compiler).unwrap();
        assert!(!compiler.is_active());
        assert_eq!(plot.world.get_block(selected), Block::RedstoneLamp { lit: true });
        assert_eq!(plot.world.get_block(outside), Block::RedstoneLamp { lit: true });
        let ticks = plot.world.scheduler().iter_entries().collect::<Vec<_>>();
        assert_eq!(ticks.iter().filter(|tick| tick.pos == selected).count(), 1);
        assert_eq!(ticks.iter().filter(|tick| tick.pos == outside).count(), 1);
        plot.world.set_block(selected, Block::Stone {});
        assert_eq!(plot.world.get_block(selected), Block::Stone {});
    }
}
