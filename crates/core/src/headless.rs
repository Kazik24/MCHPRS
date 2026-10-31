//! Native, playerless simulation sessions. Positions use plot coordinates.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};

use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld};
pub use crate::redpiler::{CompileStatistics, CompilerOptions};
use crate::redpiler::{Compiler, TaskMonitor, TickScheduler};
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

/// Inputs are applied in order before the next game tick.
pub enum Input {
    Use(BlockPos),
    Plate(BlockPos, bool),
}

/// A session owns its world and compiler until it is dropped.
pub struct Session {
    world: PlotWorld,
    bounds: (BlockPos, BlockPos),
    compiler: Compiler,
    failed: bool,
    ticks: u64,
}

fn contains((min, max): (BlockPos, BlockPos), p: BlockPos) -> bool {
    p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && p.z >= min.z && p.z <= max.z
}

impl Session {
    /// Create a fresh block-only session from a Sponge schematic. Its minimum
    /// corner is normalized to the native session origin.
    pub fn from_sponge_schematic(bytes: &[u8], max_cells: u32) -> Result<(Self, [u32; 3])> {
        use std::io::Cursor;
        let clipboard = crate::plot::worldedit::load_block_schematic(Cursor::new(bytes), max_cells)?;
        let size = [clipboard.size_x, clipboard.size_y, clipboard.size_z];
        let cells = size
            .iter()
            .try_fold(1u32, |n, d| n.checked_mul(*d))
            .ok_or_else(|| anyhow::anyhow!("schematic dimensions overflow"))?;
        if cells > max_cells || size.iter().any(|d| *d > 64) {
            bail!("schematic exceeds API dimensions or cell limit");
        }
        let origin = BlockPos::new(4, 4, 4);
        let mut blocks = Vec::with_capacity(cells as usize);
        for x in 0..size[0] as i32 {
            for y in 0..size[1] as i32 {
                for z in 0..size[2] as i32 {
                    let index = (x + z * size[0] as i32 + y * size[0] as i32 * size[2] as i32) as usize;
                    let block = Block::from_id(clipboard.data.get_entry(index));
                    if !matches!(block, Block::Air) {
                        blocks.push((origin + BlockPos::new(x, y, z), block));
                    }
                }
            }
        }
        let mut session = Self::new(Snapshot {
            min: origin,
            max: origin + BlockPos::new(size[0] as i32 - 1, size[1] as i32 - 1, size[2] as i32 - 1),
            blocks,
            entities: vec![],
            ticks: vec![],
        })?;
        let (min, max) = session.bounds;
        crate::world::for_each_block_mut_optimized(&mut session.world, min, max, |world, pos| {
            redstone::update(world.get_block(pos), world, pos, None);
        });
        session.world.take_block_changes();
        Ok((session, size))
    }

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
            ticks: 0,
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
        self.world.clear_scheduled_ticks();
        Ok(self.compiler.stats().expect("successful compile has statistics"))
    }

    pub fn advance(&mut self, ticks: u64) -> Result<()> {
        self.ready()?;
        if self.ticks.checked_add(ticks).is_none() {
            bail!("tick count overflow");
        }
        for _ in 0..ticks {
            if self.compiler.is_active() {
                if let Err(error) = self.compiler.tick() {
                    self.failed = true;
                    bail!("compiled tick failed: {error:?}");
                }
                if let Err(error) = self.compiler.try_flush(&mut self.world) {
                    self.failed = true;
                    bail!("compiled flush failed: {error}");
                }
            } else {
                self.world.tick_interpreted();
            }
            self.ticks += 1;
        }
        Ok(())
    }

    /// Apply validated inputs in order, then advance. Any error after an input fails the session.
    pub fn advance_with_inputs(&mut self, ticks: u64, inputs: &[Input]) -> Result<()> {
        self.ready()?;
        if self.ticks.checked_add(ticks).is_none() {
            bail!("tick count overflow");
        }
        for input in inputs {
            let pos = match input {
                Input::Use(pos) | Input::Plate(pos, _) => *pos,
            };
            let block = self.get(pos)?;
            let supported = match input {
                Input::Use(_) if self.compiler.is_active() => matches!(block, Block::Lever { .. } | Block::StoneButton { .. }),
                Input::Use(_) => matches!(
                    block,
                    Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneWire { .. }
                        | Block::NoteBlock { .. }
                ),
                Input::Plate(_, _) => block.pressure_plate_powered().is_some(),
            };
            let supported = supported && (!self.compiler.is_active() || matches!(input, Input::Use(_)) && self.compiler.accepts_control(pos));
            if !supported {
                bail!("unsupported input at {pos}");
            }
        }
        for input in inputs {
            let result = match input {
                Input::Use(pos) => self.use_block(*pos),
                Input::Plate(pos, powered) => self.set_pressure_plate(*pos, *powered),
            };
            if let Err(error) = result {
                self.failed = true;
                return Err(error);
            }
        }
        self.advance(ticks)
    }

    pub fn use_block(&mut self, pos: BlockPos) -> Result<()> {
        self.ready()?;
        let block = self.get(pos)?;
        if self.compiler.is_active() {
            if !matches!(block, Block::Lever { .. } | Block::StoneButton { .. }) {
                bail!("unsupported compiled control at {pos}");
            }
            if !self.compiler.accepts_control(pos) {
                bail!("control is not present in the compiled circuit at {pos}");
            }
            if let Err(error) = self.compiler.try_on_use_block(pos) {
                self.failed = true;
                bail!("compiled use failed: {error:?}");
            }
            if let Err(error) = self.compiler.try_flush(&mut self.world) {
                self.failed = true;
                bail!("compiled flush failed: {error}");
            }
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
            self.failed = true;
            bail!("pressure plate inputs are unsupported in compiled mode at {pos}");
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

    /// Read the redstone power seen from a face at a position in the session.
    pub fn power(&self, pos: BlockPos, face: BlockFace) -> Result<u8> {
        let block = self.get(pos)?;
        Ok(redstone::get_redstone_power(block, &self.world, pos, face))
    }

    /// Completed game ticks, including those completed before an execution failure.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn block_entity(&self, pos: BlockPos) -> Result<Option<&BlockEntity>> {
        self.get(pos)?;
        if self.compiler.is_active() {
            bail!("block entity inspection is unavailable in compiled mode");
        }
        Ok(self.world.get_block_entity(pos))
    }

    /// Compiled pending work is owned by the compiler and cannot be inspected here.
    pub fn pending_ticks(&self) -> Result<Vec<TickEntry>> {
        self.ready()?;
        if self.compiler.is_active() {
            bail!("pending ticks are unavailable in compiled mode");
        }
        Ok(self.world.scheduler().iter_entries().collect())
    }

    /// Return compiled state and scheduled work to the interpreter.
    pub fn reset(&mut self) -> Result<()> {
        self.ready()?;
        if self.compiler.is_active() {
            self.compiler
                .try_reset(&mut self.world, self.bounds)
                .map_err(|error| anyhow::anyhow!("compiled reset failed: {error}"))?;
        }
        Ok(())
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
    fn compiled_session_requires_full_projection() {
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

    #[test]
    fn compiled_reset_hands_pending_ticks_back_once() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.min = BlockPos::new(20, 3, 29);
        let block = Block::RedstoneLamp { lit: false };
        input.blocks.push((pos, block));
        input.ticks.push(TickEntry {
            pos,
            ticks_left: 4,
            tick_priority: TickPriority::Normal,
            block_type: Some(block.registry_id()),
        });
        let mut session = Session::new(input).unwrap();
        let before = session.pending_ticks().unwrap();
        assert!(
            session
                .compile(CompilerOptions {
                    io_only: true,
                    ..Default::default()
                })
                .is_err()
        );
        assert_eq!(session.pending_ticks().unwrap(), before);
        session.compile(CompilerOptions::default()).unwrap();
        assert!(session.pending_ticks().is_err());
        assert_eq!(session.world.scheduler().iter_entries().count(), 0);
        session.reset().unwrap();
        let ticks = session.pending_ticks().unwrap();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].pos, pos);
        assert_eq!(ticks[0].ticks_left, 4);
    }

    #[test]
    fn power_and_ordered_controls_use_the_current_state() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.blocks.push((pos, Block::from_name("lever").unwrap()));
        let mut session = Session::new(input).unwrap();
        session.use_block(pos).unwrap();
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if lever.powered));
        session.use_block(pos).unwrap();
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
        session.advance(2).unwrap();
        assert_eq!(session.ticks(), 2);
        assert!(session.power(BlockPos::new(0, 0, 0), BlockFace::Top).is_err());
        assert!(session.drain_changes().unwrap().iter().any(|(changed, _)| *changed == pos));
        assert!(session.drain_changes().unwrap().is_empty());
    }

    #[test]
    fn input_batch_validates_before_mutating_and_preserves_order() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.blocks.push((pos, Block::from_name("lever").unwrap()));
        let mut session = Session::new(input).unwrap();
        assert!(
            session
                .advance_with_inputs(1, &[Input::Use(pos), Input::Use(BlockPos::new(20, 4, 30))])
                .is_err()
        );
        assert_eq!(session.ticks(), 0);
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
        session.advance_with_inputs(1, &[Input::Use(pos), Input::Use(pos)]).unwrap();
        assert_eq!(session.ticks(), 1);
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
    }

    #[test]
    fn unsupported_compile_keeps_the_headless_session_native() {
        use mchprs_blocks::block_entities::CommandBlockEntity;
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.blocks.push((pos, Block::from_name("command_block").unwrap()));
        input.entities.push((
            pos,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                command: "say test".into(),
                automatic: true,
                ..Default::default()
            })),
        ));
        let mut session = Session::new(input).unwrap();
        assert!(session.compile(CompilerOptions::default()).is_err());
        session.advance(1).unwrap();
        assert_eq!(session.ticks(), 1);
        assert_eq!(session.get(pos).unwrap(), Block::from_name("command_block").unwrap());
    }

    #[test]
    fn drain_reports_air_and_consumes_it_once() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.blocks.push((pos, Block::from_name("stone").unwrap()));
        let mut session = Session::new(input).unwrap();
        session.world.set_block(pos, Block::Air);
        assert!(
            session
                .drain_changes()
                .unwrap()
                .iter()
                .any(|(changed, state)| *changed == pos && *state == Block::get_id(Block::Air))
        );
        assert!(session.drain_changes().unwrap().is_empty());
    }

    #[test]
    fn inspect_power_and_block_entity() {
        let pos = BlockPos::new(21, 4, 30);
        let mut input = snapshot();
        input.blocks.push((pos, Block::RedstoneBlock {}));
        let session = Session::new(input).unwrap();
        assert_eq!(session.power(pos, BlockFace::Top).unwrap(), 15);
        assert!(session.block_entity(pos).unwrap().is_none());
    }
}
