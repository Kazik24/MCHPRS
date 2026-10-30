use super::{Runtime, ScratchError};
use super::super::TickScheduler;
use mchprs_blocks::{BlockPos, blocks::Block};
#[derive(Clone)]
pub(super) enum Operation {
    Contribution(usize),
    RefreshWire(usize),
    CheckWatchedWire(usize),
    UpdateLamp(usize),
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
pub(super) struct Program {
    pub(super) positions: Vec<BlockPos>,
    pub(super) initial: Vec<Block>,
    pub(super) sources: Vec<usize>,
    pub(super) transitions: Vec<[Vec<Operation>; 2]>,
    pub(super) links: Vec<ElectricalLink>,
    pub(super) incoming: Vec<Vec<usize>>,
    pub(super) watched_wires: Vec<WatchedWire>,
}
impl Program {
    pub(super) fn new(
        positions: Vec<BlockPos>,
        initial: Vec<Block>,
        sources: Vec<usize>,
        transitions: Vec<[Vec<Operation>; 2]>,
        links: Vec<ElectricalLink>,
        mut watched_wires: Vec<WatchedWire>,
    ) -> Result<Self, ScratchError> {
        let invalid = |reason| ScratchError { position: None, reason };
        if positions.len() != initial.len() {
            return Err(invalid("sampled program position/initial length mismatch"));
        }
        let mut unique_positions = std::collections::HashSet::new();
        if positions.iter().any(|position| !unique_positions.insert(*position)) {
            return Err(invalid("sampled program has duplicate component positions"));
        }
        for block in &initial {
            match block {
                Block::Lever { .. } | Block::RedstoneLamp { .. } => {}
                Block::RedstoneWire { wire } if wire.power <= 15 => {}
                Block::RedstoneWire { .. } => return Err(invalid("sampled program wire power exceeds 15")),
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
        if transitions.len() != sources.len() {
            return Err(invalid("sampled program source transition count mismatch"));
        }
        for link in &links {
            let Some(source) = initial.get(link.source) else {
                return Err(invalid("sampled program link source is out of bounds"));
            };
            let Some(target) = initial.get(link.target) else {
                return Err(invalid("sampled program link target is out of bounds"));
            };
            if !matches!(source, Block::Lever { .. } | Block::RedstoneWire { .. }) {
                return Err(invalid("sampled program link source must be lever or wire"));
            }
            if !matches!(target, Block::RedstoneWire { .. } | Block::RedstoneLamp { .. }) {
                return Err(invalid("sampled program link target must be wire or lamp"));
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
        for source_transitions in &transitions {
            for operations in source_transitions {
                for operation in operations {
                    match *operation {
                        Operation::Contribution(link) if link >= links.len() => {
                            return Err(invalid("sampled program contribution link is out of bounds"));
                        }
                        Operation::CheckWatchedWire(record) if record >= watched_wires.len() => {
                            return Err(invalid("sampled program watched-wire index is out of bounds"));
                        }
                        Operation::RefreshWire(slot)
                            if !matches!(initial.get(slot), Some(Block::RedstoneWire { .. })) =>
                        {
                            return Err(invalid("sampled program RefreshWire target is not a wire"));
                        }
                        Operation::UpdateLamp(slot)
                            if !matches!(initial.get(slot), Some(Block::RedstoneLamp { .. })) =>
                        {
                            return Err(invalid("sampled program UpdateLamp target is not a lamp"));
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
        Ok(Self { positions, initial, sources, transitions, links, incoming, watched_wires })
    }
    pub(super) fn start(&self) -> Runtime<'_> {
        Runtime {
            program: self,
            blocks: self.initial.clone(),
            pending: TickScheduler::default(),
            contributions: self.links.iter().map(|link| {
                let strength: u8 = match self.initial[link.source] {
                    Block::Lever { lever } => if lever.powered { 15 } else { 0 },
                    Block::RedstoneWire { wire } => wire.power,
                    _ => 0,
                };
                strength.saturating_sub(link.attenuation)
            }).collect(),
            watched_strengths: self.watched_wires.iter().map(|record| match self.initial[record.wire] {
                Block::RedstoneWire { wire } => wire.power,
                _ => unreachable!(),
            }).collect(),
            phase: 0,
            updated_in_phase: vec![None; self.initial.len()],
            operations: 0,
            deliveries: 0,
        }
    }
}
