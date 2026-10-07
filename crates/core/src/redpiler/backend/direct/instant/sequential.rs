//! Compiled notification worklists and finite sampling/availability deadlines.
//! No runtime block searches, spatial wire walks or interpreter piston callbacks.
use super::*;
use std::collections::VecDeque;
use mchprs_world::PistonAction;

struct Actor { due: u64, head: bool, moving_base: bool }
struct Payload { pos: BlockPos, due: u64, owner: usize, extending: bool }
pub(super) struct Observer { pub powered: bool, due: u64 }
struct Sensor { terms: Vec<Term>, value: u8, shape: mchprs_blocks::blocks::RedstoneWire }
enum Completion { Actor(usize), Payload(usize) }

#[derive(Default)]
struct WireVisit { visited: bool, layer: u32, xbias: i32, zbias: i32, order: Option<[Option<usize>; 24]> }

pub(super) struct State {
    actors: Vec<Actor>,
    payloads: Vec<Payload>,
    pub observers: Vec<Observer>,
    sensors: Vec<Sensor>,
    samples: VecDeque<(usize, PistonAction)>,
    deferred: VecDeque<(usize, PistonAction)>,
    queued: Vec<[bool; 3]>,
    dirty: VecDeque<usize>,
    dirty_set: Vec<bool>,
    completions: VecDeque<(u64, Completion)>,
    observer_ticks: VecDeque<(u64, usize)>,
    boundary_initialized: bool,
    boundary_levels: FxHashMap<NodeId, u8>,
    wire_visits: FxHashMap<usize, WireVisit>,
    wire_queues: [VecDeque<usize>; 3],
    source_positions: FxHashMap<NodeId, BlockPos>,
}

impl State {
    pub(super) fn wire_power(&self, id: usize) -> u8 { self.sensors[id].value }
    pub(super) fn wire_dot(&self, id: usize) -> bool { crate::redstone::wire::is_dot(self.sensors[id].shape) }
    pub(super) fn payload_at(&self, program: &PreparedInstant, group: usize, pos: BlockPos) -> bool {
        let payload = &self.payloads[group];
        program.sequential.as_ref().unwrap().payloads[group] == Block::RedstoneBlock && payload.due == 0 && payload.pos == pos
    }
    pub(super) fn geometry(&self, program: &PreparedInstant, groups: &[usize], fired: &[bool], actor: usize, part: GeometryPart) -> bool {
        let p = &program.pistons[actor];
        let payload = &self.payloads[groups[actor]];
        match part {
            GeometryPart::FarPayload => program.sequential.as_ref().unwrap().payloads[groups[actor]] != Block::Air && payload.due == 0 && payload.pos == p.head.offset(p.piston.facing.into()),
            GeometryPart::NearPayload => program.sequential.as_ref().unwrap().payloads[groups[actor]] != Block::Air && payload.due == 0 && payload.pos == p.head,
            GeometryPart::Head => self.actors[actor].head,
            GeometryPart::RetractedBase => fired[actor] && !self.actors[actor].moving_base,
            GeometryPart::MovingBase => self.actors[actor].moving_base,
        }
    }
    fn dirty(&mut self, id: usize) { if !self.dirty_set[id] { self.dirty_set[id] = true; self.dirty.push_back(id); } }
    fn request(&mut self, actor: usize, action: PistonAction, deferred: bool) {
        let index = action_index(action);
        if !self.queued[actor][index] {
            self.queued[actor][index] = true;
            if deferred { self.deferred.push_back((actor, action)); } else { self.samples.push_back((actor, action)); }
        }
    }
    fn schedule_observer(&mut self, id: usize, due: u64) {
        if self.observers[id].due == 0 {
            self.observers[id].due = due;
            self.observer_ticks.push_back((due, id));
        }
    }
}

impl Runtime {
    pub(in crate::redpiler::backend::direct) fn notify_sequential_source(&mut self, source: NodeId, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        let Some(state) = &mut self.sequential else { return Vec::new() };
        let Some(&pos) = state.source_positions.get(&source) else { return Vec::new() };
        let strength = nodes[source].output_power;
        let changed = self.ready_inputs.insert(pos, strength) != Some(strength);
        let program = self.program.sequential.as_ref().unwrap();
        if let Some(sensors) = program.source_sensors.get(&pos) {
            for &sensor in sensors { state.dirty(sensor); }
        }
        if changed {
            if let Some(observers) = program.watched.get(&pos) {
                for &observer in observers { state.schedule_observer(observer, self.elapsed + 2); }
            }
        }
        let samples = program.source_samples.get(&pos).cloned().unwrap_or_default();
        self.sequential_settle_wires(nodes, false);
        for sample in samples { self.sequential_sample_update(sample, false, nodes); }
        self.output_changes(nodes)
    }
    #[cfg(test)]
    pub(crate) fn sampled_power(&self, nodes: &Nodes) -> Vec<(BlockPos, bool)> {
        if self.sequential.is_none() { return Vec::new(); }
        self.program.pistons.iter().enumerate().map(|(id, p)| (p.pos, !self.evaluate(self.program.logic.responses[id], nodes))).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_pistons(&self) -> impl Iterator<Item = (BlockPos, bool, bool)> + '_ {
        self.program.pistons.iter().enumerate().filter_map(|(id, p)| self.sequential.as_ref().map(|state|
            (p.pos, self.fired[id], state.actors[id].due == 0)))
    }
    #[cfg(test)]
    pub(crate) fn sampled_signals(&self) -> Vec<(BlockPos, u8)> {
        let Some(state) = &self.sequential else { return Vec::new() };
        let program = self.program.sequential.as_ref().unwrap();
        program.sensors.iter().zip(&state.sensors).map(|(p,s)| (p.pos, s.value))
            .chain(program.observers.iter().zip(&state.observers).map(|(p,s)| (p.pos, if s.powered {15} else {0}))).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_sources(&self, nodes: &Nodes) -> Vec<(BlockPos,u8)> {
        self.sources.iter().map(|(&pos,&id)| (pos,nodes[id].output_power)).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_geometry(&self) -> Vec<(BlockPos,Block)> {
        let Some(state) = &self.sequential else { return Vec::new() };
        let mut cells: FxHashMap<_,_> = self.program.aliases.iter().map(|&(_,pos,_)| (pos,Block::Air)).collect();
        for (id,p) in self.program.pistons.iter().enumerate() {
            cells.insert(p.pos, if state.actors[id].moving_base { Block::MovingPiston { moving:p.piston.into() }} else {Block::Piston {piston:p.piston.extend(!self.fired[id])}});
            if state.actors[id].head { cells.insert(p.head,Block::PistonHead {head:p.piston.into()}); }
            else if state.actors[id].due != 0 && !self.fired[id] { cells.insert(p.head,Block::MovingPiston {moving:p.piston.into()}); }
        }
        for (id,p) in state.payloads.iter().enumerate() {
            let material=self.program.sequential.as_ref().unwrap().payloads[id];
            if material != Block::Air { cells.insert(p.pos,if p.due!=0 {Block::MovingPiston {moving:self.program.pistons[self.program.groups[id][0]].piston.into()}} else {material}); }
        }
        cells.into_iter().collect()
    }
    pub(super) fn initialize_sequential(&mut self, nodes: &Nodes) -> Result<(), BackendError> {
        let Some(program) = &self.program.sequential else { return Ok(()) };
        let wires: FxHashMap<_,_> = program.sensors.iter().enumerate().map(|(id,s)| (s.pos,id)).collect();
        let blocks: FxHashMap<_, _> = self.program.template.iter().map(|&(pos, block, _)| (pos, block)).collect();
        let actors: Vec<_> = self.program.pistons.iter().map(|p| Actor {
            due: 0,
            moving_base: false,
            head: p.piston.extended && matches!(blocks.get(&p.head), Some(Block::PistonHead { .. })),
        }).collect();
        self.fired = self.program.pistons.iter().map(|p| !p.piston.extended).collect();
        let payloads = self.program.groups.iter().enumerate().map(|(group, actors)| {
            let p = &self.program.pistons[actors[0]];
            let home = p.head.offset(p.piston.facing.into());
            let pos = std::iter::once(home).chain(actors.iter().map(|&id| self.program.pistons[id].head))
                .find(|pos| blocks.get(pos) == Some(&program.payloads[group])).unwrap_or(home);
            Payload { pos, due: 0, owner: actors[0], extending: false }
        }).collect();
        let sensors = program.sensors.iter().map(|sensor| Ok(Sensor {
            value: sensor.initial,
            shape: sensor.shape,
            terms: sensor.terms.iter().map(|term| Ok(Term { guard: term.guard, attenuation: term.attenuation,
                source: term.source.map(|pos| if let Some(&id) = wires.get(&pos) { Ok(Source::Wire(id)) }
                    else { self.sources.get(&pos).copied().map(Source::Node).ok_or(BackendError::MissingInstantBinding { pos }) }).transpose()?,
            })).collect::<Result<_, BackendError>>()?,
        })).collect::<Result<_, BackendError>>()?;
        self.sequential = Some(State {
            actors, payloads, sensors,
            observers: program.observers.iter().map(|observer| Observer { powered: observer.powered, due: 0 }).collect(),
            samples: Default::default(), deferred: Default::default(), queued: vec![[false; 3]; self.fired.len()],
            dirty: Default::default(), dirty_set: vec![false; program.sensors.len()],
            completions: Default::default(),
            observer_ticks: Default::default(),
            boundary_initialized: false,
            boundary_levels: Default::default(),
            wire_visits: Default::default(),
            wire_queues: Default::default(),
            source_positions: self.sources.iter().map(|(&pos, &id)| (id, pos)).collect(),
        });
        let _ = nodes;
        Ok(())
    }

    fn sequential_sample(&mut self, actor: usize, deferred: bool, nodes: &Nodes) {
        let retracted = self.evaluate(self.program.logic.responses[actor], nodes);
        let p = &self.program.pistons[actor];
        let state = self.sequential.as_mut().unwrap();
        let payload = &state.payloads[self.actor_groups[actor]];
        let early = payload.pos == p.head.offset(p.piston.facing.into()) && payload.due != 0
            && payload.extending && self.program.pistons[payload.owner].piston.facing == p.piston.facing
            && (!deferred || payload.due > self.elapsed);
        let action = if !retracted { PistonAction::Extend } else if early { PistonAction::RetractWithoutPull } else { PistonAction::Retract };
        if !state.actors[actor].moving_base && retracted != self.fired[actor] { state.request(actor, action, deferred); }
    }
    fn sequential_shape_changed(&mut self, pos: BlockPos, deferred: bool, nodes: &Nodes) {
        let program = self.program.sequential.as_ref().unwrap();
        let state = self.sequential.as_mut().unwrap();
        if let Some(observers) = program.watched.get(&pos) {
            for &id in observers { state.schedule_observer(id, self.elapsed + 2); }
        }
        let sensors = program.notification_sensors.get(&pos).cloned().unwrap_or_default();
        for (id, face) in sensors {
            let raw = self.program.sequential.as_ref().unwrap().sensors[id].shapes.iter().find(|&&(guard, _)| self.evaluate(guard, nodes)).unwrap().1;
            let state = self.sequential.as_ref().unwrap();
            let mut previous = state.sensors[id].shape;
            previous.power = state.sensors[id].value;
            let mut raw = raw; raw.power = previous.power;
            let shape = crate::redstone::wire::on_neighbor_changed_from(previous, raw, face);
            if shape != previous {
                self.sequential.as_mut().unwrap().sensors[id].shape = shape;
                let program = self.program.sequential.as_ref().unwrap();
                if let Some(observers) = program.watched.get(&program.sensors[id].pos) {
                    for &observer in observers { self.sequential.as_mut().unwrap().schedule_observer(observer, self.elapsed + 2); }
                }
                let updates = program.sensors[id].shape_updates.clone();
                for update in updates { self.sequential_wire_update(update, deferred, nodes); }
            }
        }
    }
    fn sequential_notify(&mut self, pos: BlockPos, deferred: bool, nodes: &Nodes) {
        self.sequential_shape_changed(pos, deferred, nodes);
        let samples = self.program.sequential.as_ref().unwrap().notifications.get(&pos).cloned().unwrap_or_default();
        for sample in samples {
            self.sequential_sample_update(sample, deferred, nodes);
        }
    }

    fn sequential_wire_update(&mut self, update: usize, deferred: bool, nodes: &Nodes) {
        self.sequential_sample_update(self.program.sequential.as_ref().unwrap().wire_updates[update], deferred, nodes);
    }

    fn sequential_sample_update(&mut self, sample: crate::redpiler::instant::sequential::WireUpdate, deferred: bool, nodes: &Nodes) {
        use crate::redpiler::instant::sequential::WireUpdate;
        match sample {
            WireUpdate::Wire(id) => { self.sequential.as_mut().unwrap().dirty(id); self.sequential_settle_wires(nodes, deferred); }
            WireUpdate::Base(actor) => self.sequential_sample(actor, deferred, nodes),
            WireUpdate::Head(actor) if self.sequential.as_ref().unwrap().actors[actor].head => self.sequential_sample(actor, deferred, nodes),
            _ => {}
        }
    }
    fn sequential_strength(&self, terms: &[Term], nodes: &Nodes) -> u8 {
        terms.iter().filter(|term| self.evaluate(term.guard, nodes)).map(|term|
            term.source.map_or(15, |source| self.source_power(source, nodes)).saturating_sub(term.attenuation)
        ).max().unwrap_or(0)
    }
    fn sequential_settle_wires(&mut self, nodes: &Nodes, deferred: bool) {
        loop {
            let Some(id) = self.sequential.as_mut().unwrap().dirty.pop_front() else { break };
            self.sequential.as_mut().unwrap().dirty_set[id] = false;
            let strength = self.sequential_strength(&self.sequential.as_ref().unwrap().sensors[id].terms, nodes);
            if strength != self.sequential.as_ref().unwrap().sensors[id].value {
                self.sequential.as_mut().unwrap().sensors[id].value = strength;
                let state = self.sequential.as_mut().unwrap();
                let mut visits = std::mem::take(&mut state.wire_visits);
                visits.clear();
                visits.entry(id).or_default().visited = true;
                let mut queues = std::mem::take(&mut state.wire_queues);
                for queue in &mut queues { queue.clear(); }
                self.sequential_propagate_wire(id, 0, &mut visits, &mut queues);
                queues.rotate_left(1);
                let mut layer = 1;
                while !queues[0].is_empty() || !queues[1].is_empty() {
                    while let Some(update) = queues[0].pop_front() {
                        use crate::redpiler::instant::sequential::WireUpdate;
                        match self.program.sequential.as_ref().unwrap().wire_updates[update] {
                            WireUpdate::Wire(wire) => {
                                visits.entry(update).or_default().visited = true;
                                let strength = self.sequential_strength(&self.sequential.as_ref().unwrap().sensors[wire].terms, nodes);
                                if strength != self.sequential.as_ref().unwrap().sensors[wire].value {
                                    self.sequential.as_mut().unwrap().sensors[wire].value = strength;
                                    self.sequential_propagate_wire(wire, layer, &mut visits, &mut queues);
                                }
                            }
                            WireUpdate::Base(actor) => self.sequential_sample(actor, deferred, nodes),
                            WireUpdate::Head(actor) => {
                                if self.sequential.as_ref().unwrap().actors[actor].head { self.sequential_sample(actor, deferred, nodes); }
                            }
                        }
                    }
                    queues.rotate_left(1); layer += 1;
                }
                let state = self.sequential.as_mut().unwrap();
                state.wire_visits = visits;
                state.wire_queues = queues;
            }
        }
    }

    fn sequential_propagate_wire(&self, wire: usize, layer: u32, visits: &mut FxHashMap<usize, WireVisit>, queues: &mut [VecDeque<usize>; 3]) {
        let order = if let Some(order) = visits.entry(wire).or_default().order { order } else {
            let neighbors = self.program.sequential.as_ref().unwrap().sensors[wire].neighbors;
            let visited = |indices: [usize; 3]| indices.into_iter().any(|index| neighbors[index].is_some_and(|id| visits.get(&id).is_some_and(|v| v.visited)));
            let mut cx = i32::from(visited([0, 7, 8])) - i32::from(visited([1, 12, 13]));
            let mut cz = i32::from(visited([4, 17, 20])) - i32::from(visited([5, 18, 21]));
            let current = visits.get(&wire).unwrap();
            let (xbias, zbias) = (current.xbias, current.zbias);
            if cx == 0 && cz == 0 { cx = xbias; cz = zbias; }
            else if cx != 0 && cz != 0 { if xbias != 0 { cz = 0; } if zbias != 0 { cx = 0; } }
            let heading = match (cx + 1) + 3 * (cz + 1) { 0 | 1 => 0, 2 | 5 => 1, 3 | 4 => 3, 6..=8 => 2, _ => unreachable!() };
            for id in neighbors.into_iter().flatten() { let neighbor = visits.entry(id).or_default(); neighbor.xbias = cx; neighbor.zbias = cz; }
            let order = crate::redstone::wire::TURBO_ORDER[heading].map(|index| neighbors[index]);
            visits.get_mut(&wire).unwrap().order = Some(order); order
        };
        for (offset, ids) in [(1, &order[..]), (2, &order[..4])] {
            for &id in ids.iter().flatten() {
                let visit = visits.entry(id).or_default();
                if layer + offset > visit.layer { visit.layer = layer + offset; queues[offset as usize].push_back(id); }
            }
        }
    }
    fn sequential_boundary(&self, nodes: &Nodes, levels: &mut FxHashMap<NodeId, u8>, changes: &mut Vec<(NodeId, u8)>) {
        let aliases = self.aliases.iter().map(|&(id, ref supply)| {
            let Supply::Sequential { group, pos } = *supply else { unreachable!() };
            (id, if self.sequential.as_ref().unwrap().payload_at(&self.program, group, pos) { 15 } else { 0 })
        });
        let outputs = self.outputs.iter().map(|output| (output.node, self.sequential_strength(&output.terms, nodes)));
        for (id, strength) in aliases.chain(outputs) {
            let old = levels.entry(id).or_insert(nodes[id].output_power);
            if *old != strength { *old = strength; changes.push((id, strength)); }
        }
    }
    pub(super) fn advance_sequential(&mut self, nodes: &Nodes) -> Vec<(NodeId, u8)> {
        let mut samples = Vec::new();
        let mut source_changed = false;
        {
            let state = self.sequential.as_mut().unwrap();
            state.samples.append(&mut state.deferred);
            let program = self.program.sequential.as_ref().unwrap();
            for (&pos, &id) in &self.sources {
                let strength = nodes[id].output_power;
                let previous = self.ready_inputs.insert(pos, strength).unwrap();
                if previous != strength {
                    source_changed = true;
                    if let Some(sensors) = program.source_sensors.get(&pos) { for &sensor in sensors { state.dirty(sensor); } }
                    if let Some(actors) = program.source_samples.get(&pos) { samples.extend(actors); }
                    if let Some(observers) = program.watched.get(&pos) { for &observer in observers {
                        state.schedule_observer(observer, self.elapsed + 2);
                    }}
                }
            }
            // Retain the first publication and wake on input changes or due work.
            // Future deadlines remain relative to the elapsed tick advanced above.
            if state.boundary_initialized && !source_changed && state.samples.is_empty() && state.dirty.is_empty()
                && state.observer_ticks.front().is_none_or(|(due, _)| *due > self.elapsed)
                && state.completions.front().is_none_or(|(due, _)| *due > self.elapsed)
            { return Vec::new(); }
        }
        let mut changes = Vec::new();
        let mut levels = std::mem::take(&mut self.sequential.as_mut().unwrap().boundary_levels);
        levels.clear();
        for sample in samples { self.sequential_sample_update(sample, false, nodes); }
        self.sequential_settle_wires(nodes, false);
        self.sequential_boundary(nodes, &mut levels, &mut changes);
        while self.sequential.as_ref().unwrap().observer_ticks.front().is_some_and(|(due, _)| *due <= self.elapsed) {
            let (due, id) = self.sequential.as_mut().unwrap().observer_ticks.pop_front().unwrap();
            let state = self.sequential.as_mut().unwrap();
            let observer = &mut state.observers[id];
            if observer.due != due { continue; }
            observer.powered = !observer.powered;
            observer.due = 0;
            if observer.powered { state.schedule_observer(id, self.elapsed + 2); }
            let program = self.program.sequential.as_ref().unwrap();
            for &sensor in &program.observer_sensors[id] { state.dirty(sensor); }
            let samples = program.observers[id].samples.clone();
            self.sequential_settle_wires(nodes, false);
            for sample in samples { self.sequential_sample_update(sample, false, nodes); }
            self.sequential_boundary(nodes, &mut levels, &mut changes);
        }
        loop {
            self.sequential_settle_wires(nodes, false);
            let Some((actor, action)) = self.sequential.as_mut().unwrap().samples.pop_front() else { break };
            self.sequential.as_mut().unwrap().queued[actor][action_index(action)] = false;
            let requested = action != PistonAction::Extend;
            let retracted = self.evaluate(self.program.logic.responses[actor], nodes);
            let state = self.sequential.as_ref().unwrap();
            if retracted != requested || retracted == self.fired[actor] || state.actors[actor].moving_base { continue; }
            let p = &self.program.pistons[actor];
            let base = p.pos;
            let head = p.head;
            let home = head.offset(p.piston.facing.into());
            let group = self.actor_groups[actor];
            if retracted {
                self.fired[actor] = true;
                let state = self.sequential.as_mut().unwrap();
                state.actors[actor].due = self.elapsed + 2;
                state.actors[actor].moving_base = true;
                state.completions.push_back((self.elapsed + 2, Completion::Actor(actor)));
                self.sequential_notify(base, false, nodes);
            }
            let p = &self.program.pistons[actor];
            let payload = self.sequential.as_mut().unwrap().payloads.get_mut(group).unwrap();
            let mut positions = Vec::new();
            let mut moved = false;
            if retracted {
                if payload.pos == home && payload.due == 0 && p.piston.sticky && action == PistonAction::Retract { payload.pos = head; payload.due = self.elapsed + 2; positions.push(home); moved = true; }
                else if payload.pos == home && payload.due != 0 && p.piston.sticky && payload.extending && self.program.pistons[payload.owner].piston.facing == p.piston.facing { payload.due = 0; positions.push(home); }
            } else if self.program.sequential.as_ref().unwrap().payloads[group] != Block::Air && payload.pos == head {
                payload.pos = home; payload.due = self.elapsed + 2; positions.push(home); moved = true;
            }
            if moved { payload.owner = actor; payload.extending = !retracted; }
            let state = self.sequential.as_mut().unwrap();
            state.actors[actor].due = self.elapsed + 2;
            state.actors[actor].head = false;
            state.actors[actor].moving_base = retracted;
            if moved { state.completions.push_back((self.elapsed + 2, Completion::Payload(group))); }
            if !retracted { state.completions.push_back((self.elapsed + 2, Completion::Actor(actor))); }
            self.sequential_settle_wires(nodes, false);
            for pos in positions {
                if retracted { self.sequential_notify(pos, false, nodes); }
                else { self.sequential_shape_changed(pos, false, nodes); }
            }
            self.sequential_notify(head, false, nodes);
            self.fired[actor] = retracted;
            if !retracted { self.sequential_notify(base, false, nodes); }
            self.sequential_boundary(nodes, &mut levels, &mut changes);
        }
        self.sequential_boundary(nodes, &mut levels, &mut changes);
        while self.sequential.as_ref().unwrap().completions.front().is_some_and(|(due,_)| *due <= self.elapsed) {
            let (due, completion) = self.sequential.as_mut().unwrap().completions.pop_front().unwrap();
            match completion {
                Completion::Actor(actor) => {
                    let state = self.sequential.as_mut().unwrap();
                    if state.actors[actor].due != due { continue; }
                    state.actors[actor].due = 0;
                    state.actors[actor].moving_base = false;
                    state.actors[actor].head = !self.fired[actor];
                    let p = &self.program.pistons[actor];
                    let pos = if self.fired[actor] { p.pos } else { p.head };
                    self.sequential_settle_wires(nodes, true);
                    if self.fired[actor] { self.sequential_sample(actor, true, nodes); }
                    self.sequential_notify(pos, true, nodes);
                }
                Completion::Payload(group) => {
                    let payload = &mut self.sequential.as_mut().unwrap().payloads[group];
                    if payload.due != due { continue; }
                    payload.due = 0;
                    let pos = payload.pos;
                    self.sequential_settle_wires(nodes, true);
                    self.sequential_notify(pos, true, nodes);
                }
            }
            self.sequential_boundary(nodes, &mut levels, &mut changes);
        }
        self.sequential_settle_wires(nodes, true);
        self.sequential_boundary(nodes, &mut levels, &mut changes);
        let state = self.sequential.as_mut().unwrap();
        state.boundary_levels = levels;
        state.boundary_initialized = true;
        changes
    }

    pub(super) fn materialize_sequential(self, world: &mut impl World) {
        let state = self.sequential.as_ref().unwrap();
        let time = self.program.logical_tick.wrapping_add(self.elapsed);
        world.piston_state_mut().logical_tick = time;
        for &(_, pos, _) in &self.program.aliases { world.set_block(pos, Block::Air); world.delete_block_entity(pos); }
        for (id, p) in self.program.pistons.iter().enumerate() {
            world.set_block(p.pos, Block::Piston { piston: p.piston.extend(!self.fired[id]) });
            if state.actors[id].head { world.set_block(p.head, Block::PistonHead { head: p.piston.into() }); }
        }
        for (group, payload) in state.payloads.iter().enumerate() {
            let block = self.program.sequential.as_ref().unwrap().payloads[group];
            if block != Block::Air && payload.due == 0 { world.set_block(payload.pos, block); }
        }
        let mut restored = FxHashSet::default();
        for &(due, ref completion) in &state.completions {
            match *completion {
                Completion::Actor(actor) if state.actors[actor].due == due && due > self.elapsed => {
                    let p = &self.program.pistons[actor];
                    let retracted = self.fired[actor];
                    let pos = if retracted { p.pos } else { p.head };
                    if restored.insert(pos) {
                        let block = if retracted { Block::Piston { piston: p.piston.extend(false) } } else { Block::PistonHead { head: p.piston.into() } };
                        restore_motion(world, pos, p.piston, block, !retracted, true, due - self.elapsed);
                    }
                }
                Completion::Payload(group) if state.payloads[group].due == due && due > self.elapsed => {
                    let payload = &state.payloads[group];
                    if program_payload(&self.program, group) != Block::Air && restored.insert(payload.pos) {
                        restore_motion(world, payload.pos, self.program.pistons[payload.owner].piston, program_payload(&self.program, group), payload.extending, false, due - self.elapsed);
                    }
                }
                _ => {}
            }
        }
        for &(actor, action) in state.deferred.iter().chain(&state.samples) {
            let p = &self.program.pistons[actor];
            world.enqueue_piston_event(mchprs_world::PistonEvent { pos: p.pos, sticky: p.piston.sticky, facing: p.piston.facing.into(), action });
        }
        let program = self.program.sequential.as_ref().unwrap();
        let blocks: FxHashMap<_, _> = self.program.template.iter().map(|&(pos, block, _)| (pos, block)).collect();
        for (id, observer) in program.observers.iter().enumerate() {
            let Block::Observer { observer: mut block } = blocks[&observer.pos] else { unreachable!() };
            block.powered = state.observers[id].powered;
            world.set_block(observer.pos, Block::Observer { observer: block });
        }
        for &(due, id) in &state.observer_ticks {
            if state.observers[id].due == due { world.schedule_half_tick(program.observers[id].pos, (due - self.elapsed) as u32, mchprs_world::TickPriority::Normal); }
        }
        for (id, sensor) in program.sensors.iter().enumerate() {
            let mut wire = state.sensors[id].shape; wire.power = state.sensors[id].value;
            world.set_block(sensor.pos, Block::RedstoneWire { wire });
        }
    }
}

fn program_payload(program: &PreparedInstant, group: usize) -> Block { program.sequential.as_ref().unwrap().payloads[group] }

fn action_index(action: PistonAction) -> usize {
    match action { PistonAction::Extend => 0, PistonAction::Retract => 1, PistonAction::RetractWithoutPull => 2 }
}

fn restore_motion(world: &mut impl World, pos: BlockPos, piston: mchprs_blocks::blocks::RedstonePiston, carried: Block, extending: bool, source: bool, remaining: u64) {
    world.set_block(pos, Block::MovingPiston { moving: piston.into() });
    let previous = (2 - remaining) as f32 * 0.5;
    let mut entity = mchprs_blocks::block_entities::MovingPistonEntity {
        facing: piston.facing.into(), progress: 0, block_state: carried.get_id(), extending, source,
    };
    entity.set_progress(previous);
    world.set_block_entity(pos, mchprs_blocks::block_entities::BlockEntity::MovingPiston(entity));
    if let Some(index) = world.piston_motion_index(pos, None) {
        let motion = &mut world.piston_state_mut().motions[index];
        motion.previous_progress = previous;
        motion.progress = previous + 0.5;
    }
}
