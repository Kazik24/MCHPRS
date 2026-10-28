//! Candidate frame evaluation with live, batched native input cuts.
use crate::redpiler::analysis::instant::boundaries::OwnerMap;
use crate::redpiler::program_graph::*;
use rustc_hash::FxHashMap;

struct Channel {
    site: SiteId,
    port: PowerPort,
    slots: Vec<(usize, u8)>,
}

/// Evaluates a candidate program without transferring Graph ownership or applying outputs.
pub(crate) struct CandidateFrame {
    evaluation: SettledEvaluation,
    inputs: Vec<bool>,
    channels: Vec<Channel>,
    direct: Vec<(usize, LayerInputSource)>,
    feedback: Vec<(usize, usize, bool, bool)>,
}

impl CandidateFrame {
    pub(crate) fn new(graph: &CompileGraph, owners: &OwnerMap, circuit: &SettledCircuit) -> Result<Self, String> {
        Self::with_live_states(graph, owners, circuit, false)
    }

    /// Sample owned physical states from the caller's current virtual transition.
    pub(crate) fn new_live_states(graph: &CompileGraph, owners: &OwnerMap, circuit: &SettledCircuit) -> Result<Self, String> {
        Self::with_live_states(graph, owners, circuit, true)
    }

    fn with_live_states(graph: &CompileGraph, owners: &OwnerMap, circuit: &SettledCircuit, live_states: bool) -> Result<Self, String> {
        let mut channels = Vec::<Channel>::new();
        let mut channel_index = FxHashMap::default();
        let mut direct = Vec::new();
        let mut feedback = Vec::new();
        for (slot, input) in circuit.program.inputs.iter().enumerate() {
            match input.source {
                LayerInputSource::SourceAbove { site, port, threshold } => {
                    if owners.sites.get(site.index()).ok_or("candidate input site is missing")?.is_some() {
                        return Err(format!("candidate input {site:?} is internally owned"));
                    }
                    let index = *channel_index.entry((site, port)).or_insert_with(|| {
                        channels.push(Channel {
                            site,
                            port,
                            slots: Vec::new(),
                        });
                        channels.len() - 1
                    });
                    channels[index].slots.push((slot, threshold));
                }
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => {
                    let owner = match test {
                        StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource) => {
                            *owners.resources.get(resource.index()).ok_or("candidate input resource is missing")?
                        }
                        StateTest::PistonExtended(actor)
                        | StateTest::PistonHeadPresent(actor)
                        | StateTest::PistonAvailability { piston: actor, .. } => {
                            *owners.actors.get(actor.index()).ok_or("candidate input actor is missing")?
                        }
                    };
                    if owner.is_none() || live_states {
                        direct.push((slot, input.source));
                        continue;
                    }
                    let StateTest::ResourceAt { resource, contact } = test else {
                        return Err(format!("owned candidate state is not represented: {test:?}"));
                    };
                    let exact = circuit.outputs.iter().enumerate().find_map(|(index, output)| match output.target {
                        SettledTarget::ResourceState { resource: id, source } if id == resource && source == contact => Some(index),
                        _ => None,
                    });
                    let (output, source) = circuit
                        .outputs
                        .iter()
                        .enumerate()
                        .find_map(|(index, output)| match output.target {
                            SettledTarget::ResourceState { resource: id, source } if id == resource => Some((index, source)),
                            _ => None,
                        })
                        .ok_or_else(|| format!("owned feedback resource {resource:?} has no output"))?;
                    let contacts = &graph.resources.get(resource.index()).ok_or("missing feedback resource")?.contacts;
                    if !contacts.contains(&contact) || !contacts.contains(&source) || (exact.is_none() && contacts.len() != 2) {
                        return Err(format!("feedback resource {resource:?} has unsupported contacts"));
                    }
                    let initial = graph.pistons.iter().enumerate().any(|(index, piston)| {
                        owners.actors.get(index).copied().flatten().is_some()
                            && matches!(piston.lane, PistonLane::SingleResource { far } if far == contact)
                            && contacts.contains(&piston.near)
                    });
                    feedback.push((slot, exact.unwrap_or(output), exact.is_some() || contact == source, initial));
                }
                LayerInputSource::WireDot { node } => {
                    if owners.nodes.get(node.index()).ok_or("candidate wire input is missing")?.is_some() {
                        return Err(format!("candidate wire mode {node:?} is internally owned"));
                    }
                    direct.push((slot, input.source));
                }
                other => return Err(format!("unsupported candidate frame input {other:?}")),
            }
        }
        Ok(Self {
            evaluation: SettledEvaluation::new(circuit)?,
            inputs: vec![false; circuit.program.inputs.len()],
            channels,
            direct,
            feedback,
        })
    }

    /// Reads each native strength channel once and iterates owned feedback to a fixed point.
    pub(crate) fn evaluate(
        &mut self,
        strength: impl FnMut(SiteId, PowerPort) -> u8,
        state: impl FnMut(LayerInputSource) -> Result<bool, &'static str>,
    ) -> Result<usize, String> {
        self.sample_inputs(strength, state)?;
        self.solve()
    }

    pub(crate) fn sample_inputs(
        &mut self,
        mut strength: impl FnMut(SiteId, PowerPort) -> u8,
        mut state: impl FnMut(LayerInputSource) -> Result<bool, &'static str>,
    ) -> Result<(), String> {
        for channel in &self.channels {
            let value = strength(channel.site, channel.port);
            for &(slot, threshold) in &channel.slots {
                self.inputs[slot] = value > threshold;
            }
        }
        for &(slot, source) in &self.direct {
            self.inputs[slot] = state(source).map_err(str::to_owned)?;
        }
        Ok(())
    }

    pub(crate) fn solve(&mut self) -> Result<usize, String> {
        for &(slot, _, _, initial) in &self.feedback {
            self.inputs[slot] = initial;
        }
        let mut operations = 0;
        for _ in 0..128 {
            operations += self.evaluation.evaluate(&self.inputs).map_err(str::to_owned)?;
            let mut changed = false;
            for &(slot, output, polarity, _) in &self.feedback {
                let next = (self.evaluation.outputs()[output] > 0) == polarity;
                changed |= self.inputs[slot] != next;
                self.inputs[slot] = next;
            }
            if !changed {
                return Ok(operations);
            }
        }
        Err("candidate frame feedback did not converge".into())
    }

    pub(crate) fn outputs(&self) -> &[u8] {
        self.evaluation.outputs()
    }
}
