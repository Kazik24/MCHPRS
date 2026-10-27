//! Prove raw shapes and directional connectivity across every selectable contact alternative.
use super::*;
use mchprs_blocks::{BlockDirection, blocks::RedstoneWireSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Invariant geometry queries; live power and any unresolved connections retain their owners.
pub(crate) struct WireShape {
    /// Packed N/S/E/W raw connections when every side is fixed.
    pub(crate) raw: Option<u8>,
    /// Possible side ids in N/S/E/W order, preserving ambiguity for exact proofs.
    raw_outcomes: [u8; 4],
    /// Directions whose regulated connectivity is fixed for both dot and non-dot inputs.
    pub(crate) emitted_known: u8,
    /// Connected directions among the known emission bits, in N/S/E/W order.
    pub(crate) emitted: u8,
    /// Fixed solidity for the overhead block, then N/S/E/W horizontal neighbors.
    solid_known: u8,
    solid: u8,
    /// Fixed wire classes for N/S/E/W upper and lower diagonal pairs.
    diagonal_known: u8,
    diagonal: u8,
    /// Fixed direct connections, in N/S/E/W order.
    connects_known: u8,
    connects: u8,
}

impl WireShape {
    /// Prove that no native shape callback direction can change this canonical wire.
    ///
    /// The raw-side inference ranges over every selectable contact variant. Every
    /// Cartesian raw shape must preserve the canonical entry and each callback
    /// direction must leave all legal strengths unchanged.
    /// Callers must still retain any distinct Update/Turbo deliveries for this
    /// wire; this certificate suppresses only Shape occurrences.
    pub(crate) fn shape_callbacks_are_inert(graph: &CompileGraph, node: NodeId) -> bool {
        let Some(node_spec) = graph.nodes.get(node.index()) else {
            return false;
        };
        let CircuitNodeKind::Wire(wire_index) = &node_spec.kind else {
            return false;
        };
        let ComponentState::Wire(saved) = &node_spec.state else {
            return false;
        };
        let Some(wire_plan) = graph.wires.get(*wire_index).filter(|plan| plan.node == node) else {
            return false;
        };
        let Some(shape) = Self::infer(graph, wire_plan) else {
            return false;
        };
        if shape.raw_outcomes.contains(&0) {
            return false;
        }

        let directions = [
            BlockFace::Top,
            BlockFace::Bottom,
            BlockFace::North,
            BlockFace::South,
            BlockFace::East,
            BlockFace::West,
        ];
        let mut saw_raw_pattern = false;
        for pattern in 0..81 {
            let mut value = pattern;
            let sides: [u8; 4] = std::array::from_fn(|_| {
                let side = (value % 3) as u8;
                value /= 3;
                side
            });
            if sides.iter().zip(shape.raw_outcomes).any(|(&side, outcomes)| outcomes & (1 << side) == 0) {
                continue;
            }
            saw_raw_pattern = true;
            let raw_shape = RedstoneWire::new(
                RedstoneWireSide::from_id(sides[0].into()),
                RedstoneWireSide::from_id(sides[1].into()),
                RedstoneWireSide::from_id(sides[2].into()),
                RedstoneWireSide::from_id(sides[3].into()),
                saved.power,
            );
            if crate::redstone::wire::regulate_sides(*saved, raw_shape) != *saved {
                return false;
            }
            for direction in directions {
                for power in 0..=15 {
                    let current = RedstoneWire { power, ..*saved };
                    let raw = RedstoneWire { power, ..raw_shape };
                    if crate::redstone::wire::on_neighbor_changed_from(current, raw, direction) != current {
                        return false;
                    }
                }
            }
        }
        saw_raw_pattern
    }

    pub(crate) fn infer(graph: &CompileGraph, wire: &WirePlan) -> Option<Self> {
        let property = |slot: usize, test: fn(Block) -> bool| possible(graph, wire.canonical_neighbors[slot], test);
        let up = property(3, |block| block.is_solid());
        let mut outcomes = [0u8; 4];
        let mut result = Self {
            raw: None,
            raw_outcomes: [0; 4],
            emitted_known: 0,
            emitted: 0,
            solid_known: 0,
            solid: 0,
            diagonal_known: 0,
            diagonal: 0,
            connects_known: 0,
            connects: 0,
        };
        record_property(up, 0, &mut result.solid_known, &mut result.solid);
        for (index, (direction, neighbor, above, below)) in [
            (BlockDirection::North, 4, 20, 17),
            (BlockDirection::South, 5, 21, 18),
            (BlockDirection::East, 1, 13, 12),
            (BlockDirection::West, 0, 8, 7),
        ]
        .into_iter()
        .enumerate()
        {
            let connects = possible(graph, wire.canonical_neighbors[neighbor], |block| {
                crate::redstone::wire::can_connect_to_uncached(block, direction)
            });
            let solid = property(neighbor, |block| block.is_solid());
            let above = property(above, |block| matches!(block, Block::RedstoneWire { .. }));
            let below = property(below, |block| matches!(block, Block::RedstoneWire { .. }));
            record_property(solid, index + 1, &mut result.solid_known, &mut result.solid);
            record_property(above, index * 2, &mut result.diagonal_known, &mut result.diagonal);
            record_property(below, index * 2 + 1, &mut result.diagonal_known, &mut result.diagonal);
            record_property(connects, index, &mut result.connects_known, &mut result.connects);
            outcomes[index] = side_outcomes(connects, up, above, solid, below);
        }
        result.raw_outcomes = outcomes;
        let raw = outcomes.iter().all(|mask| mask.count_ones() == 1).then(|| {
            outcomes
                .iter()
                .enumerate()
                .fold(0, |bits, (index, mask)| bits | (mask.trailing_zeros() as u8) << (index * 2))
        });
        let mut possible_emitted = [0u8; 4];
        for pattern in 0..81 {
            let mut value = pattern;
            let sides: [u8; 4] = std::array::from_fn(|_| {
                let side = value % 3;
                value /= 3;
                side
            });
            if sides.iter().zip(outcomes).any(|(&side, mask)| mask & (1 << side) == 0) {
                continue;
            }
            let raw_wire = RedstoneWire::new(
                RedstoneWireSide::from_id(sides[0].into()),
                RedstoneWireSide::from_id(sides[1].into()),
                RedstoneWireSide::from_id(sides[2].into()),
                RedstoneWireSide::from_id(sides[3].into()),
                0,
            );
            for held in [
                RedstoneWire::default(),
                RedstoneWire::new(
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    0,
                ),
            ] {
                let wire = crate::redstone::wire::regulate_sides(held, raw_wire);
                for (mask, side) in possible_emitted.iter_mut().zip([wire.north, wire.south, wire.east, wire.west]) {
                    *mask |= 1 << u8::from(!side.is_none());
                }
            }
        }
        result.raw = raw;
        for (index, mask) in possible_emitted.into_iter().enumerate() {
            if mask.count_ones() == 1 {
                result.emitted_known |= 1 << index;
                if mask == 2 {
                    result.emitted |= 1 << index;
                }
            }
        }
        (result.raw.is_some() || result.emitted_known != 0 || result.solid_known != 0 || result.diagonal_known != 0 || result.connects_known != 0)
            .then_some(result)
    }

    pub(crate) fn raw_wire(self, power: u8) -> Option<RedstoneWire> {
        let bits = self.raw?;
        let side = |shift: u32| RedstoneWireSide::from_id(u32::from((bits >> shift) & 3u8));
        Some(RedstoneWire::new(side(0), side(2), side(4), side(6), power))
    }

    pub(crate) fn emits(self, direction: BlockDirection) -> Option<bool> {
        let index = match direction {
            BlockDirection::North => 0,
            BlockDirection::South => 1,
            BlockDirection::East => 2,
            BlockDirection::West => 3,
        };
        (self.emitted_known & (1 << index) != 0).then_some(self.emitted & (1 << index) != 0)
    }

    pub(crate) fn raw_wire_from(self, mut wire: RedstoneWire, read: impl Fn(usize) -> Block) -> RedstoneWire {
        let property = |known: u8, values: u8, bit: usize| (known & (1 << bit) != 0).then_some(values & (1 << bit) != 0);
        let mut up_solid = property(self.solid_known, self.solid, 0);
        for (index, (direction, neighbor, above, below)) in [
            (BlockDirection::North, 4, 20, 17),
            (BlockDirection::South, 5, 21, 18),
            (BlockDirection::East, 1, 13, 12),
            (BlockDirection::West, 0, 8, 7),
        ]
        .into_iter()
        .enumerate()
        {
            let mut sampled = None;
            let connects = property(self.connects_known, self.connects, index).unwrap_or_else(|| {
                let block = read(neighbor);
                sampled = Some(block);
                crate::world::wire_cache::connects(block, direction)
            });
            let solid = property(self.solid_known, self.solid, index + 1);
            let side = if connects {
                RedstoneWireSide::Side
            } else if up_solid != Some(true)
                && property(self.diagonal_known, self.diagonal, index * 2).unwrap_or_else(|| matches!(read(above), Block::RedstoneWire { .. }))
                && !*up_solid.get_or_insert_with(|| read(3).is_solid())
            {
                RedstoneWireSide::Up
            } else if solid != Some(true)
                && property(self.diagonal_known, self.diagonal, index * 2 + 1).unwrap_or_else(|| matches!(read(below), Block::RedstoneWire { .. }))
                && !solid.unwrap_or_else(|| sampled.unwrap_or_else(|| read(neighbor)).is_solid())
            {
                RedstoneWireSide::Side
            } else {
                RedstoneWireSide::None
            };
            match direction {
                BlockDirection::North => wire.north = side,
                BlockDirection::South => wire.south = side,
                BlockDirection::East => wire.east = side,
                BlockDirection::West => wire.west = side,
            }
        }
        wire
    }
}

fn record_property(possible: u8, bit: usize, known: &mut u8, values: &mut u8) {
    if possible.count_ones() == 1 {
        *known |= 1 << bit;
        if possible == 2 {
            *values |= 1 << bit;
        }
    }
}

// Property masks enumerate both answers when a state-dependent class is unsupported.
fn possible(graph: &CompileGraph, site: SiteId, test: impl Fn(Block) -> bool) -> u8 {
    let component = |node: NodeId| {
        let node = &graph.nodes[node.index()];
        match (&node.kind, &node.state) {
            (CircuitNodeKind::Wire(_), ComponentState::Wire(wire)) => Some(Block::RedstoneWire { wire: *wire }),
            (CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }, _) => {
                node.raw_state.map(Block::from_id)
            }
            _ => None,
        }
    };
    let answer = |block: Option<Block>| block.map_or(3, |block| 1 << u8::from(test(block)));
    match graph.sites[site.index()].source {
        SiteSource::Constant(profile) => answer(Some(Block::from_id(graph.profiles[profile.index()].raw_state))),
        SiteSource::Component(node) => answer(component(node)),
        SiteSource::Contact(contact) => {
            let variants = &graph.contacts[contact.index()].variants;
            if variants.is_empty() {
                return 3;
            }
            variants.iter().fold(0, |mask, variant| {
                mask | answer(match variant.receiver {
                    Some(CallbackReceiver::Component(node)) => component(node),
                    _ => Some(Block::from_id(graph.profiles[variant.profile.index()].raw_state)),
                })
            })
        }
    }
}

fn side_outcomes(connects: u8, up: u8, above: u8, solid: u8, below: u8) -> u8 {
    let choose = |condition: u8, yes: u8, no: u8| (if condition & 2 != 0 { yes } else { 0 }) | (if condition & 1 != 0 { no } else { 0 });
    let ascent = choose(up, 1, above);
    let descent = choose(solid, 1, below);
    choose(
        connects,
        1 << RedstoneWireSide::Side.get_id(),
        choose(
            ascent,
            1 << RedstoneWireSide::Up.get_id(),
            choose(descent, 1 << RedstoneWireSide::Side.get_id(), 1 << RedstoneWireSide::None.get_id()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::plot::PlotWorld;
    use crate::redpiler::passes;
    use crate::world::World;
    use crate::world::storage::Chunk;

    const ROOT: BlockPos = BlockPos::new(8, 20, 8);

    fn fixed_dot_graph() -> CompileGraph {
        graph_with_neighbor_wires(&[])
    }

    fn graph_with_neighbor_wires(neighbors: &[BlockFace]) -> CompileGraph {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(
            ROOT,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        world.set_block(ROOT.offset(BlockFace::Bottom), Block::Stone {});
        for &face in neighbors {
            let position = ROOT.offset(face);
            world.set_block(
                position,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        }
        passes::rich::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &crate::redpiler::TaskMonitor::default(),
            crate::redpiler::analysis::AnalysisLimits::default(),
        )
        .unwrap()
        .graph
    }

    fn root_node(graph: &CompileGraph) -> NodeId {
        graph
            .wires
            .iter()
            .find(|wire| graph.nodes[wire.node.index()].position == Some(ROOT))
            .unwrap()
            .node
    }

    fn test_profile(graph: &mut CompileGraph, block: Block) -> ProfileId {
        let profile = ProfileId::new(graph.profiles.len() as u32);
        graph.profiles.push(Profile {
            raw_state: block.get_id(),
            registry_type: block.registry_id(),
            solid: block.is_solid(),
            transparent: block.is_transparent(),
            cube: block.is_cube(),
            electrical: ProfileElectrical::None,
        });
        profile
    }

    fn set_contact_variants(graph: &mut CompileGraph, site: SiteId, blocks: &[Block]) {
        let position = graph.sites[site.index()].position;
        let variants = blocks
            .iter()
            .copied()
            .map(|block| ContactVariant {
                when: GuardId::new(0),
                profile: test_profile(graph, block),
                receiver: None,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let contact = ContactId::new(graph.contacts.len() as u32);
        graph.contacts.push(ContactSpec {
            node: NodeId::new(graph.nodes.len() as u32),
            position,
            variants,
        });
        graph.sites[site.index()].source = SiteSource::Contact(contact);
    }

    #[test]
    fn conservative_side_masks_match_all_concrete_combinations() {
        for connects in 1..=3 {
            for up in 1..=3 {
                for above in 1..=3 {
                    for solid in 1..=3 {
                        for below in 1..=3 {
                            let mut expected = 0;
                            for values in 0..32 {
                                let values: [bool; 5] = std::array::from_fn(|i| values & (1 << i) != 0);
                                if [connects, up, above, solid, below]
                                    .into_iter()
                                    .zip(values)
                                    .any(|(mask, value)| mask & (1 << u8::from(value)) == 0)
                                {
                                    continue;
                                }
                                let [connects, up, above, solid, below] = values;
                                let side = if connects {
                                    RedstoneWireSide::Side
                                } else if !up && above {
                                    RedstoneWireSide::Up
                                } else if !solid && below {
                                    RedstoneWireSide::Side
                                } else {
                                    RedstoneWireSide::None
                                };
                                expected |= 1 << side.get_id();
                            }
                            assert_eq!(side_outcomes(connects, up, above, solid, below), expected);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fixed_dot_shape_callbacks_are_inert_for_every_direction_and_wire_power() {
        let graph = fixed_dot_graph();
        let node = root_node(&graph);
        let shape = WireShape::infer(
            &graph,
            &graph.wires[match &graph.nodes[node.index()].kind {
                CircuitNodeKind::Wire(index) => *index,
                _ => unreachable!(),
            }],
        )
        .unwrap();
        assert!(shape.raw.is_some());
        assert!(WireShape::shape_callbacks_are_inert(&graph, node));
    }

    #[test]
    fn fixed_straight_and_corner_shape_callbacks_are_inert() {
        for (neighbors, expected_sides) in [
            (
                vec![BlockFace::North, BlockFace::South],
                [
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::None,
                    RedstoneWireSide::None,
                ],
            ),
            (
                vec![BlockFace::North, BlockFace::East],
                [
                    RedstoneWireSide::Side,
                    RedstoneWireSide::None,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::None,
                ],
            ),
        ] {
            let mut graph = graph_with_neighbor_wires(&neighbors);
            let node = root_node(&graph);
            let CircuitNodeKind::Wire(wire_index) = &graph.nodes[node.index()].kind else {
                unreachable!();
            };
            let raw = WireShape::infer(&graph, &graph.wires[*wire_index])
                .and_then(|shape| shape.raw_wire(0))
                .expect("fixed neighbor wires determine all raw sides");
            assert_eq!(
                [raw.north, raw.south, raw.east, raw.west],
                expected_sides,
                "the fixture should cover a canonical straight or corner",
            );
            graph.nodes[node.index()].state = ComponentState::Wire(raw);
            assert!(WireShape::shape_callbacks_are_inert(&graph, node));
        }
    }

    #[test]
    fn variable_raw_side_can_be_inert_after_regulation() {
        let mut graph = graph_with_neighbor_wires(&[BlockFace::East]);
        let node = root_node(&graph);
        let CircuitNodeKind::Wire(wire_index) = &graph.nodes[node.index()].kind else {
            unreachable!();
        };
        let wire_index = *wire_index;
        let west = graph.wires[wire_index].canonical_neighbors[0];
        set_contact_variants(
            &mut graph,
            west,
            &[
                Block::Air {},
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            ],
        );
        graph.nodes[node.index()].state = ComponentState::Wire(RedstoneWire::new(
            RedstoneWireSide::None,
            RedstoneWireSide::None,
            RedstoneWireSide::Side,
            RedstoneWireSide::Side,
            0,
        ));

        let shape = WireShape::infer(&graph, &graph.wires[wire_index]).unwrap();
        assert!(shape.raw.is_none(), "the west raw side varies by contact alternative");
        assert_eq!(
            shape.raw_outcomes[3],
            (1 << RedstoneWireSide::Side.get_id()) | (1 << RedstoneWireSide::None.get_id()),
            "the ambiguous side should include both disconnected and connected forms",
        );
        assert!(WireShape::shape_callbacks_are_inert(&graph, node));
    }

    #[test]
    fn shape_callback_declines_variable_contact_geometry_and_stale_entry_sides() {
        let mut graph = fixed_dot_graph();
        let node = root_node(&graph);
        let CircuitNodeKind::Wire(wire_index) = &graph.nodes[node.index()].kind else {
            unreachable!();
        };
        let wire_index = *wire_index;
        let neighbor = graph.wires[wire_index].canonical_neighbors[1];
        set_contact_variants(
            &mut graph,
            neighbor,
            &[
                Block::Air {},
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            ],
        );
        assert!(!WireShape::shape_callbacks_are_inert(&graph, node));

        let mut stale = fixed_dot_graph();
        let stale_node = root_node(&stale);
        let ComponentState::Wire(saved) = &stale.nodes[stale_node.index()].state else {
            unreachable!();
        };
        stale.nodes[stale_node.index()].state = ComponentState::Wire(RedstoneWire {
            west: RedstoneWireSide::Side,
            ..*saved
        });
        assert!(!WireShape::shape_callbacks_are_inert(&stale, stale_node));
    }
}
