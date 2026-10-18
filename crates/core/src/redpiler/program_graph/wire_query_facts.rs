//! Checked static facts used to partially evaluate wire queries.
use super::{CallbackReceiver, CircuitDraft, CircuitNodeKind, ComponentState, NodeId, SiteId, SiteSource, WirePlan};
use mchprs_blocks::blocks::{Block, RedstoneWire, RedstoneWireSide};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
/// Property bitplanes for a wire's 24 canonical neighbors; power is never cached.
pub(crate) struct WireQueryFacts {
    /// A set bit proves the site's geometry and block class are immutable.
    pub(crate) known: u32,
    /// A set bit identifies a known solid neighbor that conducts live strong power.
    pub(crate) solid: u32,
    /// A set bit identifies a known transparent neighbor for wire step rules.
    pub(crate) transparent: u32,
    /// A set bit identifies a fixed wire owner whose strength remains dynamic.
    pub(crate) wire: u32,
    /// A set bit proves the no-dust query is zero for a nonsolid wire or air.
    pub(crate) zero_no_dust: u32,
    /// Fixed raw N/S/E/W sides, two bits each; power and dot/cross regulation remain runtime operands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) raw_sides: Option<u8>,
    /// Distinct neighbors can never dispatch another wire, so callbacks need no walk.
    pub(crate) isolated: bool,
    /// Pure wire-strength expressions retain live seed reads and captured Turbo reads separately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) transfer: Option<WireTransfer>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
/// Wire inputs after partial evaluation of fixed geometry; no event order is changed.
pub(crate) struct WireTransfer {
    /// These inputs use live geometry and strengths when starting a walk.
    pub(crate) seed: WireInputs,
    /// These inputs use the walk's captured geometry and strengths during propagation.
    pub(crate) turbo: WireInputs,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
/// Canonical neighbor-slot masks for one maximum-of-wire-strengths expression.
pub(crate) struct WireInputs {
    /// These slots contribute without further geometry checks, but their strengths remain dynamic.
    pub(crate) direct: u32,
    /// These diagonal slots require the original step-up or step-down geometry condition.
    pub(crate) conditional: u32,
}

impl WireInputs {
    pub(crate) fn selected(self, turbo: bool, mut geometry: impl FnMut(usize) -> (bool, bool)) -> u32 {
        let mut selected = self.direct;
        let mut conditional = self.conditional;
        while conditional != 0 {
            let slot = conditional.trailing_zeros() as usize;
            conditional &= conditional - 1;
            let (horizontal, ascent) = match slot {
                7 => (0, false),
                8 => (0, true),
                12 => (1, false),
                13 => (1, true),
                17 => (4, false),
                20 => (4, true),
                18 => (5, false),
                21 => (5, true),
                _ => unreachable!("validated wire input is a diagonal slot"),
            };
            let (solid, transparent) = geometry(horizontal);
            let enabled = if ascent {
                (!turbo || solid) && !transparent && !geometry(3).0
            } else {
                !solid
            };
            if enabled {
                selected |= 1 << slot;
            }
        }
        selected
    }

    pub(crate) fn valid(self) -> bool {
        const DIAGONALS: u32 = (1 << 7) | (1 << 8) | (1 << 12) | (1 << 13) | (1 << 17) | (1 << 18) | (1 << 20) | (1 << 21);
        self.direct & !(DIAGONALS | 0b111111) == 0 && self.direct & self.conditional == 0 && self.conditional & !DIAGONALS == 0
    }
}

impl WireQueryFacts {
    pub(crate) fn infer(graph: &CircuitDraft, wire: &WirePlan) -> Self {
        let mut facts = Self::default();
        let mut fixed_blocks = [Block::Air; 24];
        for (slot, site) in wire.canonical_neighbors.iter().enumerate() {
            let source = graph.sites[site.index()].source;
            let properties = match source {
                SiteSource::Constant(profile) => {
                    let profile = graph.profiles[profile.index()];
                    Some((Block::from_id(profile.raw_state), profile.solid, profile.transparent))
                }
                SiteSource::Component(node) => {
                    let node = &graph.nodes[node.index()];
                    match (&node.kind, &node.state) {
                        (CircuitNodeKind::Wire(_), ComponentState::Wire(wire)) => {
                            // Runtime wire geometry comes from typed state, not its saved raw block.
                            let block = Block::RedstoneWire { wire: *wire };
                            Some((block, block.is_solid(), block.is_transparent()))
                        }
                        (CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }, _) => node
                            .raw_state
                            .map(Block::from_id)
                            .map(|block| (block, block.is_solid(), block.is_transparent())),
                        _ => None,
                    }
                }
                // Contact selection can change geometry during a nested callback.
                SiteSource::Contact(_) => None,
            };
            if let Some((block, solid, transparent)) = properties {
                fixed_blocks[slot] = block;
                let bit = 1 << slot;
                let is_wire = matches!(block, Block::RedstoneWire { .. });
                facts.known |= bit;
                if solid {
                    facts.solid |= bit;
                }
                if transparent {
                    facts.transparent |= bit;
                }
                if is_wire {
                    facts.wire |= bit;
                }
                // power_no_dust checks solidity before excluding wire power.
                if !solid && (is_wire || block == Block::Air) {
                    facts.zero_no_dust |= bit;
                }
            }
        }
        // Only fixed sources reach this branch. Matching geometry alone would not
        // prove connection class or diode facing for a selectable contact.
        if facts.known == (1 << 24) - 1 {
            let raw = crate::redstone::wire::get_raw_sides_from_neighbors(RedstoneWire::default(), &fixed_blocks);
            facts.raw_sides = Some(
                [raw.north, raw.south, raw.east, raw.west]
                    .into_iter()
                    .enumerate()
                    .fold(0, |bits, (index, side)| bits | ((side.get_id() as u8) << (2 * index))),
            );
        }
        facts.isolated = isolated_neighborhood(graph, wire);
        // A contact may change its geometry without ever containing a wire. Prove
        // absence across every variant instead of treating all contacts as wires.
        let possible_wires = wire
            .canonical_neighbors
            .iter()
            .enumerate()
            .fold(0, |mask, (slot, site)| mask | (u32::from(!site_is_not_wire(graph, *site)) << slot));
        facts.transfer = Some(WireTransfer {
            seed: infer_inputs(facts, possible_wires, false),
            turbo: infer_inputs(facts, possible_wires, true),
        });
        facts
    }

    pub(crate) fn geometry_at(self, slot: usize) -> Option<(bool, bool)> {
        let bit = 1 << slot;
        (self.known & bit != 0).then_some((self.solid & bit != 0, self.transparent & bit != 0))
    }

    pub(crate) fn raw_wire(self, power: u8) -> Option<RedstoneWire> {
        let bits = self.raw_sides?;
        let side = |shift: u32| RedstoneWireSide::from_id(u32::from((bits >> shift) & 3u8));
        Some(RedstoneWire::new(side(0), side(2), side(4), side(6), power))
    }

    pub(crate) fn raw_sides_valid(self) -> bool {
        self.raw_sides
            .is_none_or(|bits| self.known == (1 << 24) - 1 && (0..4).all(|index| (bits >> (2 * index)) & 3 != 3))
    }
}

fn isolated_neighborhood(graph: &CircuitDraft, wire: &WirePlan) -> bool {
    wire.canonical_neighbors.iter().enumerate().all(|(slot, site)| {
        // A repeated site would share visit/layer state in the general walk.
        !wire.canonical_neighbors[..slot].contains(site) && site_is_not_wire(graph, *site)
    })
}

fn site_is_not_wire(graph: &CircuitDraft, site: SiteId) -> bool {
    match graph.sites[site.index()].source {
        SiteSource::Constant(profile) => !is_wire(Block::from_id(graph.profiles[profile.index()].raw_state)),
        SiteSource::Component(node) => node_is_not_wire(graph, node),
        SiteSource::Contact(contact) => {
            let variants = &graph.contacts[contact.index()].variants;
            !variants.is_empty()
                && variants.iter().all(|variant| {
                    let profile_is_not_wire = !is_wire(Block::from_id(graph.profiles[variant.profile.index()].raw_state));
                    let receiver_is_not_wire = match variant.receiver {
                        Some(CallbackReceiver::Component(node)) => node_is_not_wire(graph, node),
                        _ => true,
                    };
                    profile_is_not_wire && receiver_is_not_wire
                })
        }
    }
}

fn node_is_not_wire(graph: &CircuitDraft, node: NodeId) -> bool {
    let node = &graph.nodes[node.index()];
    !matches!(node.kind, CircuitNodeKind::Wire(_)) && node.raw_state.is_some_and(|raw| !is_wire(Block::from_id(raw)))
}

fn is_wire(block: Block) -> bool {
    matches!(block, Block::RedstoneWire { .. })
}

/// Partially evaluates the native seed/Turbo equations, which differ at vertical inputs and ascents.
fn infer_inputs(facts: WireQueryFacts, possible_wires: u32, turbo: bool) -> WireInputs {
    let adjacent = if turbo { 0b110011 } else { 0b111111 };
    let mut inputs = WireInputs {
        direct: possible_wires & adjacent,
        conditional: 0,
    };
    for (neighbor, below, above) in [(0, 7, 8), (1, 12, 13), (4, 17, 20), (5, 18, 21)] {
        let horizontal = facts.geometry_at(neighbor);
        let center_up = facts.geometry_at(3);
        let descent = horizontal.map(|(solid, _)| !solid);
        let ascent = conjunction([
            center_up.map(|(solid, _)| !solid),
            horizontal.map(|(_, transparent)| !transparent),
            if turbo { horizontal.map(|(solid, _)| solid) } else { Some(true) },
        ]);
        for (slot, condition) in [(below, descent), (above, ascent)] {
            if possible_wires & (1 << slot) == 0 {
                continue;
            }
            match condition {
                Some(true) => inputs.direct |= 1 << slot,
                Some(false) => {}
                None => inputs.conditional |= 1 << slot,
            }
        }
    }
    inputs
}

fn conjunction(terms: [Option<bool>; 3]) -> Option<bool> {
    if terms.contains(&Some(false)) {
        Some(false)
    } else if terms.iter().all(|term| *term == Some(true)) {
        Some(true)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_inputs_equal_native_geometry_equations_for_every_local_case() {
        // Each term of max is independent. Check every basis wire slot and the
        // complete set, with each fixed/unknown geometry combination and rotation.
        let mut cases = 0;
        for horizontal in [0, 1, 4, 5] {
            for geometry in 0..8 {
                let up_solid = geometry & 1 != 0;
                let solid = geometry & 2 != 0;
                let transparent = geometry & 4 != 0;
                let mut properties = [(false, true); 24];
                properties[3] = (up_solid, true);
                properties[horizontal] = (solid, transparent);
                for unknown in 0..4 {
                    let mut facts = WireQueryFacts::default();
                    for (slot, (solid, transparent)) in properties.iter().copied().enumerate() {
                        if slot == 3 && unknown & 1 != 0 || slot == horizontal && unknown & 2 != 0 {
                            continue;
                        }
                        facts.known |= 1 << slot;
                        facts.solid |= u32::from(solid) << slot;
                        facts.transparent |= u32::from(transparent) << slot;
                    }
                    for possible in (0..24).map(|slot| 1 << slot).chain([0, 0xffffff]) {
                        for turbo in [false, true] {
                            let mut expected = if turbo { 0b110011 } else { 0b111111 };
                            for (h, below, above) in [(0, 7, 8), (1, 12, 13), (4, 17, 20), (5, 18, 21)] {
                                if !properties[h].0 {
                                    expected |= 1 << below;
                                }
                                if (!turbo || properties[h].0) && !properties[h].1 && !up_solid {
                                    expected |= 1 << above;
                                }
                            }
                            let inputs = infer_inputs(facts, possible, turbo);
                            assert!(inputs.valid());
                            assert_eq!(inputs.selected(turbo, |slot| properties[slot]), expected & possible);
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 6656);
    }
}
