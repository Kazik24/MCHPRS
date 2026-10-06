//! Strength, event and prepared-data channels must not be conflated.
use mchprs_blocks::BlockPos;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A Minecraft electrical strength. Reject malformed values before they can
/// index the Direct backend's sixteen strength counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Strength(u8);

impl Strength {
    pub const ZERO: Self = Self(0);
    pub const FULL: Self = Self(15);

    pub fn get(self) -> u8 {
        self.0
    }

    pub fn is_powered(self) -> bool {
        self.0 != 0
    }

    pub fn attenuate(self, distance: u8) -> Self {
        Self(self.0.saturating_sub(distance))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidStrength(pub u8);

impl fmt::Display for InvalidStrength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "redstone strength {} exceeds 15", self.0)
    }
}

impl std::error::Error for InvalidStrength {}

impl TryFrom<u8> for Strength {
    type Error = InvalidStrength;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value <= 15 {
            Ok(Self(value))
        } else {
            Err(InvalidStrength(value))
        }
    }
}

impl From<Strength> for u8 {
    fn from(value: Strength) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Polarity {
    Powered,
    Unpowered,
}

impl Polarity {
    pub fn decode(self, strength: Strength) -> bool {
        strength.is_powered() == (self == Self::Powered)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElectricalRole {
    /// Prepared data changes cached input, without launching a computation.
    PreparedData { polarity: Polarity },
    /// Logical one is a nonzero-to-zero edge at this interface, after attenuation.
    FallingTrigger,
    /// A physical electrical output, independent of its Boolean expression.
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElectricalPort {
    pub id: PortId,
    pub role: ElectricalRole,
    /// World positions retained even when ordinary graph wires are optimized out.
    pub aliases: Vec<BlockPos>,
    pub initial_strength: Strength,
}

/// Sampling has its own channel: an update can sample unchanged low data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplingPort {
    pub id: PortId,
    pub aliases: Vec<BlockPos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrengthChange {
    pub previous: Strength,
    pub current: Strength,
}

impl StrengthChange {
    pub fn falling(self) -> bool {
        self.previous.is_powered() && !self.current.is_powered()
    }
}

/// Initialize from the live world; compiling an already-low net is not an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElectricalState {
    strength: Strength,
}

impl ElectricalState {
    pub fn new(initial: Strength) -> Self {
        Self { strength: initial }
    }

    pub fn strength(self) -> Strength {
        self.strength
    }

    pub fn observe(&mut self, next: Strength) -> StrengthChange {
        let change = StrengthChange {
            previous: self.strength,
            current: next,
        };
        self.strength = next;
        change
    }
}

/// A falling edge is provisional until the physical adapter's qualifying
/// recheck and launch checkpoint. Restored power cancels the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TriggerState {
    requested: bool,
    rechecked: bool,
}

impl TriggerState {
    pub fn observe(&mut self, change: StrengthChange) {
        if change.current.is_powered() {
            *self = Self::default();
        } else if change.falling() {
            self.requested = true;
            self.rechecked = false;
        }
    }

    /// Power-only quasi-connectivity changes do not supply this notification.
    pub fn recheck(&mut self) {
        self.rechecked |= self.requested;
    }

    /// The caller establishes family readiness and finalizes the wave's inputs.
    pub fn accept(&mut self, ready: bool) -> bool {
        let accepted = ready && self.requested && self.rechecked;
        // An external request during reset is outside the admitted protocol;
        // it must not be buffered into a new computation after reset finishes.
        if accepted || !ready {
            *self = Self::default();
        }
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_is_decoded_after_attenuation_and_not_from_held_zero() {
        let mut electrical = ElectricalState::new(Strength::try_from(2).unwrap().attenuate(1));
        let mut trigger = TriggerState::default();
        trigger.observe(electrical.observe(Strength::try_from(1).unwrap().attenuate(1)));
        assert!(!trigger.accept(true));
        trigger.recheck();
        assert!(trigger.accept(true));
        trigger.observe(electrical.observe(Strength::ZERO));
        trigger.recheck();
        assert!(!trigger.accept(true));
    }

    #[test]
    fn restoration_before_launch_cancels_a_request() {
        let mut electrical = ElectricalState::new(Strength::FULL);
        let mut trigger = TriggerState::default();
        trigger.observe(electrical.observe(Strength::ZERO));
        trigger.recheck();
        trigger.observe(electrical.observe(Strength::FULL));
        assert!(!trigger.accept(true));
        trigger.observe(electrical.observe(Strength::ZERO));
        trigger.recheck();
        assert!(trigger.accept(true));
    }

    #[test]
    fn requests_during_reset_are_not_replayed_when_readiness_returns() {
        let mut electrical = ElectricalState::new(Strength::FULL);
        let mut trigger = TriggerState::default();
        trigger.observe(electrical.observe(Strength::ZERO));
        trigger.recheck();
        assert!(!trigger.accept(false));
        assert!(!trigger.accept(true));
    }

    #[test]
    fn low_initialization_and_prepared_data_do_not_create_events() {
        let mut electrical = ElectricalState::new(Strength::ZERO);
        assert!(!electrical.observe(Strength::ZERO).falling());
        assert!(Polarity::Unpowered.decode(electrical.strength()));
        assert!(!Polarity::Powered.decode(electrical.strength()));
    }

    #[test]
    fn malformed_strength_is_rejected_including_deserialization() {
        assert_eq!(Strength::try_from(16), Err(InvalidStrength(16)));
        assert!(serde_json::from_str::<Strength>("255").is_err());
        assert_eq!(serde_json::to_string(&Strength::FULL).unwrap(), "15");
    }
}
