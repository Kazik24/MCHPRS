//! Capture recipes own no observations: mutable operands are sampled on discovery.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WireCapture {
    /// This immutable source's observation can be copied from the program.
    Fixed(ObservedSite),
    /// This node owns live powered/locked properties.
    Electrical(NodeId),
    /// This node owns live powered/lit properties.
    Bulb(NodeId),
    /// This node owns wire properties, including any active classic signal view.
    Wire(NodeId),
    /// This component site retains its node receiver and samples the actor's live pose.
    Piston { node: NodeId, piston: PistonId },
    /// This contact's original ordered guards select its occupant on discovery.
    Contact(ContactId),
}

impl WireCapture {
    pub(crate) fn infer(graph: &CompileGraph, site: &SiteSpec) -> Self {
        match site.source {
            SiteSource::Constant(profile) => Self::Fixed(ObservedSite {
                profile: Some(profile),
                receiver: None,
                properties: match Block::from_id(graph.profiles[profile.index()].raw_state) {
                    Block::RedstoneWire { wire } => ObservedProperties::Wire(wire),
                    _ => ObservedProperties::Passive,
                },
            }),
            SiteSource::Contact(contact) => Self::Contact(contact),
            SiteSource::Component(node) => match &graph.nodes[node.index()].state {
                ComponentState::Wire(_) => Self::Wire(node),
                ComponentState::Electrical(_) => {
                    if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CopperBulb)) {
                        Self::Bulb(node)
                    } else {
                        Self::Electrical(node)
                    }
                }
                ComponentState::Specialized => match graph.nodes[node.index()].kind {
                    CircuitNodeKind::Piston(piston) => Self::Piston { node, piston },
                    _ => Self::Fixed(ObservedSite {
                        profile: None,
                        receiver: Some(CallbackReceiver::Component(node)),
                        properties: ObservedProperties::Passive,
                    }),
                },
            },
        }
    }
}
