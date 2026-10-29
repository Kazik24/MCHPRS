use super::*;

/// A single piston compiled from physical lane geometry and callback routes.
/// Power and callback occurrences are supplied by the owning circuit executor.
#[derive(Clone)]
pub(crate) struct PhysicalPiston {
    pub(super) routes: Vec<usize>,
    pub(super) positions: [mchprs_blocks::BlockPos; 2],
    pub(super) net: EventNetlist,
}

/// Explicitly driven piston and delayed output; the caller owns source routing.
pub(crate) struct PhysicalScope {
    pub(crate) piston: PhysicalPiston,
    pub(crate) output: crate::redpiler::event_repeater::PhysicalRepeater,
}

impl PhysicalScope {
    pub(crate) fn notify(&mut self, route: usize, piston_power: u8, output_input: u8, locked: bool) -> Result<bool, &'static str> {
        if output_input > 15 {
            return Err("invalid output strength");
        }
        let moved = self.piston.fire(route, piston_power)?;
        self.output.notify(output_input, locked)?;
        Ok(moved)
    }

    pub(crate) fn tick(&mut self) -> bool {
        self.output.tick()
    }
}

impl PhysicalPiston {
    pub(crate) fn compile(
        program: &crate::redpiler::program_graph::CircuitProgram,
        state: &crate::redpiler::program_graph::CircuitState,
        actor: mchprs_blocks::BlockPos,
    ) -> Result<Self, &'static str> {
        use crate::redpiler::program_graph::{CallbackReceiver, PistonLane};
        let (piston_index, piston) = program
            .pistons()
            .iter()
            .enumerate()
            .find(|(_, piston)| piston.position == actor)
            .ok_or("piston missing")?;
        if !piston.sticky || !matches!(piston.lane, PistonLane::SingleResource { .. }) {
            return Err("unsupported piston lane");
        }
        let mut resources =
            program.resources().iter().enumerate().filter(|(index, spec)| {
                state.resources[*index].enabled && spec.contacts.contains(&piston.near) && spec.contacts.contains(&piston.far)
            });
        let resource = resources.next().ok_or("piston lane resource missing")?;
        if resources.next().is_some() {
            return Err("piston lane has multiple resources");
        }
        let at = state.resources[resource.0].at;
        let initial = if at == piston.near {
            0
        } else if at == piston.far {
            1
        } else {
            return Err("resource outside piston lane");
        };
        if state.pistons[piston_index].extended != (initial == 1) {
            return Err("piston and resource entry states disagree");
        }
        let routes = program
            .routes()
            .iter()
            .enumerate()
            .filter_map(|(index, route)| {
                route
                    .deliveries
                    .iter()
                    .any(|delivery| match delivery.receiver {
                        Some(CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id)) => id.index() == piston_index,
                        _ => false,
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if routes.is_empty() {
            return Err("piston has no callback routes");
        }
        let links = vec![Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        }];
        let actions = vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: false,
                resource: 0,
                from: 1,
                to: 0,
            },
        ];
        Ok(Self {
            routes,
            positions: [
                program.contacts()[piston.near.index()].position,
                program.contacts()[piston.far.index()].position,
            ],
            net: EventNetlist::new(vec![0], vec![initial], 2, 1, links, actions, &[])?,
        })
    }

    /// Route membership is necessary but callers must establish the delivery's live guard.
    pub(crate) fn fire(&mut self, route: usize, power: u8) -> Result<bool, &'static str> {
        if !self.routes.contains(&route) {
            return Err("unrepresented piston route");
        }
        let before = self.net.contacts[0];
        let action = if power > 0 { 0 } else { 1 };
        self.net.frame(&[power], &[action], &[])?;
        Ok(self.net.contacts[0] != before)
    }

    pub(crate) fn position(&self) -> mchprs_blocks::BlockPos {
        self.positions[self.net.contacts[0]]
    }
}
