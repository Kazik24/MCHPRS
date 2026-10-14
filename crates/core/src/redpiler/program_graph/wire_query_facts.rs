//! Checked static facts used to partially evaluate wire queries.
use super::{CallbackReceiver, CircuitDraft, CircuitNodeKind, ComponentState, NodeId, SiteId, SiteSource, WirePlan};
use mchprs_blocks::blocks::Block;

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
    /// Distinct neighbors can never dispatch another wire, so callbacks need no walk.
    pub(crate) isolated: bool,
}

impl WireQueryFacts {
    pub(crate) fn infer(graph: &CircuitDraft, wire: &WirePlan) -> Self {
        let mut facts = Self::default();
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
                        (CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. }, _) => node
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
        facts.isolated = isolated_neighborhood(graph, wire);
        facts
    }

    pub(crate) fn geometry_at(self, slot: usize) -> Option<(bool, bool)> {
        let bit = 1 << slot;
        (self.known & bit != 0).then_some((self.solid & bit != 0, self.transparent & bit != 0))
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
