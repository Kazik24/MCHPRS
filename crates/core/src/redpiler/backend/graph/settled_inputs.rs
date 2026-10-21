use super::*;

fn evaluate_input(
    source: LayerInputSource,
    mut site_power: impl FnMut(SiteId, PowerPort) -> u8,
    mut guard_matches: impl FnMut(GuardId) -> bool,
    mut state_matches: impl FnMut(StateTest) -> bool,
) -> Result<bool, &'static str> {
    match source {
        LayerInputSource::SourceAbove { site, port, threshold } => Ok(site_power(site, port) > threshold),
        LayerInputSource::PowerContribution { link, .. } | LayerInputSource::LocalResetOutput { link, .. } => {
            Ok(guard_matches(link.guard) && site_power(link.from, link.from_port) > link.attenuation)
        }
        LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => Ok(state_matches(test)),
        LayerInputSource::BoundaryEvent { .. } => Err("settled input requires a callback event"),
        LayerInputSource::BoundaryRequest { .. } => Err("settled input requires a pending piston request"),
        LayerInputSource::InstantFired { .. } => Err("settled input requires a logical firing latch"),
    }
}

impl GraphBackend {
    pub(super) fn sample_settled_input(&self, source: LayerInputSource) -> Result<bool, &'static str> {
        evaluate_input(
            source,
            |site, port| self.site_power(site, port),
            |guard| self.guard_matches(guard, None),
            |test| self.state.matches(test),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> PowerLink {
        PowerLink {
            from: SiteId::new(3),
            from_port: PowerPort::Output,
            to: PowerTarget::Conductor {
                site: SiteId::new(4),
                include_dust: false,
            },
            face: BlockFace::Top,
            attenuation: 4,
            guard: GuardId::new(5),
        }
    }

    #[test]
    fn source_above_uses_the_exact_exclusive_strength_threshold() {
        let source = LayerInputSource::SourceAbove {
            site: SiteId::new(1),
            port: PowerPort::Weak(BlockFace::East),
            threshold: 7,
        };
        assert!(evaluate_input(source, |_, _| 8, |_| true, |_| true).unwrap());
        assert!(!evaluate_input(source, |_, _| 7, |_| true, |_| true).unwrap());
    }

    #[test]
    fn contributions_require_both_the_live_guard_and_positive_remaining_power() {
        let source = LayerInputSource::PowerContribution { port: 0, link: link() };
        assert!(evaluate_input(source, |_, _| 5, |_| true, |_| false).unwrap());
        assert!(!evaluate_input(source, |_, _| 5, |_| false, |_| true).unwrap());
        assert!(!evaluate_input(source, |_, _| 4, |_| true, |_| true).unwrap());
    }

    #[test]
    fn state_boundaries_forward_the_live_state_predicate() {
        let test = StateTest::ResourceEnabled(ResourceId::new(2));
        assert!(evaluate_input(LayerInputSource::State(test), |_, _| 0, |_| false, |value| value == test).unwrap());
        assert!(
            evaluate_input(
                LayerInputSource::BoundaryState { port: 0, test },
                |_, _| 0,
                |_| false,
                |value| value == test
            )
            .unwrap()
        );
    }

    #[test]
    fn event_inputs_decline_instead_of_sampling_as_false() {
        let event = LayerInputSource::BoundaryEvent {
            port: 1,
            occurrence: RouteOccurrence {
                route: RouteId::new(2),
                ordinal: 0,
            },
        };
        assert_eq!(
            evaluate_input(event, |_, _| 15, |_| true, |_| true),
            Err("settled input requires a callback event")
        );

        let request = LayerInputSource::BoundaryRequest {
            port: 0,
            occurrence: RouteOccurrence {
                route: RouteId::new(2),
                ordinal: 1,
            },
            actor: PistonId::new(6),
        };
        assert_eq!(
            evaluate_input(request, |_, _| 15, |_| true, |_| true),
            Err("settled input requires a pending piston request")
        );

        assert_eq!(
            evaluate_input(LayerInputSource::InstantFired { actor: PistonId::new(6) }, |_, _| 15, |_| true, |_| true,),
            Err("settled input requires a logical firing latch")
        );
    }
}
