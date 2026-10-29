//! Opt-in test-only counts for repeated piston samples inside exact wire episodes.
use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) const EXACT: u8 = 0;
pub(super) const REPLAY: u8 = 1;
pub(super) const CHANNEL: u8 = 2;
pub(super) const STABLE: u8 = 3;

const KIND_NAMES: [&str; 4] = ["exact", "replay", "channel", "stable"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    samples: u64,
    qualifying: u64,
    no_action: u64,
    repeated_any: u64,
    repeated_qualifying_same_action: u64,
    repeated_no_action: u64,
    action_conflicts: u64,
    /// Upper bound: same powered/action decision as the prior sample in a queue-only transport episode.
    queue_only_same_decision: u64,
    /// Diagnostic guardrail; stable power should imply stable action while actor state is fixed.
    queue_only_same_power_different_action: u64,
}

#[derive(Default)]
struct Frame {
    kind: u8,
    active: bool,
    first_any: FxHashMap<PistonId, Option<mchprs_world::PistonAction>>,
    seen_no_action: FxHashSet<PistonId>,
    first_qualifying: FxHashMap<PistonId, mchprs_world::PistonAction>,
    /// Retained across wire writes; this is attribution only, never a correctness cache.
    queue_only_last: FxHashMap<PistonId, (bool, Option<mchprs_world::PistonAction>)>,
}

/// Test-only attribution. Legacy counts reset on wire writes; potential equal-decision counts
/// persist only inside exact/replayed queue-only transport episodes and never skip execution.
#[derive(Default)]
pub(super) struct EpisodeSampleCensus {
    enabled: bool,
    stack: Vec<Frame>,
    counts: [Counts; 4],
}

impl EpisodeSampleCensus {
    pub(super) fn enable(&mut self) {
        self.stack.clear();
        self.counts = [Counts::default(); 4];
        self.enabled = true;
    }

    pub(super) fn begin(&mut self, kind: u8, eligible: bool) {
        if !self.enabled {
            return;
        }
        if let Some(parent) = self.stack.last_mut() {
            parent.first_any.clear();
            parent.seen_no_action.clear();
            parent.first_qualifying.clear();
            parent.queue_only_last.clear();
        }
        self.stack.push(Frame {
            kind,
            active: eligible && usize::from(kind) < self.counts.len(),
            ..Default::default()
        });
    }

    pub(super) fn end(&mut self) {
        if !self.enabled {
            return;
        }
        self.stack.pop();
        if let Some(parent) = self.stack.last_mut() {
            parent.first_any.clear();
            parent.seen_no_action.clear();
            parent.first_qualifying.clear();
            parent.queue_only_last.clear();
        }
    }

    pub(super) fn invalidate(&mut self) {
        if !self.enabled {
            return;
        }
        if let Some(frame) = self.stack.last_mut() {
            frame.first_any.clear();
            frame.seen_no_action.clear();
            frame.first_qualifying.clear();
        }
    }

    pub(super) fn sample(&mut self, piston: PistonId, powered: bool, action: Option<mchprs_world::PistonAction>) {
        if !self.enabled {
            return;
        }
        let Some(frame) = self.stack.last_mut() else {
            return;
        };
        if !frame.active {
            return;
        }
        let kind = usize::from(frame.kind);
        let counts = &mut self.counts[kind];
        counts.samples += 1;
        if matches!(frame.kind, EXACT | REPLAY) {
            if let Some(&(previous_powered, previous_action)) = frame.queue_only_last.get(&piston) {
                if previous_powered == powered && previous_action == action {
                    counts.queue_only_same_decision += 1;
                } else if previous_powered == powered {
                    counts.queue_only_same_power_different_action += 1;
                }
            }
            frame.queue_only_last.insert(piston, (powered, action));
        }
        if let Some(&first) = frame.first_any.get(&piston) {
            counts.repeated_any += 1;
            if first != action {
                counts.action_conflicts += 1;
            }
        } else {
            frame.first_any.insert(piston, action);
        }

        match action {
            None => {
                counts.no_action += 1;
                if !frame.seen_no_action.insert(piston) {
                    counts.repeated_no_action += 1;
                }
            }
            Some(action) => {
                counts.qualifying += 1;
                match frame.first_qualifying.get(&piston) {
                    Some(&first) if first == action => counts.repeated_qualifying_same_action += 1,
                    Some(_) => {}
                    None => {
                        frame.first_qualifying.insert(piston, action);
                    }
                }
            }
        }
    }

    pub(super) fn to_json(&self) -> serde_json::Value {
        let scopes = KIND_NAMES
            .iter()
            .enumerate()
            .map(|(kind, name)| {
                let counts = self.counts[kind];
                serde_json::json!({
                    "kind": name,
                    "samples": counts.samples,
                    "qualifying": counts.qualifying,
                    "no_action": counts.no_action,
                    "repeated_any": counts.repeated_any,
                    "repeated_qualifying_same_action": counts.repeated_qualifying_same_action,
                    "repeated_no_action": counts.repeated_no_action,
                    "action_conflicts": counts.action_conflicts,
                    "queue_only_same_decision": counts.queue_only_same_decision,
                    "queue_only_same_power_different_action": counts.queue_only_same_power_different_action,
                    "queue_only_measurement": "consecutive identical powered/action observations across wire writes; upper bound only, no sample was skipped",
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({"enabled": self.enabled, "kind_order": KIND_NAMES, "scopes": scopes})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(index: u32) -> PistonId {
        PistonId::new(index)
    }

    #[test]
    fn counts_only_repeated_qualifying_samples_with_the_same_action() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(0), false, Some(mchprs_world::PistonAction::Retract));
        census.end();
        assert_eq!(census.counts[0].qualifying, 3);
        assert_eq!(census.counts[0].repeated_qualifying_same_action, 1);
        assert_eq!(census.counts[0].action_conflicts, 1);
    }

    #[test]
    fn no_action_does_not_poison_first_qualifying_action() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        census.sample(actor(0), false, None);
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.end();
        assert_eq!(census.counts[0].no_action, 1);
        assert_eq!(census.counts[0].qualifying, 2);
        assert_eq!(census.counts[0].repeated_qualifying_same_action, 1);
        assert_eq!(census.counts[0].repeated_no_action, 0);
    }

    #[test]
    fn invalidation_starts_a_new_constant_wire_segment() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        for _ in 0..2 {
            census.sample(actor(0), false, None);
        }
        census.invalidate();
        census.sample(actor(0), false, None);
        census.end();
        assert_eq!(census.counts[0].repeated_any, 1);
        assert_eq!(census.counts[0].repeated_no_action, 1);
    }

    #[test]
    fn queue_only_upper_bound_survives_analog_writes_and_resets_on_nested_walks() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.invalidate();
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.invalidate();
        census.sample(actor(0), false, None);
        census.invalidate();
        census.sample(actor(0), false, None);
        assert_eq!(census.counts[EXACT as usize].queue_only_same_decision, 2);
        assert_eq!(census.counts[EXACT as usize].queue_only_same_power_different_action, 0);

        census.begin(REPLAY, true);
        census.sample(actor(0), false, None);
        census.end();
        census.sample(actor(0), false, None);
        census.end();
        assert_eq!(census.counts[EXACT as usize].queue_only_same_decision, 2);
    }

    #[test]
    fn ineligible_and_nested_scopes_are_isolated() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.begin(CHANNEL, false);
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.end();
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.end();
        assert_eq!(census.counts[0].qualifying, 2);
        assert_eq!(census.counts[0].repeated_qualifying_same_action, 0);
        assert_eq!(census.counts[2], Counts::default());
    }

    #[test]
    fn repeated_no_action_and_any_are_counted_per_scope() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(STABLE, true);
        census.sample(actor(0), false, None);
        census.sample(actor(0), false, None);
        census.end();
        assert_eq!(census.counts[3].repeated_any, 1);
        assert_eq!(census.counts[3].repeated_no_action, 1);
    }

    #[test]
    fn action_conflicts_include_none_to_action_and_action_to_none() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        census.sample(actor(0), false, None);
        census.sample(actor(0), true, Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(1), false, Some(mchprs_world::PistonAction::Retract));
        census.sample(actor(1), false, None);
        census.end();
        assert_eq!(census.counts[0].repeated_any, 2);
        assert_eq!(census.counts[0].action_conflicts, 2);
    }
}

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
pub(super) mod cpus;

#[test]
#[ignore = "research: full BubbleSort piston sample/action reuse census with every-tick frozen RAM validation"]
fn bubblesort_piston_sample_reuse_census() {
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::world::World;

    let cpu = cpus::CPUS[2];
    let mut world = cpus::load_cpu(cpu);
    let sign = cpu.origin + BlockPos::new(186, 2, 184);
    assert!(matches!(world.get_block(sign), Block::WallSign { .. }));
    assert!(world.set_block(sign, Block::Air));
    world.flush_block_changes();

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &world,
        world.get_corners(),
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let lowered = prepared.lower(&monitor).unwrap();
    let mut backend = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    backend.set_test_recording(false);
    backend.enable_piston_sample_census();
    backend.test_recorder.guard_kind_census_enabled = true;

    let reference = cpus::reference(cpu);
    assert!(reference.ram_trace.first().is_some_and(|(tick, _)| *tick == 0));
    let mut trace_index = 0usize;
    let mut expected = reference.ram_trace[0].1.clone();
    assert_eq!(ram_words(&world, cpu), expected, "initial RAM");
    backend.use_block(cpu.origin + cpu.start).unwrap();

    for tick in 1..=10_036 {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        while reference
            .ram_trace
            .get(trace_index + 1)
            .is_some_and(|(changed_at, _)| *changed_at <= tick)
        {
            trace_index += 1;
            expected.clone_from(&reference.ram_trace[trace_index].1);
        }
        assert_eq!(ram_words(&world, cpu), expected, "RAM at tick {tick}");
        if tick % 2048 == 0 {
            println!("piston sample census: {tick}/10036 ticks");
        }
    }

    let final_words = ram_words(&world, cpu);
    assert_eq!(final_words.len(), 64);
    for (word, actual) in final_words[48..].iter().enumerate() {
        assert_eq!(*actual, Some(word as u16), "sorted output word {word}");
    }

    for scope in &backend.test_recorder.piston_sample_census.counts {
        assert_eq!(scope.action_conflicts, 0, "piston outcome changed within a constant-state segment");
    }
    let scope_counts = backend.piston_sample_census();
    let guard_kind_names = [
        "always",
        "state",
        "not",
        "all",
        "any",
        "live_solid",
        "live_transparent",
        "live_is_wire",
        "live_profile",
        "captured_solid",
        "captured_transparent",
        "captured_is_wire",
        "captured_profile",
        "unused",
    ];
    let guard_kind_calls = guard_kind_names
        .iter()
        .zip(backend.test_recorder.guard_kind_census.iter())
        .map(|(name, calls)| ((*name).to_owned(), calls.get()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let report = serde_json::json!({
        "ticks": 10_036,
        "schematic_sha256": cpu.sha256,
        "ram_samples_checked": 10_037,
        "final_output_words": final_words[48..],
        "piston_sample_census": scope_counts,
        "guard_kind_calls": guard_kind_calls,
        "execution_counts": format!("{:?}", backend.execution_counts()),
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/piston-sample-census/cpu.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("BubbleSort piston sample census: {}", path.display());
}

fn ram_words(world: &crate::plot::PlotWorld, cpu: cpus::Cpu) -> Vec<Option<u16>> {
    let mut trace = Vec::new();
    cpus::collect_ram(world, cpu, 0, &mut trace);
    trace.pop().expect("RAM probe returned a sample").1
}
