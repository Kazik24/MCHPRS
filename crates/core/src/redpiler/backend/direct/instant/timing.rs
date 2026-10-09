//! Electrical occupancy at a compiled piston boundary; no world callbacks.
use crate::redpiler::instant::boolean::GeometryPart;
use mchprs_blocks::BlockPos;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Extended,
    Retracting,
    Retracted,
    Extending,
}

pub(super) struct Boundary {
    pub actor: usize,
    pub reset_owners: Vec<usize>,
    pub phase: Phase,
    pub deadline: Option<u64>,
    pub observers: Vec<(BlockPos, ObserverPulse)>,
    pub requested: bool,
}

/// Native observers pulse two half ticks after a watched base changes.
pub(super) struct ObserverPulse {
    pub powered: bool,
    pub deadline: Option<u64>,
}

impl ObserverPulse {
    pub fn new(powered: bool) -> Self {
        Self {
            powered,
            deadline: None,
        }
    }

    fn notify(&mut self, now: u64) {
        if !self.powered && self.deadline.is_none() {
            self.deadline = Some(now + 2);
        }
    }

    fn advance(&mut self, now: u64) {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.powered = !self.powered;
            self.deadline = self.powered.then_some(now + 2);
        }
    }
}

impl Boundary {
    pub fn new(actor: usize, reset_owners: Vec<usize>, retracted: bool) -> Self {
        Self {
            actor,
            reset_owners,
            phase: if retracted {
                Phase::Retracted
            } else {
                Phase::Extended
            },
            deadline: None,
            observers: Vec::new(),
            requested: retracted,
        }
    }

    pub fn request(&mut self, retracted: bool, now: u64) {
        if retracted == self.requested {
            return;
        }
        self.requested = retracted;
        if (retracted && self.phase == Phase::Retracting)
            || (!retracted && self.phase == Phase::Extending)
        {
            return;
        }
        if (!retracted && self.phase == Phase::Extended)
            || (retracted && self.phase == Phase::Retracted)
        {
            self.deadline = None;
            return;
        }
        let previous = self.phase;
        self.phase = if retracted {
            Phase::Retracting
        } else {
            Phase::Extending
        };
        self.deadline = Some(now + 2);
        self.notify_observers(previous, now);
    }

    pub fn advance_observers(&mut self, now: u64) {
        for (_, observer) in &mut self.observers {
            observer.advance(now);
        }
    }

    fn notify_observers(&mut self, previous: Phase, now: u64) {
        // Extension completion changes the head, while the base stays extended.
        let base_changed = previous != self.phase
            && !matches!(
                (previous, self.phase),
                (Phase::Extending, Phase::Extended) | (Phase::Extended, Phase::Extending)
            );
        if base_changed {
            for (_, observer) in &mut self.observers {
                observer.notify(now);
            }
        }
    }

    pub fn advance(&mut self, now: u64, resetting: bool) {
        if !self.deadline.is_some_and(|deadline| deadline <= now) {
            return;
        }
        let (phase, delay) = match self.phase {
            Phase::Retracting => (Phase::Retracted, resetting.then_some(1)),
            Phase::Retracted => (Phase::Extending, Some(2)),
            Phase::Extending => (Phase::Extended, (resetting && self.requested).then_some(1)),
            Phase::Extended => (Phase::Retracting, Some(2)),
        };
        let previous = self.phase;
        self.phase = phase;
        self.deadline = delay.map(|delay| now + delay);
        self.notify_observers(previous, now);
    }

    pub fn geometry(&self, part: GeometryPart) -> bool {
        match part {
            GeometryPart::FarPayload | GeometryPart::Head => self.phase == Phase::Extended,
            GeometryPart::NearPayload | GeometryPart::RetractedBase => {
                self.phase == Phase::Retracted
            }
            GeometryPart::MovingBase => self.phase == Phase::Retracting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_observers_keep_native_pulse_deadlines() {
        let mut boundary = Boundary::new(0, vec![0], false);
        boundary
            .observers
            .push((BlockPos::new(0, 1, 0), ObserverPulse::new(false)));
        boundary.request(true, 2);
        for (tick, powered, deadline) in [
            (2, false, Some(4)),
            (3, false, Some(4)),
            (4, true, Some(6)),
            (5, true, Some(6)),
            (6, false, None),
            (7, false, None),
            (8, false, Some(10)),
        ] {
            boundary.advance_observers(tick);
            boundary.advance(tick, true);
            let observer = &boundary.observers[0].1;
            assert_eq!((observer.powered, observer.deadline), (powered, deadline));
        }
    }

    #[test]
    fn reset_boundary_repeats_its_certified_six_tick_cycle() {
        let mut boundary = Boundary::new(0, vec![0], false);
        boundary.request(true, 2);
        for (tick, expected) in [
            (2, Phase::Retracting),
            (3, Phase::Retracting),
            (4, Phase::Retracted),
            (5, Phase::Extending),
            (6, Phase::Extending),
            (7, Phase::Extended),
            (8, Phase::Retracting),
            (10, Phase::Retracted),
        ] {
            boundary.advance(tick, true);
            assert!(boundary.phase == expected, "tick {tick}");
        }
    }

    #[test]
    fn memory_boundary_holds_and_extension_supply_waits_for_completion() {
        let mut boundary = Boundary::new(0, Vec::new(), false);
        boundary.request(true, 0);
        assert!(!boundary.geometry(GeometryPart::FarPayload));
        boundary.advance(2, false);
        boundary.advance(32, false);
        assert!(boundary.geometry(GeometryPart::NearPayload));
        boundary.request(false, 32);
        assert!(!boundary.geometry(GeometryPart::FarPayload));
        boundary.advance(33, false);
        assert!(!boundary.geometry(GeometryPart::FarPayload));
        boundary.advance(34, false);
        assert!(boundary.geometry(GeometryPart::FarPayload));
        assert!(boundary.deadline.is_none());
    }

    #[test]
    fn stopping_a_cycle_finishes_its_existing_extension() {
        let mut boundary = Boundary::new(0, vec![0], false);
        boundary.request(true, 2);
        boundary.advance(4, true);
        boundary.advance(5, true);
        boundary.request(false, 6);
        boundary.advance(7, false);
        assert!(boundary.phase == Phase::Extended);
        assert!(boundary.deadline.is_none());
    }
}
