use super::{Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::{
    BlockFacing, BlockPos,
    blocks::{Block, ButtonFace, LeverFace},
};
use std::time::Instant;

#[path = "passes/mod.rs"]
mod passes;

struct CompilePhases {
    /// enables phase timing and progress diagnostics.
    enabled: bool,
    /// start time for the entire compilation.
    total: Instant,
    /// start time for the current phase.
    started: Instant,
    /// label of the current compilation phase.
    phase: &'static str,
    /// last detailed stage and its source/component index.
    detail: Option<(&'static str, usize)>,
}

impl CompilePhases {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            enabled: std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some(),
            total: now,
            started: now,
            phase: "scratch snapshot",
            detail: None,
        }
    }

    fn next(&mut self, phase: &'static str) {
        if self.enabled {
            eprintln!("Redpiler compile phase {}: {} ms", self.phase, self.started.elapsed().as_millis());
        }
        self.phase = phase;
        self.started = Instant::now();
        self.detail = None;
    }

    fn detail(&mut self, detail: &'static str, item: usize) {
        self.detail = Some((detail, item));
    }
}

impl Drop for CompilePhases {
    fn drop(&mut self) {
        if self.enabled {
            let detail = self
                .detail
                .map_or_else(String::new, |(step, item)| format!("; last step {step}, source/component {item}"));
            eprintln!(
                "Redpiler compile phase {}{}: {} ms; total {} ms",
                self.phase,
                detail,
                self.started.elapsed().as_millis(),
                self.total.elapsed().as_millis()
            );
        }
    }
}

fn check_cancelled(monitor: Option<&TaskMonitor>) -> Result<(), ScratchError> {
    if monitor.is_some_and(TaskMonitor::cancelled) {
        Err(ScratchError {
            position: None,
            reason: "compilation cancelled",
        })
    } else {
        Ok(())
    }
}

/// Return keys whose values were added, removed, or changed, in sorted order.
pub(super) fn find_changed_links<K: Ord + Copy, V: PartialEq>(
    before: &std::collections::BTreeMap<K, V>,
    after: &std::collections::BTreeMap<K, V>,
) -> Vec<K> {
    before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|source| before.get(source) != after.get(source))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PistonActor {
    /// stable actor index assigned during piston discovery.
    pub(super) id: usize,
    /// world position of the piston base component.
    pub(super) base: BlockPos,
    /// direction in which the piston extends.
    pub(super) facing: BlockFacing,
    /// whether this is a sticky piston.
    pub(super) sticky: bool,
    /// adjacent home cell immediately in front of the base.
    pub(super) home: BlockPos,
    /// far cell at the end of the piston head's travel.
    pub(super) far: BlockPos,
    /// captured initial extended state.
    pub(super) extended: bool,
    /// whether a piston head is initially present.
    pub(super) head_present: bool,
    /// connected piston-group index for overlapping motion/contact groups.
    pub(super) group: usize,
}

#[derive(Clone, Debug)]
pub(super) struct PistonToken {
    /// value carried by this movable token.
    pub(super) block: Block,
    /// initial world position, or `None` for an initially empty piston token.
    pub(super) position: Option<BlockPos>,
    /// legal cells this token may occupy as its piston group moves.
    pub(super) cells: Vec<BlockPos>,
}

pub(super) fn is_passive_geometry(block: Block) -> bool {
    let plate_power = block.pressure_plate_powered();
    !crate::redstone::has_neighbor_update(block)
        && !matches!(block, Block::Composter { .. } | Block::Lever { .. })
        && plate_power != Some(true)
        && (block.is_solid()
            || (block.is_cube() && block.is_transparent())
            || matches!(block, Block::TripwireHook { .. })
            || plate_power == Some(false))
}

/// Capture movable blocks and their legal cells, rejecting groups the runtime cannot model.
pub(super) fn discover_piston_tokens(world: &impl World, actors: &[PistonActor]) -> Result<Vec<PistonToken>, ScratchError> {
    let mut tokens = Vec::new();
    for group in 0..actors.iter().map(|p| p.group + 1).max().unwrap_or(0) {
        let members: Vec<_> = actors.iter().filter(|p| p.group == group).collect();
        let mut cells = Vec::new();
        for piston in &members {
            for cell in [piston.home, piston.far] {
                if !cells.contains(&cell) {
                    cells.push(cell);
                }
            }
        }
        let at = members[0].base;
        if cells.len() > 6 {
            return Err(ScratchError {
                position: Some(at),
                reason: "piston group exceeds six contact cells",
            });
        }
        let start = tokens.len();
        let mut scenery = Vec::new();
        for &cell in &cells {
            let block = world.get_block(cell);
            if matches!(block, Block::Air | Block::PistonHead { .. }) {
                continue;
            }
            let home = members.iter().any(|p| p.home == cell);
            if !home && !members.iter().any(|p| p.sticky && p.far == cell) {
                scenery.push(cell);
                continue;
            }
            if world.get_block_entity(cell).is_some() {
                return Err(ScratchError {
                    position: Some(cell),
                    reason: "unsupported carried block entity",
                });
            }
            if !matches!(block, Block::RedstoneBlock)
                && block.slab_type().is_none()
                && (block.pressure_plate_powered().is_some() || !is_passive_geometry(block))
            {
                return Err(ScratchError {
                    position: Some(cell),
                    reason: "unsupported movable block",
                });
            }
            if home && members.iter().filter(|p| p.home == cell).all(|p| !p.sticky) && !members.iter().any(|p| p.far == cell) {
                return Err(ScratchError {
                    position: Some(cell),
                    reason: "normal piston would push token out of its domain",
                });
            }
            tokens.push(PistonToken {
                block,
                position: Some(cell),
                cells: cells.clone(),
            });
        }
        for token in &mut tokens[start..] {
            token.cells.retain(|cell| !scenery.contains(cell));
        }
        if tokens.len() - start > 2 {
            return Err(ScratchError {
                position: Some(at),
                reason: "piston group exceeds two tokens",
            });
        }
        if tokens.len() - start == 2
            && (members.len() != 2
                || !members.iter().all(|p| p.sticky)
                || members[0].far != members[1].far
                || members[0].facing != members[1].facing.opposite())
        {
            return Err(ScratchError {
                position: Some(at),
                reason: "unsupported two-token piston group",
            });
        }
    }
    Ok(tokens)
}

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
    let mut phases = CompilePhases::new();
    let context = passes::Context {
        world,
        bounds,
        entry: &entry,
        monitor,
    };

    let (scratch, inventory) = passes::parse::run(&context, &mut phases)?;
    let index_started = Instant::now();
    let position_index: std::collections::HashMap<_, _> = inventory.positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();

    if phases.enabled {
        eprintln!("Redpiler compile position index: {} ms", index_started.elapsed().as_millis());
    }

    let (mut program, probe_indexes) = passes::electrical::run(&context, scratch, inventory, &position_index, &mut phases)?;

    passes::movement::run(&context, &mut program, &probe_indexes, &mut phases)?;
    passes::arrangements::run(&context, &mut program, &probe_indexes, &mut phases)?;

    phases.next("reactive dependency tracking");

    check_cancelled(monitor)?;

    passes::reactive::run(world, &entry, &mut program, probe_indexes.components, monitor)?;
    passes::source_order::run(&context, &mut program, &probe_indexes, &mut phases)?;
    passes::finalize::run(&mut program, monitor, &mut phases)?;
    passes::share_output_programs::run(&mut program, monitor, &mut phases)?;

    Ok(program)
}

fn update_order() -> [mchprs_blocks::BlockFace; 6] {
    use mchprs_blocks::BlockFace::*;
    [West, East, Bottom, Top, North, South]
}

/// Return notification targets in the order used by the native redstone updates.
fn update_footprint(pos: BlockPos, block: Block) -> Vec<BlockPos> {
    use mchprs_blocks::BlockFace;
    let neighbours = |pos: BlockPos| update_order().map(|face| pos.offset(face));
    let directional_targets = |rear: BlockFace| {
        let front = pos.offset(rear.opposite());
        std::iter::once(front)
            .chain(update_order().into_iter().filter(|&face| face != rear).map(|face| front.offset(face)))
            .collect()
    };
    match block {
        Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. } => {
            let rear = match block {
                Block::RedstoneRepeater { repeater } => repeater.facing.block_face(),
                Block::RedstoneComparator { comparator } => comparator.facing.block_face(),
                _ => unreachable!(),
            };
            directional_targets(rear)
        }
        Block::Observer { observer } => directional_targets(observer.facing.into()),
        Block::Lever { lever } => {
            let support = match lever.face {
                LeverFace::Floor => BlockFace::Bottom,
                LeverFace::Ceiling => BlockFace::Top,
                LeverFace::Wall => lever.facing.opposite().block_face(),
            };
            neighbours(pos).into_iter().chain(neighbours(pos.offset(support))).collect()
        }
        Block::StoneButton { button } => {
            let support = match button.face {
                ButtonFace::Floor => BlockFace::Bottom,
                ButtonFace::Ceiling => BlockFace::Top,
                ButtonFace::Wall => button.facing.opposite().block_face(),
            };
            neighbours(pos).into_iter().chain(neighbours(pos.offset(support))).collect()
        }
        Block::Piston { .. } => neighbours(pos).to_vec(),
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => {
            let mut positions = Vec::new();
            crate::redstone::surrounding_notifications(pos, |target, _, _| positions.push(target));
            positions
        }
        _ => Vec::new(),
    }
}

pub(super) fn discover_piston_actors(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: ScratchEntry,
) -> Result<Vec<PistonActor>, ScratchError> {
    discover_piston_actors_monitored(world, bounds, entry, None)
}

fn discover_piston_actors_monitored(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: ScratchEntry,
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<PistonActor>, ScratchError> {
    let scratch = ScratchWorld::from_world_monitored(world, bounds, entry, monitor)?;
    let world = scratch.world();
    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    let mut actors = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let base = BlockPos::new(x, y, z);
                let block = world.get_block(base);
                let Block::Piston { piston } = block else {
                    if matches!(block, Block::MovingPiston { .. }) {
                        return Err(error(base, "moving piston state is unsupported"));
                    }
                    continue;
                };
                if world.get_block_entity(base).is_some() {
                    return Err(error(base, "unsupported block entity in piston capture"));
                }
                let facing = piston.facing;
                let home = base.offset(facing.into());
                let far = home.offset(facing.into());
                let home_block = world.get_block(home);
                let _ = world.get_block(far);
                if world.get_block_entity(home).is_some() {
                    return Err(error(home, "unsupported block entity at piston contact"));
                }
                if world.get_block_entity(far).is_some() {
                    return Err(error(far, "unsupported block entity at piston contact"));
                }
                let head_present = match home_block {
                    Block::PistonHead { head } if head.facing == facing && head.sticky == piston.sticky && !head.short => true,
                    Block::PistonHead { .. } => return Err(error(home, "piston head does not match base state")),
                    Block::MovingPiston { .. } => return Err(error(home, "moving piston state is unsupported")),
                    _ => false,
                };
                if !piston.extended && head_present {
                    return Err(error(base, "piston base and head state disagree"));
                }
                actors.push(PistonActor {
                    id: actors.len(),
                    base,
                    facing,
                    sticky: piston.sticky,
                    home,
                    far,
                    extended: piston.extended,
                    head_present,
                    group: 0,
                });
            }
        }
    }
    scratch.finish_probe()?;
    assign_piston_groups(&mut actors);
    Ok(actors)
}

/// Group pistons whose contact cells overlap, including indirect overlaps.
pub(super) fn assign_piston_groups(actors: &mut [PistonActor]) {
    let mut parents: Vec<_> = (0..actors.len()).collect();
    let mut owner_by_cell = std::collections::HashMap::new();
    for (index, actor) in actors.iter().enumerate() {
        for cell in [actor.home, actor.far] {
            if let Some(&owner) = owner_by_cell.get(&cell) {
                let left = piston_group_root(&mut parents, index);
                let right = piston_group_root(&mut parents, owner);
                parents[left] = right;
            } else {
                owner_by_cell.insert(cell, index);
            }
        }
    }
    let mut root_groups = std::collections::HashMap::new();
    for (index, actor) in actors.iter_mut().enumerate() {
        let root = piston_group_root(&mut parents, index);
        let next = root_groups.len();
        actor.group = *root_groups.entry(root).or_insert(next);
    }
}

fn piston_group_root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}
