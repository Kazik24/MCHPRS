//! Copy compiled state and moving-piston identities into the host world.
use super::{Compiler, ExecutionError};
use crate::{
    redpilator::{PublicationMode, sampled::ScratchError},
    redstone,
    world::World,
};
use mchprs_blocks::{block_entities::BlockEntity, blocks::Block};
use mchprs_world::{AdvancePhase, PistonMotion};

pub(super) struct ExportedMotion {
    /// host motion identity used to correlate updates across publications.
    identity: u64,
    /// whether this host motion was observed in the current publication pass.
    seen: bool,
}

impl Compiler {
    /// Change visible publication without recompiling or advancing execution.
    pub fn set_publication_mode(&mut self, mode: PublicationMode) -> bool {
        let Some(runtime) = self.runtime.as_mut() else {
            return false;
        };
        if self.options.publication_mode() != mode {
            runtime.invalidate_publication();
        }
        self.options.publication = Some(mode);
        self.options.io_only = mode == PublicationMode::Io;
        true
    }

    pub(super) fn prepare_publication_buffers(&mut self, capacity: usize) {
        self.publication_parts.clear();
        self.publication_parts.reserve(capacity);
        self.publication_positions.clear();
        self.publication_positions.reserve(capacity);
        self.retired_motions.clear();
        self.retired_motions.reserve(capacity);
        self.host_motion_indices.clear();
        self.host_motion_indices.reserve(capacity);
        self.exported_motions.clear();
        self.exported_motions.reserve(capacity);
    }

    pub fn flush<W: World>(&mut self, world: &mut W) {
        if let Err(error) = self.try_flush(world) {
            tracing::error!(%error, "sampled state publication failed");
        }
    }

    pub fn try_flush<W: World>(&mut self, world: &mut W) -> Result<(), ExecutionError> {
        let mode = self.options.publication_mode();
        self.publish(world, mode == PublicationMode::Io, mode == PublicationMode::All)
    }

    /// Publish command output and note effects without publishing block geometry.
    ///
    /// Inputs: The active backend and mutable host world.
    ///
    /// Synchronize native effects into the Rust projection, publish commands and drain notes, then
    /// acknowledge the output queue to the native backend.
    pub(crate) fn flush_effects<W: World>(&mut self, world: &mut W) {
        if let Some(llvm) = self.llvm_runtime.as_ref() {
            if let Err(error) = llvm.sync_publication(self.runtime.as_mut().expect("LLVM publication projection missing")) {
                tracing::error!(?error, "LLVM effect read failed");
                return;
            }
        }
        let runtime = self.runtime.as_mut().expect("compiler inactive");
        runtime.publish_commands(world, true);
        for (component, pos, instrument, note) in runtime.notes.drain(..) {
            redstone::noteblock::play_note(world, pos, instrument, note);
            runtime.note_queued[component] = false;
        }
        if let Some(llvm) = self.llvm_runtime.as_mut() {
            llvm.acknowledge_effects(runtime.queued_command_outputs());
        }
    }

    pub(crate) fn try_flush_complete<W: World>(&mut self, world: &mut W) -> Result<(), ExecutionError> {
        self.publish(world, false, true)
    }

    /// Copy the compiled state into the world at the requested visibility level.
    ///
    /// Inputs: The host world, an IO-only flag, a full-publication flag and retained motion identities.
    ///
    /// Synchronize the native projection, reserve motion identities, reconcile blocks and piston parts,
    /// then publish effects and clear the published dirty marks.
    pub(super) fn publish<W: World>(&mut self, world: &mut W, io_only: bool, full: bool) -> Result<(), ExecutionError> {
        if let Some(llvm) = self.llvm_runtime.as_ref() {
            llvm.sync_publication(self.runtime.as_mut().expect("LLVM publication projection missing"))
                .map_err(ExecutionError)?;
        }
        self.runtime
            .as_mut()
            .expect("compiler inactive")
            .prepare_publication(io_only || !full)
            .map_err(ExecutionError)?;
        let runtime = self.runtime.as_ref().unwrap();
        if full && !io_only {
            runtime
                .write_moving_parts(&mut self.publication_parts, &mut self.publication_positions)
                .map_err(ExecutionError)?;
            // Reserve identities before host setters register any new parts.
            if world
                .piston_state()
                .next_identity
                .checked_add((self.publication_parts.len() as u64).saturating_mul(2))
                .is_none()
            {
                return Err(ExecutionError(ScratchError {
                    position: None,
                    reason: "piston motion identity limit exceeded",
                }));
            }
            self.publication_phase = !self.publication_phase;
            let mut next_identity = world.piston_state().next_identity;
            for part in &self.publication_parts {
                let entry = self.exported_motions.entry(part.key).or_insert_with(|| {
                    next_identity += 1;
                    ExportedMotion {
                        identity: next_identity,
                        seen: self.publication_phase,
                    }
                });
                entry.seen = self.publication_phase;
            }
            world.piston_state_mut().next_identity = next_identity;
            self.retired_motions.clear();
            self.exported_motions.retain(|key, entry| {
                if entry.seen == self.publication_phase {
                    true
                } else {
                    self.retired_motions.insert(entry.identity);
                    world.delete_block_entity(key.3);
                    false
                }
            });
        }
        for (pos, block, comparator) in runtime.publication_changes(full) {
            if io_only && !is_output(block) {
                continue;
            }
            if full && !io_only && self.publication_positions.contains(&pos) {
                continue;
            }
            let previous = world.get_block(pos);
            if !full && previous.registry_id() != block.registry_id() {
                continue;
            }
            if previous != block {
                world.set_block(pos, block);
            }
            if !io_only && let Some(output_strength) = comparator {
                if let Some(BlockEntity::Comparator { output_strength: current }) = world.get_block_entity_mut(pos) {
                    *current = output_strength;
                }
            }
        }
        if full && !io_only {
            for part in &self.publication_parts {
                if world.get_block(part.pos) != part.block {
                    world.set_block(part.pos, part.block);
                }
                if let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity_mut(part.pos) {
                    *entity = part.entity;
                } else {
                    world.set_block_entity(part.pos, BlockEntity::MovingPiston(part.entity));
                }
            }
            let logical_tick = runtime.logical_tick();
            let state = world.piston_state_mut();
            self.host_motion_indices.clear();
            let mut index = 0;
            state.motions.retain(|motion| {
                if self.retired_motions.contains(&motion.identity) {
                    return false;
                }
                if self.publication_positions.contains(&motion.pos) {
                    if self.host_motion_indices.contains_key(&motion.pos) {
                        return false;
                    }
                    self.host_motion_indices.insert(motion.pos, index);
                }
                index += 1;
                true
            });
            for part in &self.publication_parts {
                let identity = self.exported_motions[&part.key].identity;
                if let Some(&index) = self.host_motion_indices.get(&part.pos) {
                    let motion = &mut state.motions[index];
                    motion.identity = identity;
                    motion.progress = part.progress;
                    motion.previous_progress = (part.progress - 0.5).max(0.0);
                    motion.last_tick = logical_tick;
                } else {
                    state.motions.push_back(PistonMotion {
                        pos: part.pos,
                        identity,
                        progress: part.progress,
                        previous_progress: (part.progress - 0.5).max(0.0),
                        last_tick: logical_tick,
                        carried_entity: None,
                    });
                }
            }
            state.logical_tick = logical_tick;
            state.phase = AdvancePhase::BetweenTicks;
            state.scheduled_advanced = false;
            state.events.retain(|event| !runtime.owns_piston(event.pos));
            state.events.extend(runtime.pending_piston_event_iter());
            state.movement_work.clear();
            state.movement_cursor = 0;
        }
        let runtime = self.runtime.as_mut().unwrap();
        runtime.publish_commands(world, io_only);
        for (component, pos, instrument, note) in runtime.notes.drain(..) {
            redstone::noteblock::play_note(world, pos, instrument, note);
            runtime.note_queued[component] = false;
        }
        runtime.mark_published(io_only || !full);
        if let Some(llvm) = self.llvm_runtime.as_mut() {
            llvm.acknowledge_effects(runtime.queued_command_outputs());
        }
        Ok(())
    }
}

fn is_output(block: Block) -> bool {
    matches!(block, Block::Lever { .. } | Block::StoneButton { .. } | Block::RedstoneLamp { .. })
        || block.is_copper_bulb()
        || block.pressure_plate_powered().is_some()
}
