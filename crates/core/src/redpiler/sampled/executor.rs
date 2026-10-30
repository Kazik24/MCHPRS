use super::super::TickScheduler;
use super::{Operation, Program, ScheduledBlockTick};
use mchprs_blocks::blocks::Block;
use mchprs_world::TickPriority;

pub(super) struct Runtime<'a> {
    pub(super) program: &'a Program,
    pub(super) blocks: Vec<Block>,
    pub(super) pending: TickScheduler<ScheduledBlockTick>,
    pub(super) contributions: Vec<u8>,
    pub(super) watched_strengths: Vec<u8>,
    pub(super) phase: u64,
    pub(super) updated_in_phase: Vec<Option<u64>>,
    pub(super) operations: usize,
    pub(super) deliveries: usize,
    pub(super) scheduled_repeater_ticks: usize,
}

impl Runtime<'_> {
    pub(super) fn input_strength(&self, target: usize) -> u8 {
        self.program.incoming[target].iter().map(|&id| self.contributions[id]).max().unwrap_or(0)
    }

    pub(super) fn output(&self, source: usize) -> u8 {
        match self.blocks[source] {
            Block::Lever { lever } => {
                if lever.powered {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneWire { wire } => wire.power,
            Block::RedstoneRepeater { repeater } => {
                if repeater.powered {
                    15
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub(super) fn begin_phase(&mut self) {
        self.phase = self.phase.wrapping_add(1);
        self.updated_in_phase.fill(None);
    }

    pub(super) fn dispatch_receiver(&mut self, receiver: usize) {
        self.deliveries += 1;
        let power = self.input_strength(receiver);
        let Block::RedstoneLamp { lit } = self.blocks[receiver] else {
            unreachable!()
        };
        if power > 0 && !lit {
            self.blocks[receiver] = Block::RedstoneLamp { lit: true };
        } else if power == 0 && lit {
            let tick = ScheduledBlockTick {
                pos: self.program.positions[receiver],
                block_type: Some(self.blocks[receiver].registry_id()),
            };
            if !self.pending.contains(&tick) {
                self.pending.schedule_tick(tick, 2, TickPriority::Normal);
            }
        }
    }

    pub(super) fn update_once(&mut self, receiver: usize) {
        if self.updated_in_phase[receiver] == Some(self.phase) {
            return;
        }
        self.updated_in_phase[receiver] = Some(self.phase);
        self.dispatch_receiver(receiver);
    }

    pub(super) fn check_watched_wire(&mut self, record_index: usize) {
        let wire = self.program.watched_wires[record_index].wire;
        let new_strength = self.input_strength(wire);
        if new_strength == self.watched_strengths[record_index] {
            return;
        }
        self.watched_strengths[record_index] = new_strength;
        let watcher_count = self.program.watched_wires[record_index].watchers.len();
        for index in 0..watcher_count {
            let watcher = self.program.watched_wires[record_index].watchers[index];
            self.update_once(watcher);
        }
    }

    pub(super) fn execute_output_program(&mut self, component: usize, powered: bool) {
        for operation in &self.program.output_programs[component][usize::from(powered)] {
            match *operation {
                Operation::Contribution(link_index) => {
                    let link = &self.program.links[link_index];
                    let old_contribution = self.contributions[link_index];
                    let contribution = self.output(link.source).saturating_sub(link.attenuation);
                    self.contributions[link_index] = contribution;
                    if (old_contribution > 0) != (contribution > 0) {
                        self.updated_in_phase[link.target] = None;
                    }
                }
                Operation::RefreshWire(slot) => {
                    let Block::RedstoneWire { mut wire } = self.blocks[slot] else {
                        unreachable!()
                    };
                    wire.power = self.input_strength(slot);
                    self.blocks[slot] = Block::RedstoneWire { wire };
                }
                Operation::CheckWatchedWire(record) => self.check_watched_wire(record),
                Operation::UpdateLamp(slot) => self.dispatch_receiver(slot),
                Operation::UpdateRepeater(slot) => self.update_repeater(slot),
            }
            self.operations += 1;
        }
    }

    pub(super) fn update_repeater(&mut self, component: usize) {
        let Block::RedstoneRepeater { repeater } = self.blocks[component] else {
            unreachable!()
        };
        if repeater.locked {
            return;
        }
        let rear_powered = self.input_strength(component) > 0;
        if rear_powered == repeater.powered {
            return;
        }
        let tick = ScheduledBlockTick {
            pos: self.program.positions[component],
            block_type: Some(self.blocks[component].registry_id()),
        };
        if self.pending.contains(&tick) {
            return;
        }
        let output_is_diode = self
            .program
            .repeaters
            .iter()
            .find(|record| record.component == component)
            .is_some_and(|record| record.output_is_diode);
        let priority = if output_is_diode {
            TickPriority::Highest
        } else if !rear_powered {
            TickPriority::Higher
        } else {
            TickPriority::High
        };
        self.pending.schedule_tick(tick, repeater.delay as usize, priority);
    }

    pub(super) fn set_input(&mut self, source_index: usize, powered: bool) {
        let source = self.program.sources[source_index];
        let Block::Lever { lever } = self.blocks[source] else { unreachable!() };
        if lever.powered == powered {
            return;
        }
        self.begin_phase();
        let Block::Lever { mut lever } = self.blocks[source] else {
            unreachable!()
        };
        lever.powered = powered;
        self.blocks[source] = Block::Lever { lever };
        self.execute_output_program(source, powered);
    }

    pub(super) fn drain_ticks(&mut self) {
        while let Some(tick) = self.pending.this_tick().pop_first() {
            if let Some(component) = self.program.positions.iter().position(|&pos| pos == tick.pos) {
                match self.blocks[component] {
                    Block::RedstoneLamp { .. } => self.dispatch_receiver_tick(component),
                    Block::RedstoneRepeater { .. } => {
                        self.scheduled_repeater_ticks += 1;
                        self.tick_repeater(component);
                    }
                    _ => {}
                }
                self.operations += 1;
            }
        }
    }

    fn tick_repeater(&mut self, component: usize) {
        let Block::RedstoneRepeater { mut repeater } = self.blocks[component] else {
            unreachable!()
        };
        if repeater.locked {
            return;
        }
        let rear_powered = self.input_strength(component) > 0;
        if repeater.powered {
            if rear_powered {
                return;
            }
            repeater.powered = false;
            self.blocks[component] = Block::RedstoneRepeater { repeater };
            self.begin_phase();
            self.execute_output_program(component, false);
        } else {
            if !rear_powered {
                let tick = ScheduledBlockTick {
                    pos: self.program.positions[component],
                    block_type: Some(self.blocks[component].registry_id()),
                };
                self.pending.schedule_tick(tick, repeater.delay as usize, TickPriority::Higher);
            }
            repeater.powered = true;
            self.blocks[component] = Block::RedstoneRepeater { repeater };
            self.begin_phase();
            self.execute_output_program(component, true);
        }
    }

    pub(super) fn dispatch_receiver_tick(&mut self, receiver: usize) {
        if self.input_strength(receiver) == 0 {
            self.blocks[receiver] = Block::RedstoneLamp { lit: false };
        }
    }

    pub(super) fn tick(&mut self) {
        self.drain_ticks();
        self.pending.end_last_tick_move_next();
        self.drain_ticks();
    }
}
