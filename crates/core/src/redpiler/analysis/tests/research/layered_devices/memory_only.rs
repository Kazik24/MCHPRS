//! Shared RAM probes for transport and stable-logic comparisons.
use super::*;

// BubbleSort's frozen RAM decoder defines all four physical memory banks.
pub(super) fn memory_probes(graph: &CompileGraph, name: &str) -> Vec<(PistonId, BlockPos)> {
    let positions: Vec<_> = if name == "cpu_bubblesort" {
        [7, 17, 27, 37]
            .into_iter()
            .flat_map(|y| (0..16).flat_map(move |row| (0..12).map(move |bit| cpus::CPUS[2].origin + BlockPos::new(175 + 4 * bit, y, 62 - 4 * row))))
            .collect()
    } else if name == "counter" {
        graph
            .plans
            .bud_memory_reads
            .iter()
            .map(|read| graph.pistons[read.storage.index()].position)
            .collect()
    } else {
        let actors: BTreeSet<_> = graph
            .plans
            .instant_events
            .iter()
            .flat_map(|kernel| kernel.actors.iter().copied())
            .collect();
        graph
            .pistons
            .iter()
            .enumerate()
            .filter(|(id, _)| !actors.contains(&PistonId::new(*id as u32)))
            .map(|(_, piston)| piston.position)
            .collect()
    };
    positions
        .into_iter()
        .map(|position| {
            let id = graph
                .pistons
                .iter()
                .position(|piston| piston.position == position)
                .expect("memory piston");
            (PistonId::new(id as u32), position)
        })
        .collect()
}

pub(super) fn native_storage_bit(world: &PlotWorld, position: BlockPos) -> Option<bool> {
    match world.get_block(position) {
        Block::Piston { piston } => Some(!piston.extended),
        Block::MovingPiston { .. } => None,
        block => panic!("native storage at {position:?}: {block:?}"),
    }
}

pub(super) fn graph_storage_bit(backend: &GraphBackend, actor: PistonId) -> Option<bool> {
    let state = backend.circuit_state().pistons[actor.index()];
    (!state.availability.base_pending()).then_some(!state.extended)
}
