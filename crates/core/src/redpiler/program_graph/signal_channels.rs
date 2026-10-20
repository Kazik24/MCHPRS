//! Exact ordered transitions for closed corridors with one shared binary input.
use super::*;
use crate::redpiler::{TaskMonitor, backend::graph::transport::TransportReference};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SignalChannels {
    /// Aligned with the certified transport nets; absent entries retain exact transport.
    pub(crate) channels: Vec<Option<SignalChannel>>,
    pub(crate) stats: SignalChannelStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SignalChannelStats {
    pub(crate) nets: usize,
    pub(crate) wires: usize,
    pub(crate) transitions: usize,
    pub(crate) operations: usize,
    pub(crate) declines: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SignalChannelInput {
    pub(crate) source: DriveSource,
    pub(crate) condition: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignalChannel {
    pub(crate) input: SignalChannelInput,
    pub(crate) profiles: [Box<[u8]>; 2],
    /// First wire distinguishing the two settled profiles, or zero if equal.
    pub(crate) discriminator: usize,
    /// Index is old input * wire count + entry root; new input is its complement.
    /// None means the entry seed does not change and has no observable effects.
    pub(crate) transitions: Box<[Option<SignalTranscript>]>,
}

fn input_family(graph: &CompileGraph, net: &TransportNet) -> Result<(SignalChannelInput, [Vec<u8>; 2]), &'static str> {
    // shortcut: bound per-corridor transcript expansion; widen after measuring larger families.
    if net.wires.len() > 64 {
        return Err("corridor expansion limit");
    }
    let mut input = None;
    let mut drives = [Vec::with_capacity(net.wires.len()), Vec::with_capacity(net.wires.len())];
    for wire in &net.wires {
        let CircuitNodeKind::Wire(index) = graph.nodes[wire.node.index()].kind else {
            return Err("non-wire member");
        };
        let facts = graph.wires[index]
            .execution
            .query_facts
            .unwrap_or_else(|| WireQueryFacts::infer(graph, &graph.wires[index]));
        let transfer = facts.transfer.ok_or("missing transfer")?;
        if transfer.seed.conditional != 0 || transfer.turbo.conditional != 0 || transfer.seed.direct != transfer.turbo.direct {
            return Err("seed and transport inputs differ");
        }
        let query = graph
            .plans
            .instant_drives
            .by_wire
            .get(index)
            .copied()
            .flatten()
            .ok_or("uncompiled drive")?;
        let query = &graph.plans.instant_drives.queries[query as usize];
        let mut high = query.constant;
        for term in &graph.plans.instant_drives.terms[query.terms.start as usize..query.terms.end as usize] {
            let key = SignalChannelInput {
                source: term.source,
                condition: term.condition,
            };
            if input.is_some_and(|old| old != key) {
                return Err("multiple independent inputs");
            }
            input = Some(key);
            let amplitude = match term.source {
                DriveSource::Constant(value) => value,
                DriveSource::Powered(_) | DriveSource::Output(_) => 15,
            };
            high = high.max(amplitude.saturating_sub(term.attenuation));
        }
        drives[0].push(query.constant);
        drives[1].push(high);
    }
    Ok((input.ok_or("constant-only corridor")?, drives))
}

fn settled_profile(net: &TransportNet, drives: &[u8]) -> Box<[u8]> {
    let mut powers = drives.to_vec();
    // A positive path can contain at most fourteen unit-loss wire edges.
    for _ in 0..15 {
        let mut changed = false;
        for (index, wire) in net.wires.iter().enumerate() {
            let mut power = powers[index];
            let mut inputs = wire.inputs;
            while inputs != 0 {
                let slot = inputs.trailing_zeros() as usize;
                inputs &= inputs - 1;
                let other = net.site_wire[wire.neighbors[slot] as usize] as usize;
                power = power.max(powers[other].saturating_sub(1));
            }
            changed |= power != powers[index];
            powers[index] = power;
        }
        if !changed {
            break;
        }
    }
    powers.into_boxed_slice()
}

fn minimize_transcript(graph: &CompileGraph, net: &TransportNet, program: &mut SignalTranscript) {
    let mut ops = std::mem::take(&mut program.ops).into_vec();
    minimize_signal_operations(net, &mut ops, &mut program.selections, |wire, slot| {
        let CircuitNodeKind::Wire(index) = graph.nodes[net.wires[wire as usize].node.index()].kind else {
            unreachable!()
        };
        let facts = graph.wires[index]
            .execution
            .callback_selection
            .or_else(|| WireCallbackSelection::infer(graph, index))?;
        (facts.known & (1 << slot) != 0).then_some(facts.admitted & (1 << slot) != 0)
    });
    program.ops = ops.into_boxed_slice();
}

pub(crate) fn infer_signal_channels(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SignalChannels, String> {
    if monitor.cancelled() {
        return Err("cancelled compiling signal channels".into());
    }
    let mut result = SignalChannels {
        channels: vec![None; graph.plans.instant_transport.nets.len()],
        ..Default::default()
    };
    let mut reference = None;
    let operation_limit = 250_000usize.saturating_mul(monitor.budget_multiplier());
    let mut proof_work = 0usize;
    for (index, net) in graph.plans.instant_transport.nets.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled compiling signal channels".into());
        }
        let (input, drives) = match input_family(graph, net) {
            Ok(value) => value,
            Err(reason) => {
                *result.stats.declines.entry(reason.into()).or_default() += 1;
                continue;
            }
        };
        if proof_work >= operation_limit {
            *result.stats.declines.entry("operation budget".into()).or_default() += 1;
            continue;
        }
        if reference.is_none() {
            reference = Some(TransportReference::new(graph)?);
        }
        let reference = reference.as_mut().unwrap();
        let profiles = [settled_profile(net, &drives[0]), settled_profile(net, &drives[1])];
        let mut transitions = Vec::with_capacity(2 * net.wires.len());
        let mut ops = 0usize;
        let mut accepted = true;
        for old in 0..2 {
            for root in 0..net.wires.len() {
                match reference.compile(net, root as u32, &profiles[old], &drives[1 - old], monitor) {
                    Ok(mut program) => {
                        proof_work = proof_work.saturating_add(program.ops.len());
                        minimize_transcript(graph, net, &mut program);
                        ops += program.ops.len();
                        transitions.push((!program.ops.is_empty()).then_some(program));
                    }
                    Err(_) if monitor.cancelled() => return Err("cancelled compiling signal channel transition".into()),
                    Err(_) => {
                        accepted = false;
                        break;
                    }
                }
                if proof_work > operation_limit {
                    accepted = false;
                    break;
                }
            }
            if !accepted {
                break;
            }
        }
        if !accepted {
            *result.stats.declines.entry("transition proof budget".into()).or_default() += 1;
            continue;
        }
        let discriminator = profiles[0].iter().zip(&profiles[1]).position(|(a, b)| a != b).unwrap_or(0);
        result.stats.nets += 1;
        result.stats.wires += net.wires.len();
        result.stats.transitions += transitions.iter().filter(|program| program.is_some()).count();
        result.stats.operations += ops;
        result.channels[index] = Some(SignalChannel {
            input,
            profiles,
            discriminator,
            transitions: transitions.into_boxed_slice(),
        });
    }
    Ok(result)
}
