//! Opt-in test-only counts for repeated piston samples inside exact wire episodes.
use super::*;
use rustc_hash::FxHashMap;

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
}

#[derive(Default)]
struct Frame {
    kind: u8,
    active: bool,
    first_any: FxHashMap<PistonId, Option<mchprs_world::PistonAction>>,
    first_qualifying: FxHashMap<PistonId, mchprs_world::PistonAction>,
}

/// Test-only attribution. Memoization is scoped to a queue-only episode segment;
/// wire writes and nested episodes clear the actor memo before it can be reused.
#[derive(Default)]
pub(super) struct EpisodeSampleCensus {
    enabled: bool,
    stack: Vec<Frame>,
    counts: [Counts; 4],
}

impl EpisodeSampleCensus {
    pub(super) fn enable(&mut self) {
        self.enabled = true;
    }

    pub(super) fn begin(&mut self, kind: u8, eligible: bool) {
        if !self.enabled {
            return;
        }
        if let Some(parent) = self.stack.last_mut() {
            parent.first_any.clear();
            parent.first_qualifying.clear();
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
            parent.first_qualifying.clear();
        }
    }

    pub(super) fn invalidate(&mut self) {
        if !self.enabled {
            return;
        }
        if let Some(frame) = self.stack.last_mut() {
            frame.first_any.clear();
            frame.first_qualifying.clear();
        }
    }

    pub(super) fn sample(&mut self, piston: PistonId, action: Option<mchprs_world::PistonAction>) {
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
        if let Some(first) = frame.first_any.get(&piston) {
            counts.repeated_any += 1;
            if action.is_none() && first.is_none() {
                counts.repeated_no_action += 1;
            }
        } else {
            frame.first_any.insert(piston, action);
        }

        match action {
            None => counts.no_action += 1,
            Some(action) => {
                counts.qualifying += 1;
                match frame.first_qualifying.get(&piston) {
                    Some(&first) if first == action => counts.repeated_qualifying_same_action += 1,
                    Some(_) => counts.action_conflicts += 1,
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
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(0), Some(mchprs_world::PistonAction::Retract));
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
        census.sample(actor(0), None);
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
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
            census.sample(actor(0), None);
        }
        census.invalidate();
        census.sample(actor(0), None);
        census.end();
        assert_eq!(census.counts[0].repeated_any, 1);
        assert_eq!(census.counts[0].repeated_no_action, 1);
    }

    #[test]
    fn ineligible_and_nested_scopes_are_isolated() {
        let mut census = EpisodeSampleCensus::default();
        census.enable();
        census.begin(EXACT, true);
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
        census.begin(CHANNEL, false);
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
        census.end();
        census.sample(actor(0), Some(mchprs_world::PistonAction::Extend));
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
        census.sample(actor(0), None);
        census.sample(actor(0), None);
        census.end();
        assert_eq!(census.counts[3].repeated_any, 1);
        assert_eq!(census.counts[3].repeated_no_action, 1);
    }
}
