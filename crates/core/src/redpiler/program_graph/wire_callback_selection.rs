//! Compile Turbo admission only; discovery, captured operands and queue order remain native.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) struct WireCallbackSelection {
    /// Slots whose first matching captured delivery is statically decided.
    pub(crate) known: u32,
    /// Known slots that admit a callback to the captured site's receiver.
    pub(crate) admitted: u32,
}

impl WireCallbackSelection {
    pub(crate) const ALL: u32 = (1 << 24) - 1;
    pub(crate) fn valid(self) -> bool {
        self.known & !Self::ALL == 0 && self.admitted & !self.known == 0
    }

    pub(crate) fn infer(graph: &CompileGraph, index: usize) -> Option<Self> {
        let wire = &graph.wires[index];
        let route = wire.turbo_route?;
        let deliveries = &graph.routes[route.index()].deliveries;
        let indexed = graph.plans.turbo_callback_indices.get(index).copied().flatten();
        let mut result = Self { known: 0, admitted: 0 };
        for (slot, &target) in wire.canonical_neighbors.iter().enumerate() {
            let candidates = indexed.map_or(deliveries.as_ref(), |index| {
                &deliveries[index.offsets[slot] as usize..index.offsets[slot + 1] as usize]
            });
            let mut admission = Some(false);
            for delivery in candidates {
                if delivery.target != target || delivery.receiver.is_none() || delivery.view != OperandView::Captured {
                    continue;
                }
                match fixed_guard(graph, delivery.guard, target, 0) {
                    Some(false) => {}
                    answer => {
                        admission = answer;
                        break;
                    }
                }
            }
            if let Some(admitted) = admission {
                result.known |= 1 << slot;
                if admitted {
                    result.admitted |= 1 << slot;
                }
            }
        }
        (result.known != 0).then_some(result)
    }
}

/// None is genuinely dynamic, never a false predicate. Boolean identities can
/// still decide a composite containing an unknown child.
fn fixed_guard(graph: &CompileGraph, id: GuardId, captured: SiteId, depth: usize) -> Option<bool> {
    if depth > 64 {
        return None;
    }
    Some(match &graph.guards[id.index()] {
        Guard::Always => true,
        Guard::State(_) => return None,
        Guard::Site {
            site,
            view: OperandView::Captured,
            ..
        } if *site != captured => false,
        Guard::Site { site, test, .. } => match graph.sites[site.index()].source {
            SiteSource::Contact(_) => return None,
            SiteSource::Constant(profile) => {
                let record = graph.profiles[profile.index()];
                match test {
                    SiteTest::Solid => record.solid,
                    SiteTest::Transparent => record.transparent,
                    SiteTest::IsWire => matches!(Block::from_id(record.raw_state), Block::RedstoneWire { .. }),
                    SiteTest::Profile(expected) => *expected == profile,
                }
            }
            SiteSource::Component(node) => {
                if matches!(test, SiteTest::Profile(_)) {
                    false
                } else {
                    let node = &graph.nodes[node.index()];
                    if !matches!(
                        node.kind,
                        CircuitNodeKind::Electrical(_)
                            | CircuitNodeKind::Wire(_)
                            | CircuitNodeKind::Observer { .. }
                            | CircuitNodeKind::BuiltinReset { .. }
                    ) {
                        return None;
                    }
                    let block = if let ComponentState::Wire(wire) = node.state {
                        Block::RedstoneWire { wire }
                    } else {
                        Block::from_id(node.raw_state?)
                    };
                    match test {
                        SiteTest::Solid => block.is_solid(),
                        SiteTest::Transparent => block.is_transparent(),
                        SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                        SiteTest::Profile(_) => unreachable!(),
                    }
                }
            }
        },
        Guard::Not(child) => !fixed_guard(graph, *child, captured, depth + 1)?,
        Guard::All(children) => {
            let mut unknown = false;
            for child in children {
                match fixed_guard(graph, *child, captured, depth + 1) {
                    Some(false) => return Some(false),
                    None => unknown = true,
                    _ => {}
                }
            }
            if unknown {
                return None;
            }
            true
        }
        Guard::Any(children) => {
            let mut unknown = false;
            for child in children {
                match fixed_guard(graph, *child, captured, depth + 1) {
                    Some(true) => return Some(true),
                    None => unknown = true,
                    _ => {}
                }
            }
            if unknown {
                return None;
            }
            false
        }
    })
}
