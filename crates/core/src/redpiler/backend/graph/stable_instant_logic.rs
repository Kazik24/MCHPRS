//! Stable-input execution samples shared sources once and evaluates compiled signal expressions.
use super::*;

#[derive(Default)]
pub(super) struct StableLogicScratch {
    inputs: Vec<u8>,
    input_ready: Vec<bool>,
    sampled_inputs: Vec<usize>,
    powers: Vec<(u32, u8)>,
    changed: Vec<bool>,
    changed_wires: Vec<u32>,
    expressions: Vec<u32>,
    outputs: Vec<u32>,
    deliveries: Vec<(SiteId, ObservedSite, RouteId)>,
}

/// Derived only: materialized wires correspond to this last committed input snapshot.
#[derive(Default)]
pub(super) struct StableLogicCache {
    inputs: Vec<u8>,
    pub(super) valid: bool,
    revision: u64,
}

impl StableLogicCache {
    pub(super) fn invalidate(&mut self) {
        self.valid = false;
        self.revision = self.revision.wrapping_add(1);
    }
}

impl GraphBackend {
    pub(super) fn invalidate_wire_programs(&mut self, node: NodeId) {
        if self.stable_logic_cache.is_empty() && self.value_region_cache.is_empty() {
            return;
        }
        if let Some((net, wire)) = self.program.transport_wire(node) {
            if let Some(cache) = self.stable_logic_cache.get_mut(net as usize) {
                cache.invalidate();
            }
            if let Some(cache) = self.value_region_cache.get_mut(net as usize)
                && let Some(values) = self.program.instant_value_region_net(net)
            {
                cache.invalidate(values.wire_region[wire as usize]);
            }
        }
    }

    fn stable_logic_input(&self, logic: &StableLogicNet, index: usize, scratch: &mut StableLogicScratch) -> u8 {
        if !scratch.input_ready[index] {
            let input = logic.inputs[index];
            scratch.inputs[index] = if self.drive_condition(self.program.instant_drive_program(), input.condition) {
                match input.source {
                    DriveSource::Constant(value) => value,
                    DriveSource::Powered(source) => u8::from(self.node_state(source).powered) * 15,
                    DriveSource::Output(source) => self.stored_component_strength(source),
                }
            } else {
                0
            };
            scratch.input_ready[index] = true;
            scratch.sampled_inputs.push(index);
        }
        scratch.inputs[index]
    }

    fn stable_logic_expression(&self, logic: &StableLogicNet, expression: &DriveExpr, scratch: &mut StableLogicScratch) -> u8 {
        let mut power = expression.constant;
        for term in &logic.terms[expression.terms.start as usize..expression.terms.end as usize] {
            if power == 15 {
                break;
            }
            let index = term.input as usize;
            power = power.max(self.stable_logic_input(logic, index, scratch).saturating_sub(term.attenuation));
        }
        power
    }

    pub(super) fn try_stable_instant_logic(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<bool, GraphExecutionError> {
        if self.classic_proof.is_some() || super::fixed_point::enabled() {
            return Ok(false);
        }
        #[cfg(test)]
        if self.test_recorder.wire_tapes.is_some() || self.test_recorder.unchecked_classic.is_some() {
            return Ok(false);
        }
        let Some((net_index, root)) = self.program.transport_wire(node) else {
            return Ok(false);
        };
        let Some(logic) = self.program.stable_logic_net(net_index).cloned() else {
            return Ok(false);
        };
        if self.wire_plan(node).execution.abstract_instant_net {
            return Ok(false);
        }
        if let Some(observed) = captured
            && let ObservedProperties::Wire(wire) = observed.properties
            && wire != self.wire_state(node)
        {
            self.invalidate_wire_programs(node);
            self.counts.stable_logic_fallbacks += 1;
            return Ok(false);
        }
        let net = self.program.instant_transport_net(net_index).clone();
        let cache_index = net_index as usize;
        if self.stable_logic_cache.len() <= cache_index {
            self.stable_logic_cache.resize_with(cache_index + 1, StableLogicCache::default);
        }
        let mut scratch = self.stable_logic_scratch.pop().unwrap_or_default();
        scratch.inputs.resize(scratch.inputs.len().max(logic.inputs.len()), 0);
        scratch.input_ready.resize(scratch.input_ready.len().max(logic.inputs.len()), false);
        self.counts.stable_logic_invocations += 1;
        #[cfg(test)]
        self.test_recorder
            .piston_sample_census
            .begin(piston_sample_census::STABLE, !net.ordered_receivers);
        let result = (|| {
            let root_power = self.stable_logic_expression(&logic, &logic.expressions[root as usize], &mut scratch);
            self.counts.stable_logic_wire_values += 1;
            if root_power == self.wire_state(node).power {
                return Ok(());
            }
            // Sources can change without wire writes. Compare every live input to the
            // committed snapshot; cache validity describes old wires, not current sources.
            for index in 0..logic.inputs.len() {
                self.stable_logic_input(&logic, index, &mut scratch);
            }
            scratch.expressions.clear();
            let cache = &self.stable_logic_cache[cache_index];
            if cache.valid {
                for (input, (&old, &new)) in cache.inputs.iter().zip(&scratch.inputs).enumerate() {
                    if old != new {
                        let range = &logic.input_users[input];
                        scratch
                            .expressions
                            .extend_from_slice(&logic.input_user_expressions[range.start as usize..range.end as usize]);
                    }
                }
                scratch.expressions.sort_unstable();
                scratch.expressions.dedup();
            } else {
                scratch.expressions.extend(0..logic.expressions.len() as u32);
            }
            debug_assert!(
                scratch.expressions.binary_search(&root).is_ok(),
                "changed root must have a changed input or invalid cache"
            );
            self.stable_logic_cache[cache_index].valid = false;
            scratch.powers.clear();
            for index in 0..scratch.expressions.len() {
                let wire = scratch.expressions[index];
                let power = if wire == root {
                    root_power
                } else {
                    self.counts.stable_logic_wire_values += 1;
                    self.stable_logic_expression(&logic, &logic.expressions[wire as usize], &mut scratch)
                };
                scratch.powers.push((wire, power));
            }
            scratch.changed.resize(logic.expressions.len(), false);
            for &(index, power) in &scratch.powers {
                let wire = &net.wires[index as usize];
                let mut state = self.wire_state(wire.node);
                if state.power != power {
                    scratch.changed[index as usize] = true;
                    scratch.changed_wires.push(index);
                    state.power = power;
                    self.set_wire(wire.node, state);
                }
            }
            scratch.outputs.clear();
            for &wire in scratch.changed_wires.iter().chain(std::iter::once(&root)) {
                let range = &logic.wire_outputs[wire as usize];
                scratch
                    .outputs
                    .extend_from_slice(&logic.wire_output_indices[range.start as usize..range.end as usize]);
            }
            scratch.outputs.sort_unstable();
            scratch.outputs.dedup();
            // Select every output before dispatch; queue-only receivers cannot change sources.
            for &index in &scratch.outputs {
                let output = &logic.outputs[index as usize];
                let mut observed = None;
                for candidate in &output.candidates {
                    if !scratch.changed[candidate.wire as usize] && candidate.wire != root {
                        continue;
                    }
                    let observation = observed.get_or_insert_with(|| self.observe_wire_site(output.site).expect("validated stable logic output"));
                    let wire = &net.wires[candidate.wire as usize];
                    let CircuitNodeKind::Wire(index) = self.program.nodes()[wire.node.index()].kind else {
                        unreachable!()
                    };
                    let admitted = candidate
                        .admission
                        .unwrap_or_else(|| self.wire_route_admitted(index, wire.route, candidate.slot as usize, output.site, Some(observation)));
                    if admitted {
                        scratch.deliveries.push((output.site, observation.clone(), wire.route));
                        break;
                    }
                }
            }
            let revision = self.stable_logic_cache[cache_index].revision;
            for (site, observed, route) in &scratch.deliveries {
                self.counts.stable_logic_boundary_calls += 1;
                self.dispatch_turbo_nonwire(*site, observed, 1, Some(*route))?;
            }
            let cache = &mut self.stable_logic_cache[cache_index];
            if cache.revision == revision {
                cache.inputs.clear();
                cache.inputs.extend_from_slice(&scratch.inputs[..logic.inputs.len()]);
                cache.valid = true;
            }
            Ok(())
        })();
        #[cfg(test)]
        self.test_recorder.piston_sample_census.end();
        scratch.deliveries.clear();
        for wire in scratch.changed_wires.drain(..) {
            scratch.changed[wire as usize] = false;
        }
        for input in scratch.sampled_inputs.drain(..) {
            scratch.input_ready[input] = false;
        }
        self.stable_logic_scratch.push(scratch);
        result.map(|()| true)
    }
}
