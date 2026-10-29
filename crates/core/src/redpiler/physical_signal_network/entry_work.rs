use crate::plot::PlotWorld;
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_world::{PistonEvent, PistonMotion, TickEntry};

/// Captured world work and the caller's selected ownership region.
pub(crate) struct EntryWork {
    bounds: (BlockPos, BlockPos),
    logical_tick: u64,
    phase: mchprs_world::AdvancePhase,
    scheduled: Vec<TickEntry>,
    piston_events: Vec<PistonEvent>,
    motions: Vec<PistonMotion>,
    movement_work: Vec<(BlockPos, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::backend::graph::{
        piston_probe::tests::{ACTOR, BOUNDS, LEVER, fixture},
        wire_probe::physical_program,
    };
    use crate::redpiler::physical_signal_network::PhysicalSignalNetwork;
    use crate::redpiler::program_graph::ComponentState;
    use mchprs_blocks::BlockFace;
    use mchprs_world::{PistonAction, TickPriority};

    #[test]
    fn admission_checks_owned_work_and_preserves_unrelated_work() {
        let mut world = fixture();
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        world.piston_state_mut().logical_tick = 37;
        let outside = BlockPos::new(30, 20, 5);
        world.restore_scheduled_tick(TickEntry {
            pos: outside,
            ticks_left: 3,
            tick_priority: TickPriority::High,
            block_type: None,
        });
        world.piston_state_mut().events.push_back(PistonEvent {
            pos: outside,
            sticky: true,
            facing: BlockFace::East,
            action: PistonAction::Extend,
        });
        let outside_ticks: Vec<_> = world.scheduler().iter_entries().collect();
        let outside_events = world.piston_state().events.clone();
        let compiler_entry = EntryWork::capture_from_scheduler(&world, BOUNDS, &outside_ticks);
        assert!(PhysicalSignalNetwork::compile(&program, &state, &compiler_entry).is_ok());
        let runtime = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
        assert_eq!(runtime.current_tick(), 37);
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), outside_ticks);
        assert_eq!(world.piston_state().events, outside_events);

        let original_block = world.get_block(ACTOR);
        let resources = state.resources.clone();
        world.restore_scheduled_tick(TickEntry {
            pos: LEVER,
            ticks_left: 2,
            tick_priority: TickPriority::Normal,
            block_type: None,
        });
        let ticks: Vec<_> = world.scheduler().iter_entries().collect();
        let error = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS))
            .err()
            .unwrap();
        assert!(
            error.contains("scheduled component tick") && error.contains(&LEVER.to_string()),
            "{error}"
        );
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
        assert_eq!(world.piston_state().events, outside_events);
        assert_eq!(world.get_block(ACTOR), original_block);
        assert_eq!(state.resources, resources);

        let mut action_world = fixture();
        action_world.piston_state_mut().events.push_back(PistonEvent {
            pos: ACTOR,
            sticky: true,
            facing: BlockFace::East,
            action: PistonAction::Extend,
        });
        let actions = action_world.piston_state().events.clone();
        let error = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&action_world, BOUNDS))
            .err()
            .unwrap();
        assert!(error.contains("queued piston action") && error.contains(&ACTOR.to_string()), "{error}");
        assert_eq!(action_world.piston_state().events, actions);
        action_world.piston_state_mut().events.clear();
        action_world.piston_state_mut().motions.push_back(PistonMotion {
            pos: ACTOR,
            identity: 4,
            progress: 0.5,
            previous_progress: 0.0,
            last_tick: 0,
            carried_entity: None,
        });
        let error = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&action_world, BOUNDS))
            .err()
            .unwrap();
        assert!(error.contains("piston movement") && error.contains(&ACTOR.to_string()), "{error}");
        assert_eq!(action_world.piston_state().motions.len(), 1);
        action_world.piston_state_mut().motions.clear();
        action_world.piston_state_mut().movement_work.push((ACTOR, 4));
        let error = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&action_world, BOUNDS))
            .err()
            .unwrap();
        assert!(
            error.contains("pending piston movement work") && error.contains(&ACTOR.to_string()),
            "{error}"
        );
        assert_eq!(action_world.piston_state().movement_work, vec![(ACTOR, 4)]);

        let mut inconsistent = state.clone();
        let source = program.position_index()[&LEVER].index();
        let ComponentState::Electrical(node) = &mut inconsistent.components[source] else {
            panic!("lever missing")
        };
        node.pending_tick = true;
        let error = PhysicalSignalNetwork::compile(&program, &inconsistent, &EntryWork::capture(&fixture(), BOUNDS))
            .err()
            .unwrap();
        assert!(
            error.contains("inconsistent component pending flag") && error.contains(&LEVER.to_string()),
            "{error}"
        );
    }
}

impl EntryWork {
    pub(crate) fn capture(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Self {
        Self::capture_from_scheduler(world, bounds, &world.scheduler().iter_entries().collect::<Vec<_>>())
    }

    /// Capture the actual world piston inventory and the scheduler entries being transferred
    /// at the compiler boundary. The caller must pass the scheduler's unfiltered inventory.
    pub(crate) fn capture_from_scheduler(world: &impl World, bounds: (BlockPos, BlockPos), scheduled: &[TickEntry]) -> Self {
        let pistons = world.piston_state();
        Self {
            bounds,
            logical_tick: pistons.logical_tick,
            phase: pistons.phase,
            scheduled: scheduled.to_vec(),
            piston_events: pistons.events.iter().copied().collect(),
            motions: pistons.motions.iter().cloned().collect(),
            movement_work: pistons.movement_work.clone(),
        }
    }

    pub(super) fn logical_tick(&self) -> u64 {
        self.logical_tick
    }

    fn owns(&self, pos: BlockPos) -> bool {
        let (a, b) = self.bounds;
        (a.x..=b.x).contains(&pos.x) && (a.y..=b.y).contains(&pos.y) && (a.z..=b.z).contains(&pos.z)
    }

    pub(super) fn validate(
        &self,
        program: &crate::redpiler::program_graph::CircuitProgram,
        state: &crate::redpiler::program_graph::CircuitState,
    ) -> Result<(), String> {
        use crate::redpiler::program_graph::ComponentState;
        if self.phase != mchprs_world::AdvancePhase::BetweenTicks {
            return Err(format!("entry is not between ticks: {:?}", self.phase));
        }
        for node in program.nodes() {
            if let Some(position) = node.position
                && !self.owns(position)
            {
                return Err(format!("compiled node outside selected region at {position}"));
            }
        }
        if let Some(tick) = self.scheduled.iter().find(|tick| self.owns(tick.pos)) {
            return Err(format!("scheduled component tick at {}", tick.pos));
        }
        if let Some(event) = self.piston_events.iter().find(|event| {
            let mut position = event.pos;
            (0..=13).any(|_| {
                let owned = self.owns(position);
                position = position.offset(event.facing);
                owned
            })
        }) {
            return Err(format!("queued piston action at {}", event.pos));
        }
        if let Some(motion) = self.motions.iter().find(|motion| self.owns(motion.pos)) {
            return Err(format!("piston movement at {}", motion.pos));
        }
        if let Some((pos, _)) = self.movement_work.iter().find(|(pos, _)| self.owns(*pos)) {
            return Err(format!("pending piston movement work at {pos}"));
        }
        for (index, component) in state.components.iter().enumerate() {
            if matches!(component, ComponentState::Electrical(node) if node.pending_tick) {
                let position = program
                    .nodes()
                    .get(index)
                    .and_then(|node| node.position)
                    .ok_or("pending component has no position")?;
                if self.owns(position) {
                    return Err(format!("inconsistent component pending flag at {position}"));
                }
            }
        }
        Ok(())
    }
}
