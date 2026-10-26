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
    StickyRetract(u64),
    StickyExtend(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockKind {
    TorchObserver,
    StickyObserver,
}

/// Clock boundaries consumed by the logical data machine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockStep {
    pub(crate) compute: bool,
    pub(crate) resample: bool,
    pub(crate) publish: bool,
    pub(crate) commit: bool,
}

/// Small source-qualified clock phase machine. It owns no physical graph or route state.
#[derive(Debug)]
pub(crate) struct LogicalClock {
    kind: ClockKind,
    trigger_node: NodeId,
    torch_node: Option<NodeId>,
    clock_piston: PistonId,
    clock_extended: bool,
    pulse_nodes: Box<[NodeId]>,
    last_trigger: bool,
    torch_powered: bool,
    pulse_on: bool,
    pending_torch: Option<(u64, bool)>,
    phase: Phase,
    tick: u64,
}

impl LogicalClock {
    /// Admit the existing two-actor torch controller or one sticky base-observer clock actor.
    pub(crate) fn new(_program: &CircuitProgram, state: &CircuitState, island: &SettledTimingIsland) -> Result<Self, String> {
        let clock = island
            .actor_specs
            .iter()
            .find(|actor| actor.piston == island.clock)
            .ok_or("clock piston spec is missing")?;
        let sticky_observer_clock = island.actors.len() == 1 && clock.sticky && matches!(clock.lane, PistonLane::SingleResource { .. });
        let torch_observer_clock = island.actors.len() == 2
            && clock.lane == PistonLane::Empty
            && island
                .actor_specs
                .iter()
                .filter(|actor| actor.piston != island.clock)
                .any(|actor| actor.sticky && matches!(actor.lane, PistonLane::SingleResource { .. }));
        if !sticky_observer_clock && !torch_observer_clock {
            return Err("logical clock is outside the qualified observer families".into());
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
        if levers.len() != 1 || levers[0].node != island.trigger {
            return Err("logical clock requires one source lever".into());
        }
        if (sticky_observer_clock && !torches.is_empty()) || (torch_observer_clock && torches.len() != 1) {
            return Err("logical clock has an unsupported torch topology".into());
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
        if torch_observer_clock && torch_powered == last_trigger {
            return Err("logical clock torch and lever are not in the recognized stable phase".into());
        }
        if sticky_observer_clock {
            let base_observers = island.node_specs.iter().filter(|node| {
                matches!(node.kind, ClockNodeKind::Observer | ClockNodeKind::BuiltinReset) && node.watched_contact == Some(clock.base)
            });
            if base_observers.count() != 1 {
                return Err("sticky logical clock requires one observer watching its base".into());
            }
            if island.node_specs.iter().any(|node| node.kind == ClockNodeKind::Repeater) {
                return Err("sticky logical clock does not admit repeaters".into());
            }
        }
        Ok(Self {
            kind: if sticky_observer_clock {
                ClockKind::StickyObserver
            } else {
                ClockKind::TorchObserver
            },
            trigger_node: island.trigger,
            torch_node: torches.first().map(|node| node.node),
            clock_piston: island.clock,
            clock_extended: clock_state.extended,
            pulse_nodes: pulse_nodes.into_boxed_slice(),
            last_trigger,
            torch_powered,
            pulse_on: false,
            pending_torch: None,
            phase: if sticky_observer_clock && !last_trigger {
                Phase::StickyRetract(1)
            } else {
                Phase::Idle
            },
            tick: 0,
        })
    }

    /// Apply the sampled lever level and advance one game tick of the source-derived phase machine.
    pub(crate) fn advance(&mut self, trigger: bool) -> Result<ClockStep, &'static str> {
        if self.kind == ClockKind::StickyObserver {
            return self.advance_sticky_observer(trigger);
        }
        let resample = trigger && trigger != self.last_trigger;
        if trigger != self.last_trigger {
            self.last_trigger = trigger;
            let desired_torch = !trigger;
            self.pending_torch = (desired_torch != self.torch_powered).then_some((self.tick + TORCH_SCHEDULE_TICKS, desired_torch));
        }
        self.tick = self.tick.checked_add(1).ok_or("logical clock tick overflow")?;
        let mut result = ClockStep::default();
        result.resample = resample;
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
        } else if Some(node) == self.torch_node {
            Some(self.torch_powered)
        } else if self.pulse_nodes.contains(&node) {
            Some(self.pulse_on)
        } else {
            None
        }
    }

    pub(crate) fn state_value(&self, test: StateTest) -> Option<bool> {
        match test {
            StateTest::PistonExtended(piston) | StateTest::PistonHeadPresent(piston) if piston == self.clock_piston => Some(self.clock_extended),
            _ => None,
        }
    }

    /// Whether the source-derived controller is stopped at an idle boundary where external
    /// input changes can be observed without racing a pending register commit.
    pub(crate) fn is_quiescent(&self) -> bool {
        match self.kind {
            ClockKind::TorchObserver => self.phase == Phase::Idle && self.pending_torch.is_none() && !self.last_trigger && self.torch_powered,
            ClockKind::StickyObserver => self.phase == Phase::Idle && self.last_trigger && !self.pulse_on,
        }
    }

    fn advance_sticky_observer(&mut self, trigger: bool) -> Result<ClockStep, &'static str> {
        self.tick = self.tick.checked_add(1).ok_or("logical clock tick overflow")?;
        let resample = trigger && trigger != self.last_trigger;
        if trigger != self.last_trigger {
            self.last_trigger = trigger;
            self.pulse_on = false;
            self.phase = if trigger { Phase::Idle } else { Phase::StickyRetract(self.tick) };
        }
        let mut result = ClockStep::default();
        result.resample = resample;
        self.phase = match self.phase {
            Phase::StickyRetract(due) if due <= self.tick && !self.last_trigger => {
                self.clock_extended = false;
                self.pulse_on = false;
                result.compute = true;
                result.publish = true;
                result.commit = true;
                Phase::StickyExtend(self.tick + ACTION_DEFERRAL_TICKS + OBSERVER_SCHEDULE_TICKS)
            }
            Phase::StickyExtend(due) if due <= self.tick && !self.last_trigger => {
                self.clock_extended = true;
                self.pulse_on = true;
                Phase::StickyRetract(self.tick + ACTION_DEFERRAL_TICKS + OBSERVER_SCHEDULE_TICKS)
            }
            phase => phase,
        };
        Ok(result)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sticky_clock() -> LogicalClock {
        LogicalClock {
            kind: ClockKind::StickyObserver,
            trigger_node: NodeId::new(0),
            torch_node: None,
            clock_piston: PistonId::new(0),
            clock_extended: true,
            pulse_nodes: Box::new([NodeId::new(1)]),
            last_trigger: false,
            torch_powered: false,
            pulse_on: false,
            pending_torch: None,
            phase: Phase::StickyRetract(1),
            tick: 0,
        }
    }

    #[test]
    fn sticky_observer_clock_publishes_on_each_retraction_edge_and_stops_with_lever() {
        let mut clock = sticky_clock();
        let mut edges = Vec::new();
        for tick in 1..=13 {
            let step = clock.advance(false).unwrap();
            if step.compute {
                assert!(step.publish && step.commit);
                edges.push(tick);
            }
            assert_eq!(clock.source_value(NodeId::new(1)), Some(matches!(tick, 4 | 5 | 6 | 10 | 11 | 12)));
        }
        assert_eq!(edges, [1, 7, 13]);

        for _ in 0..8 {
            let step = clock.advance(true).unwrap();
            assert!(!step.compute && !step.publish && !step.commit);
        }
        let step = clock.advance(false).unwrap();
        assert!(
            step.compute && step.publish && step.commit,
            "the falling lever edge publishes on the same tick"
        );
    }
}
