//! Qualified torch/observer clock family for settled-device execution.
//!
//! This deliberately models one recognized controller pattern, not arbitrary circuits. The
//! delays below are primitive scheduler delays from the source graph; the repeating cadence
//! emerges from the phase transitions and is not stored as a period.
use crate::redpiler::program_graph::*;

const TORCH_SCHEDULE_TICKS: u64 = 2;
const OBSERVER_SCHEDULE_TICKS: u64 = 2;
const ACTION_DEFERRAL_TICKS: u64 = 1;
const OBSERVER_REST_TICKS: u64 = 1;
const EXTENSION_REMAINDER_TICKS: u64 = 1;
const QUEUED_ACTION_TICKS: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    ObserverOn(u64),
    Publish(u64),
    ObserverOff(u64),
    Ready(u64),
    QueuedAction(u64),
}

/// Clock boundaries consumed by the logical data machine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockStep {
    pub(crate) compute: bool,
    pub(crate) publish: bool,
    pub(crate) commit: bool,
}

/// Small source-qualified clock phase machine. It owns no physical graph or route state.
#[derive(Debug)]
pub(crate) struct LogicalClock {
    trigger_node: NodeId,
    torch_node: NodeId,
    pulse_nodes: Box<[NodeId]>,
    last_trigger: bool,
    torch_powered: bool,
    pulse_on: bool,
    pending_torch: Option<(u64, bool)>,
    phase: Phase,
    tick: u64,
}

impl LogicalClock {
    // shortcut: only this stationary clock family is modeled; extend after native cadence tests.
    /// Admit only the empty clock piston plus one sticky resource-phase actor, one torch and one lever.
    pub(crate) fn new(_program: &CircuitProgram, state: &CircuitState, island: &SettledTimingIsland) -> Result<Self, String> {
        if island.actors.len() != 2 {
            return Err("logical clock requires exactly two timing pistons".into());
        }
        let clock = island
            .actor_specs
            .iter()
            .find(|actor| actor.piston == island.clock)
            .ok_or("clock piston spec is missing")?;
        if clock.lane != PistonLane::Empty {
            return Err("logical clock piston must have an empty lane".into());
        }
        let phase_actors = island.actor_specs.iter().filter(|actor| actor.piston != island.clock).collect::<Vec<_>>();
        if phase_actors.len() != 1 || !phase_actors[0].sticky || !matches!(phase_actors[0].lane, PistonLane::SingleResource { .. }) {
            return Err("logical clock requires one sticky single-resource phase actor".into());
        }
        let torches = island
            .node_specs
            .iter()
            .filter(|node| node.kind == ClockNodeKind::Torch)
            .collect::<Vec<_>>();
        let levers = island
            .node_specs
            .iter()
            .filter(|node| node.kind == ClockNodeKind::Lever)
            .collect::<Vec<_>>();
        if torches.len() != 1 || levers.len() != 1 || levers[0].node != island.trigger {
            return Err("logical clock requires one source lever and one torch".into());
        }
        let clock_state = state.pistons.get(island.clock.index()).ok_or("clock piston state is missing")?;
        if !clock_state.extended || !clock_state.head_present || clock_state.availability != PistonAvailability::Ready {
            return Err("logical clock requires an extended stationary clock piston".into());
        }
        let mut torch_powered = false;
        let mut last_trigger = false;
        let mut pulse_nodes = Vec::new();
        for node in island.node_specs.iter() {
            let component = state.components.get(node.node.index()).ok_or("clock component state is missing")?;
            let ComponentState::Electrical(value) = component else {
                return Err("clock source has unsupported component state".into());
            };
            if value.pending_tick {
                return Err("logical clock activation requires a stationary controller".into());
            }
            match node.kind {
                ClockNodeKind::Lever => last_trigger = value.powered,
                ClockNodeKind::Torch => torch_powered = value.powered,
                ClockNodeKind::Observer | ClockNodeKind::BuiltinReset => {
                    if value.powered {
                        return Err("logical clock activation requires its observer/reset pulse off".into());
                    }
                    pulse_nodes.push(node.node);
                }
                ClockNodeKind::Repeater => return Err("logical clock does not admit repeaters".into()),
            }
        }
        if torch_powered == last_trigger {
            return Err("logical clock torch and lever are not in the recognized stable phase".into());
        }
        Ok(Self {
            trigger_node: island.trigger,
            torch_node: torches[0].node,
            pulse_nodes: pulse_nodes.into_boxed_slice(),
            last_trigger,
            torch_powered,
            pulse_on: false,
            pending_torch: None,
            phase: Phase::Idle,
            tick: 0,
        })
    }

    /// Apply the sampled lever level and advance one game tick of the source-derived phase machine.
    pub(crate) fn advance(&mut self, trigger: bool) -> Result<ClockStep, &'static str> {
        if trigger != self.last_trigger {
            self.last_trigger = trigger;
            let desired_torch = !trigger;
            self.pending_torch = (desired_torch != self.torch_powered).then_some((self.tick + TORCH_SCHEDULE_TICKS, desired_torch));
        }
        self.tick = self.tick.checked_add(1).ok_or("logical clock tick overflow")?;
        let mut result = ClockStep::default();
        if let Some((due, desired)) = self.pending_torch
            && due <= self.tick
        {
            self.pending_torch = None;
            let was_powered = self.torch_powered;
            self.torch_powered = desired;
            if was_powered && !desired {
                self.begin_cycle(&mut result);
            }
        }
        self.advance_phase(&mut result);
        Ok(result)
    }

    /// Expose only controller source values needed by the external boundary adapter.
    pub(crate) fn source_value(&self, node: NodeId) -> Option<bool> {
        if node == self.trigger_node {
            Some(self.last_trigger)
        } else if node == self.torch_node {
            Some(self.torch_powered)
        } else if self.pulse_nodes.contains(&node) {
            Some(self.pulse_on)
        } else {
            None
        }
    }

    fn begin_cycle(&mut self, result: &mut ClockStep) {
        result.compute = true;
        self.phase = Phase::ObserverOn(self.tick + OBSERVER_SCHEDULE_TICKS);
    }

    fn advance_phase(&mut self, result: &mut ClockStep) {
        self.phase = match self.phase {
            Phase::Idle => Phase::Idle,
            Phase::ObserverOn(due) if due <= self.tick => {
                self.pulse_on = true;
                Phase::Publish(self.tick + ACTION_DEFERRAL_TICKS)
            }
            Phase::Publish(due) if due <= self.tick => {
                result.publish = true;
                Phase::ObserverOff(self.tick + OBSERVER_REST_TICKS)
            }
            Phase::ObserverOff(due) if due <= self.tick => {
                self.pulse_on = false;
                result.commit = true;
                Phase::Ready(self.tick + EXTENSION_REMAINDER_TICKS)
            }
            Phase::Ready(due) if due <= self.tick => Phase::QueuedAction(self.tick + QUEUED_ACTION_TICKS),
            Phase::QueuedAction(due) if due <= self.tick => {
                if !self.torch_powered {
                    result.compute = true;
                    Phase::ObserverOn(self.tick + OBSERVER_SCHEDULE_TICKS)
                } else {
                    Phase::Idle
                }
            }
            phase => phase,
        };
    }
}
