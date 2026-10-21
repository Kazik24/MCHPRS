//! Source-derived metadata for a small physical timing island.
use super::*;

/// Physical clock-control nodes retained while a settled logical circuit advances its registers.
///
/// This describes topology only. Runtime activation still has to preserve the controller's
/// event scheduling and certify a stable boundary before committing logical outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledTimingIsland {
    /// The empty-lane piston whose base observer participates in its Quasi input.
    pub(crate) clock: PistonId,
    /// Piston actors whose resource or pose state drives the clock power cone.
    pub(crate) actors: Box<[PistonId]>,
    /// Power-cone nodes and base observers retained for physical event delivery.
    pub(crate) nodes: Box<[NodeId]>,
    /// The unique external lever found in the recursively traced power cone.
    pub(crate) trigger: NodeId,
    /// Observer nodes watching a retained actor base, including the clock output observers.
    pub(crate) observers: Box<[NodeId]>,
}
