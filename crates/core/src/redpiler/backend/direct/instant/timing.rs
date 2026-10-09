//! Electrical occupancy at a compiled piston boundary; no world callbacks.
use crate::redpiler::instant::boolean::GeometryPart;

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
    requested: bool,
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
            requested: retracted,
        }
    }

    pub fn request(&mut self, retracted: bool, now: u64) {
        if retracted == self.requested {
            return;
        }
        self.requested = retracted;
        // A moving base cannot extend until its retraction finishes.
        if self.phase == Phase::Retracting || (!retracted && self.phase == Phase::Extending) {
            return;
        }
        if (!retracted && self.phase == Phase::Extended)
            || (retracted && self.phase == Phase::Retracted)
        {
            self.deadline = None;
            return;
        }
        self.phase = if retracted {
            Phase::Retracting
        } else {
            Phase::Extending
        };
        self.deadline = Some(now + 2);
    }

    pub fn advance(&mut self, now: u64, resetting: bool) {
        if !self.deadline.is_some_and(|deadline| deadline <= now) {
            return;
        }
        let (phase, delay) = match self.phase {
            Phase::Retracting => (
                Phase::Retracted,
                (resetting || !self.requested).then_some(1),
            ),
            Phase::Retracted => (Phase::Extending, Some(2)),
            Phase::Extending => (Phase::Extended, (resetting && self.requested).then_some(1)),
            Phase::Extended => (Phase::Retracting, Some(2)),
        };
        self.phase = phase;
        self.deadline = delay.map(|delay| now + delay);
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

    #[test]
    fn restored_power_waits_for_retraction_completion_before_extending() {
        let mut boundary = Boundary::new(0, vec![0], false);
        boundary.request(true, 1);
        boundary.request(false, 2);
        assert!(boundary.phase == Phase::Retracting);
        assert_eq!(boundary.deadline, Some(3));
        for (tick, phase) in [
            (3, Phase::Retracted),
            (4, Phase::Extending),
            (5, Phase::Extending),
            (6, Phase::Extended),
        ] {
            boundary.advance(tick, false);
            assert!(boundary.phase == phase, "tick {tick}");
            assert_eq!(boundary.geometry(GeometryPart::FarPayload), tick == 6);
        }
        assert!(boundary.deadline.is_none());
    }
}
