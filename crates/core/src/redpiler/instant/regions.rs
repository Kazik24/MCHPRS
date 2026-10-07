//! Wave regions follow moving geometry and sampling, rather than plot borders.
use crate::redpiler::analysis::{
    families,
    topology::{SourceKind, Topology},
    AnalysisReport,
};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use petgraph::unionfind::UnionFind;
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) fn split(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
) -> Result<Vec<AnalysisReport>, String> {
    let mut regions = UnionFind::new(report.pistons.len());
    let mut owners: FxHashMap<BlockPos, usize> = FxHashMap::default();
    let mut reads: FxHashMap<BlockPos, Vec<usize>> = FxHashMap::default();
    for (actor, piston) in report.pistons.iter().enumerate() {
        for pos in [piston.pos, piston.head, piston.payload]
            .into_iter()
            .chain(families::reset_positions(&report.recognition[actor]))
        {
            if let Some(other) = owners.insert(pos, actor) {
                regions.union(actor, other);
            }
        }
    }
    for &pos in &report.observers {
        let Block::Observer { observer } = world.get_block(pos) else {
            unreachable!()
        };
        let target = pos.offset(BlockFace::from(observer.facing));
        if let Some(actor) = report
            .pistons
            .iter()
            .position(|p| p.pos == target || p.pos.offset(BlockFace::Top) == pos)
        {
            if let Some(other) = owners.insert(pos, actor) {
                regions.union(actor, other);
            }
        } else {
            return Err(format!("observer at {pos:?} has no supported region owner"));
        }
    }
    let mobile = report
        .payload_groups
        .iter()
        .enumerate()
        .flat_map(|(id, g)| g.positions.iter().map(move |&pos| (pos, id)))
        .collect();
    let mut topology = Topology::new(
        world,
        report.bounds,
        monitor,
        crate::redpiler::analysis::AnalysisLimits::for_budget(monitor.budget_multiplier())
            .max_dependency_steps,
        mobile,
    );
    let mut update_inputs = FxHashMap::default();
    for (actor, recognition) in report.recognition.iter().enumerate() {
        for pos in recognition
            .inputs
            .wires
            .iter()
            .copied()
            .chain(recognition.inputs.sources.iter().map(|d| d.source))
        {
            reads.entry(pos).or_default().push(actor);
        }
        // Sampling is independent of power: connect BUD cells to their clock.
        for update in &report.ports.pistons[actor].updates {
            reads.entry(update.source).or_default().push(actor);
            if matches!(world.get_block(update.source), Block::RedstoneWire { .. }) {
                if !update_inputs.contains_key(&update.source) {
                    update_inputs.insert(
                        update.source,
                        topology
                            .wire_inputs(update.source)
                            .map_err(|e| e.to_string())?,
                    );
                }
                let dependencies = &update_inputs[&update.source];
                for pos in dependencies
                    .wires
                    .iter()
                    .copied()
                    .chain(dependencies.sources.iter().map(|d| d.source))
                {
                    reads.entry(pos).or_default().push(actor);
                }
            }
        }
    }
    for group in &report.payload_groups {
        for &member in &group.members[1..] {
            regions.union(group.members[0], member);
        }
    }
    for output in &report.ports.outputs {
        let actor = report.payload_groups[output.group].members[0];
        for pos in output
            .dependencies
            .wires
            .iter()
            .copied()
            .chain(output.dependencies.sources.iter().map(|d| d.source))
        {
            reads.entry(pos).or_default().push(actor);
        }
    }
    for (&pos, &owner) in &owners {
        if let Some(actors) = reads.get(&pos) {
            for &actor in actors {
                regions.union(owner, actor);
            }
        }
    }
    // ponytail: conservatively join moving geometry within two cells of a
    // dependency; use exact conditional footprints if close independent clocks need splitting.
    for piston in &report.pistons {
        let actor = owners[&piston.pos];
        for pos in [piston.pos, piston.head, piston.payload] {
            for face in BlockFace::values() {
                for neighbor in std::iter::once(pos.offset(face)).chain(
                    BlockFace::values()
                        .into_iter()
                        .map(|other| pos.offset(face).offset(other)),
                ) {
                    if let Some(actors) = reads.get(&neighbor) {
                        for &other in actors {
                            regions.union(actor, other);
                        }
                    }
                }
            }
        }
    }
    let mut members: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for actor in 0..report.pistons.len() {
        members.entry(regions.find(actor)).or_default().push(actor);
    }
    let mut members: Vec<_> = members.into_values().collect();
    members.sort_by_key(|actors| actors[0]);
    if members.len() == 1 {
        return Ok(vec![report.clone()]);
    }
    Ok(members
        .into_iter()
        .map(|actors| {
            let actor_map: FxHashMap<_, _> = actors
                .iter()
                .enumerate()
                .map(|(new, &old)| (old, new))
                .collect();
            let groups: Vec<_> = report
                .payload_groups
                .iter()
                .enumerate()
                .filter(|(_, g)| actor_map.contains_key(&g.members[0]))
                .map(|(id, _)| id)
                .collect();
            let group_map: FxHashMap<_, _> = groups
                .iter()
                .enumerate()
                .map(|(new, &old)| (old, new))
                .collect();
            let remap = |inputs: &mut crate::redpiler::analysis::topology::PowerDependencies| {
                for dependency in &mut inputs.sources {
                    if let SourceKind::MobilePayload { group } = &mut dependency.kind {
                        *group = group_map[group];
                    }
                }
            };
            let mut region = report.clone();
            region.pistons = actors
                .iter()
                .map(|&id| report.pistons[id].clone())
                .collect();
            region.payload_groups = groups
                .iter()
                .map(|&id| {
                    let mut group = report.payload_groups[id].clone();
                    group.members = group.members.iter().map(|id| actor_map[id]).collect();
                    group
                })
                .collect();
            region.recognition = actors
                .iter()
                .map(|&id| {
                    let mut recognition = report.recognition[id].clone();
                    recognition.piston = actor_map[&id];
                    remap(&mut recognition.inputs);
                    recognition
                })
                .collect();
            region.group_recognition = groups
                .iter()
                .map(|&id| {
                    let mut group = report.group_recognition[id].clone();
                    group.group = group_map[&id];
                    group
                })
                .collect();
            region.ports.pistons = actors
                .iter()
                .map(|&id| {
                    let mut ports = report.ports.pistons[id].clone();
                    ports.piston = actor_map[&id];
                    ports
                })
                .collect();
            region
                .ports
                .connections
                .retain(|c| actor_map.contains_key(&c.to_piston));
            for c in &mut region.ports.connections {
                c.to_piston = actor_map[&c.to_piston];
                c.from_group = group_map[&c.from_group];
            }
            region
                .ports
                .outputs
                .retain(|o| group_map.contains_key(&o.group));
            for o in &mut region.ports.outputs {
                o.group = group_map[&o.group];
                remap(&mut o.dependencies);
            }
            let positions: FxHashSet<_> = owners
                .iter()
                .filter_map(|(&pos, owner)| actor_map.contains_key(owner).then_some(pos))
                .collect();
            region.observers.retain(|pos| positions.contains(pos));
            region
                .ports
                .reset_exposures
                .retain(|e| positions.contains(&e.source));
            region
        })
        .collect())
}
