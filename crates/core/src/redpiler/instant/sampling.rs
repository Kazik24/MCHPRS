//! Explicit independently delivered BUD write events; data values are not clocks.
use super::outputs::PowerTerm;
use mchprs_blocks::BlockPos;

pub(crate) struct SamplingEvent {
    pub source: SamplingSource,
    pub actors: Vec<usize>,
}

pub(crate) enum SamplingSource {
    /// A certified empty ordinary generator's settled base/head pose edge.
    Generator(usize),
    /// A static, independently driven dust net's delivered strength change.
    Power { pos: BlockPos, initial: u8, terms: Vec<PowerTerm> },
}
