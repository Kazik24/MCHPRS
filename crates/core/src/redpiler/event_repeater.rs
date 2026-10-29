//! Explicit repeater adapter for experimental event execution; not installed by the compiler.
use crate::redpiler::TickScheduler;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{CircuitNodeKind, CircuitProgram, CircuitState, ComponentState};
use mchprs_blocks::BlockPos;
use mchprs_world::TickPriority;

#[derive(Clone)]
pub(crate) struct PhysicalRepeater {
    delay: usize,
    facing_diode: bool,
    powered: bool,
    locked: bool,
    input: u8,
    pending: TickScheduler<()>,
}

impl PhysicalRepeater {
    /// The owner must bind every input and callback; this adapter does not prove isolation.
    pub(crate) fn compile(program: &CircuitProgram, state: &CircuitState, position: BlockPos, initial_input: u8) -> Result<Self, &'static str> {
        let node = *program.position_index().get(&position).ok_or("repeater is not in the graph")?;
        let CircuitNodeKind::Electrical(NodeType::Repeater { delay, facing_diode }) = program.nodes()[node.index()].kind else {
            return Err("output is not a repeater");
        };
        let ComponentState::Electrical(initial) = &state.components[node.index()] else {
            return Err("repeater state is unavailable");
        };
        if !(1..=4).contains(&delay) || initial.pending_tick || initial_input > 15 {
            return Err("repeater delay or entry work is unsupported");
        }
        Ok(Self {
            delay: delay as usize,
            facing_diode,
            powered: initial.powered,
            locked: initial.repeater_locked,
            input: initial_input,
            pending: TickScheduler::default(),
        })
    }

    /// Deliver a live input notification between scheduler operations.
    pub(crate) fn notify(&mut self, input: u8, locked: bool) -> Result<(), &'static str> {
        if input > 15 {
            return Err("repeater input exceeds redstone strength");
        }
        self.input = input;
        self.locked = locked;
        let powered = input > 0;
        if !locked && self.pending.iter().next().is_none() && powered != self.powered {
            let priority = if self.facing_diode {
                TickPriority::Highest
            } else if powered {
                TickPriority::High
            } else {
                TickPriority::Higher
            };
            self.pending.schedule_tick((), self.delay, priority);
        }
        Ok(())
    }

    fn run_current(&mut self) -> bool {
        let mut changed = false;
        while self.pending.this_tick().pop_first().is_some() {
            if self.locked {
                continue;
            }
            let should_power = self.input > 0;
            if self.powered && !should_power {
                self.powered = false;
                changed = true;
            } else if !self.powered {
                if !should_power {
                    self.pending.schedule_tick((), self.delay, TickPriority::Higher);
                }
                self.powered = true;
                changed = true;
            }
        }
        changed
    }

    /// Advance both scheduled half-steps of one game tick.
    pub(crate) fn tick(&mut self) -> bool {
        let before = self.run_current();
        self.pending.end_last_tick_move_next();
        let after = self.run_current();
        before || after
    }

    pub(crate) fn powered(&self) -> bool {
        self.powered
    }

    pub(crate) fn pending(&self) -> Vec<(usize, TickPriority)> {
        self.pending.iter().map(|(_, delay, priority)| (delay, priority)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::backend::graph::wire_probe::{
        physical_program,
        tests::{BOUNDS, LEFT, RECEIVER, RIGHT, fixture},
    };
    use crate::redstone;
    use crate::world::World;
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::blocks::Block;

    fn input(world: &crate::plot::PlotWorld) -> u8 {
        let Block::RedstoneRepeater { repeater } = world.get_block(RECEIVER) else {
            panic!("missing repeater")
        };
        redstone::diode_get_input_strength(world, RECEIVER, repeater.facing)
    }

    fn set_lever(world: &mut crate::plot::PlotWorld, position: BlockPos, powered: bool) {
        let Block::Lever { mut lever } = world.get_block(position) else {
            panic!("missing lever")
        };
        lever.powered = powered;
        world.set_block(position, Block::Lever { lever });
        redstone::update_surrounding_blocks(world, position);
        redstone::update_surrounding_blocks(world, position.offset(BlockFace::Bottom));
    }

    #[test]
    fn explicit_repeater_input_events_match_native_pulses_and_rearm() {
        let mut world = fixture();
        let Block::RedstoneRepeater { mut repeater } = world.get_block(RECEIVER) else {
            panic!("missing repeater")
        };
        repeater.powered = true;
        world.set_block(RECEIVER, Block::RedstoneRepeater { repeater });
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut adapter = PhysicalRepeater::compile(&program, &state, RECEIVER, input(&world)).unwrap();
        for tick in 0..24 {
            for (at, source, powered) in [
                (0, LEFT, false),
                (1, RIGHT, false),
                (5, LEFT, true),
                (6, LEFT, false),
                (12, RIGHT, true),
                (18, RIGHT, false),
            ] {
                if tick == at {
                    set_lever(&mut world, source, powered);
                    adapter.notify(input(&world), false).unwrap();
                }
            }
            world.tick_interpreted();
            adapter.tick();
            let Block::RedstoneRepeater { repeater } = world.get_block(RECEIVER) else {
                panic!("missing repeater")
            };
            assert_eq!(adapter.powered(), repeater.powered, "tick {tick}");
            let actual: Vec<_> = world
                .scheduler()
                .iter_entries()
                .filter(|entry| entry.pos == RECEIVER)
                .map(|entry| (entry.ticks_left as usize, entry.tick_priority))
                .collect();
            assert_eq!(adapter.pending(), actual, "pending tick {tick}");
        }
    }
}
