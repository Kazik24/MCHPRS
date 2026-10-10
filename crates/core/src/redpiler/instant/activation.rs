//! Notification-gated responses are actuator state, not sampled memory.
use super::outputs::PowerTerm;
use crate::redpiler::analysis::ports::UpdateKind;
use crate::redpiler::analysis::topology::{SourceKind, Topology};
use crate::redpiler::analysis::{AnalysisLimits, AnalysisReport};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Delivery {
    pub source: BlockPos,
    pub actor: usize,
    pub recipient: BlockPos,
    pub direction: Option<BlockFace>,
    pub requires_extended: bool,
}

pub(crate) struct Wire {
    pub pos: BlockPos,
    pub initial: u8,
    pub terms: Vec<PowerTerm>,
    pub deliveries: Vec<Delivery>,
}

pub(crate) struct Source {
    pub pos: BlockPos,
    pub deliveries: Vec<Delivery>,
}

#[derive(Default)]
pub(crate) struct Activation {
    pub actors: FxHashSet<usize>,
    pub wires: Vec<Wire>,
    pub sources: Vec<Source>,
    pub pose_deliveries: FxHashMap<BlockPos, Vec<Delivery>>,
}

pub(crate) fn recognize(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
    resets: &FxHashSet<BlockPos>,
) -> Result<Activation, String> {
    let targets = report.pistons.iter().map(|p| p.pos).collect();
    let data = report
        .recognition
        .iter()
        .flat_map(|r| &r.inputs.sources)
        .filter(|s| {
            !matches!(
                s.kind,
                SourceKind::MobilePayload { .. } | SourceKind::Constant
            )
        })
        .filter(|s| !resets.contains(&s.source))
        .map(|s| s.source)
        .collect();
    let feedback =
        super::logic::sequential::feedback_sources(world, report, monitor, &targets, &data)?;
    let mut groups = vec![usize::MAX; report.pistons.len()];
    let mut mobile = FxHashMap::default();
    for (group, payload) in report.payload_groups.iter().enumerate() {
        for &actor in &payload.members {
            groups[actor] = group;
        }
        for &pos in &payload.positions {
            mobile.insert(pos, group);
        }
    }
    let mut topology = Topology::new(
        world,
        report.bounds,
        monitor,
        AnalysisLimits::for_budget(monitor.budget_multiplier()).max_dependency_steps,
        mobile,
    );
    let mut result = Activation::default();
    let mut wires = FxHashMap::default();
    for (actor, piston) in report.pistons.iter().enumerate() {
        let mut notifying_sources = Vec::new();
        let mut has_unnotified_source = false;
        let has_self_payload = report.recognition[actor]
            .inputs
            .sources
            .iter()
            .any(|source| matches!(source.kind, SourceKind::MobilePayload { group } if group == groups[actor]));
        for source in &report.recognition[actor].inputs.sources {
            if resets.contains(&source.source) || source.kind == SourceKind::Constant {
                continue;
            }
            if super::sampling::data_notifies(world, source.source, piston.pos) {
                if matches!(
                    source.kind,
                    SourceKind::Ordinary | SourceKind::Observer | SourceKind::MobilePayload { .. }
                ) {
                    notifying_sources.push(source.source);
                }
            } else if !matches!(source.kind, SourceKind::MobilePayload { group } if group == groups[actor]) {
                has_unnotified_source = true;
            }
        }
        notifying_sources.sort_by_key(|pos| (pos.y, pos.z, pos.x));
        notifying_sources.dedup();
        if !notifying_sources.is_empty() || has_self_payload && has_unnotified_source {
            result.actors.insert(actor);
            for source in notifying_sources {
                let delivery = Delivery {
                    source,
                    actor,
                    recipient: piston.pos,
                    direction: None,
                    requires_extended: false,
                };
                if let Some(trigger) = result.sources.iter_mut().find(|t| t.pos == source) {
                    trigger.deliveries.push(delivery);
                } else {
                    result.sources.push(Source {
                        pos: source,
                        deliveries: vec![delivery],
                    });
                }
            }
        }
        if !resets.iter().any(|&pos| matches!(world.get_block(pos), mchprs_blocks::blocks::Block::Observer { observer } if pos.offset(observer.facing.into()) == piston.pos))
            || !feedback[&piston.pos].iter().any(|&source| !super::sampling::data_notifies(world, source, piston.pos)) {
            continue;
        }
        let mut actor_wires = FxHashSet::default();
        let mut representable = true;
        let mut has_external_mobile = false;
        for update in &report.ports.pistons[actor].updates {
            if update.kind != UpdateKind::WireNotification {
                continue;
            }
            let inputs = topology
                .wire_inputs(update.source)
                .map_err(|e| e.to_string())?;
            has_external_mobile |= inputs.sources.iter().any(
                |s| matches!(s.kind, SourceKind::MobilePayload { group } if group != groups[actor]),
            );
            if !inputs.outside_bounds.is_empty()
                || inputs.wires.iter().any(|&pos| pos != update.source)
            {
                representable = false;
                break;
            }
            actor_wires.insert(update.source);
        }
        // Keep connected callback nets on the existing physical update path.
        if representable && has_external_mobile && !actor_wires.is_empty() {
            result.actors.insert(actor);
            wires.extend(actor_wires.into_iter().map(|pos| (pos, ())));
        }
    }
    let mut positions: Vec<_> = wires.into_keys().collect();
    positions.sort_by_key(|p| (p.y, p.z, p.x));
    for pos in positions {
        let mchprs_blocks::blocks::Block::RedstoneWire { wire } = world.get_block(pos) else {
            unreachable!()
        };
        result.wires.push(Wire {
            pos,
            initial: wire.power,
            terms: Vec::new(),
            deliveries: wire_deliveries(report, &result.actors, pos),
        });
    }
    for piston in &report.pistons {
        for source in [piston.pos, piston.head] {
            let mut deliveries = Vec::new();
            for face in [
                BlockFace::West,
                BlockFace::East,
                BlockFace::Bottom,
                BlockFace::Top,
                BlockFace::North,
                BlockFace::South,
            ] {
                let recipient = source.offset(face);
                for &actor in &result.actors {
                    let receiver = &report.pistons[actor];
                    if recipient == receiver.pos || recipient == receiver.head {
                        deliveries.push(Delivery {
                            source,
                            actor,
                            recipient,
                            direction: Some(face.opposite()),
                            requires_extended: recipient == receiver.head,
                        });
                    }
                }
            }
            if !deliveries.is_empty() {
                result.pose_deliveries.insert(source, deliveries);
            }
        }
    }
    Ok(result)
}

fn wire_deliveries(
    report: &AnalysisReport,
    actors: &FxHashSet<usize>,
    pos: BlockPos,
) -> Vec<Delivery> {
    let mut result = Vec::new();
    let mut visit = |recipient, direction| {
        for (actor, piston) in report.pistons.iter().enumerate() {
            if actors.contains(&actor) && (recipient == piston.pos || recipient == piston.head) {
                result.push(Delivery {
                    source: pos,
                    actor,
                    recipient,
                    direction,
                    requires_extended: recipient == piston.head,
                });
            }
        }
    };
    // An isolated wire starts a Turbo walk with zero heading bias (west).
    let neighbors = crate::world::wire_cache::positions(pos);
    for index in crate::redstone::wire::TURBO_ORDER[3] {
        visit(neighbors[index], None);
    }
    for face in BlockFace::values() {
        let neighbor = pos.offset(face);
        visit(neighbor, Some(face.opposite()));
        for second in BlockFace::values() {
            visit(neighbor.offset(second), Some(second.opposite()));
        }
    }
    result
}
