//! Ownership and physical aliases used while preparing a candidate graph.
//! Reset internals are represented by the region protocol, not ordinary nodes.
use crate::redpiler::analysis::families::reset_internals;
use crate::redpiler::analysis::topology::{PowerDependency, SourceKind};
use crate::redpiler::analysis::AnalysisReport;
use mchprs_blocks::BlockPos;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug)]
pub(crate) struct Boundaries<'a> {
    pub report: &'a AnalysisReport,
    mobile: FxHashMap<BlockPos, usize>,
    internals: FxHashSet<BlockPos>,
    consumers: FxHashSet<BlockPos>,
    hidden: FxHashSet<BlockPos>,
    retained: FxHashSet<BlockPos>,
    pub executable: bool,
}

impl<'a> Boundaries<'a> {
    pub fn new(report: &'a AnalysisReport) -> Self {
        let mobile = report
            .payload_groups
            .iter()
            .enumerate()
            .flat_map(|(group, g)| g.positions.iter().map(move |&pos| (pos, group)))
            .collect();
        let internals = reset_internals(&report.recognition);
        let consumers = report.ports.outputs.iter().map(|o| o.consumer).collect();
        Self {
            report,
            mobile,
            internals,
            consumers,
            hidden: Default::default(),
            retained: Default::default(),
            executable: false,
        }
    }

    pub fn executable(
        report: &'a AnalysisReport,
        wires: &FxHashSet<BlockPos>,
        sources: &[BlockPos],
    ) -> Self {
        let mut result = Self::new(report);
        result.executable = true;
        result.hidden.extend(wires.iter().copied());
        result.hidden.extend(
            report
                .pistons
                .iter()
                .flat_map(|p| [p.pos, p.head, p.payload]),
        );
        result.internals.extend(report.observers.iter().copied());
        result.retained.extend(sources.iter().copied());
        result
    }

    pub fn is_retained(&self, pos: BlockPos) -> bool {
        self.retained.contains(&pos)
    }

    pub fn mobile_group(&self, pos: BlockPos) -> Option<usize> {
        self.mobile.get(&pos).copied()
    }
    pub fn is_internal(&self, pos: BlockPos) -> bool {
        self.internals.contains(&pos)
    }
    pub fn is_owned(&self, pos: BlockPos) -> bool {
        self.is_internal(pos) || self.mobile.contains_key(&pos) || self.hidden.contains(&pos)
    }
    pub fn is_output(&self, pos: BlockPos) -> bool {
        self.consumers.contains(&pos)
    }

    pub fn internal_dependency(&self, piston: usize, dependency: &PowerDependency) -> bool {
        self.is_internal(dependency.source)
            || match dependency.kind {
                SourceKind::MobilePayload { group } => {
                    self.report.payload_groups[group].members.contains(&piston)
                }
                _ => false,
            }
    }
}
