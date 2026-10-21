//! Source-derived description of a small logical clock controller.
use super::*;

/// Static controller state needed by the pure logical clock kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledTimingIsland {
    pub(crate) clock: PistonId,
    pub(crate) actors: Box<[PistonId]>,
    pub(crate) nodes: Box<[NodeId]>,
    pub(crate) trigger: NodeId,
    pub(crate) actor_specs: Box<[ClockActorSpec]>,
    pub(crate) node_specs: Box<[ClockNodeSpec]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockActorSpec {
    pub(crate) piston: PistonId,
    pub(crate) node: NodeId,
    pub(crate) base: ContactId,
    pub(crate) near: ContactId,
    pub(crate) far: ContactId,
    pub(crate) lane: PistonLane,
    pub(crate) resource: Option<ResourceId>,
    pub(crate) sticky: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockNodeSpec {
    pub(crate) node: NodeId,
    pub(crate) kind: ClockNodeKind,
    pub(crate) watched_contact: Option<ContactId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClockNodeKind {
    Lever,
    Torch,
    Repeater,
    Observer,
    BuiltinReset,
}
