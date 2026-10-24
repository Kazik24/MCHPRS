//! Cold native replay findings for hypothetical single-source falling transitions.
use crate::redpiler::TaskMonitor;
use mchprs_blocks::BlockPos;

/// The physical source type removed by one independent cold replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TimingSourceKind {
    /// A fixed redstone block supply is destroyed and its normal updates are delivered.
    RedstoneBlock,
    /// A powered lever is toggled off with its normal two notification paths.
    Lever,
}

/// Why a bounded source probe could not produce a complete result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TimingUnknownKind {
    /// The supplied plot snapshot was not idle when analysis began.
    PendingSnapshot,
    /// The task was cancelled before the source replay completed.
    Cancelled,
    /// The source/action could not be replayed with supported native semantics.
    Unsupported,
    /// A source exceeded a fixed analysis work limit.
    WorkBound,
    /// Eligible sources remained after the candidate cap.
    SourceCap,
}

/// One source and why its falling transition could not be replayed completely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimingProbeUnknown {
    /// This is the source position when one was identified.
    pub(crate) source: Option<(TimingSourceKind, BlockPos)>,
    /// This identifies the unsupported or bounded replay condition.
    pub(crate) kind: TimingUnknownKind,
}

/// A nearby compiled wire whose power changed during an accepted consumer action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimingWireChange {
    /// This is a wire found within the bounded local observation neighborhood.
    pub(crate) position: BlockPos,
    /// This is the observed wire strength before consumer acceptance.
    pub(crate) before: u8,
    /// This is the observed wire strength after consumer acceptance.
    pub(crate) after: u8,
}

/// A native replay proving that a later accepted action inhibited an already-accepted piston.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimingHazard {
    /// This is the hypothetical fixed source removed for the replay.
    pub(crate) source_kind: TimingSourceKind,
    /// This is the absolute source position.
    pub(crate) source: BlockPos,
    /// This piston accepted its retract while its effective power was false.
    pub(crate) consumer: BlockPos,
    /// This is the later accepted piston action that changed consumer power.
    pub(crate) inhibitor: BlockPos,
    /// This is the public nano queue cohort containing consumer acceptance, one-based.
    pub(crate) consumer_cohort: u32,
    /// This is the public nano queue cohort containing the later power change, one-based.
    pub(crate) inhibit_cohort: u32,
    /// This is an optional local output-wire strength change at the consumer action.
    pub(crate) wire_change: Option<TimingWireChange>,
}

/// Bounded results from independent, read-only source-falling replays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TimingProbeReport {
    /// This counts all graph-derived source candidates before the probe cap.
    pub(crate) source_candidates: usize,
    /// This counts source snapshots replayed to completion.
    pub(crate) sources_tested: usize,
    /// These candidates were skipped or ended before a complete first piston-event phase.
    pub(crate) unknown: Vec<TimingProbeUnknown>,
    /// These are observed late-inhibition effects from completed source-falling replays.
    pub(crate) hazards: Vec<TimingHazard>,
    /// This is true when the source cap omitted otherwise eligible candidates.
    pub(crate) source_cap_reached: bool,
    /// This is true when any candidate hit its bounded work limit.
    pub(crate) work_cap_reached: bool,
}

/// Render timing probe results without claiming an intended circuit truth table.
pub(crate) fn report(probe: &TimingProbeReport, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    check_cancelled(monitor)?;
    let incomplete = probe.source_candidates.saturating_sub(probe.sources_tested);
    let mut lines = vec![format!(
        "Timing: {} late-inhibit hazard{} observed; {}/{} hypothetical source-falling probes completed{}",
        probe.hazards.len(),
        if probe.hazards.len() == 1 { "" } else { "s" },
        probe.sources_tested,
        probe.source_candidates,
        if incomplete == 0 {
            String::new()
        } else {
            format!("; {incomplete} incomplete")
        }
    )];
    lines.push(
        "Timing: independent source-fall probes from this stable snapshot; one native queue snapshot is one nanotick; other controls held".into(),
    );
    if probe.source_candidates == 0 {
        lines.push("Timing: no eligible fixed redstone-block or powered-lever source".into());
    }

    let mut shown = 0;
    const MAX_TIMING_HAZARDS: usize = 4;
    for hazard in probe.hazards.iter().take(MAX_TIMING_HAZARDS) {
        check_cancelled(monitor)?;
        let gap = hazard.inhibit_cohort.saturating_sub(hazard.consumer_cohort);
        let cohort_relation = if gap == 0 {
            format!("later within same nanotick (cohort {})", hazard.consumer_cohort)
        } else {
            format!("+{gap} nanotick{}", if gap == 1 { "" } else { "s" })
        };
        let wire = hazard.wire_change.map_or_else(String::new, |change| {
            format!(
                "Effect: wire ({}, {}, {}) power {}→{}",
                change.position.x, change.position.y, change.position.z, change.before, change.after,
            )
        });
        lines.push(format!(
            "Timing: late inhibit at ({}, {}, {}): {cohort_relation} (cohort {}→{}); caused by ({}, {}, {}), falling {} at ({}, {}, {})",
            hazard.consumer.x,
            hazard.consumer.y,
            hazard.consumer.z,
            hazard.consumer_cohort,
            hazard.inhibit_cohort,
            hazard.inhibitor.x,
            hazard.inhibitor.y,
            hazard.inhibitor.z,
            source_name(hazard.source_kind),
            hazard.source.x,
            hazard.source.y,
            hazard.source.z,
        ));
        lines.push(if wire.is_empty() {
            "Effect: accepted retraction moved the consumer base".into()
        } else {
            wire
        });
        shown += 1;
    }
    if probe.hazards.len() > shown {
        lines.push(format!("Timing: showing {shown} of {} observed hazards", probe.hazards.len()));
    }
    if !probe.unknown.is_empty() || probe.source_cap_reached || probe.work_cap_reached {
        let mut reasons = std::collections::BTreeMap::<TimingUnknownKind, usize>::new();
        for unknown in &probe.unknown {
            check_cancelled(monitor)?;
            *reasons.entry(unknown.kind).or_default() += 1;
        }
        if probe.source_cap_reached {
            let omitted = probe.source_candidates.saturating_sub(probe.sources_tested + probe.unknown.len());
            if omitted > 0 {
                *reasons.entry(TimingUnknownKind::SourceCap).or_default() += omitted;
            }
        }
        let summary = reasons
            .into_iter()
            .map(|(kind, count)| format!("{count} {}", unknown_name(kind)))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Timing: incomplete probes: {summary}"));
    } else if !probe.hazards.is_empty() {
        lines.push("Timing: only the tested source-falling transitions were assessed; other controls and combinations remain unknown".into());
    } else if probe.source_candidates > 0 {
        lines.push("Timing: no late inhibition was observed in the completed probes; other controls and combinations remain unknown".into());
    }
    check_cancelled(monitor)?;
    Ok(lines)
}

fn source_name(kind: TimingSourceKind) -> &'static str {
    match kind {
        TimingSourceKind::RedstoneBlock => "redstone-block removal",
        TimingSourceKind::Lever => "lever falling edge",
    }
}

fn unknown_name(kind: TimingUnknownKind) -> &'static str {
    match kind {
        TimingUnknownKind::PendingSnapshot => "pending snapshot",
        TimingUnknownKind::Cancelled => "cancelled",
        TimingUnknownKind::Unsupported => "unsupported replay",
        TimingUnknownKind::WorkBound => "hit a work bound",
        TimingUnknownKind::SourceCap => "omitted by source cap",
    }
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("timing diagnostics cancelled".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hazard(consumer_cohort: u32, inhibit_cohort: u32) -> TimingHazard {
        TimingHazard {
            source_kind: TimingSourceKind::RedstoneBlock,
            source: BlockPos::new(1, 2, 3),
            consumer: BlockPos::new(4, 5, 6),
            inhibitor: BlockPos::new(7, 8, 9),
            consumer_cohort,
            inhibit_cohort,
            wire_change: Some(TimingWireChange {
                position: BlockPos::new(4, 5, 8),
                before: 15,
                after: 0,
            }),
        }
    }

    #[test]
    fn timing_report_shows_nanotick_gap_and_uses_clear_same_cohort_wording() {
        let mut probe = TimingProbeReport {
            source_candidates: 1,
            sources_tested: 1,
            ..Default::default()
        };
        probe.hazards.push(hazard(2, 3));
        probe.hazards.push(hazard(5, 5));

        let lines = report(&probe, &TaskMonitor::default()).unwrap();
        assert!(lines.iter().any(|line| line.contains("+1 nanotick (cohort 2→3)")));
        assert!(lines.iter().any(|line| line.contains("later within same nanotick (cohort 5)")));
        assert!(!lines.join("\n").contains("0 nanotick"));
    }

    #[test]
    fn timing_details_are_capped_to_four_two_line_witnesses() {
        let mut probe = TimingProbeReport {
            source_candidates: 5,
            sources_tested: 5,
            ..Default::default()
        };
        probe.hazards.extend((0..5).map(|index| hazard(index + 1, index + 2)));

        let lines = report(&probe, &TaskMonitor::default()).unwrap();
        assert_eq!(lines.iter().filter(|line| line.starts_with("Timing: late inhibit at ")).count(), 4);
        assert!(lines.iter().any(|line| line.contains("showing 4 of 5 observed hazards")));
        assert!(lines.len() <= 12, "{lines:?}");
    }
}
