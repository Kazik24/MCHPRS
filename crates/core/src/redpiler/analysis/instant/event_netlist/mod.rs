//! Source-qualified candidates for the experimental event-netlist compiler.
//!
//! Discovery and cuts do not transfer ownership. Corridor witnesses use settled
//! samples, so they can miss intermediate pulses; the frame has no complete
//! event/reset lifecycle or native output handoff. The private gate is a fixture
//! experiment only. These passes stay test-only until those gaps are closed.
mod boundary;
mod corridor_events;
mod coverage_cuts;
mod cuts;
mod discovery;
mod frame;
mod ingress;
mod native_cuts;
mod ports;
mod private_not;
mod receiver_growth;

pub(crate) use boundary::{callback_sources, candidate_boundary};
pub(crate) use corridor_events::{WireStrengthChange, corridor_events};
pub(crate) use coverage_cuts::cut_uncovered_callbacks;
pub(crate) use cuts::{CutSource, SourceCut, SourceOwner, source_cuts};
pub(crate) use discovery::discover;
pub(crate) use frame::CandidateFrame;
pub(crate) use ingress::actor_ingress;
pub(crate) use native_cuts::{CutReason, cut_unsupported_resources};
pub(crate) use ports::{PortRole, port_role};
pub(crate) use private_not::{PrivateNot, lower_private_not};
pub(crate) use receiver_growth::grow_comparators;

#[cfg(test)]
mod tests;
