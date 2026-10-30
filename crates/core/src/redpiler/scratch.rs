//! Shared isolated native-interpreter world used by private compilers.
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld};
use crate::redpiler::TickScheduler;
use crate::redpiler::backend::ScheduledBlockTick;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use mchprs_world::{AdvancePhase, PistonState, TickEntry};
use std::collections::{HashMap, VecDeque};

pub(crate) struct ScratchEntry {
    pub(crate) scheduled: Vec<TickEntry>,
    pub(crate) piston: PistonState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScratchError {
    pub(crate) position: Option<BlockPos>,
    pub(crate) reason: &'static str,
}

pub(crate) struct ScratchWorld {
    world: PlotWorld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireWatcher {
    pub(crate) wire: BlockPos,
    pub(crate) piston: BlockPos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireSource {
    pub(crate) wire: BlockPos,
    pub(crate) source: BlockPos,
    pub(crate) attenuation: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WireDiscovery {
    pub(crate) watchers: Vec<WireWatcher>,
    pub(crate) sources: Vec<WireSource>,
}
pub(crate) fn discovered_wire_strength(world: &impl World, discovery: &WireDiscovery, wire: BlockPos) -> u8 {
    discovery
        .sources
        .iter()
        .filter(|dependency| dependency.wire == wire)
        .map(|dependency| {
            crate::redstone::source_strength(world.get_block(dependency.source), world, dependency.source).saturating_sub(dependency.attenuation)
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn wire_strength_changed(before: u8, after: u8) -> bool {
    before != after
}

pub(crate) fn changed_wire_watchers(discovery: &WireDiscovery, before: &[(BlockPos, u8)], after: &[(BlockPos, u8)]) -> Vec<WireWatcher> {
    let after: HashMap<_, _> = after.iter().copied().collect();
    discovery
        .watchers
        .iter()
        .copied()
        .filter(|watch| {
            before
                .iter()
                .find(|(wire, _)| *wire == watch.wire)
                .is_some_and(|(_, power)| after.get(&watch.wire).is_some_and(|next| *power != *next))
        })
        .collect()
}

pub(crate) fn max_attenuated_strength(contributions: impl IntoIterator<Item = (u8, u8)>) -> u8 {
    contributions
        .into_iter()
        .map(|(strength, attenuation)| strength.saturating_sub(attenuation))
        .max()
        .unwrap_or(0)
}

pub(crate) fn discover_watched_wire_sources(world: &impl World, bounds: (BlockPos, BlockPos)) -> Result<WireDiscovery, ScratchError> {
    let (min, max) = bounds;
    let mut wires = Vec::new();
    let mut pistons = Vec::new();
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                let pos = BlockPos::new(x, y, z);
                match world.get_block(pos) {
                    Block::RedstoneWire { .. } => wires.push(pos),
                    Block::Piston { .. } => pistons.push(pos),
                    _ => {}
                }
            }
        }
    }
    let watchers = wires
        .iter()
        .flat_map(|&wire| {
            pistons.iter().filter_map(move |&piston| {
                let distance = wire.x.abs_diff(piston.x) + wire.y.abs_diff(piston.y) + wire.z.abs_diff(piston.z);
                (distance <= 2).then_some(WireWatcher { wire, piston })
            })
        })
        .collect();

    // Dust attenuation follows the shortest represented wire path; dynamic source
    // states are included even when currently off (e.g. an unpowered lever).
    let mut sources = HashMap::new();
    for &wire in &wires {
        let mut distances = HashMap::from([(wire, 0u8)]);
        let mut queue = VecDeque::from([wire]);
        while let Some(current) = queue.pop_front() {
            let distance = distances[&current];
            for face in BlockFace::values() {
                let neighbor = current.offset(face);
                match world.get_block(neighbor) {
                    Block::RedstoneWire { .. } => {
                        if distance >= 14 || matches!(face, BlockFace::Top | BlockFace::Bottom) {
                            continue;
                        }
                        let next = distance + 1;
                        if distances.get(&neighbor).is_none_or(|old| next < *old) {
                            distances.insert(neighbor, next);
                            queue.push_back(neighbor);
                        }
                    }
                    block if is_supported_dynamic_source(block) || block == Block::RedstoneBlock => {
                        if source_emits_to(block, world, neighbor, face) {
                            let attenuation = distance;
                            sources
                                .entry((wire, neighbor))
                                .and_modify(|old: &mut u8| *old = (*old).min(attenuation))
                                .or_insert(attenuation);
                        }
                    }
                    block if block.is_solid() => {
                        for source_face in BlockFace::values() {
                            let source_pos = neighbor.offset(source_face);
                            let source_block = world.get_block(source_pos);
                            if (is_supported_dynamic_source(source_block) || source_block == Block::RedstoneBlock)
                                && crate::redstone::power::emits_strong_power(source_block, world, source_pos, source_face, false)
                            {
                                let attenuation = distance;
                                sources
                                    .entry((wire, source_pos))
                                    .and_modify(|old: &mut u8| *old = (*old).min(attenuation))
                                    .or_insert(attenuation);
                            } else if !matches!(source_block, Block::RedstoneWire { .. })
                                && crate::redstone::power::emits_strong_power(source_block, world, source_pos, source_face, false)
                            {
                                return Err(ScratchError {
                                    position: Some(source_pos),
                                    reason: "unsupported watched-wire source dependency",
                                });
                            }
                        }
                    }
                    block if source_emits_to(block, world, neighbor, face) => {
                        return Err(ScratchError {
                            position: Some(neighbor),
                            reason: "unsupported watched-wire source dependency",
                        });
                    }
                    _ => {}
                }
            }
            if distance < 14 {
                let above_solid = world.get_block(current.offset(BlockFace::Top)).is_solid();
                for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
                    let adjacent = current.offset(face);
                    let block = world.get_block(adjacent);
                    let diagonal = if !block.is_solid() {
                        Some(adjacent.offset(BlockFace::Bottom))
                    } else if !above_solid && !block.is_transparent() {
                        Some(adjacent.offset(BlockFace::Top))
                    } else {
                        None
                    };
                    if let Some(neighbor) = diagonal
                        && matches!(world.get_block(neighbor), Block::RedstoneWire { .. })
                    {
                        let next = distance + 1;
                        if distances.get(&neighbor).is_none_or(|old| next < *old) {
                            distances.insert(neighbor, next);
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }
    }
    let mut sources: Vec<_> = sources
        .into_iter()
        .map(|((wire, source), attenuation)| WireSource { wire, source, attenuation })
        .collect();
    sources.sort_by_key(|source| {
        (
            source.wire.x,
            source.wire.y,
            source.wire.z,
            source.source.x,
            source.source.y,
            source.source.z,
        )
    });
    Ok(WireDiscovery { watchers, sources })
}

fn is_supported_dynamic_source(block: Block) -> bool {
    matches!(
        block,
        Block::Lever { .. }
            | Block::StoneButton { .. }
            | Block::RedstoneTorch { .. }
            | Block::RedstoneWallTorch { .. }
            | Block::RedstoneRepeater { .. }
            | Block::RedstoneComparator { .. }
            | Block::Observer { .. }
    )
}

fn source_emits_to(block: Block, world: &impl World, pos: BlockPos, face: BlockFace) -> bool {
    crate::redstone::power::emits_weak_power(block, world, pos, face, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redstone;
    use crate::world::RedstoneUpdateObservation;
    use mchprs_blocks::{
        BlockDirection, BlockFace, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston, RedstoneWire, RedstoneWireSide},
    };

    const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(38, 26, 38), BlockPos::new(44, 34, 42));
    const LEVER: BlockPos = BlockPos::new(40, 30, 40);
    const WIRE: BlockPos = BlockPos::new(41, 30, 40);
    const PISTON: BlockPos = BlockPos::new(42, 30, 40);

    fn fixture() -> PlotWorld {
        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::<ScheduledBlockTick>::default());
        world.set_block(BlockPos::new(40, 29, 40), Block::Stone {});
        world.set_block(
            LEVER,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        world.set_block(
            WIRE,
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::None,
                    RedstoneWireSide::None,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    0,
                ),
            },
        );
        world.set_block(
            PISTON,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: false,
                    extended: false,
                },
            },
        );
        world
    }

    fn entry(world: &PlotWorld) -> ScratchEntry {
        ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(),
            piston: world.piston_state().clone(),
        }
    }

    fn drive(world: &mut PlotWorld) {
        world.set_block(
            LEVER,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        redstone::update_surrounding_blocks(world, LEVER);
        redstone::update_surrounding_blocks(world, WIRE);
        redstone::update_surrounding_blocks(world, BlockPos::new(40, 29, 40));
    }

    fn sample(world: &PlotWorld) -> (Vec<Block>, serde_json::Value, Vec<TickEntry>) {
        let blocks = [BlockPos::new(40, 29, 40), LEVER, WIRE, PISTON, PISTON.offset(BlockFace::East)]
            .into_iter()
            .map(|pos| world.get_block(pos))
            .collect();
        (
            blocks,
            serde_json::to_value(world.piston_state()).unwrap(),
            world.scheduler().iter_entries().collect(),
        )
    }

    #[test]
    fn scratch_copy_runs_interpreter_passively_and_is_independent() {
        let host = fixture();
        let host_before = sample(&host);
        let mut source_run = fixture();
        drive(&mut source_run);
        let mut traced = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        let mut untraced = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        traced.world_mut().observe_redstone_updates(true);
        drive(traced.world_mut());
        drive(untraced.world_mut());
        traced.finish_probe().unwrap();
        untraced.finish_probe().unwrap();
        assert_eq!(sample(traced.world()), sample(&source_run));
        assert_eq!(sample(traced.world()), sample(untraced.world()));
        let events: Vec<RedstoneUpdateObservation> = traced.world_mut().take_redstone_update_observations();
        assert!(
            events
                .iter()
                .any(|event| event.pos == PISTON && matches!(event.block, Block::Piston { piston } if !piston.extended) && event.direction.is_some()),
            "piston delivery absent: {events:?}"
        );
        assert_eq!(sample(&host), host_before);

        let other = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        traced.world_mut().set_block(LEVER, Block::Air);
        assert_ne!(traced.world().get_block(LEVER), other.world().get_block(LEVER));
        assert_eq!(sample(&host), host_before);
    }

    #[test]
    fn scratch_reports_unknown_boundary_and_pending_entry_work() {
        let host = fixture();
        let scratch = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
        let outside = BlockPos::new(BOUNDS.1.x + 1, BOUNDS.1.y, BOUNDS.1.z);
        let _ = scratch.world().get_block(outside);
        assert_eq!(
            scratch.finish_probe().unwrap_err(),
            ScratchError {
                position: Some(outside),
                reason: "scratch probe read outside captured boundary"
            }
        );

        let mut pending = entry(&host);
        pending.piston.events.push_back(mchprs_world::PistonEvent {
            pos: PISTON,
            sticky: false,
            facing: BlockFace::East,
            action: mchprs_world::PistonAction::Extend,
        });
        let error = match ScratchWorld::from_world(&host, BOUNDS, pending) {
            Ok(_) => panic!("pending piston work was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ScratchError {
                position: Some(PISTON),
                reason: "scratch entry has queued piston work"
            }
        );
    }

    #[test]
    fn watched_wire_discovery_separates_dependencies_from_notifications() {
        let mut world = fixture();
        let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
        assert!(discovery.watchers.contains(&WireWatcher { wire: WIRE, piston: PISTON }));
        assert!(discovery.sources.contains(&WireSource {
            wire: WIRE,
            source: LEVER,
            attenuation: 0
        }));
        let saved = match world.get_block(WIRE) {
            Block::RedstoneWire { wire } => wire.power,
            _ => unreachable!(),
        };
        assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), saved);

        // A nearby watcher is only a candidate. Without an interpreter delivery,
        // discovering an electrical dependency does not manufacture a callback.
        world.observe_redstone_updates(true);
        assert!(world.take_redstone_update_observations().is_empty());
        assert!(wire_strength_changed(14, 15));
        assert!(!wire_strength_changed(14, 14));
    }

    #[test]
    fn changed_watched_wire_is_the_notification_condition() {
        let discovery = WireDiscovery {
            watchers: vec![WireWatcher { wire: WIRE, piston: PISTON }],
            sources: vec![],
        };
        assert_eq!(changed_wire_watchers(&discovery, &[(WIRE, 0)], &[(WIRE, 1)]), discovery.watchers);
    }

    #[test]
    fn masked_source_change_does_not_change_wire_or_notify() {
        let discovery = WireDiscovery {
            watchers: vec![WireWatcher { wire: WIRE, piston: PISTON }],
            sources: vec![],
        };
        let before = max_attenuated_strength([(15, 1), (14, 2)]);
        let after = max_attenuated_strength([(15, 1), (12, 2)]);
        assert_eq!((before, after), (14, 14));
        assert!(changed_wire_watchers(&discovery, &[(WIRE, before)], &[(WIRE, after)]).is_empty());
    }

    #[test]
    fn positive_to_positive_strength_change_remains_observable() {
        let before = max_attenuated_strength([(15, 1)]);
        let after = max_attenuated_strength([(14, 1)]);
        assert_eq!((before, after), (14, 13));
        assert!(wire_strength_changed(before, after));
    }

    #[test]
    fn nearby_power_dependency_without_wire_change_does_not_notify() {
        let discovery = WireDiscovery {
            watchers: vec![WireWatcher { wire: WIRE, piston: PISTON }],
            sources: vec![],
        };
        assert!(changed_wire_watchers(&discovery, &[(WIRE, 7)], &[(WIRE, 7)]).is_empty());
    }

    #[test]
    fn initially_off_unsupported_source_is_rejected() {
        let mut world = fixture();
        let pos = BlockPos::new(41, 30, 41);
        world.set_block(pos, Block::StonePressurePlate { powered: false });
        let error = discover_watched_wire_sources(&world, BOUNDS).unwrap_err();
        assert_eq!(error.position, Some(pos));
        assert_eq!(error.reason, "unsupported watched-wire source dependency");
    }

    #[test]
    fn direct_source_strength_matches_interpreter_dust() {
        let mut world = fixture();
        let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
        assert!(discovery.sources.contains(&WireSource {
            wire: WIRE,
            source: LEVER,
            attenuation: 0
        }));
        let Block::RedstoneWire { wire } = world.get_block(WIRE) else {
            panic!("wire missing")
        };
        assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
        drive(&mut world);
        let Block::RedstoneWire { wire } = world.get_block(WIRE) else {
            panic!("wire missing")
        };
        assert_eq!(wire.power, 15);
        assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
    }

    #[test]
    fn directional_sources_match_native_power_queries() {
        use mchprs_blocks::blocks::{RedstoneObserver, RedstoneRepeater};
        let source = WIRE.offset(BlockFace::North);
        for facing in [BlockDirection::North, BlockDirection::South] {
            let mut world = fixture();
            let block = Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing,
                    powered: true,
                    ..Default::default()
                },
            };
            world.set_block(source, block);
            let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
            assert_eq!(
                discovered_wire_strength(&world, &discovery, WIRE),
                redstone::get_redstone_power(block, &world, source, BlockFace::North)
            );
        }
        for facing in [BlockFacing::North, BlockFacing::South] {
            let mut world = fixture();
            let block = Block::Observer {
                observer: RedstoneObserver { facing, powered: true },
            };
            world.set_block(source, block);
            let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
            assert_eq!(
                discovered_wire_strength(&world, &discovery, WIRE),
                redstone::get_redstone_power(block, &world, source, BlockFace::North)
            );
        }
    }

    #[test]
    fn dust_distance_fourteen_and_unpowered_continuation_match_interpreter() {
        let mut world = fixture();
        world.set_block(LEVER, Block::Air);
        world.set_block(WIRE, Block::Air);
        world.set_block(PISTON, Block::Air);
        let source = BlockPos::new(39, 30, 40);
        let mut lever = Lever::new(LeverFace::Floor, BlockDirection::North, false);
        world.set_block(source, Block::Lever { lever });
        for x in 40..=55 {
            world.set_block(
                BlockPos::new(x, 30, 40),
                Block::RedstoneWire {
                    wire: RedstoneWire::new(
                        RedstoneWireSide::None,
                        RedstoneWireSide::None,
                        RedstoneWireSide::Side,
                        RedstoneWireSide::Side,
                        0,
                    ),
                },
            );
        }
        let bounds = (BlockPos::new(37, 26, 37), BlockPos::new(58, 34, 43));
        let discovery = discover_watched_wire_sources(&world, bounds).unwrap();
        for x in 40..=55 {
            let wire = BlockPos::new(x, 30, 40);
            assert_eq!(discovered_wire_strength(&world, &discovery, wire), 0);
            assert_eq!(discovery.sources.iter().any(|link| link.wire == wire && link.source == source), x <= 54);
        }
        lever.powered = true;
        world.set_block(source, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut world, source);
        for x in 40..=55 {
            let pos = BlockPos::new(x, 30, 40);
            let Block::RedstoneWire { wire } = world.get_block(pos) else {
                panic!("wire missing")
            };
            assert_eq!(discovered_wire_strength(&world, &discovery, pos), wire.power, "wire at {pos:?}");
        }
    }

    #[test]
    fn competing_levers_mask_a_source_change_in_native_dust() {
        let mut world = fixture();
        let second = WIRE.offset(BlockFace::North);
        let mut lever = Lever::new(LeverFace::Floor, BlockDirection::North, false);
        world.set_block(second, Block::Lever { lever });
        let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
        for source in [LEVER, second] {
            assert!(
                discovery
                    .sources
                    .iter()
                    .any(|link| link.wire == WIRE && link.source == source && link.attenuation == 0)
            );
        }
        drive(&mut world);
        lever.powered = true;
        world.set_block(second, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut world, second);
        let before = world.get_block(WIRE);
        let Block::RedstoneWire { wire } = before else { panic!("wire missing") };
        assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
        lever.powered = false;
        world.set_block(second, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut world, second);
        assert_eq!(world.get_block(WIRE), before);
        assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
    }

    #[test]
    fn ascending_and_descending_dust_match_interpreter() {
        for source_above in [false, true] {
            let mut world = fixture();
            world.set_block(LEVER, Block::Air);
            world.set_block(PISTON, Block::Stone {});
            let lower = WIRE;
            let upper = PISTON.offset(BlockFace::Top);
            let source = if source_above { BlockPos::new(43, 31, 40) } else { LEVER };
            let mut lever = Lever::new(LeverFace::Floor, BlockDirection::North, false);
            world.set_block(source, Block::Lever { lever });
            for pos in [lower, upper] {
                world.set_block(
                    pos,
                    Block::RedstoneWire {
                        wire: RedstoneWire::new(
                            RedstoneWireSide::None,
                            RedstoneWireSide::None,
                            RedstoneWireSide::Side,
                            RedstoneWireSide::Side,
                            0,
                        ),
                    },
                );
            }
            let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
            let destination = if source_above { lower } else { upper };
            assert!(
                discovery
                    .sources
                    .iter()
                    .any(|link| link.wire == destination && link.source == source && link.attenuation == 1)
            );
            for pos in [lower, upper] {
                assert_eq!(discovered_wire_strength(&world, &discovery, pos), 0);
            }
            lever.powered = true;
            world.set_block(source, Block::Lever { lever });
            redstone::update_surrounding_blocks(&mut world, source);
            for pos in [lower, upper] {
                let Block::RedstoneWire { wire } = world.get_block(pos) else {
                    panic!("wire missing")
                };
                assert_eq!(
                    discovered_wire_strength(&world, &discovery, pos),
                    wire.power,
                    "wire at {pos:?}, source_above={source_above}"
                );
            }
        }
    }
}

impl ScratchWorld {
    pub(crate) fn from_world(source: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Self, ScratchError> {
        let (min, max) = bounds;
        if min.x < 0
            || min.y < 0
            || min.z < 0
            || max.x >= PLOT_BLOCK_WIDTH
            || max.y >= PLOT_BLOCK_HEIGHT
            || max.z >= PLOT_BLOCK_WIDTH
            || min.x > max.x
            || min.y > max.y
            || min.z > max.z
        {
            return Err(ScratchError {
                position: Some(min),
                reason: "invalid scratch capture bounds",
            });
        }
        if entry.piston.phase != AdvancePhase::BetweenTicks {
            return Err(ScratchError {
                position: None,
                reason: "scratch entry is mid-tick",
            });
        }
        if let Some(tick) = entry.scheduled.first() {
            return Err(ScratchError {
                position: Some(tick.pos),
                reason: "scratch entry has scheduled block work",
            });
        }
        if let Some(event) = entry.piston.events.front() {
            return Err(ScratchError {
                position: Some(event.pos),
                reason: "scratch entry has queued piston work",
            });
        }
        if let Some(motion) = entry.piston.motions.front() {
            return Err(ScratchError {
                position: Some(motion.pos),
                reason: "scratch entry has piston motion",
            });
        }
        if let Some((pos, _)) = entry.piston.movement_work.first() {
            return Err(ScratchError {
                position: Some(*pos),
                reason: "scratch entry has pending movement work",
            });
        }

        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::<ScheduledBlockTick>::default());
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let pos = BlockPos::new(x, y, z);
                    let raw = source.get_block_raw(pos);
                    if raw != Block::Air.get_id() {
                        world.set_block_raw(pos, raw);
                    }
                    if let Some(entity) = source.get_block_entity(pos) {
                        world.set_block_entity(pos, entity.clone());
                    }
                }
            }
        }
        world.piston_state_mut().clone_from(&entry.piston);
        world.configure_scratch(bounds);
        Ok(Self { world })
    }

    pub(crate) fn world(&self) -> &PlotWorld {
        &self.world
    }
    pub(crate) fn world_mut(&mut self) -> &mut PlotWorld {
        &mut self.world
    }

    pub(crate) fn finish_probe(&self) -> Result<(), ScratchError> {
        if let Some(position) = self.world.take_scratch_boundary_read() {
            return Err(ScratchError {
                position: Some(position),
                reason: "scratch probe read outside captured boundary",
            });
        }
        Ok(())
    }
}
