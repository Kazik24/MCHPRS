use super::*;

/// One observed clock request; pending Graph work remains with the caller.
pub(crate) struct ObservedClockEdge {
    pub source: mchprs_blocks::BlockPos,
    pub route: usize,
    pub actor: mchprs_blocks::BlockPos,
    pub from: mchprs_blocks::BlockPos,
    pub to: mchprs_blocks::BlockPos,
    pub window_end_pending: crate::redpiler::backend::graph::piston_probe::PendingWork,
    net: EventNetlist,
}

impl ObservedClockEdge {
    /// Accept only the observed callback occurrence; live power is revalidated.
    pub(crate) fn fire(&mut self, source: mchprs_blocks::BlockPos, route: usize, power: u8) -> Result<bool, &'static str> {
        if source != self.source || route != self.route {
            return Err("unrepresented clock request");
        }
        let before = self.net.contacts[0];
        self.net.frame(&[power], &[0], &[])?;
        Ok(self.net.contacts[0] != before)
    }
}

impl EventNetlist {
    /// Build one falling-edge transfer from a bounded physical observation.
    /// The caller must handle pending Graph work; the recorded pending state is at the window end, not the edge.
    pub(crate) fn from_clock_edge(trial: &crate::redpiler::backend::graph::piston_probe::PistonTrial) -> Result<ObservedClockEdge, &'static str> {
        let index = *trial.clock_retractions.first().ok_or("no observed falling clock event")?;
        let attempt = trial.attempts.get(index).ok_or("invalid clock event index")?;
        let before = attempt.before.as_ref().ok_or("clock event has no resource")?;
        let after = attempt.after.as_ref().ok_or("clock event has no resource result")?;
        if trial.clock != Some(attempt.actor)
            || trial.input_events.len() != 1
            || !attempt.accepted
            || attempt.powered
            || attempt.action != mchprs_world::PistonAction::Retract
            || !before.enabled
            || before.id != after.id
            || before.position == after.position
            || trial.attempts[..index].iter().any(|earlier| earlier.accepted)
        {
            return Err("unsupported clock edge");
        }
        let net = Self::new(
            vec![0],
            vec![0],
            2,
            1,
            vec![Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            }],
            vec![Action {
                input: 0,
                threshold: 0,
                when_powered: false,
                resource: 0,
                from: 0,
                to: 1,
            }],
            &[],
        )?;
        Ok(ObservedClockEdge {
            source: attempt.source,
            route: attempt.route,
            actor: attempt.actor,
            from: before.position,
            to: after.position,
            window_end_pending: trial.pending.clone(),
            net,
        })
    }

    /// Replay one observed, source-qualified transfer; this does not generalize the trial to other states.
    pub(crate) fn from_piston_trial(trial: &crate::redpiler::backend::graph::piston_probe::PistonTrial) -> Result<Self, &'static str> {
        if !trial.settled
            || trial.input_events.len() != 1
            || trial.initial_resources.len() != 1
            || trial.final_resources.len() != 1
            || trial.attempts.len() != 1
        {
            return Err("piston trial is not a single-resource single-request transition");
        }
        let attempt = &trial.attempts[0];
        let before = attempt.before.as_ref().ok_or("piston request has no resource")?;
        let after = attempt.after.as_ref().ok_or("piston request has no resource result")?;
        let initial = &trial.initial_resources[0];
        let final_state = &trial.final_resources[0];
        if !attempt.accepted
            || !before.enabled
            || !final_state.enabled
            || before != initial
            || before.id != after.id
            || after.id != final_state.id
            || before.position == after.position
            || after.position != final_state.position
            || !matches!(
                (attempt.action, attempt.powered),
                (mchprs_world::PistonAction::Extend, true) | (mchprs_world::PistonAction::Retract, false)
            )
        {
            return Err("piston trial is not a settled resource transfer");
        }
        Self::new(
            vec![u8::from(attempt.powered) * 15],
            vec![0],
            2,
            1,
            vec![Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            }],
            vec![Action {
                input: 0,
                threshold: 0,
                when_powered: attempt.powered,
                resource: 0,
                from: 0,
                to: 1,
            }],
            &[],
        )
    }
}
