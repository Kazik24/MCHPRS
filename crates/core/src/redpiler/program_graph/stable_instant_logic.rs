//! Bounded max-plus compilation for certified stable-input transport corridors.
//!
//! For each admitted transport net this derives the max weighted source value
//! that can reach every member wire: each internal wire edge costs one signal
//! level, so paths of length 15 or more contribute zero. This is valid only
//! under the `--stable-instant-inputs` contract; values are sampled afresh for
//! every net invocation. Runtime materializes stable wire values, then samples
//! distinct external outputs and retains typed reset/memory commit helpers.
//! Internal queues and transients can differ from physical Turbo ordering, so
//! RAM, output, and future continuation remain correctness observables that
//! require validation; corridor closure alone is not a semantic proof.
//!
//! Inputs are interned by `(DriveSource, condition)` once per net. Each wire
//! expression stores compact `(input, attenuation)` terms, allowing the runtime
//! to evaluate source values and guards once and reuse them across the net.
//! Compilation is all-or-nothing per net and bounded by aggregate work, term,
//! and memory budgets. Unsupported or over-budget nets stay on the existing
//! exact transport path.
use super::*;
use crate::redpiler::TaskMonitor;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_STABLE_WORK: usize = 8_000_000;
const MAX_STABLE_TERMS: usize = 2_000_000;
const MAX_STABLE_BYTES: usize = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 4;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StableInstantLogic {
    /// Rows align with the certified instant transport nets.
    pub(crate) nets: Vec<Option<StableLogicNet>>,
    pub(crate) stats: StableLogicStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct StableLogicStats {
    pub(crate) nets: usize,
    pub(crate) admitted: usize,
    pub(crate) declined: usize,
    pub(crate) wires: usize,
    pub(crate) expressions: usize,
    pub(crate) inputs: usize,
    pub(crate) terms: usize,
    pub(crate) outputs: usize,
    pub(crate) work: usize,
    pub(crate) bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StableLogicNet {
    /// One max-plus expression per transport wire, in the same order.
    pub(crate) expressions: Box<[DriveExpr]>,
    /// Distinct live source/guard values shared by all expressions in this net.
    pub(crate) inputs: Box<[StableLogicInput]>,
    /// Local expression terms; ranges index this table.
    pub(crate) terms: Box<[StableLogicTerm]>,
    /// Expressions affected by each shared input, in ascending expression order.
    pub(crate) input_users: Box<[std::ops::Range<u32>]>,
    pub(crate) input_user_expressions: Box<[u32]>,
    /// Possible external callback sites, preserving source wire and Turbo slot order.
    pub(crate) outputs: Box<[StableLogicOutput]>,
    /// Output-site indices affected by each wire, in ascending output order.
    pub(crate) wire_outputs: Box<[std::ops::Range<u32>]>,
    pub(crate) wire_output_indices: Box<[u32]>,
}

struct DependencyIndices {
    input_users: Box<[std::ops::Range<u32>]>,
    input_user_expressions: Box<[u32]>,
    wire_outputs: Box<[std::ops::Range<u32>]>,
    wire_output_indices: Box<[u32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct StableLogicInput {
    pub(crate) source: DriveSource,
    pub(crate) condition: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct StableLogicTerm {
    pub(crate) input: u32,
    pub(crate) attenuation: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StableLogicOutput {
    pub(crate) site: SiteId,
    pub(crate) candidates: Box<[StableLogicDelivery]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct StableLogicDelivery {
    pub(crate) wire: u32,
    pub(crate) slot: u8,
    /// `Some(false)` is omitted by the builder; `None` keeps live admission.
    pub(crate) admission: Option<bool>,
}

#[derive(Clone, Default)]
struct WeightedExpr {
    constant: u8,
    terms: BTreeMap<(DriveSource, u32), u8>,
}

impl WeightedExpr {
    fn include(&mut self, source: &DriveExpr, source_terms: &[DriveTerm], distance: u8) {
        self.constant = self.constant.max(source.constant.saturating_sub(distance));
        if self.constant == 15 {
            self.terms.clear();
            return;
        }
        for term in source_terms {
            let attenuation = term.attenuation.saturating_add(distance);
            if attenuation < 15 {
                self.terms
                    .entry((term.source, term.condition))
                    .and_modify(|old| *old = (*old).min(attenuation))
                    .or_insert(attenuation);
            }
        }
    }
}

/// Infer each net independently. A failed certificate or budget check declines
/// that whole net before any partial program can be published.
pub(crate) fn infer_stable_instant_logic(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<StableInstantLogic, String> {
    if monitor.cancelled() {
        return Err("cancelled compiling stable instant logic".into());
    }
    let drives = &graph.plans.instant_drives;
    let mut result = StableInstantLogic::default();
    let mut work_used = 0usize;
    let mut remaining_terms = MAX_STABLE_TERMS;
    let mut remaining_bytes = MAX_STABLE_BYTES;
    for (net_index, net) in graph.plans.instant_transport.nets.iter().enumerate() {
        if net_index % 32 == 0 && monitor.cancelled() {
            return Err("cancelled compiling stable instant logic".into());
        }
        result.stats.nets += 1;
        let work_before = work_used;
        let term_budget = remaining_terms.min(remaining_bytes / 96);
        let Some((compiled, bytes)) = compile_net(graph, net, drives, monitor, MAX_STABLE_WORK, &mut work_used, term_budget, remaining_bytes)? else {
            result.stats.work += work_used.saturating_sub(work_before);
            result.nets.push(None);
            result.stats.declined += 1;
            continue;
        };
        remaining_terms = remaining_terms.saturating_sub(compiled.terms.len());
        remaining_bytes = remaining_bytes.saturating_sub(bytes);
        result.stats.admitted += 1;
        result.stats.wires += compiled.expressions.len();
        result.stats.expressions += compiled.expressions.len();
        result.stats.inputs += compiled.inputs.len();
        result.stats.terms += compiled.terms.len();
        result.stats.outputs += compiled.outputs.len();
        result.stats.work += work_used.saturating_sub(work_before);
        result.stats.bytes += bytes;
        result.nets.push(Some(compiled));
    }
    Ok(result)
}

fn compile_net(
    graph: &CompileGraph,
    net: &TransportNet,
    drives: &InstantDriveProgram,
    monitor: &TaskMonitor,
    work_budget: usize,
    work_used: &mut usize,
    term_budget: usize,
    byte_budget: usize,
) -> Result<Option<(StableLogicNet, usize)>, String> {
    let count = net.wires.len();
    if count < 2 || net.ordered_receivers {
        return Ok(None);
    }

    let mut driver_exprs = Vec::with_capacity(count);
    for wire in net.wires.iter() {
        let Some(CircuitNodeKind::Wire(wire_index)) = graph.nodes.get(wire.node.index()).map(|node| &node.kind) else {
            return Ok(None);
        };
        let facts = graph.wires[*wire_index]
            .execution
            .query_facts
            .unwrap_or_else(|| WireQueryFacts::infer(graph, &graph.wires[*wire_index]));
        let Some(transfer) = facts.transfer else {
            return Ok(None);
        };
        if transfer.seed.conditional != 0 || transfer.turbo.conditional != 0 || transfer.seed.direct != transfer.turbo.direct {
            return Ok(None);
        }
        let Some(query_index) = drives.by_wire.get(*wire_index).copied().flatten() else {
            return Ok(None);
        };
        let Some(expr) = drives.queries.get(query_index as usize) else {
            return Ok(None);
        };
        let start = expr.terms.start as usize;
        let end = expr.terms.end as usize;
        let Some(terms) = drives.terms.get(start..end) else {
            return Ok(None);
        };
        if terms.iter().any(|term| drives.conditions.get(term.condition as usize).is_none()) {
            return Ok(None);
        }
        driver_exprs.push((expr, terms));
    }

    // Edges point from a source wire to the target whose `inputs` mask names
    // that source. The transport compiler has already certified every set bit.
    let mut outgoing = vec![Vec::<usize>::new(); count];
    for (target, wire) in net.wires.iter().enumerate() {
        let mut mask = wire.inputs;
        while mask != 0 {
            if !charge_work(work_used, work_budget) {
                return Ok(None);
            }
            let slot = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let site = wire.neighbors[slot];
            let Some(&source) = net.site_wire.get(site as usize) else {
                return Ok(None);
            };
            if source == u32::MAX || source as usize >= count {
                return Ok(None);
            }
            outgoing[source as usize].push(target);
        }
    }
    let minimum_working_bytes = count.saturating_mul(std::mem::size_of::<WeightedExpr>() + std::mem::size_of::<Vec<usize>>());
    if minimum_working_bytes > byte_budget {
        return Ok(None);
    }
    let mut outputs = vec![WeightedExpr::default(); count];
    let mut aggregate_terms = 0usize;
    let mut distances = vec![u8::MAX; count];
    let mut touched = Vec::with_capacity(count);
    let mut queue = VecDeque::with_capacity(count);
    let mut visits = 0usize;
    for source in 0..count {
        if source % 64 == 0 && monitor.cancelled() {
            return Err("cancelled compiling stable instant logic".into());
        }
        let source_expr = driver_exprs[source].0;
        let source_terms = driver_exprs[source].1;
        if source_expr.constant == 0 && source_terms.is_empty() {
            continue;
        }
        distances[source] = 0;
        touched.push(source);
        queue.push_back(source);
        while let Some(current) = queue.pop_front() {
            if !charge_work(work_used, work_budget) {
                return Ok(None);
            }
            visits += 1;
            if visits % 4096 == 0 && monitor.cancelled() {
                return Err("cancelled compiling stable instant logic".into());
            }
            let distance = distances[current];
            if !charge_work_count(work_used, work_budget, source_terms.len()) {
                return Ok(None);
            }
            let old_terms = outputs[current].terms.len();
            outputs[current].include(source_expr, source_terms, distance);
            aggregate_terms = aggregate_terms.saturating_sub(old_terms).saturating_add(outputs[current].terms.len());
            if aggregate_terms > term_budget {
                return Ok(None);
            }
            let working_bytes = minimum_working_bytes.saturating_add(aggregate_terms.saturating_mul(96));
            if working_bytes > byte_budget {
                return Ok(None);
            }
            let next_distance = distances[current].saturating_add(1);
            if next_distance >= 15 {
                continue;
            }
            for &target in &outgoing[current] {
                if !charge_work(work_used, work_budget) {
                    return Ok(None);
                }
                if distances[target] == u8::MAX {
                    distances[target] = next_distance;
                    touched.push(target);
                    queue.push_back(target);
                }
            }
        }
        for target in touched.drain(..) {
            distances[target] = u8::MAX;
        }
    }

    let mut shared_inputs = BTreeSet::new();
    for expr in &outputs {
        shared_inputs.extend(expr.terms.keys().copied());
    }
    let inputs = shared_inputs
        .into_iter()
        .map(|(source, condition)| StableLogicInput { source, condition })
        .collect::<Vec<_>>();
    let input_indices = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| ((input.source, input.condition), index as u32))
        .collect::<BTreeMap<_, _>>();

    let mut terms = Vec::new();
    let mut expressions = Vec::with_capacity(count);
    for expr in outputs {
        let start = u32::try_from(terms.len()).map_err(|_| "stable logic term table overflow")?;
        for ((source, condition), attenuation) in expr.terms {
            let Some(&input) = input_indices.get(&(source, condition)) else {
                return Ok(None);
            };
            terms.push(StableLogicTerm { input, attenuation });
            if terms.len() > term_budget {
                return Ok(None);
            }
        }
        let end = u32::try_from(terms.len()).map_err(|_| "stable logic term table overflow")?;
        expressions.push(DriveExpr {
            constant: expr.constant,
            terms: start..end,
        });
    }
    if inputs.len() > term_budget || terms.len() > term_budget {
        return Ok(None);
    }
    let Some(outputs) = compile_outputs(graph, net)? else {
        return Ok(None);
    };
    let output_candidates = outputs.iter().map(|output| output.candidates.len()).sum::<usize>();
    let worst_index_bytes = inputs
        .len()
        .checked_mul(std::mem::size_of::<std::ops::Range<u32>>())
        .and_then(|bytes| bytes.checked_add(terms.len().saturating_mul(std::mem::size_of::<u32>())))
        .and_then(|bytes| bytes.checked_add(expressions.len().saturating_mul(std::mem::size_of::<std::ops::Range<u32>>())))
        .and_then(|bytes| bytes.checked_add(output_candidates.saturating_mul(std::mem::size_of::<u32>())));
    let Some(worst_index_bytes) = worst_index_bytes else {
        return Ok(None);
    };
    if worst_index_bytes > byte_budget {
        return Ok(None);
    }
    let base_bytes = expressions.len() * std::mem::size_of::<DriveExpr>()
        + inputs.len() * std::mem::size_of::<StableLogicInput>()
        + terms.len() * std::mem::size_of::<StableLogicTerm>()
        + outputs
            .iter()
            .map(|output| std::mem::size_of::<StableLogicOutput>() + output.candidates.len() * std::mem::size_of::<StableLogicDelivery>())
            .sum::<usize>();
    let index_build_scratch = terms.len() * std::mem::size_of::<u32>()
        + output_candidates * std::mem::size_of::<u32>()
        + inputs.len() * std::mem::size_of::<Vec<u32>>()
        + expressions.len() * std::mem::size_of::<Vec<u32>>();
    if base_bytes.saturating_add(worst_index_bytes).saturating_add(index_build_scratch) > byte_budget {
        return Ok(None);
    }
    let Some(indices) = build_dependency_indices(&expressions, &terms, inputs.len(), &outputs, count) else {
        return Ok(None);
    };
    let bytes = base_bytes
        + indices.input_users.len() * std::mem::size_of::<std::ops::Range<u32>>()
        + indices.input_user_expressions.len() * std::mem::size_of::<u32>()
        + indices.wire_outputs.len() * std::mem::size_of::<std::ops::Range<u32>>()
        + indices.wire_output_indices.len() * std::mem::size_of::<u32>();
    if bytes > byte_budget || bytes > MAX_STABLE_BYTES {
        return Ok(None);
    }
    let program = StableLogicNet {
        expressions: expressions.into_boxed_slice(),
        inputs: inputs.into_boxed_slice(),
        terms: terms.into_boxed_slice(),
        input_users: indices.input_users,
        input_user_expressions: indices.input_user_expressions,
        outputs: outputs.into_boxed_slice(),
        wire_outputs: indices.wire_outputs,
        wire_output_indices: indices.wire_output_indices,
    };
    Ok(Some((program, bytes)))
}

fn build_dependency_indices(
    expressions: &[DriveExpr],
    terms: &[StableLogicTerm],
    input_count: usize,
    outputs: &[StableLogicOutput],
    wire_count: usize,
) -> Option<DependencyIndices> {
    let mut input_users = vec![Vec::<u32>::new(); input_count];
    for (expression_index, expression) in expressions.iter().enumerate() {
        let expression_terms = terms.get(expression.terms.start as usize..expression.terms.end as usize)?;
        let mut used_inputs = expression_terms.iter().map(|term| term.input).collect::<Vec<_>>();
        used_inputs.sort_unstable();
        used_inputs.dedup();
        for input in used_inputs {
            input_users.get_mut(input as usize)?.push(u32::try_from(expression_index).ok()?);
        }
    }

    let mut wire_outputs = vec![Vec::<u32>::new(); wire_count];
    for (output_index, output) in outputs.iter().enumerate() {
        let output_index = u32::try_from(output_index).ok()?;
        for candidate in output.candidates.iter() {
            wire_outputs.get_mut(candidate.wire as usize)?.push(output_index);
        }
    }
    let (input_user_ranges, input_user_expressions) = flatten_index_lists(input_users)?;
    let (wire_output_ranges, wire_output_indices) = flatten_index_lists(wire_outputs)?;
    Some(DependencyIndices {
        input_users: input_user_ranges,
        input_user_expressions,
        wire_outputs: wire_output_ranges,
        wire_output_indices,
    })
}

fn flatten_index_lists(mut lists: Vec<Vec<u32>>) -> Option<(Box<[std::ops::Range<u32>]>, Box<[u32]>)> {
    let mut offsets = Vec::with_capacity(lists.len());
    let mut values = Vec::new();
    let mut cursor = 0u32;
    for list in &mut lists {
        list.sort_unstable();
        list.dedup();
        let start = cursor;
        cursor = cursor.checked_add(u32::try_from(list.len()).ok()?)?;
        values.extend(list.iter().copied());
        offsets.push(start..cursor);
    }
    Some((offsets.into_boxed_slice(), values.into_boxed_slice()))
}

fn charge_work(used: &mut usize, budget: usize) -> bool {
    if *used >= budget {
        return false;
    }
    *used += 1;
    true
}

fn charge_work_count(used: &mut usize, budget: usize, count: usize) -> bool {
    let Some(end) = used.checked_add(count) else {
        return false;
    };
    if end > budget {
        return false;
    }
    *used = end;
    true
}

fn compile_outputs(graph: &CompileGraph, net: &TransportNet) -> Result<Option<Vec<StableLogicOutput>>, String> {
    let mut grouped = BTreeMap::<SiteId, BTreeSet<StableLogicDelivery>>::new();
    for (wire_index, transport_wire) in net.wires.iter().enumerate() {
        let Some(CircuitNodeKind::Wire(original_index)) = graph.nodes.get(transport_wire.node.index()).map(|node| &node.kind) else {
            return Ok(None);
        };
        let wire = &graph.wires[*original_index];
        let Some(route_id) = wire.turbo_route.filter(|route| *route == transport_wire.route) else {
            return Ok(None);
        };
        let route = &graph.routes[route_id.index()];
        let selection = wire
            .execution
            .callback_selection
            .or_else(|| WireCallbackSelection::infer(graph, *original_index));
        for (slot, &site_local) in transport_wire.neighbors.iter().enumerate() {
            if site_local == u32::MAX || net.site_wire.get(site_local as usize).is_none_or(|&owner| owner != u32::MAX) {
                continue;
            }
            let Some(&site) = net.sites.get(site_local as usize) else { continue };
            let has_receiver = route
                .deliveries
                .iter()
                .any(|delivery| delivery.target == site && delivery.view == OperandView::Captured && delivery.receiver.is_some());
            if !has_receiver {
                continue;
            }
            let admission = selection.and_then(|selection| ((selection.known & (1 << slot)) != 0).then_some((selection.admitted & (1 << slot)) != 0));
            if admission == Some(false) {
                continue;
            }
            grouped.entry(site).or_default().insert(StableLogicDelivery {
                wire: wire_index as u32,
                slot: slot as u8,
                admission,
            });
        }
    }
    Ok(Some(
        grouped
            .into_iter()
            .map(|(site, candidates)| StableLogicOutput {
                site,
                candidates: candidates.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_logic_dependency_indices_are_sorted_and_deduplicated() {
        let expressions = [
            DriveExpr { constant: 0, terms: 0..3 },
            DriveExpr { constant: 0, terms: 3..5 },
            DriveExpr { constant: 0, terms: 5..5 },
        ];
        let terms = [
            StableLogicTerm { input: 2, attenuation: 1 },
            StableLogicTerm { input: 0, attenuation: 1 },
            StableLogicTerm { input: 2, attenuation: 2 },
            StableLogicTerm { input: 1, attenuation: 1 },
            StableLogicTerm { input: 0, attenuation: 3 },
        ];
        let outputs = [
            StableLogicOutput {
                site: SiteId::new(4),
                candidates: vec![
                    StableLogicDelivery {
                        wire: 2,
                        slot: 0,
                        admission: None,
                    },
                    StableLogicDelivery {
                        wire: 0,
                        slot: 1,
                        admission: Some(true),
                    },
                    StableLogicDelivery {
                        wire: 2,
                        slot: 3,
                        admission: None,
                    },
                ]
                .into_boxed_slice(),
            },
            StableLogicOutput {
                site: SiteId::new(8),
                candidates: vec![
                    StableLogicDelivery {
                        wire: 2,
                        slot: 2,
                        admission: None,
                    },
                    StableLogicDelivery {
                        wire: 1,
                        slot: 0,
                        admission: None,
                    },
                ]
                .into_boxed_slice(),
            },
        ];
        let indices = build_dependency_indices(&expressions, &terms, 3, &outputs, 3).unwrap();
        assert_eq!(&*indices.input_users, &[0..2, 2..3, 3..4]);
        assert_eq!(&*indices.input_user_expressions, &[0, 1, 1, 0]);
        assert_eq!(&*indices.wire_outputs, &[0..1, 1..2, 2..4]);
        assert_eq!(&*indices.wire_output_indices, &[0, 1, 0, 1]);
    }

    fn expr(constant: u8, terms: &[(DriveSource, u32, u8)]) -> (DriveExpr, Vec<DriveTerm>) {
        (
            DriveExpr {
                constant,
                terms: 0..terms.len() as u32,
            },
            terms
                .iter()
                .map(|&(source, condition, attenuation)| DriveTerm {
                    source,
                    condition,
                    attenuation,
                })
                .collect(),
        )
    }

    #[test]
    fn weighted_sources_use_shortest_path_and_keep_conditions_separate() {
        let a = DriveSource::Powered(NodeId::new(10));
        let b = DriveSource::Output(NodeId::new(11));
        let (source_a, terms_a) = expr(0, &[(a, 1, 2), (b, 7, 3)]);
        let (source_b, terms_b) = expr(5, &[(a, 1, 4)]);
        let mut target = WeightedExpr::default();
        target.include(&source_a, &terms_a, 2);
        target.include(&source_b, &terms_b, 1);
        assert_eq!(target.constant, 4);
        assert_eq!(target.terms.get(&(a, 1)), Some(&4));
        assert_eq!(target.terms.get(&(b, 7)), Some(&5));
    }

    #[test]
    fn weighted_sources_drop_zero_paths_and_constant_fifteen_dominates() {
        let source = DriveSource::Output(NodeId::new(2));
        let (expr, terms) = expr(0, &[(source, 3, 14)]);
        let mut target = WeightedExpr::default();
        target.include(&expr, &terms, 1);
        assert!(target.terms.is_empty());
        let mut dominant = WeightedExpr::default();
        dominant.include(&DriveExpr { constant: 15, terms: 0..0 }, &[], 0);
        dominant.include(&expr, &terms, 0);
        assert_eq!(dominant.constant, 15);
        assert!(dominant.terms.is_empty());
    }
}
