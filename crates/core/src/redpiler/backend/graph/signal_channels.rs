//! Guard every entry before effects; admitted transitions execute no interior wire walk.
use super::*;

impl GraphBackend {
    pub(super) fn try_signal_channel(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<bool, GraphExecutionError> {
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
        let Some(channel) = self.program.signal_channel(net_index).cloned() else {
            return Ok(false);
        };
        let fallback = |this: &mut Self| {
            this.counts.signal_channel_fallbacks += 1;
            Ok(false)
        };
        if !self.transport_entry_admitted(net_index) {
            return fallback(self);
        }
        if let Some(observed) = captured
            && let ObservedProperties::Wire(wire) = observed.properties
            && wire != self.wire_state(node)
        {
            return fallback(self);
        }
        let new = if !self.drive_condition(self.program.instant_drive_program(), channel.input.condition) {
            0
        } else {
            match channel.input.source {
                DriveSource::Constant(value) => usize::from(value != 0),
                DriveSource::Powered(source) => usize::from(self.node_state(source).powered),
                DriveSource::Output(source) => match self.stored_component_strength(source) {
                    0 => 0,
                    15 => 1,
                    _ => return fallback(self),
                },
            }
        };
        let net = self.program.instant_transport_net(net_index).clone();
        let discriminator = channel.discriminator;
        let observed = self.wire_state(net.wires[discriminator].node).power;
        let old = if observed == channel.profiles[0][discriminator] {
            0
        } else if observed == channel.profiles[1][discriminator] {
            1
        } else {
            return fallback(self);
        };
        if net
            .wires
            .iter()
            .zip(&channel.profiles[old])
            .any(|(wire, &power)| self.wire_state(wire.node).power != power)
        {
            return fallback(self);
        }
        self.counts.signal_channel_invocations += 1;
        if old != new
            && let Some(program) = &channel.transitions[old * net.wires.len() + root as usize]
        {
            self.counts.signal_channel_operations += program.ops.len() as u64;
            self.run_signal_transcript(&net, program)?;
        }
        Ok(true)
    }
}
