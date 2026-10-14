//! Exact Turbo transcript for a certified south-facing private reset's A/B output.
//!
//! Recognition proves the sole source at A's north side, fixed line geometry,
//! receiver order B/R/H then R/A/T, and no other admitted receivers. R/T update
//! their own repeater state and schedule ticks; H only samples its actor. None
//! can reenter or mutate the pair, source or captured admission guards here.
//! A full round computes A' = max(source, A.saturating_sub(2)); legal four-bit
//! strengths therefore finish within eight full rounds and one final B visit.

#[cfg(test)]
use super::graph_test_trace;
use super::*;

impl GraphBackend {
    pub(super) fn evaluate_instant_output(&mut self, plan: &WirePlan, instant: InstantResetPlan) -> Result<(), GraphExecutionError> {
        let partner = self.wire_plan(instant.partner).clone();
        let CircuitNodeKind::Wire(root_index) = self.program.nodes()[plan.node.index()].kind else {
            unreachable!("instant output root was validated as a wire");
        };
        let CircuitNodeKind::Wire(partner_index) = self.program.nodes()[partner.node.index()].kind else {
            unreachable!("instant output partner was validated as a wire");
        };
        let root_route = plan.turbo_route.expect("instant output root has a validated Turbo route");
        let partner_route = partner.turbo_route.expect("instant output partner has a validated Turbo route");
        let [source_site, head_site, repeater_site, tail_site] = [
            plan.canonical_neighbors[4],
            plan.canonical_neighbors[22],
            plan.canonical_neighbors[23],
            partner.canonical_neighbors[23],
        ];
        let mut root_wire = self.wire_state(plan.node);
        let mut partner_wire = self.wire_state(partner.node);
        let [source, head, repeater, tail] = [source_site, head_site, repeater_site, tail_site].map(|site| {
            self.resolve_site(site, None)
                .expect("instant output neighborhood was validated during preparation")
                .observed()
        });
        // B is the first delivery, so both neighborhoods precede every external callback.
        let root_repeater = self.wire_route_delivery(root_index, root_route, 23, repeater_site, &repeater).is_some();
        let root_head = self.wire_route_delivery(root_index, root_route, 22, head_site, &head).is_some();
        let partner_repeater = self
            .wire_route_delivery(partner_index, partner_route, 5, repeater_site, &repeater)
            .is_some();
        let partner_tail = self.wire_route_delivery(partner_index, partner_route, 23, tail_site, &tail).is_some();
        let source_power = self.power_no_dust(source_site, BlockFace::North, Some(&source));
        #[cfg(test)]
        graph_test_trace!(self, format!("wire instant output {:?} partner={:?}", plan.node, partner.node));

        let mut layer = 1;
        loop {
            // Fresh A has heading 3: B, R, H. B has heading 2: R, A, T.
            let partner_power = root_wire.power.saturating_sub(1);
            let partner_changed = partner_power != partner_wire.power;
            if partner_changed {
                partner_wire.power = partner_power;
                self.set_wire(partner.node, partner_wire);
            }
            if root_repeater {
                self.dispatch_turbo_nonwire(repeater_site, &repeater, layer)?;
            }
            if root_head {
                self.dispatch_turbo_nonwire(head_site, &head, layer)?;
            }
            if !partner_changed {
                return Ok(());
            }

            layer += 1;
            if partner_repeater {
                self.dispatch_turbo_nonwire(repeater_site, &repeater, layer)?;
            }
            let root_power = source_power.max(partner_wire.power.saturating_sub(1));
            let root_changed = root_power != root_wire.power;
            if root_changed {
                root_wire.power = root_power;
                self.set_wire(plan.node, root_wire);
            }
            // T remains ahead of the next B delivery even when A changed.
            if partner_tail {
                self.dispatch_turbo_nonwire(tail_site, &tail, layer)?;
            }
            if !root_changed {
                return Ok(());
            }
            layer += 1;
        }
    }
}
