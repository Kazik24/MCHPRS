//! Closed, opt-in event backend. Admission precedes all world writes.
use super::components::NodeType;
use super::event_netlist::CompiledPistonActors;
use super::physical_signal_network::{EntryWork, SelectionSnapshot};
use super::program_graph::{CircuitNodeKind, CircuitProgram, ComponentState, InputPort, LoweredCircuit, PowerPort, PowerTarget, SiteId, SiteSource};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;

// Reject quasi inputs which could ever be powered, including through conductors
// or a movable contact. A cyclic or deep dependency is unsupported as well.
fn may_emit(program: &CircuitProgram, site: SiteId, port: PowerPort, depth: usize, visited: &mut usize) -> bool {
    *visited += 1;
    if depth > 32 || *visited > 4096 {
        return true;
    }
    if let PowerPort::Conducted { include_dust } = port {
        return program
            .power_links_to(PowerTarget::Conductor { site, include_dust })
            .iter()
            .any(|link| may_emit(program, link.from, link.from_port, depth + 1, visited));
    }
    let emits = |block| match port {
        PowerPort::Weak(face) => super::power_rules::emits_weak(block, face),
        PowerPort::Strong(face) => super::power_rules::emits_strong(block, face),
        PowerPort::Output | PowerPort::Stored => true,
        PowerPort::Conducted { .. } => unreachable!(),
    };
    let spec = &program.sites()[site.index()];
    match spec.source {
        SiteSource::Component(node) => {
            matches!(program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) || program.nodes()[node.index()].block.is_some_and(emits)
        }
        SiteSource::Constant(profile) => spec.source_strength != 0 && program.profile_block(profile).is_none_or(emits),
        SiteSource::Contact(contact) => program.contacts()[contact.index()].variants.iter().any(|variant| {
            program
                .profile_block(variant.profile)
                .is_none_or(|block| matches!(block, mchprs_blocks::blocks::Block::RedstoneBlock) && emits(block))
        }),
    }
}

pub(super) struct EventBackend {
    actors: CompiledPistonActors,
    snapshot: SelectionSnapshot,
    bounds: (BlockPos, BlockPos),
    stop_failed: bool,
}

impl EventBackend {
    pub(super) fn compile<W: World>(world: &W, bounds: (BlockPos, BlockPos), ticks: &[TickEntry], lowered: LoweredCircuit) -> Result<Self, String> {
        let program = &lowered.program;
        let mut levers = 0;
        let mut lamps = 0;
        for node in program.nodes() {
            match node.kind {
                CircuitNodeKind::Wire(_)
                | CircuitNodeKind::Piston(_)
                | CircuitNodeKind::Contact(_)
                | CircuitNodeKind::Electrical(NodeType::Constant) => {}
                CircuitNodeKind::Electrical(NodeType::Lever) => levers += 1,
                CircuitNodeKind::Electrical(NodeType::Lamp) => lamps += 1,
                _ => return Err("event admission: unsupported component or observer".into()),
            }
        }
        if levers != 1 || lamps != 1 || program.wires().len() != 1 || program.pistons().len() != 1 || program.resources().len() != 1 {
            return Err("event admission: requires one lever, dust, sticky piston, resource and lamp".into());
        }
        if program.profile_block(program.resources()[0].material) != Some(mchprs_blocks::blocks::Block::RedstoneBlock) {
            return Err("event admission: movable resource must be a redstone block".into());
        }
        let inside = |pos: BlockPos| {
            pos.x >= bounds.0.x && pos.x <= bounds.1.x && pos.y >= bounds.0.y && pos.y <= bounds.1.y && pos.z >= bounds.0.z && pos.z <= bounds.1.z
        };
        if let Some(site) = program.sites().iter().find(|site| !inside(site.position)) {
            return Err(format!("event admission: dependency crosses selection at {}", site.position));
        }
        let piston = &program.pistons()[0];
        let wire_node = program.wires()[0].node;
        let lever_node = program
            .nodes()
            .iter()
            .find(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
            .unwrap();
        let lamp_node = program
            .nodes()
            .iter()
            .find(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
            .unwrap();
        let wire_position = program.nodes()[wire_node.index()]
            .position
            .ok_or("event admission: dust has no position")?;
        let far_position = program.contacts()[piston.far.index()].position;
        if !piston.facing.is_horizontal()
            || wire_position != piston.position.offset(piston.facing.opposite())
            || lever_node.position != Some(wire_position.offset(piston.facing.opposite()))
            || lamp_node.position != Some(far_position.offset(piston.facing))
        {
            return Err("event admission: unsupported piston lane and receiver topology".into());
        }
        let ComponentState::Wire(wire) = lowered.state.components[wire_node.index()] else {
            return Err("event admission: dust state unavailable".into());
        };
        use mchprs_blocks::blocks::RedstoneWireSide::{None, Side};
        let shape_supported = match piston.facing {
            mchprs_blocks::BlockFace::East | mchprs_blocks::BlockFace::West => {
                wire.east == Side && wire.west == Side && wire.north == None && wire.south == None
            }
            mchprs_blocks::BlockFace::North | mchprs_blocks::BlockFace::South => {
                wire.north == Side && wire.south == Side && wire.east == None && wire.west == None
            }
            _ => false,
        };
        if !shape_supported {
            return Err("event admission: shape-dependent dust is unsupported".into());
        }
        if lowered.state.pistons[0].extended || !lowered.state.resources[0].enabled || lowered.state.resources[0].at != piston.near {
            return Err("event admission: piston and resource must start at rest in the near position".into());
        }
        let wire_reaches = |port| {
            program
                .power_links_to(PowerTarget::Input { node: piston.node, port })
                .iter()
                .any(|link| matches!(program.sites()[link.from.index()].source, SiteSource::Component(node) if node == wire_node))
        };
        let dust_powers_piston = wire_reaches(InputPort::Main);
        if !dust_powers_piston {
            return Err("event admission: piston is not directly driven by dust".into());
        }
        if program
            .power_links_to(PowerTarget::Input {
                node: piston.node,
                port: InputPort::Main,
            })
            .iter()
            .any(|link| {
                !matches!(program.sites()[link.from.index()].source, SiteSource::Component(node) if node == wire_node)
                    && may_emit(program, link.from, link.from_port, 0, &mut 0)
            })
        {
            return Err("event admission: piston has an additional power source".into());
        }
        if wire_reaches(InputPort::Quasi) {
            return Err("event admission: quasi-powered dust piston is unsupported".into());
        }
        if program
            .power_links_to(PowerTarget::Input {
                node: piston.node,
                port: InputPort::Quasi,
            })
            .iter()
            .any(|link| may_emit(program, link.from, link.from_port, 0, &mut 0))
        {
            return Err("event admission: potentially powered quasi input is unsupported".into());
        }
        let entry = EntryWork::capture_from_scheduler(world, bounds, ticks);
        let mut actors = CompiledPistonActors::compile(program, &lowered.state, &entry)?;
        actors.signals_mut().enable_dust_corridor().map_err(str::to_owned)?;
        let snapshot = SelectionSnapshot::capture(actors.signals(), world, bounds).map_err(str::to_owned)?;
        Ok(Self {
            actors,
            snapshot,
            bounds,
            stop_failed: false,
        })
    }

    fn ready(&self) -> Result<(), &'static str> {
        if self.stop_failed {
            Err("event restoration must be retried")
        } else {
            Ok(())
        }
    }

    pub(super) fn use_block(&mut self, position: BlockPos) -> Result<(), &'static str> {
        self.ready()?;
        if self.actors.has_pending_movement() {
            return Err("piston movement or request is pending");
        }
        let powered = self.actors.signals().lever_power(position).ok_or("control is not an admitted lever")?;
        self.actors.set_lever(position, !powered)
    }

    pub(super) fn accepts_control(&self, position: BlockPos) -> bool {
        !self.stop_failed && self.actors.signals().lever_power(position).is_some()
    }
    pub(super) fn begin_cycle(&mut self) -> Result<(), &'static str> {
        self.ready()?;
        if self.actors.has_pending_movement() {
            return Err("piston movement or request is pending");
        }
        self.actors.begin_computation_cycle();
        Ok(())
    }
    pub(super) fn tick(&mut self) -> Result<(), &'static str> {
        self.ready()?;
        self.actors.advance_tick().map(|_| ())
    }
    pub(super) fn flush<W: World>(&mut self, world: &mut W) -> Result<(), &'static str> {
        self.ready()?;
        let result = self.actors.signals().project(world, self.bounds);
        if matches!(result, Err("world refused projection rollback")) {
            self.stop_failed = true;
        }
        result
    }
    pub(super) fn stop<W: World>(&mut self, world: &mut W) -> Result<(), &'static str> {
        let result = self.actors.stop(world, &self.snapshot);
        self.stop_failed = result.is_err();
        result
    }
    pub(super) fn counts(&self) -> (u64, u64, u64, u64) {
        let (links, settles) = self.actors.signals().dust_corridor_work();
        (self.actors.signals().current_tick(), self.actors.movement_count(), links, settles)
    }
}
