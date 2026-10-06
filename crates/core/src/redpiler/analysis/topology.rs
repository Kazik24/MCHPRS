//! Bounded, read-only electrical dependency queries. Signal strength and
//! qualifying notifications are deliberately separate views.
use super::{contains, AnalysisError, PistonDescriptor, TaskMonitor};
use crate::redstone::power::{emits_strong_power, emits_weak_power};
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum PowerRoute {
    Direct,
    QuasiConnectivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum SourceKind {
    Ordinary,
    Constant,
    Observer,
    /// A position that may contain the group's payload, rather than an
    /// immutable redstone block. Near and far positions retain one identity.
    MobilePayload {
        group: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PowerDependency {
    pub source: BlockPos,
    pub kind: SourceKind,
    pub attenuation: u8,
    pub route: PowerRoute,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PowerDependencies {
    pub sources: Vec<PowerDependency>,
    /// Wires are retained as aliases even if ordinary optimization removes them.
    pub wires: Vec<BlockPos>,
    pub outside_bounds: Vec<BlockPos>,
}

impl PowerDependencies {
    pub(super) fn normalize(&mut self) {
        self.sources.sort_by_key(|d| {
            (
                d.source.y,
                d.source.z,
                d.source.x,
                d.route as u8,
                d.attenuation,
            )
        });
        // Keep the shortest path for each source/channel.
        self.sources
            .dedup_by(|a, b| a.source == b.source && a.route == b.route);
        for positions in [&mut self.wires, &mut self.outside_bounds] {
            positions.sort_by_key(|p| (p.y, p.z, p.x));
            positions.dedup();
        }
    }
}

pub(crate) struct Topology<'a, W: World> {
    pub world: &'a W,
    bounds: (BlockPos, BlockPos),
    monitor: &'a TaskMonitor,
    remaining: usize,
    pub visited: usize,
    mobile: FxHashMap<BlockPos, usize>,
}

impl<'a, W: World> Topology<'a, W> {
    pub fn new(
        world: &'a W,
        bounds: (BlockPos, BlockPos),
        monitor: &'a TaskMonitor,
        max_steps: usize,
        mobile: FxHashMap<BlockPos, usize>,
    ) -> Self {
        Self {
            world,
            bounds,
            monitor,
            remaining: max_steps,
            visited: 0,
            mobile,
        }
    }

    pub fn step(&mut self) -> Result<(), AnalysisError> {
        if self.monitor.cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        if self.remaining == 0 {
            return Err(AnalysisError::DependencyLimit);
        }
        self.remaining -= 1;
        self.visited += 1;
        Ok(())
    }

    pub fn read(&mut self, pos: BlockPos) -> Result<Option<Block>, AnalysisError> {
        self.step()?;
        Ok(contains(self.bounds, pos).then(|| self.world.get_block(pos)))
    }

    pub fn mobile_group(&self, pos: BlockPos) -> Option<usize> {
        self.mobile.get(&pos).copied()
    }

    pub fn piston_inputs(
        &mut self,
        p: &PistonDescriptor,
    ) -> Result<PowerDependencies, AnalysisError> {
        let mut result = PowerDependencies::default();
        for side in BlockFace::values() {
            if side != BlockFace::from(p.piston.facing) {
                self.signal(p.pos.offset(side), side, PowerRoute::Direct, &mut result)?;
            }
        }
        let above = p.pos.offset(BlockFace::Top);
        for side in BlockFace::values() {
            self.signal(
                above.offset(side),
                side,
                PowerRoute::QuasiConnectivity,
                &mut result,
            )?;
        }
        result.normalize();
        Ok(result)
    }

    pub fn signal_inputs(
        &mut self,
        pos: BlockPos,
        side: BlockFace,
    ) -> Result<PowerDependencies, AnalysisError> {
        let mut result = PowerDependencies::default();
        self.signal(pos, side, PowerRoute::Direct, &mut result)?;
        result.normalize();
        Ok(result)
    }

    pub fn wire_inputs(&mut self, pos: BlockPos) -> Result<PowerDependencies, AnalysisError> {
        let mut result = PowerDependencies::default();
        self.walk_wire(pos, 0, PowerRoute::Direct, &mut result)?;
        result.normalize();
        Ok(result)
    }

    pub fn comparator_side_inputs(
        &mut self,
        pos: BlockPos,
        side: BlockFace,
    ) -> Result<PowerDependencies, AnalysisError> {
        let mut result = PowerDependencies::default();
        if let Some(block) = self.block(pos, &mut result)? {
            if self.mobile_group(pos).is_some() || block == Block::RedstoneBlock {
                self.add_source(pos, block, 0, PowerRoute::Direct, &mut result);
            } else if matches!(block, Block::RedstoneWire { .. }) {
                self.walk_wire(pos, 0, PowerRoute::Direct, &mut result)?;
            } else if crate::redstone::is_diode(block)
                && emits_weak_power(block, self.world, pos, side, false)
            {
                self.add_source(pos, block, 0, PowerRoute::Direct, &mut result);
            }
        }
        result.normalize();
        Ok(result)
    }

    fn block(
        &mut self,
        pos: BlockPos,
        result: &mut PowerDependencies,
    ) -> Result<Option<Block>, AnalysisError> {
        let block = self.read(pos)?;
        if block.is_none() {
            result.outside_bounds.push(pos);
        }
        Ok(block)
    }

    fn source(&self, pos: BlockPos, block: Block) -> SourceKind {
        if let Some(group) = self.mobile_group(pos) {
            SourceKind::MobilePayload { group }
        } else if block == Block::RedstoneBlock {
            SourceKind::Constant
        } else if matches!(block, Block::Observer { .. }) {
            SourceKind::Observer
        } else {
            SourceKind::Ordinary
        }
    }

    fn add_source(
        &self,
        pos: BlockPos,
        block: Block,
        attenuation: u8,
        route: PowerRoute,
        result: &mut PowerDependencies,
    ) {
        if attenuation < 15 {
            result.sources.push(PowerDependency {
                source: pos,
                kind: self.source(pos, block),
                attenuation,
                route,
            });
        }
    }

    fn signal(
        &mut self,
        pos: BlockPos,
        side: BlockFace,
        route: PowerRoute,
        result: &mut PowerDependencies,
    ) -> Result<(), AnalysisError> {
        let Some(block) = self.block(pos, result)? else {
            return Ok(());
        };
        // Future payload supply must be found even when this position currently
        // contains a piston head. Its current block is not a constant source.
        if self.mobile_group(pos).is_some() {
            self.add_source(pos, block, 0, route, result);
        } else if block.is_solid() {
            for face in BlockFace::values() {
                self.strong(pos.offset(face), face, 0, route, result)?;
            }
        } else if emits_weak_power(block, self.world, pos, side, true) {
            if matches!(block, Block::RedstoneWire { .. }) {
                self.walk_wire(pos, 0, route, result)?;
            } else {
                self.add_source(pos, block, 0, route, result);
            }
        }
        Ok(())
    }

    fn strong(
        &mut self,
        pos: BlockPos,
        side: BlockFace,
        distance: u8,
        route: PowerRoute,
        result: &mut PowerDependencies,
    ) -> Result<(), AnalysisError> {
        let Some(block) = self.block(pos, result)? else {
            return Ok(());
        };
        // A redstone block does not strongly power a conducting block.
        if self.mobile_group(pos).is_none()
            && emits_strong_power(block, self.world, pos, side, true)
        {
            if matches!(block, Block::RedstoneWire { .. }) {
                self.walk_wire(pos, distance, route, result)?;
            } else {
                self.add_source(pos, block, distance, route, result);
            }
        }
        Ok(())
    }

    fn walk_wire(
        &mut self,
        root: BlockPos,
        distance: u8,
        route: PowerRoute,
        result: &mut PowerDependencies,
    ) -> Result<(), AnalysisError> {
        let mut queue = VecDeque::from([(root, distance)]);
        let mut visited = FxHashSet::default();
        while let Some((pos, distance)) = queue.pop_front() {
            if distance >= 15 || !visited.insert(pos) {
                continue;
            }
            result.wires.push(pos);
            let Some(Block::RedstoneWire { .. }) = self.block(pos, result)? else {
                continue;
            };
            let above = self.block(pos.offset(BlockFace::Top), result)?;
            for face in BlockFace::values() {
                let neighbor = pos.offset(face);
                let Some(block) = self.block(neighbor, result)? else {
                    continue;
                };
                if self.mobile_group(neighbor).is_some() {
                    self.add_source(neighbor, block, distance, route, result);
                } else if block.is_solid() {
                    // Dust takes non-dust strong power through a conductor.
                    for side in BlockFace::values() {
                        let source = neighbor.offset(side);
                        if let Some(source_block) = self.block(source, result)? {
                            if emits_strong_power(source_block, self.world, source, side, false) {
                                self.add_source(source, source_block, distance, route, result);
                            }
                        }
                    }
                } else if emits_weak_power(block, self.world, neighbor, face, false) {
                    self.add_source(neighbor, block, distance, route, result);
                }
                if !face.is_horizontal() {
                    continue;
                }
                if matches!(block, Block::RedstoneWire { .. }) {
                    queue.push_back((neighbor, distance + 1));
                }
                if above.is_some_and(|b| !b.is_solid()) && !block.is_transparent() {
                    let up = neighbor.offset(BlockFace::Top);
                    if matches!(self.block(up, result)?, Some(Block::RedstoneWire { .. })) {
                        queue.push_back((up, distance + 1));
                    }
                }
                if !block.is_solid() {
                    let down = neighbor.offset(BlockFace::Bottom);
                    if matches!(self.block(down, result)?, Some(Block::RedstoneWire { .. })) {
                        queue.push_back((down, distance + 1));
                    }
                }
            }
        }
        Ok(())
    }
}
