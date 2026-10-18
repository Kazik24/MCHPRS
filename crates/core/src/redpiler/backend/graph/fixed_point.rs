//! Experimental final-state dust propagation; deliberately loses transient callbacks.
use super::*;

#[cfg(test)]
thread_local! {
    static MODE: Cell<Option<bool>> = const { Cell::new(None) };
}

pub(super) fn enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = MODE.get() {
        return enabled;
    }
    cfg!(feature = "unsafe-turbo-fixed-point")
}

#[cfg(test)]
pub(super) fn with_mode<R>(enabled: bool, work: impl FnOnce() -> R) -> R {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            MODE.set(self.0);
        }
    }
    let _restore = Restore(MODE.replace(Some(enabled)));
    work()
}

impl GraphBackend {
    pub(super) fn evaluate_fixed_point_wire(&mut self, root: NodeId) -> Result<(), GraphExecutionError> {
        let (wires, edges) = self.collect_fixed_point_network(root);
        let initial: Vec<_> = wires.iter().map(|&node| self.wire_state(node).power).collect();
        let sources: Vec<_> = wires
            .iter()
            .map(|&node| {
                let plan = self.wire_plan(node).clone();
                self.seed_power_general(&plan).1
            })
            .collect();
        // Start from zero so cycles cannot retain stale power; publish only the final values.
        let mut strengths = vec![0u8; wires.len()];
        let mut next = strengths.clone();
        // Strengths are four-bit and attenuate on each wire hop: fifteen rounds suffice.
        for _ in 0..15 {
            let mut changed = false;
            for (index, neighbors) in edges.iter().enumerate() {
                let incoming = neighbors.iter().map(|&neighbor| strengths[neighbor].saturating_sub(1)).max().unwrap_or(0);
                next[index] = sources[index].max(incoming);
                changed |= next[index] != strengths[index];
            }
            std::mem::swap(&mut strengths, &mut next);
            if !changed {
                break;
            }
        }
        for (&node, power) in wires.iter().zip(strengths) {
            let mut wire = self.wire_state(node);
            wire.power = power;
            self.set_wire(node, wire);
        }

        // Admit all boundary notifications before dispatch; nested callbacks may change contacts.
        let network: FxHashSet<_> = wires.iter().copied().collect();
        let mut seen = FxHashSet::default();
        let mut boundary = Vec::new();
        for (&node, old_power) in wires.iter().zip(initial) {
            if node != root && self.wire_state(node).power == old_power {
                continue;
            }
            let plan = self.wire_plan(node).clone();
            let Some(route) = plan.turbo_route else {
                continue;
            };
            let CircuitNodeKind::Wire(index) = self.program.nodes()[node.index()].kind else {
                unreachable!()
            };
            for (slot, site) in plan.canonical_neighbors.into_iter().enumerate() {
                let observed = self.resolve_site(site, None).expect("validated wire neighborhood").observed();
                if matches!(observed.receiver, Some(CallbackReceiver::Component(receiver))
                    if network.contains(&receiver))
                {
                    continue;
                }
                if self.wire_route_delivery(index, route, slot, site, &observed).is_some() && seen.insert(site) {
                    boundary.push((site, observed));
                }
            }
        }
        for (site, observed) in boundary {
            self.dispatch_turbo_nonwire(site, &observed, 1)?;
        }
        Ok(())
    }

    fn collect_fixed_point_network(&self, root: NodeId) -> (Vec<NodeId>, Vec<Vec<usize>>) {
        let mut wires = vec![root];
        let mut slots = FxHashMap::default();
        slots.insert(root, 0);
        let mut edges = Vec::new();
        let mut cursor = 0;
        while cursor < wires.len() {
            let mut incoming = Vec::new();
            for node in self.fixed_point_wire_neighbors(wires[cursor]) {
                let slot = *slots.entry(node).or_insert_with(|| {
                    let slot = wires.len();
                    wires.push(node);
                    slot
                });
                incoming.push(slot);
            }
            edges.push(incoming);
            cursor += 1;
        }
        (wires, edges)
    }

    pub(super) fn fixed_point_wire_neighbors(&self, node: NodeId) -> Vec<NodeId> {
        let plan = self.wire_plan(node);
        let view = |index| {
            self.resolve_site_view(plan.canonical_neighbors[index], None)
                .expect("validated wire neighborhood")
        };
        let up_solid = view(3).solid;
        let mut indices = vec![0, 1, 2, 3, 4, 5];
        // Match seed_power_general's ascent and descent rules, not Turbo's notification radius.
        for (side, above, below) in [(4, 20, 17), (5, 21, 18), (1, 13, 12), (0, 8, 7)] {
            let neighbor = view(side);
            if !up_solid && !neighbor.transparent {
                indices.push(above);
            }
            if !neighbor.solid {
                indices.push(below);
            }
        }
        indices
            .into_iter()
            .filter_map(|index| match view(index).block {
                Block::RedstoneWire { .. } => match self.program.sites()[plan.canonical_neighbors[index].index()].source {
                    SiteSource::Component(node) => Some(node),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "tests/fixed_point.rs"]
mod tests;
