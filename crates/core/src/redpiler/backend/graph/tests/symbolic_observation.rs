//! Independent Graph observations for a compiled symbolic phase, including cold future witnesses.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::{Compiler as SymbolicCompiler, execution::Execution};
use mchprs_redpiler_opt::symbolic::Formula;
use mchprs_world::PistonAction;
use std::ops::Range;

pub(super) struct Observation {
    /// This selects every scalar component, actor and shared-resource value after the phase.
    state: Range<usize>,
    /// These select ordered revision increments, actors first and resources second.
    revisions: Vec<Range<usize>>,
    /// This selects actual matching scheduler membership for each node.
    pending: Range<usize>,
    /// These retain guarded scheduler occurrences in native iteration order.
    ticks: Vec<(usize, NodeId, usize, mchprs_world::TickPriority)>,
    /// These retain guarded FIFO requests without sorting or deduplicating them.
    requests: Vec<(usize, PistonId, PistonAction)>,
    /// These retain independently witnessed, ordered deferred publications.
    publications: Vec<Publication>,
    /// These retain the phase's accepted block actions in causal order.
    effects: Vec<(usize, usize, PistonId, PistonAction)>,
}

struct Publication {
    /// This selects the actor publication occurrence.
    when: usize,
    /// This identifies the actuator and its revision-write prefix at acceptance.
    actor: (PistonId, usize),
    /// This identifies an optional independently guarded payload and its captured revision prefix.
    resource: Option<(usize, ResourceId, usize, ContactId)>,
    /// This is the deadline relative to the observed phase, preserving stale work too.
    delay: u64,
    /// This retains the direction of connection publication.
    extending: bool,
}

fn append(roots: &mut Vec<Formula>, values: impl IntoIterator<Item = Formula>) -> Range<usize> {
    let start = roots.len();
    roots.extend(values);
    start..roots.len()
}

fn root(roots: &mut Vec<Formula>, value: Formula) -> usize {
    append(roots, [value]).start
}

impl Observation {
    pub(super) fn revision_deltas(&self, values: &[bool]) -> Vec<u64> {
        self.revisions
            .iter()
            .map(|range| values[range.clone()].iter().filter(|&&selected| selected).count() as u64)
            .collect()
    }

    pub(super) fn capture(compiler: &SymbolicCompiler<'_>, execution: &Execution, first_effect: usize, roots: &mut Vec<Formula>) -> Self {
        let state = append(roots, symbolic_execution_tests::state_roots(compiler));
        let revisions = compiler
            .state
            .actors
            .iter()
            .map(|actor| &actor.revision)
            .chain(compiler.state.resources.iter().map(|resource| &resource.revision))
            .map(|revision| append(roots, revision.writes.iter().copied()))
            .collect();
        let pending = append(roots, execution.work.pending.iter().copied());
        let ticks = execution
            .work
            .scheduler
            .iter()
            .map(|(event, delay, priority)| (root(roots, event.when), event.value, delay, priority))
            .collect();
        let requests = execution
            .work
            .requests
            .iter()
            .map(|event| (root(roots, event.when), event.value.actor, event.value.action))
            .collect();
        let publications = execution
            .publications
            .iter()
            .map(|event| Publication {
                when: root(roots, event.when),
                actor: (event.actor, event.stamp),
                resource: event
                    .resource
                    .as_ref()
                    .map(|payload| (root(roots, payload.when), payload.resource, payload.stamp, payload.contact)),
                delay: event.due_tick.wrapping_sub(execution.tick),
                extending: event.extending,
            })
            .collect();
        let effects = execution.accepted[first_effect..]
            .iter()
            .map(|event| {
                (
                    root(roots, event.when),
                    root(roots, event.previous_extended),
                    event.request.actor,
                    event.request.action,
                )
            })
            .collect();
        Self {
            state,
            revisions,
            pending,
            ticks,
            requests,
            publications,
            effects,
        }
    }

    pub(super) fn assert_matches(&self, values: &[bool], exact: &GraphBackend, origins: &[u64], first_effect: usize) {
        assert_eq!(
            values[self.state.clone()],
            symbolic_execution_tests::exact_state(exact),
            "full scalar state"
        );
        let increment = |owner: usize, stamp: usize| -> u64 {
            let range = &self.revisions[owner];
            assert!(stamp <= range.len());
            values[range.start..range.start + stamp].iter().filter(|&&selected| selected).count() as u64
        };
        let revision = |owner: usize, stamp: usize| origins[owner].wrapping_add(increment(owner, stamp));
        let actual_revisions = exact
            .state
            .pistons
            .iter()
            .map(|actor| actor.revision)
            .chain(exact.state.resources.iter().map(|resource| resource.revision));
        for (owner, actual) in actual_revisions.enumerate() {
            assert_eq!(revision(owner, self.revisions[owner].len()), actual, "revision owner {owner}");
        }
        for (node, &pending) in values[self.pending.clone()].iter().enumerate() {
            let matching = exact.program.nodes()[node].block.is_some() && exact.is_pending(NodeId::new(node as u32));
            assert_eq!(pending, matching, "pending node {node}");
        }
        let ticks: Vec<_> = self
            .ticks
            .iter()
            .filter_map(|&(guard, node, delay, priority)| values[guard].then_some((node, delay, priority)))
            .collect();
        let actual_ticks: Vec<_> = exact
            .future
            .scheduler
            .iter()
            .map(|(event, delay, priority)| (event.node.unwrap(), delay, priority))
            .collect();
        assert_eq!(ticks, actual_ticks, "ordered scheduled work");
        let requests: Vec<_> = self
            .requests
            .iter()
            .filter_map(|&(guard, actor, action)| values[guard].then_some((actor, action)))
            .collect();
        let actual_requests: Vec<_> = exact.future.piston_requests.iter().map(|event| (event.piston, event.action)).collect();
        assert_eq!(requests, actual_requests, "ordered requests");
        let publications: Vec<_> = self
            .publications
            .iter()
            .filter(|event| values[event.when])
            .map(|event| {
                let (actor, stamp) = event.actor;
                let payload = event.resource.and_then(|(when, resource, stamp, contact)| {
                    values[when].then(|| (resource, revision(exact.state.pistons.len() + resource.index(), stamp), contact))
                });
                (actor, revision(actor.index(), stamp), payload, event.delay, event.extending)
            })
            .collect();
        let actual_publications: Vec<_> = exact
            .future
            .deferred_pistons
            .iter()
            .map(|event| {
                (
                    event.piston,
                    event.revision,
                    event.resource.map(|payload| (payload.resource, payload.revision, payload.contact)),
                    event.due_tick.wrapping_sub(exact.future.clock.logical_tick),
                    event.extending,
                )
            })
            .collect();
        assert_eq!(publications, actual_publications, "ordered publications and revision witnesses");
        let effects: Vec<_> = self
            .effects
            .iter()
            .filter_map(|&(guard, extended, actor, action)| values[guard].then_some((actor, action, values[extended])))
            .collect();
        let actual_effects: Vec<_> = exact.effects[first_effect..]
            .iter()
            .map(|effect| {
                let PresentationEffect::BlockAction {
                    position,
                    action: crate::world::BlockAction::Piston { action, piston },
                } = effect
                else {
                    panic!("this kernel family admits only piston presentation effects");
                };
                let actor = exact.program.pistons().iter().position(|actor| actor.position == *position).unwrap();
                (PistonId::new(actor as u32), *action, piston.extended)
            })
            .collect();
        assert_eq!(effects, actual_effects, "ordered presentation effects");
    }
}
