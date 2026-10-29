//! A bounded, opt-in truth table sampled from fresh physical Graph executions.
//! It describes instantaneous repeater and lamp inputs only, not callbacks or delayed transitions.
//! Fixed boundary values are sampled at entry; this does not prove exclusive ownership.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::AnalysisLimits;
use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct WireNetlist {
    sources: Vec<BlockPos>,
    receivers: Vec<BlockPos>,
    values: Vec<Vec<u8>>,
    mask: usize,
}

impl WireNetlist {
    /// Apply a level change and return the sampled input strengths in receiver order.
    pub(crate) fn set_input(&mut self, source: usize, powered: bool) -> Option<&[u8]> {
        if source >= self.sources.len() {
            return None;
        }
        let bit = 1 << source;
        self.mask = if powered { self.mask | bit } else { self.mask & !bit };
        Some(&self.values[self.mask])
    }

    pub(crate) fn outputs(&self) -> &[u8] {
        &self.values[self.mask]
    }

    pub(crate) fn receivers(&self) -> &[BlockPos] {
        &self.receivers
    }

    pub(crate) fn sources(&self) -> &[BlockPos] {
        &self.sources
    }

    pub(crate) fn samples(&self) -> &[Vec<u8>] {
        &self.values
    }

    pub(crate) fn input_mask(&self) -> usize {
        self.mask
    }
}

/// Sample every combination; no result is used to suppress physical Graph work.
pub(crate) fn probe(world: &PlotWorld, bounds: (BlockPos, BlockPos), sources: &[BlockPos], receivers: &[BlockPos]) -> Result<WireNetlist, String> {
    if sources.len() > 8 {
        return Err("probe supports at most eight sources".into());
    }
    let mut baseline = physical_graph(world, bounds)?;
    let mut unique = HashSet::new();
    for &position in sources {
        let node = *baseline.positions.get(&position).ok_or("source missing from graph")?;
        if !unique.insert(position) || !matches!(baseline.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever)) {
            return Err("probe sources must be distinct levers".into());
        }
    }
    unique.clear();
    let mut receiver_nodes = Vec::with_capacity(receivers.len());
    for &position in receivers {
        let node = *baseline.positions.get(&position).ok_or("receiver missing from graph")?;
        if !unique.insert(position)
            || !matches!(
                baseline.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp)
            )
            || baseline.node_state(node).repeater_locked
        {
            return Err("probe receivers must be distinct unlocked repeaters or lamps".into());
        }
        receiver_nodes.push(node);
    }
    for node in baseline.program.nodes() {
        match node.kind {
            CircuitNodeKind::Wire(_) | CircuitNodeKind::Electrical(NodeType::Wire | NodeType::Constant) => {}
            CircuitNodeKind::Electrical(NodeType::Lever) if node.position.is_some_and(|pos| sources.contains(&pos)) => {}
            CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp) if node.position.is_some_and(|pos| receivers.contains(&pos)) => {}
            _ => return Err("probe encountered an unsupported or undeclared component".into()),
        }
    }
    let entry_values: Vec<_> = receiver_nodes
        .iter()
        .map(|&node| baseline.input_strength(node, InputPort::Main))
        .collect();
    let mut initial_mask = 0usize;
    for (index, &source) in sources.iter().enumerate() {
        let node = baseline.positions[&source];
        if baseline.node_state(node).powered {
            initial_mask |= 1 << index;
            baseline.use_block(source).map_err(|error| error.to_string())?;
        }
    }
    check_probe_work(&baseline, &receiver_nodes)?;
    if receiver_nodes.iter().any(|&node| baseline.input_strength(node, InputPort::Main) != 0) {
        return Err("probe receiver has an undeclared or unsettled input".into());
    }

    let mut values = Vec::with_capacity(1 << sources.len());
    for mask in 0..1usize << sources.len() {
        let mut scratch = GraphBackend::new_unactivated(baseline.program.clone(), baseline.state.clone(), baseline.future.clock.logical_tick)
            .map_err(|error| error.to_string())?;
        for (index, &source) in sources.iter().enumerate() {
            if mask & (1 << index) != 0 {
                scratch.use_block(source).map_err(|error| error.to_string())?;
            }
        }
        check_probe_work(&scratch, &receiver_nodes)?;
        values.push(receiver_nodes.iter().map(|&node| scratch.input_strength(node, InputPort::Main)).collect());
    }
    if values[initial_mask] != entry_values {
        return Err("probe entry is not settled for its current inputs".into());
    }
    Ok(WireNetlist {
        sources: sources.to_vec(),
        receivers: receivers.to_vec(),
        values,
        mask: initial_mask,
    })
}

pub(crate) fn physical_graph(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Result<GraphBackend, String> {
    if world.piston_state().phase != mchprs_world::AdvancePhase::BetweenTicks
        || !world.piston_state().events.is_empty()
        || !world.piston_state().motions.is_empty()
        || world.scheduler().iter_entries().next().is_some()
    {
        return Err("probe requires a stationary world without pending work".into());
    }
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::default();
    let prepared = passes::rich::prepare(world, bounds, Vec::new(), &monitor, AnalysisLimits::default()).map_err(|error| error.to_string())?;
    let lowered = prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .map_err(|error| error.to_string())?
        .lower(&monitor)
        .map_err(|error| error.to_string())?;
    GraphBackend::validate_supported(&lowered.program).map_err(|error| error.to_string())?;
    if lowered.program.fixed_classic().is_some() {
        return Err("probe requires an unoptimized physical graph".into());
    }
    GraphBackend::new_unactivated(lowered.program, lowered.state, lowered.logical_tick).map_err(|error| error.to_string())
}

/// Return the validated O0 program and entry state for read-only topology discovery.
pub(crate) fn physical_program(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Result<(CircuitProgram, CircuitState, u64), String> {
    let graph = physical_graph(world, bounds)?;
    Ok((graph.program, graph.state, graph.future.clock.logical_tick))
}

fn check_probe_work(graph: &GraphBackend, receivers: &[NodeId]) -> Result<(), String> {
    if !graph.future.piston_requests.is_empty()
        || !graph.future.deferred_pistons.is_empty()
        || graph.counts.piston_actions != 0
        || graph
            .future
            .scheduler
            .iter()
            .any(|(tick, _, _)| !tick.node.is_some_and(|node| receivers.contains(&node)))
        || receivers.iter().any(|&node| graph.node_state(node).repeater_locked)
    {
        return Err("probe encountered unsupported scheduled, piston, or repeater-lock work".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::redstone;
    use crate::world::storage::Chunk;
    use mchprs_blocks::BlockDirection;
    use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneRepeater};

    pub(crate) const LEFT: BlockPos = BlockPos::new(3, 8, 8);
    pub(crate) const RIGHT: BlockPos = BlockPos::new(11, 8, 8);
    pub(crate) const RECEIVER: BlockPos = BlockPos::new(6, 8, 9);
    pub(crate) const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(1, 6, 6), BlockPos::new(14, 10, 11));

    fn set_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
        let Block::Lever { mut lever } = world.get_block(position) else {
            panic!("expected lever")
        };
        lever.powered = powered;
        world.set_block(position, Block::Lever { lever });
        redstone::update_surrounding_blocks(world, position);
        redstone::update_surrounding_blocks(world, position.offset(BlockFace::Bottom));
    }

    pub(crate) fn fixture() -> PlotWorld {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        for x in 3..=11 {
            world.set_block(BlockPos::new(x, 7, 8), Block::Stone {});
        }
        world.set_block(RECEIVER.offset(BlockFace::Bottom), Block::Stone {});
        for x in 4..=10 {
            world.set_block(BlockPos::new(x, 8, 8), Block::from_name("redstone_wire").unwrap());
        }
        for position in [LEFT, RIGHT] {
            world.set_block(
                position,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
        }
        world.set_block(
            RECEIVER,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay: 1,
                    facing: BlockDirection::North,
                    locked: false,
                    powered: false,
                },
            },
        );
        for x in 4..=10 {
            let pos = BlockPos::new(x, 8, 8);
            let wire = redstone::wire::get_state_for_placement(&world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        set_lever(&mut world, LEFT, true);
        set_lever(&mut world, RIGHT, true);
        world.clear_scheduled_ticks();
        world
    }

    #[test]
    fn physical_graph_probe_finds_each_unmasked_source_and_matches_native_strength() {
        let world = fixture();
        let Block::RedstoneWire { wire } = world.get_block(BlockPos::new(6, 8, 8)) else {
            panic!("expected wire")
        };
        assert_eq!(wire.power, 13, "the stronger source masks the other in the live world");
        let original: Vec<_> = (3..=11).map(|x| world.get_block_raw(BlockPos::new(x, 8, 8))).collect();
        let original_ticks: Vec<_> = world.scheduler().iter_entries().collect();
        let mut actual = probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER]).unwrap();
        assert_eq!(actual.receivers(), &[RECEIVER]);
        assert_eq!(actual.outputs(), &[13], "entry contains both active sources");
        let mut expected = Vec::new();
        for mask in 0..4 {
            let mut scratch = world.activation_snapshot();
            set_lever(&mut scratch, LEFT, false);
            set_lever(&mut scratch, RIGHT, false);
            let Block::RedstoneWire { wire } = scratch.get_block(BlockPos::new(6, 8, 8)) else {
                panic!("expected wire")
            };
            assert_eq!(wire.power, 0, "quiet baseline");
            if mask & 1 != 0 {
                set_lever(&mut scratch, LEFT, true);
            }
            if mask & 2 != 0 {
                set_lever(&mut scratch, RIGHT, true);
            }
            let Block::RedstoneWire { wire } = scratch.get_block(BlockPos::new(6, 8, 8)) else {
                panic!("expected wire")
            };
            expected.push(wire.power);
            actual.set_input(0, mask & 1 != 0).unwrap();
            assert_eq!(actual.set_input(1, mask & 2 != 0).unwrap(), &[wire.power], "mask {mask}");
        }
        assert_eq!(expected, [0, 13, 11, 13]);
        assert!(actual.set_input(2, true).is_none());
        assert_eq!(
            (3..=11).map(|x| world.get_block_raw(BlockPos::new(x, 8, 8))).collect::<Vec<_>>(),
            original
        );
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), original_ticks);
    }

    #[test]
    fn physical_graph_probe_rejects_undeclared_power_and_pending_work() {
        let mut world = fixture();
        let external = BlockPos::new(6, 8, 7);
        world.set_block(external, Block::RedstoneBlock);
        redstone::update(world.get_block(BlockPos::new(6, 8, 8)), &mut world, BlockPos::new(6, 8, 8), None);
        world.clear_scheduled_ticks();
        assert!(probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER]).unwrap_err().contains("undeclared"));
        world.schedule_tick(RECEIVER, 2, TickPriority::High);
        let before: Vec<_> = world.scheduler().iter_entries().collect();
        assert!(probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER]).unwrap_err().contains("pending"));
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before);
    }

    #[test]
    fn physical_graph_probe_rejects_stateful_components_and_excess_sources() {
        let mut world = fixture();
        world.set_block(BlockPos::new(13, 8, 10), Block::from_name("observer").unwrap());
        assert!(probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER]).is_err());
        assert!(probe(&world, BOUNDS, &[LEFT; 9], &[RECEIVER]).unwrap_err().contains("at most eight"));
    }

    #[test]
    fn physical_graph_probe_samples_lamp_inputs_against_native() {
        let mut world = fixture();
        let lamp = BlockPos::new(9, 7, 8);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let wire_pos = BlockPos::new(9, 8, 8);
        let wire = redstone::wire::get_state_for_placement(&world, wire_pos);
        world.set_block(wire_pos, Block::RedstoneWire { wire });
        set_lever(&mut world, LEFT, true);
        set_lever(&mut world, RIGHT, true);
        world.clear_scheduled_ticks();

        let mut table = probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER, lamp]).unwrap();
        let mut observed_high = false;
        for mask in 0..4 {
            let mut scratch = world.activation_snapshot();
            set_lever(&mut scratch, LEFT, false);
            set_lever(&mut scratch, RIGHT, false);
            if mask & 1 != 0 {
                set_lever(&mut scratch, LEFT, true);
            }
            if mask & 2 != 0 {
                set_lever(&mut scratch, RIGHT, true);
            }
            let native = redstone::redstone_lamp_should_be_lit(&scratch, lamp);
            table.set_input(0, mask & 1 != 0).unwrap();
            let values = table.set_input(1, mask & 2 != 0).unwrap();
            assert_eq!(values[1] > 0, native, "mask {mask}");
            observed_high |= native;
        }
        assert!(observed_high, "lamp fixture must exercise a powered input");
    }
}
