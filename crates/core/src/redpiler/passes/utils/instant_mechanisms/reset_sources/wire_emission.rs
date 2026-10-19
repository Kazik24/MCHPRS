//! Conservative emission over every compiled contact alternative, without choosing a saved pose.
use super::*;
use mchprs_blocks::BlockDirection;

/// `None` keeps an uncertain geometry alternative; only proven false can cut a reset path.
type Fact = Option<bool>;

pub(super) fn may_emit(graph: &CompileGraph, node: NodeId, port: PowerPort) -> bool {
    let face = match port {
        PowerPort::Stored | PowerPort::Output => return true,
        PowerPort::Weak(face) | PowerPort::Strong(face) => face,
        PowerPort::Conducted { .. } => return false,
    };
    if matches!(face, BlockFace::Top | BlockFace::Bottom) {
        return face == BlockFace::Top;
    }
    let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
        return false;
    };
    let neighbors = graph.wires[index].canonical_neighbors;
    let headroom = site_fact(graph, neighbors[3], Block::is_solid).map(|value| !value);
    let mut connected = [None; 4];
    for (index, (side, neighbor, above, below)) in [
        (BlockDirection::North, 4, 20, 17),
        (BlockDirection::South, 5, 21, 18),
        (BlockDirection::East, 1, 13, 12),
        (BlockDirection::West, 0, 8, 7),
    ]
    .into_iter()
    .enumerate()
    {
        let direct = site_fact(graph, neighbors[neighbor], |block| {
            crate::redstone::wire::can_connect_to_uncached(block, side)
        });
        let up = all([headroom, site_fact(graph, neighbors[above], is_wire)]);
        let open = site_fact(graph, neighbors[neighbor], Block::is_solid).map(|value| !value);
        let down = all([open, site_fact(graph, neighbors[below], is_wire)]);
        connected[index] = any([direct, up, down]);
    }
    let (side, perpendicular) = match face.unwrap_direction().opposite() {
        BlockDirection::North => (0, [2, 3]),
        BlockDirection::South => (1, [2, 3]),
        BlockDirection::East => (2, [0, 1]),
        BlockDirection::West => (3, [0, 1]),
    };
    let complete_axis = all(perpendicular.map(|side| connected[side].map(|value| !value)));
    let regulated = any([connected[side], complete_axis]);
    let ComponentState::Wire(saved) = graph.nodes[node.index()].state else {
        return false;
    };
    let emitted = if crate::redstone::wire::is_dot(saved) {
        all([regulated, any(connected)])
    } else {
        regulated
    };
    emitted != Some(false)
}

fn is_wire(block: Block) -> bool {
    matches!(block, Block::RedstoneWire { .. })
}

fn site_fact(graph: &CompileGraph, site: SiteId, predicate: impl Fn(Block) -> bool) -> Fact {
    match graph.sites[site.index()].source {
        SiteSource::Constant(profile) => Some(predicate(Block::from_id(graph.profiles[profile.index()].raw_state))),
        SiteSource::Component(node) => {
            let node = &graph.nodes[node.index()];
            let block = match node.state {
                ComponentState::Wire(wire) => Block::RedstoneWire { wire },
                _ => Block::from_id(node.raw_state?),
            };
            Some(predicate(block))
        }
        SiteSource::Contact(contact) => {
            let mut values = graph.contacts[contact.index()]
                .variants
                .iter()
                .map(|variant| predicate(Block::from_id(graph.profiles[variant.profile.index()].raw_state)));
            let first = values.next()?;
            values.all(|value| value == first).then_some(first)
        }
    }
}

fn all<const N: usize>(values: [Fact; N]) -> Fact {
    if values.contains(&Some(false)) {
        Some(false)
    } else if values.contains(&None) {
        None
    } else {
        Some(true)
    }
}

fn any<const N: usize>(values: [Fact; N]) -> Fact {
    if values.contains(&Some(true)) {
        Some(true)
    } else if values.contains(&None) {
        None
    } else {
        Some(false)
    }
}
