//! Ownership and physical aliases used while preparing a candidate graph.
//! Reset internals are represented by the region protocol, not ordinary nodes.
use super::outputs::OutputPort;
use crate::redpiler::analysis::families::reset_internals;
use crate::redpiler::analysis::ports::ConsumerInput;
use crate::redpiler::analysis::topology::{PowerDependency, SourceKind};
use crate::redpiler::analysis::AnalysisReport;
use crate::redpiler::compile_graph::LinkType;
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
    propagation_wires: FxHashSet<BlockPos>,
    callback_writers: FxHashSet<BlockPos>,
    pub executable: bool,
    pub outputs: &'a [OutputPort],
    output_channels: FxHashMap<(BlockPos, bool), usize>,
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
            propagation_wires: Default::default(),
            callback_writers: Default::default(),
            executable: false,
            outputs: &[],
            output_channels: Default::default(),
        }
    }

    pub fn executable(
        report: &'a AnalysisReport,
        wires: &FxHashSet<BlockPos>,
        sources: &[BlockPos],
        outputs: &'a [OutputPort],
        owned: &FxHashSet<BlockPos>,
        empty_far: &FxHashSet<BlockPos>,
        propagation_wires: &FxHashSet<BlockPos>,
    ) -> Result<Self, String> {
        let mut result = Self::new(report);
        result.executable = true;
        result.outputs = outputs;
        for (id, port) in outputs.iter().enumerate() {
            if result
                .output_channels
                .insert(
                    (port.consumer, port.input == ConsumerInput::ComparatorSide),
                    id,
                )
                .is_some()
            {
                return Err(format!(
                    "consumer at {:?} has duplicate instant output channel ownership",
                    port.consumer
                ));
            }
        }
        result
            .consumers
            .extend(outputs.iter().map(|port| port.consumer));
        result.hidden.extend(wires.iter().copied());
        result.hidden.extend(owned.iter().copied());
        result.propagation_wires = propagation_wires.clone();
        result.hidden.retain(|pos| !propagation_wires.contains(pos));
        // Compile-owned reset/clock observers have no ordinary graph source;
        // electrical search must omit them just as logical extraction does.
        result.internals.extend(
            report
                .observers
                .iter()
                .copied()
                .filter(|pos| owned.contains(pos)),
        );
        result.mobile.retain(|pos, _| !empty_far.contains(pos));
        result.retained.extend(sources.iter().copied());
        // Extracted live sources outrank a provisional reset-family label.
        // Otherwise identify_nodes removes a source that the plan must bind.
        result
            .internals
            .retain(|pos| !result.retained.contains(pos));
        result.hidden.retain(|pos| !result.retained.contains(pos));
        Ok(result)
    }

    pub fn projects(&self, pos: BlockPos, input: LinkType) -> bool {
        self.output_channels
            .contains_key(&(pos, input == LinkType::Side))
    }

    pub fn is_propagation_wire(&self, pos: BlockPos) -> bool {
        self.propagation_wires.contains(&pos)
    }

    pub fn require_callbacks(&mut self, writer: BlockPos) {
        self.callback_writers.insert(writer);
    }

    pub fn needs_callbacks(&self, pos: BlockPos) -> bool {
        self.callback_writers.contains(&pos)
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
