//! Shared isolated native-interpreter world used by private compilers.
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld};
use crate::world::scheduler::{ScheduledBlockTick, TickScheduler};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_world::{AdvancePhase, PistonState, TickEntry};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub(crate) struct ScratchEntry {
    /// native scheduled block ticks captured at the snapshot boundary.
    pub(crate) scheduled: Vec<TickEntry>,
    /// native piston events and in-flight motions captured at the snapshot boundary.
    pub(crate) piston: PistonState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchError {
    /// world position associated with the failure, or `None` for global failures.
    pub position: Option<BlockPos>,
    /// stable explanation of the unsupported state or failed operation.
    pub reason: &'static str,
}

pub(crate) struct ScratchWorld {
    /// isolated mutable interpreter world used for compile-time probes.
    world: PlotWorld,
    /// baseline block/entity snapshot used to detect untracked probe writes.
    baseline: std::sync::Arc<HashMap<BlockPos, (u32, Option<mchprs_blocks::block_entities::BlockEntity>)>>,
    /// scheduled native ticks currently held by the scratch world.
    scheduled: Vec<TickEntry>,
    /// current piston state within the isolated probe world.
    piston: mchprs_world::PistonState,
    /// inclusive component capture bounds supplied to the compiler.
    bounds: (BlockPos, BlockPos),
    /// inclusive region whose reads are permitted during a probe.
    read_bounds: (BlockPos, BlockPos),
    /// chunks admitted for reads by the scratch-world boundary contract.
    read_chunks: HashSet<(i32, i32)>,
    /// positions contractually treated as air beyond loaded chunks.
    contract_air: HashSet<BlockPos>,
}

mod discovery;
pub(crate) use discovery::*;

#[cfg(test)]
mod tests;

impl ScratchWorld {
    #[cfg(test)]
    pub(crate) fn from_world(source: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Self, ScratchError> {
        Self::from_world_monitored(source, bounds, entry, None)
    }

    /// Create an isolated, bounded snapshot for interpreter probes.
    ///
    /// Inputs: The source world, capture bounds, scheduled/piston entry state and optional monitor.
    ///
    /// Validate coordinates and settled entry state, clone loaded chunks, record a reset baseline and
    /// boundary-air contract, then restore pending ticks and configure scratch read tracking.
    pub(crate) fn from_world_monitored(
        source: &impl World,
        bounds: (BlockPos, BlockPos),
        entry: ScratchEntry,
        monitor: Option<&super::TaskMonitor>,
    ) -> Result<Self, ScratchError> {
        let (min, max) = bounds;
        let invalid_bounds = || ScratchError {
            position: Some(min),
            reason: "invalid scratch capture bounds",
        };
        let (plot_x, plot_z) = source.plot_coordinates();
        let Some(plot_min_x) = plot_x.checked_mul(PLOT_BLOCK_WIDTH) else {
            return Err(invalid_bounds());
        };
        let Some(plot_min_z) = plot_z.checked_mul(PLOT_BLOCK_WIDTH) else {
            return Err(invalid_bounds());
        };
        let Some(plot_max_x) = plot_min_x.checked_add(PLOT_BLOCK_WIDTH - 1) else {
            return Err(invalid_bounds());
        };
        let Some(plot_max_z) = plot_min_z.checked_add(PLOT_BLOCK_WIDTH - 1) else {
            return Err(invalid_bounds());
        };
        if min.x < plot_min_x
            || min.y < 0
            || min.z < plot_min_z
            || max.x > plot_max_x
            || max.y >= PLOT_BLOCK_HEIGHT
            || max.z > plot_max_z
            || min.x > max.x
            || min.y > max.y
            || min.z > max.z
        {
            return Err(invalid_bounds());
        }
        let Some(probe_min_x) = min.x.checked_sub(1) else {
            return Err(invalid_bounds());
        };
        let Some(probe_min_y) = min.y.checked_sub(1) else {
            return Err(invalid_bounds());
        };
        let Some(probe_min_z) = min.z.checked_sub(1) else {
            return Err(invalid_bounds());
        };
        let Some(probe_max_x) = max.x.checked_add(1) else {
            return Err(invalid_bounds());
        };
        let Some(probe_max_y) = max.y.checked_add(1) else {
            return Err(invalid_bounds());
        };
        let Some(probe_max_z) = max.z.checked_add(1) else {
            return Err(invalid_bounds());
        };
        let probe_min = BlockPos::new(probe_min_x, probe_min_y, probe_min_z);
        let probe_max = BlockPos::new(probe_max_x, probe_max_y, probe_max_z);
        let read_bounds = (
            BlockPos::new(plot_min_x, 0, plot_min_z),
            BlockPos::new(plot_max_x, PLOT_BLOCK_HEIGHT - 1, plot_max_z),
        );
        if entry.piston.phase != AdvancePhase::BetweenTicks {
            return Err(ScratchError {
                position: None,
                reason: "scratch entry is mid-tick",
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
        if let Some(tick) = entry.scheduled.iter().find(|tick| !contains(bounds, tick.pos)) {
            return Err(ScratchError {
                position: Some(tick.pos),
                reason: "scratch entry has scheduled work outside compiled region",
            });
        }

        if let Some(tick) = entry.scheduled.iter().find(|tick| {
            tick.ticks_left as usize >= crate::world::scheduler::NUM_QUEUES
                || (tick.ticks_left == 0 && tick.tick_priority != mchprs_world::TickPriority::NanoTick)
        }) {
            return Err(ScratchError {
                position: Some(tick.pos),
                reason: "scratch entry has invalid scheduled delay or priority",
            });
        }

        let mut read_chunks = HashSet::new();
        let plot_chunk_min_x = plot_min_x.div_euclid(16);
        let plot_chunk_max_x = plot_max_x.div_euclid(16);
        let plot_chunk_min_z = plot_min_z.div_euclid(16);
        let plot_chunk_max_z = plot_max_z.div_euclid(16);
        let mut chunks = Vec::with_capacity((PLOT_WIDTH * PLOT_WIDTH) as usize);
        for chunk_x in plot_chunk_min_x..=plot_chunk_max_x {
            if monitor.is_some_and(super::TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            for chunk_z in plot_chunk_min_z..=plot_chunk_max_z {
                if let Some(chunk) = source.get_chunk(chunk_x, chunk_z) {
                    read_chunks.insert((chunk_x, chunk_z));
                    chunks.push(chunk.staging_clone());
                } else {
                    chunks.push(Chunk::empty(chunk_x, chunk_z));
                }
            }
        }
        for chunk_x in min.x.div_euclid(16)..=max.x.div_euclid(16) {
            for chunk_z in min.z.div_euclid(16)..=max.z.div_euclid(16) {
                if !read_chunks.contains(&(chunk_x, chunk_z)) {
                    return Err(ScratchError {
                        position: Some(BlockPos::new(chunk_x * 16, min.y, chunk_z * 16)),
                        reason: "scratch capture contains an unloaded chunk",
                    });
                }
            }
        }
        let mut world = PlotWorld::from_chunks(plot_x, plot_z, chunks, TickScheduler::<ScheduledBlockTick>::default());
        let mut baseline = HashMap::new();
        for y in min.y..=max.y {
            if monitor.is_some_and(super::TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            crate::world::for_each_block_optimized(source, BlockPos::new(min.x, y, min.z), BlockPos::new(max.x, y, max.z), |pos| {
                let raw = source.get_block_raw(pos);
                let entity = source.get_block_entity(pos).cloned();
                if raw != Block::Air.get_id() || entity.is_some() {
                    baseline.insert(pos, (raw, entity));
                }
            });
        }

        // Out-of-plot Air is admitted only in the selection's adjacent probe context.
        let mut contract_air = HashSet::new();
        for y in probe_min.y..=probe_max.y {
            if monitor.is_some_and(super::TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            for z in probe_min.z..=probe_max.z {
                for x in probe_min.x..=probe_max.x {
                    let pos = BlockPos::new(x, y, z);
                    if contains(bounds, pos) {
                        continue;
                    }
                    let in_plot =
                        (plot_min_x..=plot_max_x).contains(&x) && (plot_min_z..=plot_max_z).contains(&z) && (0..PLOT_BLOCK_HEIGHT).contains(&y);
                    if !in_plot {
                        if source.get_block(pos) == Block::Air && source.get_block_entity(pos).is_none() {
                            contract_air.insert(pos);
                        }
                        continue;
                    }
                }
            }
        }
        for tick in entry.scheduled.iter().cloned() {
            world.restore_scheduled_tick(tick);
        }
        world.piston_state_mut().clone_from(&entry.piston);
        world.configure_scratch(bounds, read_bounds, read_chunks.clone(), contract_air.clone());
        Ok(Self {
            world,
            baseline: std::sync::Arc::new(baseline),
            scheduled: entry.scheduled,
            piston: entry.piston,
            bounds,
            read_bounds,
            read_chunks,
            contract_air,
        })
    }

    pub(crate) fn world(&self) -> &PlotWorld {
        &self.world
    }
    pub(crate) fn world_mut(&mut self) -> &mut PlotWorld {
        &mut self.world
    }

    pub(crate) fn checkpoint(&mut self) -> Result<(), ScratchError> {
        self.finish_probe()?;
        let (blocks, entities) = self.world.take_scratch_changes();
        for pos in blocks.union(&entities) {
            let raw = self.world.get_block_raw(*pos);
            let entity = self.world.get_block_entity(*pos).cloned();
            if raw == Block::Air.get_id() && entity.is_none() {
                std::sync::Arc::make_mut(&mut self.baseline).remove(pos);
            } else {
                std::sync::Arc::make_mut(&mut self.baseline).insert(*pos, (raw, entity));
            }
        }
        self.scheduled = self.world.scheduler().iter_entries().collect();
        self.piston = self.world.piston_state().clone();
        self.revert();
        Ok(())
    }

    fn fork_basis(&mut self) -> Self {
        self.revert();
        let mut world = self.world.activation_snapshot();
        world.configure_scratch(self.bounds, self.read_bounds, self.read_chunks.clone(), self.contract_air.clone());
        Self {
            world,
            baseline: self.baseline.clone(),
            scheduled: self.scheduled.clone(),
            piston: self.piston.clone(),
            bounds: self.bounds,
            read_bounds: self.read_bounds,
            read_chunks: self.read_chunks.clone(),
            contract_air: self.contract_air.clone(),
        }
    }

    /// Run independent probes with bounded reusable scratch snapshots.
    ///
    /// Inputs: The jobs, optional monitor and probe callback, using this scratch world as the basis.
    ///
    /// Partition work across the configured probe workers, restore isolated scratch state between jobs and
    /// collect results in input order while propagating cancellation/errors.
    pub(crate) fn probe_jobs<J: Sync, R: Send>(
        &mut self,
        jobs: &[J],
        monitor: Option<&super::TaskMonitor>,
        probe: impl Fn(&mut Self, &J) -> Result<R, ScratchError> + Sync,
    ) -> Result<Vec<R>, ScratchError> {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        if let Some(monitor) = monitor {
            monitor.set_progress(0, jobs.len(), "probes");
        }
        let workers = rayon::current_num_threads().min((jobs.len() / 128).max(1));
        if workers == 1 {
            return jobs
                .iter()
                .map(|job| {
                    self.revert();
                    if monitor.is_some_and(super::TaskMonitor::cancelled) {
                        return Err(ScratchError {
                            position: None,
                            reason: "compilation cancelled",
                        });
                    }
                    let result = probe(self, job)?;
                    if let Some(monitor) = monitor {
                        monitor.advance_progress();
                    }
                    Ok(result)
                })
                .collect();
        }
        let next = AtomicUsize::new(0);
        let stopped = AtomicBool::new(false);
        let scratches = (0..workers)
            .map(|_| {
                if monitor.is_some_and(super::TaskMonitor::cancelled) {
                    Err(ScratchError {
                        position: None,
                        reason: "compilation cancelled",
                    })
                } else {
                    Ok(self.fork_basis())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Reuse one isolated snapshot per worker instead of copying the world for every probe.
        let worker_results: Vec<_> = scratches
            .into_par_iter()
            .map(|mut scratch| {
                let mut results = Vec::new();
                while !stopped.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(index) else { break };
                    scratch.revert();
                    let result = if monitor.is_some_and(super::TaskMonitor::cancelled) {
                        Err(ScratchError {
                            position: None,
                            reason: "compilation cancelled",
                        })
                    } else {
                        probe(&mut scratch, job)
                    };
                    if result.is_err() {
                        stopped.store(true, Ordering::Relaxed);
                    } else if let Some(monitor) = monitor {
                        monitor.advance_progress();
                    }
                    results.push((index, result));
                }
                results
            })
            .collect();
        let mut results: Vec<_> = worker_results.into_iter().flatten().collect();
        // Workers finish in arbitrary order; restore the caller's job order before returning.
        results.sort_unstable_by_key(|(index, _)| *index);
        results.into_iter().map(|(_, result)| result).collect()
    }

    /// Restore a scratch probe to its captured baseline.
    ///
    /// Inputs: This scratch world's tracked modified positions and original blocks/entities, ticks and piston state.
    ///
    /// Restore changed block/entity records, reset probe bookkeeping and queues, then reinstall the
    /// captured scheduled ticks without rebuilding the snapshot.
    pub(crate) fn revert(&mut self) {
        let (blocks, entities) = self.world.take_scratch_changes();
        let mut changed: Vec<_> = blocks.union(&entities).copied().collect();
        changed.sort_unstable_by_key(|pos| (pos.y, pos.z, pos.x));
        for pos in changed {
            let (raw, entity) = self.baseline.get(&pos).cloned().unwrap_or((Block::Air.get_id(), None));
            if self.world.get_block_raw(pos) != raw {
                self.world.set_block_raw(pos, raw);
            }
            self.world.delete_block_entity(pos);
            if let Some(entity) = entity {
                self.world.set_block_entity(pos, entity);
            }
        }
        self.world.reset_scratch_probe_state(&self.piston);
        for tick in self.scheduled.iter().cloned() {
            self.world.restore_scheduled_tick(tick);
        }
    }

    /// Reject probe reads or effects that leave the captured contract.
    ///
    /// Inputs: This scratch world's tracked boundary accesses, pending events, motion and scheduled work.
    ///
    /// Check boundary writes and reads plus every pending-work position, returning the first unsafe
    /// position instead of accepting an incomplete observation.
    pub(crate) fn finish_probe(&self) -> Result<(), ScratchError> {
        if let Some(position) = self.world.take_scratch_boundary_effect() {
            return Err(ScratchError {
                position: Some(position),
                reason: "scratch probe effect outside compiled region",
            });
        }
        for event in &self.world.piston_state().events {
            if !contains(self.bounds, event.pos) {
                return Err(ScratchError {
                    position: Some(event.pos),
                    reason: "scratch probe queued piston work outside compiled region",
                });
            }
        }
        for (pos, _) in &self.world.piston_state().movement_work {
            if !contains(self.bounds, *pos) {
                return Err(ScratchError {
                    position: Some(*pos),
                    reason: "scratch probe queued piston work outside compiled region",
                });
            }
        }
        for motion in &self.world.piston_state().motions {
            if !contains(self.bounds, motion.pos) {
                return Err(ScratchError {
                    position: Some(motion.pos),
                    reason: "scratch probe piston motion outside compiled region",
                });
            }
        }
        if let Some(tick) = self.world.scheduler().iter_entries().find(|tick| !contains(self.bounds, tick.pos)) {
            return Err(ScratchError {
                position: Some(tick.pos),
                reason: "scratch probe scheduled work outside compiled region",
            });
        }
        if let Some(position) = self.world.take_scratch_boundary_read() {
            return Err(ScratchError {
                position: Some(position),
                reason: "scratch probe read outside captured boundary",
            });
        }
        Ok(())
    }
}

fn contains((min, max): (BlockPos, BlockPos), pos: BlockPos) -> bool {
    (min.x..=max.x).contains(&pos.x) && (min.y..=max.y).contains(&pos.y) && (min.z..=max.z).contains(&pos.z)
}
