//! Finite local shape closure, independent of the circuit's data-input valuations.
use super::*;
use crate::redpiler::program_graph::{WireShapeMemory as ShapeMemory, wire_sides as sides};

fn wire(value: [RedstoneWireSide; 4]) -> RedstoneWire {
    RedstoneWire {
        north: value[0],
        south: value[1],
        east: value[2],
        west: value[3],
        power: 0,
    }
}

fn alternatives(graph: &CompileGraph, site: SiteId) -> Vec<Block> {
    match graph.sites[site.index()].source {
        SiteSource::Component(node) => vec![Block::from_id(graph.nodes[node.index()].raw_state.expect("validated electrical site"))],
        SiteSource::Constant(profile) => vec![Block::from_id(graph.profiles[profile.index()].raw_state)],
        SiteSource::Contact(contact) => graph.contacts[contact.index()]
            .variants
            .iter()
            .map(|variant| Block::from_id(graph.profiles[variant.profile.index()].raw_state))
            .collect(),
    }
}

fn booleans(graph: &CompileGraph, site: SiteId, fact: impl Fn(Block) -> bool) -> Vec<bool> {
    let mut values = Vec::new();
    for block in alternatives(graph, site) {
        let value = fact(block);
        if !values.contains(&value) {
            values.push(value);
        }
    }
    values
}

fn raw_shapes(graph: &CompileGraph, plan: &WirePlan) -> Vec<[RedstoneWireSide; 4]> {
    // Independent profile facts may add impossible combinations, but never omit a legal local geometry.
    let above_solid = booleans(graph, plan.canonical_neighbors[3], |block| block.is_solid());
    let mut choices: [Vec<RedstoneWireSide>; 4] = std::array::from_fn(|_| Vec::new());
    for (side, (neighbor, above, below)) in [(4, 20, 17), (5, 21, 18), (1, 13, 12), (0, 8, 7)].into_iter().enumerate() {
        let adjacent = alternatives(graph, plan.canonical_neighbors[neighbor]);
        let above_wire = booleans(graph, plan.canonical_neighbors[above], |block| {
            matches!(block, Block::RedstoneWire { .. })
        });
        let below_wire = booleans(graph, plan.canonical_neighbors[below], |block| {
            matches!(block, Block::RedstoneWire { .. })
        });
        for &solid in &above_solid {
            for &block in &adjacent {
                for &up in &above_wire {
                    for &down in &below_wire {
                        let mut neighbors = [Block::Air; 24];
                        neighbors[3] = if solid { Block::Stone {} } else { Block::Air };
                        neighbors[neighbor] = block;
                        if up {
                            neighbors[above] = Block::RedstoneWire { wire: Default::default() };
                        }
                        if down {
                            neighbors[below] = Block::RedstoneWire { wire: Default::default() };
                        }
                        let raw = crate::redstone::wire::get_raw_sides_from_neighbors(RedstoneWire::default(), &neighbors);
                        let value = sides(raw)[side];
                        if !choices[side].contains(&value) {
                            choices[side].push(value);
                        }
                    }
                }
            }
        }
    }
    // This is at most 3^4 local shapes; it never enumerates the circuit's operand masks.
    let mut shapes = vec![[RedstoneWireSide::None; 4]];
    for (index, choices) in choices.iter().enumerate() {
        let mut next = Vec::new();
        for shape in shapes {
            for &choice in choices {
                let mut shape = shape;
                shape[index] = choice;
                next.push(shape);
            }
        }
        shapes = next;
    }
    shapes
}

pub(super) fn infer(compiler: &mut Compiler<'_>) -> Result<Vec<ShapeMemory>> {
    let graph = compiler.graph;
    let mut memories = Vec::new();
    for plan in &graph.wires {
        let ComponentState::Wire(initial) = graph.nodes[plan.node.index()].state else {
            return Err(CompileError::UnsupportedNode(plan.node));
        };
        let raw = raw_shapes(graph, plan);
        let mut values = vec![sides(initial)];
        let mut index = 0;
        while index < values.len() {
            let current = wire(values[index]);
            for &shape in &raw {
                for face in BlockFace::values() {
                    compiler.step()?;
                    let updated = crate::redstone::wire::on_neighbor_changed_from(current, wire(shape), face);
                    let updated = sides(updated);
                    if !values.contains(&updated) {
                        values.push(updated);
                    }
                }
            }
            index += 1;
        }
        values.sort_by_key(|shape| shape.map(|side| side as u8));
        memories.push(ShapeMemory { node: plan.node, values });
    }
    Ok(memories)
}
