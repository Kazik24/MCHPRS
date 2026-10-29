//! Native, playerless simulation sessions. Positions use plot coordinates.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use mchprs_blocks::BlockPos;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::Block;
use mchprs_world::{TickEntry, TickPriority};

use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld};
use crate::redpiler::{CompileStatistics, Compiler, CompilerOptions, TaskMonitor, TickScheduler};
use crate::redstone;
use crate::world::{World, storage::Chunk};

/// A complete sparse snapshot in plot coordinates; unspecified cells are air.
pub struct Snapshot {
    pub min: BlockPos,
    pub max: BlockPos,
    pub blocks: Vec<(BlockPos, Block)>,
    pub entities: Vec<(BlockPos, BlockEntity)>,
    /// Entries use the scheduler's half-tick delays and retain their input order.
    pub ticks: Vec<TickEntry>,
}

/// A session owns its world and compiler until it is dropped.
pub struct Session {
    world: PlotWorld,
    bounds: (BlockPos, BlockPos),
    compiler: Compiler,
    failed: bool,
}

fn contains((min, max): (BlockPos, BlockPos), p: BlockPos) -> bool {
    p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && p.z >= min.z && p.z <= max.z
}

impl Session {
    /// Validate all records before creating a fresh world.
    pub fn new(snapshot: Snapshot) -> Result<Self> {
        let bounds = (snapshot.min, snapshot.max);
        let (min, max) = bounds;
        if min.x < 0
            || min.y < 0
            || min.z < 0
            || max.x >= PLOT_BLOCK_WIDTH
            || max.z >= PLOT_BLOCK_WIDTH
            || max.y >= PLOT_BLOCK_HEIGHT
            || max.x < min.x
            || max.y < min.y
            || max.z < min.z
            || max.x - min.x >= 256
            || max.y - min.y >= 256
            || max.z - min.z >= 256
        {
            bail!("invalid region bounds");
        }
        let mut blocks = HashMap::new();
        for &(pos, block) in &snapshot.blocks {
            if !contains(bounds, pos) || blocks.insert(pos, block).is_some() {
                bail!("invalid or duplicate block position {pos}");
            }
            if matches!(block, Block::MovingPiston { .. }) {
                bail!("moving pistons cannot be loaded without their motion state");
            }
        }
        let mut entities = HashSet::new();
        for (pos, entity) in &snapshot.entities {
            let block = blocks.get(pos).copied().unwrap_or(Block::Air);
            let compatible = match entity {
                BlockEntity::Comparator { output_strength } => matches!(block, Block::RedstoneComparator { .. }) && *output_strength <= 15,
                BlockEntity::Container { comparator_override, ty, .. } => ContainerType::from_block(block) == Some(*ty) && *comparator_override <= 15,
                BlockEntity::Sign(_) => block.is_sign(),
                BlockEntity::CommandBlock(_) => block.is_command_block(),
                BlockEntity::MovingPiston(_) => false,
            };
            if !contains(bounds, *pos) || !entities.insert(*pos) || !compatible {
                bail!("invalid or duplicate block entity position {pos}");
            }
        }
        for tick in &snapshot.ticks {
            if !contains(bounds, tick.pos)
                || tick.ticks_left >= 32
                || (tick.ticks_left == 0 && tick.tick_priority != TickPriority::NanoTick)
                || tick
                    .block_type
                    .is_some_and(|id| blocks.get(&tick.pos).copied().unwrap_or(Block::Air).registry_id() != id)
            {
                bail!("invalid scheduled tick at {}", tick.pos);
            }
        }

        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
        for (pos, block) in snapshot.blocks {
            world.set_block(pos, block);
        }
        for (pos, entity) in snapshot.entities {
            world.set_block_entity(pos, entity);
        }
        for tick in snapshot.ticks {
            world.restore_scheduled_tick(tick);
        }
        world.take_block_changes();
        Ok(Self {
            world,
            bounds,
            compiler: Compiler::default(),
            failed: false,
        })
    }

    fn ready(&self) -> Result<()> {
        if self.failed {
            bail!("session failed");
        }
        Ok(())
    }

    /// Compile once; a rejected compile leaves the interpreter snapshot intact.
    pub fn compile(&mut self, options: CompilerOptions) -> Result<&CompileStatistics> {
        self.ready()?;
        if self.compiler.is_active() {
            bail!("session is already compiled");
        }
        if options.io_only {
            bail!("headless sessions require full state projection");
        }
        let ticks = self.world.scheduler().iter_entries().collect();
        self.compiler
            .compile(&self.world, self.bounds, options, ticks, Arc::new(TaskMonitor::default()))
            .map_err(|error| anyhow::anyhow!("compile failed: {error:?}"))?;
        Ok(self.compiler.stats().expect("successful compile has statistics"))
    }

    pub fn advance(&mut self, ticks: u64) -> Result<()> {
        self.ready()?;
        for _ in 0..ticks {
            if self.compiler.is_active() {
                if let Err(error) = self.compiler.tick() {
                    self.failed = true;
                    bail!("compiled tick failed: {error:?}");
                }
                self.compiler.flush(&mut self.world);
            } else {
                self.world.tick_interpreted();
            }
        }
        Ok(())
    }

    pub fn use_block(&mut self, pos: BlockPos) -> Result<()> {
        self.ready()?;
        let block = self.get(pos)?;
        if self.compiler.is_active() {
            if !matches!(block, Block::Lever { .. } | Block::StoneButton { .. }) {
                bail!("unsupported compiled control at {pos}");
            }
            if let Err(error) = self.compiler.try_on_use_block(pos) {
                self.failed = true;
                bail!("compiled use failed: {error:?}");
            }
            self.compiler.flush(&mut self.world);
        } else if crate::interaction::use_redstone(block, &mut self.world, pos, None) != Some(crate::interaction::ActionResult::Success) {
            bail!("unsupported control at {pos}");
        }
        Ok(())
    }

    pub fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<()> {
        self.ready()?;
        let block = self.get(pos)?;
        if block.pressure_plate_powered().is_none() {
            bail!("not a pressure plate at {pos}");
        }
        if self.compiler.is_active() {
            if let Err(error) = self.compiler.try_set_pressure_plate(pos, powered) {
                self.failed = true;
                bail!("compiled plate failed: {error:?}");
            }
            self.compiler.flush(&mut self.world);
        } else {
            redstone::set_pressure_plate(&mut self.world, pos, block, powered);
        }
        Ok(())
    }

    pub fn get(&self, pos: BlockPos) -> Result<Block> {
        self.ready()?;
        if !contains(self.bounds, pos) {
            bail!("position outside region: {pos}");
        }
        Ok(self.world.get_block(pos))
    }

    pub fn drain_changes(&mut self) -> Result<Vec<(BlockPos, u32)>> {
        self.ready()?;
        Ok(self
            .world
            .take_block_changes()
            .into_iter()
            .filter(|(pos, _)| contains(self.bounds, *pos))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            min: BlockPos::new(20, 4, 30),
            max: BlockPos::new(22, 5, 31),
            blocks: vec![],
            entities: vec![],
            ticks: vec![],
        }
    }

    #[test]
    fn snapshot_validation_and_load_preserve_native_state() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        let comparator = Block::from_name("comparator").unwrap();
        input.blocks.push((pos, comparator));
        input.entities.push((pos, BlockEntity::Comparator { output_strength: 9 }));
        input.ticks.push(TickEntry {
            pos,
            ticks_left: 3,
            tick_priority: TickPriority::High,
            block_type: Some(comparator.registry_id()),
        });
        input.ticks.push(TickEntry {
            pos,
            ticks_left: 3,
            tick_priority: TickPriority::High,
            block_type: Some(comparator.registry_id()),
        });
        let mut session = Session::new(input).unwrap();
        assert_eq!(session.get(pos).unwrap(), comparator);
        assert_eq!(session.get(BlockPos::new(20, 4, 30)).unwrap(), Block::Air);
        assert!(matches!(
            session.world.get_block_entity(pos),
            Some(BlockEntity::Comparator { output_strength: 9 })
        ));
        let ticks: Vec<_> = session.world.scheduler().iter_entries().collect();
        assert_eq!(ticks.len(), 2);
        assert!(
            ticks
                .iter()
                .all(|tick| tick.ticks_left == 3 && tick.pos == pos && tick.tick_priority == TickPriority::High)
        );
        assert!(session.drain_changes().unwrap().is_empty());

        let mut duplicate = snapshot();
        duplicate.blocks = vec![(pos, comparator), (pos, Block::Air)];
        assert!(Session::new(duplicate).is_err());
        let mut invalid_tick = snapshot();
        invalid_tick.ticks.push(TickEntry {
            pos,
            ticks_left: 32,
            tick_priority: TickPriority::Normal,
            block_type: None,
        });
        assert!(Session::new(invalid_tick).is_err());
        let mut outside = snapshot();
        outside.blocks.push((BlockPos::new(19, 4, 30), comparator));
        assert!(Session::new(outside).is_err());
        let mut wrong_entity = snapshot();
        wrong_entity.blocks.push((pos, Block::from_name("chest").unwrap()));
        wrong_entity.entities.push((pos, BlockEntity::Comparator { output_strength: 2 }));
        assert!(Session::new(wrong_entity).is_err());
    }

    #[test]
    fn interpreter_controls_and_drain_use_plot_positions() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.blocks.push((pos, Block::from_name("lever").unwrap()));
        let mut session = Session::new(input).unwrap();
        session.use_block(pos).unwrap();
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if lever.powered));
        assert!(session.drain_changes().unwrap().iter().any(|(changed, _)| *changed == pos));
        assert!(session.drain_changes().unwrap().is_empty());
        assert!(session.use_block(BlockPos::new(20, 4, 30)).is_err());
        assert!(session.get(BlockPos::new(0, 0, 0)).is_err());
        session.advance(1).unwrap();
    }

    #[test]
    fn compiled_session_and_invalid_projection_option() {
        let mut session = Session::new(snapshot()).unwrap();
        let options = CompilerOptions {
            io_only: true,
            ..Default::default()
        };
        assert!(session.compile(options).is_err());
        assert_eq!(session.get(BlockPos::new(20, 4, 30)).unwrap(), Block::Air);
        session.compile(CompilerOptions::default()).unwrap();
        session.advance(1).unwrap();
        assert!(session.drain_changes().unwrap().is_empty());
    }
}
