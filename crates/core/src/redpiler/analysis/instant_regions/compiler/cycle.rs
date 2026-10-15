//! AOT recurrence proofs compare functions and ordered future words, never sampled trajectories.
use super::execution::Execution;
use super::work::{Guarded, Request};
use super::*;
use mchprs_world::TickPriority;

#[derive(Clone)]
pub(crate) struct Snapshot {
    /// These canonical functions retain every scalar component, shape, actor and shared-payload value.
    pub(crate) scalars: Vec<Formula>,
    /// These functions retain scheduler membership separately from component display flags.
    pub(crate) pending: Vec<Formula>,
    /// These guarded occurrences retain relative deadlines and priority/FIFO order.
    pub(crate) ticks: Vec<Guarded<Scheduled>>,
    /// These guarded occurrences retain the pending actuator request word.
    pub(crate) requests: Vec<Guarded<Request>>,
    /// These guarded occurrences retain independent actor and payload publication witnesses.
    pub(crate) publications: Vec<Guarded<Publication>>,
    /// These bit vectors retain revision increments needed to restore an arbitrary runtime origin.
    pub(crate) revision_deltas: Vec<Vec<Formula>>,
    /// These occurrences retain accepted piston actions emitted since the previous frame.
    pub(crate) effects: Vec<Guarded<Effect>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Effect {
    /// This identifies the actor that owns the presentation action.
    pub(crate) actor: PistonId,
    /// This preserves the exact accepted action kind.
    pub(crate) action: mchprs_world::PistonAction,
    /// This is the extension bit captured when the action was accepted.
    pub(crate) extended: Formula,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scheduled {
    /// This identifies the scheduled component owner.
    pub(crate) node: NodeId,
    /// This is the half-tick delay relative to the scheduler cursor.
    pub(crate) delay: usize,
    /// This preserves the dispatch priority within that deadline.
    pub(crate) priority: TickPriority,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Publication {
    /// This identifies the actor whose connections will be published.
    pub(crate) actor: PistonId,
    /// These little-endian bits count increments since its captured revision, not since compilation.
    pub(crate) revision_age: Vec<Formula>,
    /// This independently guarded payload witness is absent when no payload was transferred.
    pub(crate) resource: Option<Guarded<Payload>>,
    /// This is the publication deadline relative to the current logical tick.
    pub(crate) delay: u64,
    /// This distinguishes extension publication from retraction publication.
    pub(crate) extending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Payload {
    /// This identifies the conserved resource independently of the acting piston.
    pub(crate) resource: ResourceId,
    /// These bits count resource increments since its independently captured revision.
    pub(crate) revision_age: Vec<Formula>,
    /// This preserves the contact captured when the payload was transferred.
    pub(crate) contact: ContactId,
}

impl Snapshot {
    pub(crate) fn capture(compiler: &mut Compiler<'_>, execution: &Execution, first_effect: usize) -> Result<Self> {
        compiler.step()?;
        let scalars = compiler.state.scalar_roots();
        let pending = execution.work.pending.clone();
        let ticks = execution
            .work
            .scheduler
            .iter()
            .filter(|(event, _, _)| event.when != Formula::FALSE)
            .map(|(event, delay, priority)| Guarded {
                when: event.when,
                value: Scheduled {
                    node: event.value,
                    delay,
                    priority,
                },
            })
            .collect();
        let requests = execution
            .work
            .requests
            .iter()
            .filter(|event| event.when != Formula::FALSE)
            .copied()
            .collect();
        let mut publications = Vec::new();
        for event in &execution.publications {
            if event.when == Formula::FALSE {
                continue;
            }
            let revision_age = compiler
                .logic
                .count(&compiler.state.actors[event.actor.index()].revision.writes[event.stamp..])?;
            let resource = match &event.resource {
                Some(payload) if payload.when != Formula::FALSE => Some(Guarded {
                    when: payload.when,
                    value: Payload {
                        resource: payload.resource,
                        contact: payload.contact,
                        revision_age: compiler
                            .logic
                            .count(&compiler.state.resources[payload.resource.index()].revision.writes[payload.stamp..])?,
                    },
                }),
                _ => None,
            };
            publications.push(Guarded {
                when: event.when,
                value: Publication {
                    actor: event.actor,
                    revision_age,
                    resource,
                    delay: event.due_tick.wrapping_sub(execution.tick),
                    extending: event.extending,
                },
            });
        }
        let revision_deltas = compiler.state.revision_counts(&mut compiler.logic)?;
        let effects = execution.accepted[first_effect..]
            .iter()
            .map(|event| Guarded {
                when: event.when,
                value: Effect {
                    actor: event.request.actor,
                    action: event.request.action,
                    extended: event.previous_extended,
                },
            })
            .collect();
        Ok(Self {
            scalars,
            pending,
            ticks,
            requests,
            publications,
            revision_deltas,
            effects,
        })
    }

    /// Append this frame's exact values and return their runtime root layout.
    pub(crate) fn append_roots(&self, roots: &mut Vec<Formula>) -> KernelFrameLayout {
        fn append(roots: &mut Vec<Formula>, values: impl IntoIterator<Item = Formula>) -> std::ops::Range<usize> {
            let start = roots.len();
            roots.extend(values);
            start..roots.len()
        }
        fn root(roots: &mut Vec<Formula>, value: Formula) -> usize {
            append(roots, [value]).start
        }

        let state = append(roots, self.scalars.iter().copied());
        let revision_deltas = self.revision_deltas.iter().map(|bits| append(roots, bits.iter().copied())).collect();
        let pending = append(roots, self.pending.iter().copied());
        let ticks = self
            .ticks
            .iter()
            .map(|event| KernelTickRoot {
                enabled: root(roots, event.when),
                node: event.value.node,
                delay: event.value.delay,
                priority: event.value.priority,
            })
            .collect();
        let requests = self
            .requests
            .iter()
            .map(|event| KernelRequestRoot {
                enabled: root(roots, event.when),
                actor: event.value.actor,
                action: event.value.action,
            })
            .collect();
        let publications = self
            .publications
            .iter()
            .map(|event| {
                let actor_revision_age = append(roots, event.value.revision_age.iter().copied());
                let resource = event.value.resource.as_ref().map(|payload| KernelPayloadRoot {
                    enabled: root(roots, payload.when),
                    resource: payload.value.resource,
                    revision_age: append(roots, payload.value.revision_age.iter().copied()),
                    contact: payload.value.contact,
                });
                KernelPublicationRoot {
                    enabled: root(roots, event.when),
                    actor: event.value.actor,
                    actor_revision_age,
                    resource,
                    delay: event.value.delay,
                    extending: event.value.extending,
                }
            })
            .collect();
        let effects = self
            .effects
            .iter()
            .map(|event| KernelEffectRoot {
                enabled: root(roots, event.when),
                extended: root(roots, event.value.extended),
                actor: event.value.actor,
                action: event.value.action,
            })
            .collect();
        KernelFrameLayout {
            state,
            revision_deltas,
            pending,
            ticks,
            requests,
            publications,
            effects,
        }
    }

    /// Equal normalized state and future imply equal subsequent work under the same frozen inputs.
    pub(crate) fn proves_same(&self, other: &Self, compiler: &mut Compiler<'_>) -> Result<bool> {
        if self.scalars != other.scalars || self.pending != other.pending {
            return Ok(false);
        }
        if !unique_word_proves_equal(
            compiler,
            &self.ticks,
            &other.ticks,
            |event| (event.node, event.delay, event.priority),
            |_, _, _| Ok(Formula::TRUE),
        )? {
            return Ok(false);
        }
        if !unique_word_proves_equal(
            compiler,
            &self.requests,
            &other.requests,
            |event| (event.actor, event.action),
            |_, _, _| Ok(Formula::TRUE),
        )? {
            return Ok(false);
        }
        unique_word_proves_equal(
            compiler,
            &self.publications,
            &other.publications,
            |event| (event.actor, event.delay, event.extending),
            publication_equal,
        )
    }
}

fn equal_bits(compiler: &mut Compiler<'_>, a: &[Formula], b: &[Formula]) -> Result<Formula> {
    let mut equal = Formula::TRUE;
    for bit in 0..a.len().max(b.len()) {
        let a = a.get(bit).copied().unwrap_or(Formula::FALSE);
        let b = b.get(bit).copied().unwrap_or(Formula::FALSE);
        let differs = compiler.logic.xor(a, b)?;
        let same = compiler.logic.not(differs)?;
        equal = compiler.logic.and(equal, same)?;
    }
    Ok(equal)
}

fn payload_equal(compiler: &mut Compiler<'_>, a: &Payload, b: &Payload) -> Result<Formula> {
    if a.resource != b.resource || a.contact != b.contact {
        return Ok(Formula::FALSE);
    }
    equal_bits(compiler, &a.revision_age, &b.revision_age)
}

fn publication_equal(compiler: &mut Compiler<'_>, a: &Publication, b: &Publication) -> Result<Formula> {
    if a.actor != b.actor || a.delay != b.delay || a.extending != b.extending {
        return Ok(Formula::FALSE);
    }
    let actor = equal_bits(compiler, &a.revision_age, &b.revision_age)?;
    let resource = match (&a.resource, &b.resource) {
        (None, None) => Formula::TRUE,
        (Some(payload), None) | (None, Some(payload)) => compiler.logic.not(payload.when)?,
        (Some(a), Some(b)) => {
            let different_presence = compiler.logic.xor(a.when, b.when)?;
            let same_presence = compiler.logic.not(different_presence)?;
            let same_value = payload_equal(compiler, &a.value, &b.value)?;
            let selected_value = compiler.logic.select(a.when, same_value, Formula::TRUE)?;
            compiler.logic.and(same_presence, selected_value)?
        }
    };
    Ok(compiler.logic.and(actor, resource)?)
}

/// For unique keys, equal membership, values and pairwise order prove equal selected words.
pub(crate) fn unique_word_proves_equal<T: PartialEq, K: std::hash::Hash + Eq + Clone>(
    compiler: &mut Compiler<'_>,
    a: &[Guarded<T>],
    b: &[Guarded<T>],
    key: impl Fn(&T) -> K,
    mut value_equal: impl FnMut(&mut Compiler<'_>, &T, &T) -> Result<Formula>,
) -> Result<bool> {
    if a == b {
        return Ok(true);
    }
    let Some(left) = groups(compiler, a, &key)? else { return Ok(false) };
    let Some(right) = groups(compiler, b, &key)? else { return Ok(false) };
    if left.items.len() != right.items.len() {
        return Ok(false);
    }
    for key in &left.order {
        let a = &left.items[key];
        let Some(b) = right.items.get(key) else { return Ok(false) };
        if a.present != b.present {
            return Ok(false);
        }
        // Membership equality and uniqueness guarantee one counterpart whenever either value exists.
        for &(_, event) in &a.events {
            for &(_, other) in &b.events {
                compiler.step()?;
                let both = compiler.logic.and(event.when, other.when)?;
                if both == Formula::FALSE {
                    continue;
                }
                let equal = value_equal(compiler, &event.value, &other.value)?;
                let differs = compiler.logic.not(equal)?;
                if compiler.logic.and(both, differs)? != Formula::FALSE {
                    return Ok(false);
                }
            }
        }
    }
    for (index, a) in left.order.iter().enumerate() {
        for b in &left.order[index + 1..] {
            compiler.step()?;
            if compiler.logic.and(left.items[a].present, left.items[b].present)? == Formula::FALSE {
                continue;
            }
            let left_direction = separated(&left.items[a], &left.items[b]);
            let right_direction = separated(&right.items[a], &right.items[b]);
            if left_direction.is_some() && left_direction == right_direction {
                continue;
            }
            if before(compiler, &left.items[a], &left.items[b])? != before(compiler, &right.items[a], &right.items[b])? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

struct Group<'a, T> {
    /// These retain each conditional occurrence's original position and value.
    events: Vec<(usize, &'a Guarded<T>)>,
    /// This is the membership predicate; selected occurrences in this group are proven disjoint.
    present: Formula,
}

struct Groups<'a, T, K> {
    /// These keys follow first occurrence order so proof construction is deterministic.
    order: Vec<K>,
    /// These groups index the finite event identities without encoding a global word rank.
    items: FxHashMap<K, Group<'a, T>>,
}

fn groups<'a, T, K: std::hash::Hash + Eq + Clone>(
    compiler: &mut Compiler<'_>,
    word: &'a [Guarded<T>],
    key: &impl Fn(&T) -> K,
) -> Result<Option<Groups<'a, T, K>>> {
    let mut result = Groups {
        order: Vec::new(),
        items: FxHashMap::default(),
    };
    for (position, event) in word.iter().enumerate().filter(|(_, event)| event.when != Formula::FALSE) {
        compiler.step()?;
        let key = key(&event.value);
        let group = result.items.entry(key.clone()).or_insert_with(|| {
            result.order.push(key);
            Group {
                events: Vec::new(),
                present: Formula::FALSE,
            }
        });
        if compiler.logic.and(group.present, event.when)? != Formula::FALSE {
            return Ok(None);
        }
        group.present = compiler.logic.or(group.present, event.when)?;
        group.events.push((position, event));
    }
    Ok(Some(result))
}

fn separated<T>(a: &Group<'_, T>, b: &Group<'_, T>) -> Option<bool> {
    if a.events.last().unwrap().0 < b.events.first().unwrap().0 {
        Some(true)
    } else if b.events.last().unwrap().0 < a.events.first().unwrap().0 {
        Some(false)
    } else {
        None
    }
}

fn before<T>(compiler: &mut Compiler<'_>, a: &Group<'_, T>, b: &Group<'_, T>) -> Result<Formula> {
    let mut seen = Formula::FALSE;
    let mut result = Formula::FALSE;
    let mut index = 0;
    for &(position, event) in &b.events {
        while index < a.events.len() && a.events[index].0 < position {
            compiler.step()?;
            seen = compiler.logic.or(seen, a.events[index].1.when)?;
            index += 1;
        }
        let earlier = compiler.logic.and(seen, event.when)?;
        result = compiler.logic.or(result, earlier)?;
    }
    Ok(result)
}
