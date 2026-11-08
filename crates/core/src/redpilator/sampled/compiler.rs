//! Compile a world snapshot through the ordered discovery and instruction passes.
use super::{
    Program, ScratchEntry, ScratchError, SourceOrderCache,
    passes::{self, CompilePhases},
};
use crate::{redpilator::TaskMonitor, world::World};
use mchprs_blocks::BlockPos;
use std::{collections::HashMap, time::Instant};

pub(crate) fn compile(world: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Program, ScratchError> {
    compile_monitored(world, bounds, entry, None)
}

/// Build a sampled program from a world snapshot, checking for cancellation between stages.
pub(crate) fn compile_monitored(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: ScratchEntry,
    monitor: Option<&TaskMonitor>,
) -> Result<Program, ScratchError> {
    compile_monitored_with_source_cache(world, bounds, entry, monitor, None)
}

pub(crate) fn compile_monitored_with_source_cache(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: ScratchEntry,
    monitor: Option<&TaskMonitor>,
    source_cache: Option<&SourceOrderCache>,
) -> Result<Program, ScratchError> {
    let mut phases = CompilePhases::new(monitor);
    let context = passes::Context {
        world,
        bounds,
        entry: &entry,
        monitor,
    };

    let (scratch, inventory) = passes::parse::run(&context, &mut phases)?;
    let index_started = Instant::now();
    let position_index: HashMap<_, _> = inventory.positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();

    if phases.enabled {
        eprintln!("Redpilator compile position index: {} ms", index_started.elapsed().as_millis());
    }

    let (mut program, probe_indexes) = passes::electrical::run(&context, scratch, inventory, &position_index, &mut phases)?;

    passes::movement::run(&context, &mut program, &probe_indexes, &mut phases)?;
    passes::arrangements::run(&context, &mut program, &probe_indexes, &mut phases)?;
    passes::reactive::run(&context, &mut program, &probe_indexes, &mut phases)?;
    passes::source_order::run(&context, &mut program, &probe_indexes, &mut phases, source_cache)?;
    passes::finalize::run(&mut program, &mut phases)?;
    passes::share_output_programs::run(&mut program, &mut phases)?;

    Ok(program)
}
