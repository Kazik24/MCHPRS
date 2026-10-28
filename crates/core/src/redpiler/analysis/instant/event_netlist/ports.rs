//! Classify existing directed witnesses without changing their source identity.
use crate::redpiler::program_graph::{BoundaryDirection, BoundaryPort, BoundaryPortKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortRole {
    /// The owner samples another source; that source remains independently owned.
    Read,
    /// The original route occurrence enters the owner.
    Event,
    /// A Turbo event also reads the original captured neighbor.
    Capture,
    /// An owned change may be observed or delivered outside the owner.
    Effect,
}

pub(crate) fn port_role(port: BoundaryPort) -> PortRole {
    match (port.direction, port.kind) {
        (BoundaryDirection::Output, _) => PortRole::Effect,
        (BoundaryDirection::Input, BoundaryPortKind::Callback { .. }) => PortRole::Event,
        (BoundaryDirection::Input, BoundaryPortKind::CapturedWire { .. }) => PortRole::Capture,
        (BoundaryDirection::Input, _) => PortRole::Read,
    }
}
