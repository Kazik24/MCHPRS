use super::super::TickScheduler;
use super::{Runtime, ScratchError};
use mchprs_blocks::{BlockPos, blocks::Block};

#[derive(Clone)]
pub(super) enum Operation {
    Contribution(usize),
    RefreshWire(usize),
    CheckWatchedWire(usize),
    UpdateLamp(usize),
    UpdateRepeater(usize),
}

#[derive(Clone)]
pub(super) struct ElectricalLink {
    pub(super) source: usize,
    pub(super) target: usize,
    pub(super) attenuation: u8,
}

pub(super) struct WatchedWire {
    pub(super) wire: usize,
    pub(super) watchers: Vec<usize>,
}

pub(super) struct RepeaterRecord {
    pub(super) component: usize,
    pub(super) output_is_diode: bool,
    pub(super) side_sources: Option<Vec<usize>>,
}

pub(super) struct Program {
    pub(super) pistons: Vec<super::compiler::PistonActor>,
    pub(super) tokens: Vec<super::compiler::PistonToken>,
    pub(super) positions: Vec<BlockPos>,
    pub(super) initial: Vec<Block>,
    pub(super) sources: Vec<usize>,
    pub(super) output_programs: Vec<[Vec<Operation>; 2]>,
    pub(super) links: Vec<ElectricalLink>,
    pub(super) incoming: Vec<Vec<usize>>,
    pub(super) watched_wires: Vec<WatchedWire>,
    pub(super) repeaters: Vec<RepeaterRecord>,
}

impl Program {
    pub(super) fn new(
        positions: Vec<BlockPos>,
        initial: Vec<Block>,
        sources: Vec<usize>,
        output_programs: Vec<[Vec<Operation>; 2]>,
        links: Vec<ElectricalLink>,
        mut watched_wires: Vec<WatchedWire>,
        repeaters: Vec<RepeaterRecord>,
    ) -> Result<Self, ScratchError> {
        let invalid = |reason| ScratchError { position: None, reason };
        if positions.len() != initial.len() {
            return Err(invalid("sampled program position/initial length mismatch"));
        }
        if output_programs.len() != initial.len() {
            return Err(invalid("sampled output program count mismatch"));
        }
        let mut unique_positions = std::collections::HashSet::new();
        if positions.iter().any(|position| !unique_positions.insert(*position)) {
            return Err(invalid("sampled program has duplicate component positions"));
        }
        for block in &initial {
            match block {
                Block::Lever { .. } | Block::RedstoneLamp { .. } | Block::RedstoneBlock => {}
                Block::RedstoneWire { wire } if wire.power <= 15 => {}
                Block::RedstoneWire { .. } => return Err(invalid("sampled program wire power exceeds 15")),
                Block::RedstoneRepeater { repeater } if (1..=4).contains(&repeater.delay) => {}
                Block::RedstoneRepeater { .. } => return Err(invalid("sampled repeater delay is outside 1..=4")),
                _ => return Err(invalid("sampled program contains unsupported component")),
            }
        }
        let mut unique_sources = std::collections::HashSet::new();
        for &source in &sources {
            let Some(block) = initial.get(source) else {
                return Err(invalid("sampled program source index is out of bounds"));
            };
            if !unique_sources.insert(source) {
                return Err(invalid("sampled program source is duplicated"));
            }
            if !matches!(block, Block::Lever { .. }) {
                return Err(invalid("sampled program source is not a lever"));
            }
        }
        let mut repeater_components = std::collections::HashSet::new();
        for record in &repeaters {
            if !matches!(initial.get(record.component), Some(Block::RedstoneRepeater { .. })) {
                return Err(invalid("sampled repeater record index is not a repeater"));
            }
            if !repeater_components.insert(record.component) {
                return Err(invalid("sampled repeater record is duplicated"));
            }
            if record.side_sources.iter().flatten().any(|&source| !matches!(initial.get(source), Some(Block::RedstoneRepeater { .. }))) {
                return Err(invalid("sampled repeater side source is not a repeater"));
            }
        }
        for (index, block) in initial.iter().enumerate() {
            if matches!(block, Block::RedstoneRepeater { .. }) && !repeater_components.contains(&index) {
                return Err(invalid("sampled repeater has no repeater record"));
            }
        }
        for link in &links {
            let Some(source) = initial.get(link.source) else {
                return Err(invalid("sampled program link source is out of bounds"));
            };
            let Some(target) = initial.get(link.target) else {
                return Err(invalid("sampled program link target is out of bounds"));
            };
            if !matches!(source, Block::Lever { .. } | Block::RedstoneWire { .. } | Block::RedstoneRepeater { .. } | Block::RedstoneBlock) {
                return Err(invalid("sampled program link source must be lever, wire or repeater"));
            }
            if !matches!(
                target,
                Block::RedstoneWire { .. } | Block::RedstoneLamp { .. } | Block::RedstoneRepeater { .. }
            ) {
                return Err(invalid("sampled program link target must be wire, lamp or repeater"));
            }
            if link.attenuation > 15 {
                return Err(invalid("sampled program attenuation exceeds 15"));
            }
        }
        for record in &mut watched_wires {
            if !matches!(initial.get(record.wire), Some(Block::RedstoneWire { .. })) {
                return Err(invalid("sampled watched-wire index is not a wire"));
            }
            let mut recipients = std::collections::HashSet::new();
            record.watchers.retain(|watcher| recipients.insert(*watcher));
            for &watcher in &record.watchers {
                if !matches!(initial.get(watcher), Some(Block::RedstoneLamp { .. })) {
                    return Err(invalid("sampled watched-wire receiver is not a lamp"));
                }
            }
        }
        for programs in &output_programs {
            for operations in programs {
                for operation in operations {
                    match *operation {
                        Operation::Contribution(link) if link >= links.len() => {
                            return Err(invalid("sampled program contribution link is out of bounds"));
                        }
                        Operation::CheckWatchedWire(record) if record >= watched_wires.len() => {
                            return Err(invalid("sampled program watched-wire index is out of bounds"));
                        }
                        Operation::RefreshWire(slot) if !matches!(initial.get(slot), Some(Block::RedstoneWire { .. })) => {
                            return Err(invalid("sampled program RefreshWire target is not a wire"));
                        }
                        Operation::UpdateLamp(slot) if !matches!(initial.get(slot), Some(Block::RedstoneLamp { .. })) => {
                            return Err(invalid("sampled program UpdateLamp target is not a lamp"));
                        }
                        Operation::UpdateRepeater(slot) if !matches!(initial.get(slot), Some(Block::RedstoneRepeater { .. })) => {
                            return Err(invalid("sampled program UpdateRepeater target is not a repeater"));
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut incoming = vec![vec![]; initial.len()];
        for (id, link) in links.iter().enumerate() {
            incoming[link.target].push(id);
        }
        Ok(Self {
            pistons: Vec::new(),
            tokens: Vec::new(),
            positions,
            initial,
            sources,
            output_programs,
            links,
            incoming,
            watched_wires,
            repeaters,
        })
    }

    pub(super) fn start(&self) -> Runtime<'_> {
        Runtime {
            program: self,
            piston_extended: self.pistons.iter().map(|p| p.extended).collect(),
            piston_head_present: self.pistons.iter().map(|p| p.head_present).collect(),
            piston_head_moving: vec![false; self.pistons.len()],
            token_positions: self.tokens.iter().map(|t| t.position).collect(),
            piston_events: std::collections::VecDeque::new(),
            queued_piston_events: vec![0; self.pistons.len()],
            motions: Vec::new(),
            motion_time: 0,
            accepted_piston_actions: 0,
            token_movements: 0,
            motion_completions: 0,
            blocks: self.initial.clone(),
            pending: TickScheduler::default(),
            contributions: self
                .links
                .iter()
                .map(|link| {
                    let strength: u8 = match self.initial[link.source] {
                        Block::Lever { lever } => {
                            if lever.powered {
                                15
                            } else {
                                0
                            }
                        }
                        Block::RedstoneWire { wire } => wire.power,
                        Block::RedstoneBlock => 15,
                        Block::RedstoneRepeater { repeater } => {
                            if repeater.powered {
                                15
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };
                    strength.saturating_sub(link.attenuation)
                })
                .collect(),
            watched_strengths: self
                .watched_wires
                .iter()
                .map(|record| match self.initial[record.wire] {
                    Block::RedstoneWire { wire } => wire.power,
                    _ => unreachable!(),
                })
                .collect(),
            phase: 0,
            updated_in_phase: vec![None; self.initial.len()],
            operations: 0,
            deliveries: 0,
            scheduled_repeater_ticks: 0,
            tick_operations: 0,
            failure: None,
        }
    }
}
