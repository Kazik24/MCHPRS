use super::*;

/// Boundary metadata for one interpreter motion. Derived only during reset
/// from an event deadline; graph execution never stores progress.
struct MotionProjection {
    position: BlockPos,
    actor: RedstonePiston,
    carried: Block,
    extending: bool,
    source: bool,
    /// Ticks until the event publishes; the interpreter restores on the
    /// movement visit after progress reaches 1.
    remaining: u64,
}

impl MotionProjection {}

impl GraphBackend {
    fn is_io(&self, node: NodeId) -> bool {
        matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Observer { .. }
                | CircuitNodeKind::BuiltinReset { .. }
                | CircuitNodeKind::Electrical(
                    NodeType::Lever
                        | NodeType::Button
                        | NodeType::PressurePlate
                        | NodeType::Lamp
                        | NodeType::CopperBulb
                        | NodeType::Hopper
                        | NodeType::NoteBlock { .. }
                        | NodeType::CommandBlock { .. }
                )
        )
    }

    /// Presentation only: writes current graph state and drains effects. It
    /// reads no World state, so suppressed display cannot alter execution.
    pub(crate) fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        if self.kernel_cursor.is_some() {
            assert!(self.materialize_kernel_state(), "certified kernel projection must materialize");
        }
        for index in 0..self.dirty.len() {
            let node = NodeId::try_from_index(index).unwrap();
            // Hidden nodes stay dirty; reset materializes every node regardless.
            if !self.dirty[index] || (io_only && !self.is_io(node)) {
                continue;
            }
            self.write_node(world, node);
            self.dirty[index] = false;
        }
        if !io_only {
            for contact in 0..self.program.contacts().len() {
                let contact = ContactId::new(contact as u32);
                let position = self.program.contacts()[contact.index()].position;
                if self.cell_owned_by_node(position) {
                    continue;
                }
                // Clients see pending cells as empty; reset supplies interpreter motion.
                let block = match self.contact_block(contact) {
                    Block::MovingPiston { .. } => Block::Air,
                    block => block,
                };
                world.set_block(position, block);
            }
        }
        for effect in self.effects.drain(..) {
            match effect {
                PresentationEffect::BlockAction { .. } if io_only => {}
                PresentationEffect::BlockAction { position, action } => world.block_action(position, action),
                PresentationEffect::Sound {
                    position,
                    id,
                    category,
                    volume,
                    pitch,
                } => world.play_sound(position, id, category, volume, pitch),
            }
        }
    }

    /// Whether a node's state owns its cell. Constants are compile-time
    /// templates; a moved redstone resource is owned by its contacts instead.
    fn owns_cell(&self, node: NodeId) -> bool {
        matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Wire(_)
        ) || matches!(
            &self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(kind) if *kind != NodeType::Constant
        )
    }

    fn cell_owned_by_node(&self, position: BlockPos) -> bool {
        self.positions.get(&position).is_some_and(|&node| self.owns_cell(node))
    }

    fn write_node<W: World>(&self, world: &mut W, node: NodeId) {
        let spec = &self.program.nodes()[node.index()];
        let Some(position) = spec.position else {
            return;
        };
        if !self.owns_cell(node) {
            return;
        }
        world.set_block(position, self.node_block(node));
        if matches!(spec.kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. })) {
            world.set_block_entity(
                position,
                BlockEntity::Comparator {
                    output_strength: self.node_state(node).output_strength,
                },
            );
        }
        if matches!(spec.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }))
            && let Some(entity) = self.node_state(node).command_entity.clone()
        {
            world.set_block_entity(position, BlockEntity::CommandBlock(Box::new(entity)));
        }
    }

    /// Hand the current graph future back to the interpreter at a tick
    /// boundary and drop graph-owned pending work. Pending transitions are
    /// reproduced, never settled.
    pub(crate) fn reset<W: World>(&mut self, world: &mut W) {
        self.flush(world, false);
        self.kernel_cursor = None;
        self.project_future(world);
        self.dirty.fill(false);
        self.scheduler.clear();
        self.pending.clear();
        self.piston_requests.clear();
        self.deferred_pistons.clear();
    }

    /// Write the interpreter state equivalent to the current graph future:
    /// component and contact blocks, motions derived from live deferred
    /// events, ordinary scheduled ticks, queued requests, and the clock.
    /// Reads no World state and changes no graph state.
    pub(crate) fn project_future<W: World>(&self, world: &mut W) {
        if self.kernel_cursor.is_some() {
            let mut projected = GraphBackend::new(self.program.clone(), self.state.clone(), self.clock.logical_tick)
                .expect("a validated graph program can project its kernel state");
            projected.clock = self.clock;
            projected.kernel_cursor = self.kernel_cursor.clone();
            assert!(projected.materialize_kernel_state(), "certified kernel projection must materialize");
            projected.kernel_cursor = None;
            projected.project_future(world);
            return;
        }
        debug_assert_eq!(self.clock.phase, GraphPhase::BetweenTicks);
        let interpreter = world.piston_state_mut();
        interpreter.logical_tick = self.clock.logical_tick;
        interpreter.phase = mchprs_world::AdvancePhase::BetweenTicks;
        interpreter.scheduled_advanced = false;
        interpreter.events.clear();
        interpreter.movement_work.clear();
        interpreter.movement_cursor = 0;

        for index in 0..self.program.nodes().len() {
            self.write_node(world, NodeId::try_from_index(index).unwrap());
        }
        let motions = self.pending_motions();
        for index in 0..self.program.contacts().len() {
            let contact = ContactId::new(index as u32);
            let position = self.program.contacts()[index].position;
            if self.cell_owned_by_node(position) || motions.iter().any(|motion| motion.position == position) {
                continue;
            }
            let block = self.contact_block(contact);
            debug_assert!(
                !matches!(block, Block::MovingPiston { .. }),
                "pending contact at {position:?} has no live deferred event"
            );
            world.set_block(position, block);
        }
        for motion in &motions {
            motion.materialize(world);
        }

        for (tick, delay, priority) in self.scheduler.iter() {
            world.restore_scheduled_tick(TickEntry {
                pos: tick.position,
                block_type: tick.block_type,
                ticks_left: delay as u32,
                tick_priority: priority,
            });
        }
        for request in &self.piston_requests {
            let spec = self.program.pistons()[request.piston.index()];
            world.enqueue_piston_event(PistonEvent {
                pos: self.program.contacts()[spec.base.index()].position,
                sticky: spec.sticky,
                facing: spec.facing,
                action: request.action,
            });
        }
    }

    /// Interpreter motions implied by live deferred events, in native creation
    /// order. Stale actor or resource parts were interrupted or replaced and
    /// have no interpreter motion.
    fn pending_motions(&self) -> Vec<MotionProjection> {
        let mut motions = Vec::new();
        for event in &self.deferred_pistons {
            let spec = self.program.pistons()[event.piston.index()];
            let piston = RedstonePiston {
                facing: spec.facing.into(),
                sticky: spec.sticky,
                extended: false,
            };
            let remaining = event.due_tick.saturating_sub(self.clock.logical_tick);
            debug_assert!((1..=2).contains(&remaining), "deferred event outside its window");
            let position = |contact: ContactId| self.program.contacts()[contact.index()].position;
            let actor = (self.state.pistons[event.piston.index()].revision == event.revision).then(|| MotionProjection {
                position: position(if event.extending { spec.near } else { spec.base }),
                actor: piston,
                carried: if event.extending {
                    Block::PistonHead { head: piston.into() }
                } else {
                    Block::Piston { piston }
                },
                extending: event.extending,
                source: true,
                remaining,
            });
            let resource = event.resource.and_then(|transfer| {
                let state = self.state.resources[transfer.resource.index()];
                (state.revision == transfer.revision && state.at == transfer.contact && !state.enabled).then(|| MotionProjection {
                    position: position(transfer.contact),
                    actor: piston,
                    carried: self
                        .program
                        .profile_block(self.program.resources()[transfer.resource.index()].material)
                        .expect("resource material profile was validated"),
                    extending: event.extending,
                    source: false,
                    remaining,
                })
            });
            // Native extension installs the payload before the head; retraction
            // moves the base before the pulled payload.
            if event.extending {
                motions.extend(resource);
                motions.extend(actor);
            } else {
                motions.extend(actor);
                motions.extend(resource);
            }
        }
        motions
    }
}

impl MotionProjection {
    fn materialize<W: World>(&self, world: &mut W) {
        let advances = 3.0 - self.remaining as f32;
        let progress = (advances * 0.5).min(1.0);
        let previous_progress = (advances - 1.0) * 0.5;
        world.set_block(self.position, Block::MovingPiston { moving: self.actor.into() });
        let mut entity = MovingPistonEntity {
            facing: self.actor.facing.into(),
            progress: 0,
            block_state: self.carried.get_id(),
            extending: self.extending,
            source: self.source,
        };
        entity.set_progress(previous_progress);
        world.set_block_entity(self.position, BlockEntity::MovingPiston(entity));
        // Worlds that do not register motions with their entities get one here.
        let index = world.piston_motion_index(self.position, None);
        let state = world.piston_state_mut();
        let index = index.unwrap_or_else(|| {
            state.next_identity += 1;
            state.motions.push_back(mchprs_world::PistonMotion {
                pos: self.position,
                identity: state.next_identity,
                progress,
                previous_progress,
                last_tick: state.logical_tick,
                carried_entity: None,
            });
            state.motions.len() - 1
        });
        let motion = &mut state.motions[index];
        motion.progress = progress;
        motion.previous_progress = previous_progress;
        motion.last_tick = state.logical_tick;
    }
}
