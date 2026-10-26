use super::*;

/// Index contact selectors that only depend on graph state. Contact profile
/// lookup ignores callback receiver properties, so site observations stay on
/// the conservative full-projection path.
pub(super) fn contact_projection_dependencies(program: &CircuitProgram) -> (Vec<Vec<ContactId>>, Vec<Vec<ContactId>>, Vec<ContactId>) {
    let mut pistons = vec![Vec::new(); program.pistons().len()];
    let mut resources = vec![Vec::new(); program.resources().len()];
    let mut always_dirty = Vec::new();
    let mut visited = vec![0u32; program.guards().len()];
    let mut pending = Vec::new();
    let mut piston_dependencies = FxHashSet::default();
    let mut resource_dependencies = FxHashSet::default();

    for (index, contact) in program.contacts().iter().enumerate() {
        let contact_id = ContactId::new(index as u32);
        let stamp = u32::try_from(index).expect("contact count was validated") + 1;
        pending.clear();
        pending.extend(contact.variants.iter().map(|variant| variant.when));
        piston_dependencies.clear();
        resource_dependencies.clear();
        let mut unsupported = false;

        while let Some(guard) = pending.pop() {
            let Some(visited) = visited.get_mut(guard.index()) else {
                unsupported = true;
                break;
            };
            if *visited == stamp {
                continue;
            }
            *visited = stamp;
            match &program.guards()[guard.index()] {
                Guard::Always => {}
                Guard::State(StateTest::PistonExtended(piston) | StateTest::PistonHeadPresent(piston))
                | Guard::State(StateTest::PistonAvailability { piston, .. }) => {
                    if piston.index() < pistons.len() {
                        piston_dependencies.insert(*piston);
                    } else {
                        unsupported = true;
                        break;
                    }
                }
                Guard::State(StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource)) => {
                    if resource.index() < resources.len() {
                        resource_dependencies.insert(*resource);
                    } else {
                        unsupported = true;
                        break;
                    }
                }
                Guard::Site { .. } => {
                    unsupported = true;
                    break;
                }
                Guard::Not(child) => pending.push(*child),
                Guard::All(children) | Guard::Any(children) => pending.extend(children.iter().copied()),
            }
        }

        if unsupported {
            always_dirty.push(contact_id);
            continue;
        }
        for &piston in &piston_dependencies {
            pistons[piston.index()].push(contact_id);
        }
        for &resource in &resource_dependencies {
            resources[resource.index()].push(contact_id);
        }
    }

    (pistons, resources, always_dirty)
}

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
    /// Existing World block IDs only suppress redundant writes; execution stays graph-owned.
    pub(crate) fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
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
            if self.full_contact_projection {
                for index in 0..self.program.contacts().len() {
                    self.write_contact(world, ContactId::new(index as u32));
                }
                self.contact_dirty.fill(false);
                self.dirty_contacts.clear();
            } else {
                self.refresh_contact_projection();
                let mut contacts = std::mem::take(&mut self.dirty_contacts);
                contacts.sort_unstable_by_key(|contact| contact.index());
                for contact in contacts.drain(..) {
                    self.write_contact(world, contact);
                    self.contact_dirty[contact.index()] = false;
                }
                self.dirty_contacts = contacts;
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

    fn write_contact<W: World>(&self, world: &mut W, contact: ContactId) {
        let position = self.program.contacts()[contact.index()].position;
        if self.cell_owned_by_node(position) {
            return;
        }
        // Clients see pending cells as empty; reset supplies interpreter motion.
        let block = match self.contact_block(contact) {
            Block::MovingPiston { .. } => Block::Air,
            block => block,
        };
        if world.get_block_raw(position) != block.get_id() {
            world.set_block(position, block);
        }
    }

    fn refresh_contact_projection(&mut self) {
        let mut changed = Vec::new();
        for (index, (projected, current)) in self.projected_pistons.iter_mut().zip(&self.state.pistons).enumerate() {
            if projected != current {
                changed.extend(self.contacts_by_piston[index].iter().copied());
                *projected = *current;
            }
        }
        for (index, (projected, current)) in self.projected_resources.iter_mut().zip(&self.state.resources).enumerate() {
            if projected != current {
                changed.extend(self.contacts_by_resource[index].iter().copied());
                *projected = *current;
            }
        }
        changed.extend(self.always_dirty_contacts.iter().copied());
        for contact in changed {
            let index = contact.index();
            if !self.contact_dirty[index] {
                self.contact_dirty[index] = true;
                self.dirty_contacts.push(contact);
            }
        }
    }

    fn write_node<W: World>(&self, world: &mut W, node: NodeId) {
        let spec = &self.program.nodes()[node.index()];
        let Some(position) = spec.position else {
            return;
        };
        if !self.owns_cell(node) {
            return;
        }
        let block = self.node_block(node);
        if world.get_block_raw(position) != block.get_id() {
            world.set_block(position, block);
        }
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
            world.set_block_entity(position, BlockEntity::CommandBlock(entity));
        }
    }

    /// Hand the current graph future back to the interpreter at a tick
    /// boundary and drop graph-owned pending work. Pending transitions are
    /// reproduced, never settled.
    pub(crate) fn reset<W: World>(&mut self, world: &mut W) {
        for cache in &mut self.stable_logic_cache {
            cache.invalidate();
        }
        self.flush(world, false);
        self.project_future(world);
        self.logical_devices.clear();
        self.logical_device_owner.clear();
        self.native_action_clock_owner.fill(None);
        self.logical_input_callbacks.clear();
        self.logical_device_admission = crate::redpiler::LogicalDeviceAdmission::NotApplicable;
        self.dirty.fill(false);
        self.contact_dirty.fill(false);
        self.dirty_contacts.clear();
        self.projected_pistons.clone_from_slice(&self.state.pistons);
        self.projected_resources.clone_from_slice(&self.state.resources);
        self.future.scheduler.clear();
        self.future.pending.clear();
        self.future.pending_nodes.fill(0);
        self.future.piston_requests.clear();
        self.future.deferred_pistons.clear();
    }

    /// Write the interpreter state equivalent to the current graph future:
    /// component and contact blocks, motions derived from live deferred
    /// events, ordinary scheduled ticks, queued requests, and the clock.
    /// Reads no World state and changes no graph state.
    pub(crate) fn project_future<W: World>(&self, world: &mut W) {
        debug_assert_eq!(self.future.clock.phase, GraphPhase::BetweenTicks);
        let interpreter = world.piston_state_mut();
        interpreter.logical_tick = self.future.clock.logical_tick;
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

        for (tick, delay, priority) in self.future.scheduler.iter() {
            world.restore_scheduled_tick(TickEntry {
                pos: tick.position,
                block_type: tick.block_type,
                ticks_left: delay as u32,
                tick_priority: priority,
            });
        }
        for request in &self.future.piston_requests {
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
        for event in &self.future.deferred_pistons {
            let spec = self.program.pistons()[event.piston.index()];
            let piston = RedstonePiston {
                facing: spec.facing.into(),
                sticky: spec.sticky,
                extended: false,
            };
            let remaining = event.due_tick.saturating_sub(self.future.clock.logical_tick);
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
