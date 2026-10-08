//! Bounded dust routing over an immutable snapshot. Safety is structural: current
//! power is never evidence that a source, consumer or update channel is harmless.
use crate::interaction;
use crate::plot::PlotWorld;
use crate::redpiler::analysis::ports::{consumer_roots, ConsumerInput};
use crate::redstone::{self, power, wire};
use crate::world::{storage::Chunk, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use mchprs_world::{PistonState, TickPriority};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::Cell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const MAX_PLACEMENTS: usize = 512;
const MAX_CELLS: usize = 131_072;
const MAX_STATES: usize = 8_192;
const MAX_VISITS: usize = 131_072;
const SEARCH_TIME: Duration = Duration::from_millis(50);
const SLICE_TIME: Duration = Duration::from_millis(1);
const MOTION_REACH: i32 = 13;
static SNAPSHOT_BYTES: AtomicUsize = AtomicUsize::new(0);
const GLOBAL_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

struct Reservation(usize);

impl Reservation {
    fn new(bytes: usize) -> Result<Self, String> {
        SNAPSHOT_BYTES
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                (used + bytes <= GLOBAL_SNAPSHOT_BYTES).then_some(used + bytes)
            })
            .map(|_| Self(bytes))
            .map_err(|_| {
                "Wire snapshot memory budget reached; try again after another route finishes."
                    .into()
            })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        SNAPSHOT_BYTES.fetch_sub(self.0, Ordering::Relaxed);
    }
}

fn contains(bounds: (BlockPos, BlockPos), p: BlockPos) -> bool {
    p.min(bounds.0) == bounds.0 && p.max(bounds.1) == bounds.1
}

/// Preserve every geometric property; only outputs independent of the safety
/// predicates are ignored. In particular piston extension and dust sides remain.
use crate::world::wire_cache::routing_state as geometry;

#[derive(Clone)]
pub(super) struct Snapshot {
    data: Arc<Data>,
    versions: Arc<Vec<(i32, i32, (u64, u64))>>,
}

struct Data {
    _reservation: Reservation,
    bounds: (BlockPos, BlockPos),
    route_bounds: (BlockPos, BlockPos),
    world_bounds: (BlockPos, BlockPos),
    size: BlockPos,
    blocks: Vec<u32>,
}

impl Data {
    fn position(&self, i: usize) -> BlockPos {
        let x = i % self.size.x as usize;
        let yz = i / self.size.x as usize;
        self.bounds.0
            + BlockPos::new(
                x as i32,
                (yz / self.size.z as usize) as i32,
                (yz % self.size.z as usize) as i32,
            )
    }

    fn index(&self, p: BlockPos) -> Option<usize> {
        contains(self.bounds, p).then(|| {
            let p = p - self.bounds.0;
            ((p.y * self.size.z + p.z) * self.size.x + p.x) as usize
        })
    }
}

pub(super) struct Capture {
    data: Option<Data>,
    next: usize,
    versions: Vec<(i32, i32, (u64, u64))>,
}

impl Capture {
    pub(super) fn new(world: &PlotWorld, start: BlockPos, end: BlockPos) -> Result<Self, String> {
        let world_bounds = world.get_corners();
        if world.is_cursed() {
            return Err(
                "Wire routing requires ordinary placement rules (cursed plots are unsupported)."
                    .into(),
            );
        }
        if !contains(world_bounds, start)
            || !contains(world_bounds, end)
            || start.y == 0
            || end.y == 0
        {
            return Err(
                "Both wire endpoints must be inside the plot with room for support.".into(),
            );
        }
        let padding = BlockPos::new(4, 2, 4);
        let route_bounds = (
            (start.min(end) - padding).max(world_bounds.0 + BlockPos::new(0, 1, 0)),
            (start.max(end) + padding).min(world_bounds.1),
        );
        let halo = BlockPos::new(MOTION_REACH, MOTION_REACH + 1, MOTION_REACH);
        let bounds = (
            (route_bounds.0 - halo).max(world_bounds.0),
            (route_bounds.1 + halo).min(world_bounds.1),
        );
        let size = bounds.1 - bounds.0 + BlockPos::new(1, 1, 1);
        let cells = size.x as usize * size.y as usize * size.z as usize;
        if cells > MAX_CELLS {
            return Err("Wire snapshot budget reached; place a shorter segment.".into());
        }
        let mut versions = Vec::new();
        for x in bounds.0.x.div_euclid(16)..=bounds.1.x.div_euclid(16) {
            for z in bounds.0.z.div_euclid(16)..=bounds.1.z.div_euclid(16) {
                let chunk = world
                    .get_chunk(x, z)
                    .ok_or("Wire routing requires loaded safety context.")?;
                versions.push((x, z, chunk.routing_snapshot_version()));
            }
        }
        Ok(Self {
            data: Some(Data {
                _reservation: Reservation::new(cells * 4)?,
                bounds,
                route_bounds,
                world_bounds,
                size,
                blocks: vec![0; cells],
            }),
            next: 0,
            versions,
        })
    }

    pub(super) fn step(&mut self, world: &PlotWorld) -> Result<Option<Snapshot>, String> {
        let Some(data) = self.data.as_mut() else {
            return Ok(None);
        };
        let started = Instant::now();
        let mut visited = 0;
        while self.next < data.blocks.len() && visited < 1024 && started.elapsed() < SLICE_TIME {
            let p = data.position(self.next);
            let chunk = world
                .get_chunk(p.x.div_euclid(16), p.z.div_euclid(16))
                .ok_or("Wire safety context was unloaded.")?;
            if chunk.sections[p.y as usize / 16].block_count() == 0 {
                let count = (16 - p.x.rem_euclid(16)).min(data.bounds.1.x - p.x + 1) as usize;
                self.next += count;
            } else {
                data.blocks[self.next] = geometry(world.get_block(p));
                self.next += 1;
            }
            visited += 1;
        }
        if self.next == data.blocks.len() {
            Ok(Some(Snapshot {
                data: Arc::new(self.data.take().unwrap()),
                versions: Arc::new(std::mem::take(&mut self.versions)),
            }))
        } else {
            Ok(None)
        }
    }
}

impl Snapshot {
    pub(super) fn contains_target(&self, start: BlockPos, end: BlockPos) -> bool {
        contains(self.data.route_bounds, start) && contains(self.data.route_bounds, end)
    }

    pub(super) fn is_current(&self, world: &PlotWorld) -> bool {
        !world.is_cursed()
            && self.data.world_bounds == world.get_corners()
            && self.versions.iter().all(|&(x, z, version)| {
                world
                    .get_chunk(x, z)
                    .is_some_and(|chunk| chunk.routing_snapshot_version() == version)
            })
    }

    #[cfg(test)]
    pub(super) fn bytes(&self) -> usize {
        self.data.blocks.len() * std::mem::size_of::<u32>()
    }

    #[cfg(test)]
    pub(super) fn validate_live(&self, world: &PlotWorld) -> bool {
        let mut check = GeometryCheck::new(self.clone());
        loop {
            if let Some(valid) = check.step(world) {
                return valid;
            }
        }
    }
}

/// Dirty chunks are checked in bounded slices. New edits to an already checked
/// chunk restart its check, rather than producing a mixed-time certificate.
pub(super) struct GeometryCheck {
    snapshot: Snapshot,
    next: usize,
    versions: Vec<(i32, i32, (u64, u64))>,
    dirty: FxHashSet<(i32, i32)>,
    initialized: bool,
}

impl GeometryCheck {
    pub(super) fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            next: 0,
            versions: Vec::new(),
            dirty: FxHashSet::default(),
            initialized: false,
        }
    }

    /// Call only after `step` returns true; the immutable cells remain shared.
    pub(super) fn checked_snapshot(&self) -> Snapshot {
        Snapshot {
            data: self.snapshot.data.clone(),
            versions: Arc::new(self.versions.clone()),
        }
    }

    pub(super) fn step(&mut self, world: &PlotWorld) -> Option<bool> {
        if self.snapshot.data.world_bounds != world.get_corners() || world.is_cursed() {
            return Some(false);
        }
        let started = Instant::now();
        if !self.initialized {
            for &(x, z, old) in self.snapshot.versions.iter() {
                let Some(chunk) = world.get_chunk(x, z) else {
                    return Some(false);
                };
                let version = chunk.routing_snapshot_version();
                self.versions.push((x, z, version));
                if version != old {
                    self.dirty.insert((x, z));
                }
            }
            self.initialized = true;
        }
        if self.dirty.is_empty() {
            return Some(true);
        }
        let data = &self.snapshot.data;
        let mut inspected = 0;
        while self.next < data.blocks.len() && inspected < 1024 && started.elapsed() < SLICE_TIME {
            let p = data.position(self.next);
            self.next += 1;
            if self
                .dirty
                .contains(&(p.x.div_euclid(16), p.z.div_euclid(16)))
                && geometry(world.get_block(p)) != data.blocks[self.next - 1]
            {
                return Some(false);
            }
            inspected += 1;
        }
        if self.next != data.blocks.len() {
            return None;
        }
        for &mut (x, z, ref mut version) in &mut self.versions {
            let Some(chunk) = world.get_chunk(x, z) else {
                return Some(false);
            };
            let current = chunk.routing_snapshot_version();
            if current != *version {
                self.dirty.insert((x, z));
                *version = current;
                self.next = 0;
            }
        }
        (self.next == data.blocks.len()).then_some(true)
    }
}

pub(super) struct Plan {
    pub(super) path: Vec<BlockPos>,
    /// Supports precede dust so normal placement callbacks remain valid.
    pub(super) placements: Vec<(BlockPos, Block)>,
    pub(super) reads: Snapshot,
}

pub(super) enum SearchResult {
    Found(Plan),
    NoPath,
    BudgetExceeded,
    Cancelled,
    Invalid(String),
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    started: Instant,
    visits: Cell<usize>,
    missing: Cell<bool>,
}

impl Budget<'_> {
    fn check(&self) -> Result<(), SearchResult> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(SearchResult::Cancelled);
        }
        if self.visits.get() > MAX_VISITS || self.started.elapsed() >= SEARCH_TIME {
            return Err(SearchResult::BudgetExceeded);
        }
        if self.missing.get() {
            return Err(SearchResult::Invalid(
                "Routing needs safety context outside the captured region.".into(),
            ));
        }
        Ok(())
    }
}

struct View<'a> {
    snapshot: &'a Snapshot,
    edits: FxHashMap<BlockPos, Block>,
    budget: &'a Budget<'a>,
    piston_state: PistonState,
}

impl View<'_> {
    fn original(&self, pos: BlockPos) -> Block {
        self.budget.visits.set(self.budget.visits.get() + 1);
        if let Some(index) = self.snapshot.data.index(pos) {
            Block::from_id(self.snapshot.data.blocks[index])
        } else if !contains(self.snapshot.data.world_bounds, pos) {
            Block::Air {}
        } else {
            self.budget.missing.set(true);
            Block::Unknown { id: u32::MAX }
        }
    }
}

// The view is read-only; mutable World operations are deliberately unreachable.
impl World for View<'_> {
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        self.edits
            .get(&pos)
            .copied()
            .unwrap_or_else(|| self.original(pos))
            .get_id()
    }
    fn set_block_raw(&mut self, _: BlockPos, _: u32) -> bool {
        unreachable!("read-only routing view")
    }
    fn delete_block_entity(&mut self, _: BlockPos) {
        unreachable!("read-only routing view")
    }
    fn get_block_entity(&self, _: BlockPos) -> Option<&BlockEntity> {
        None
    }
    fn get_block_entity_mut(&mut self, _: BlockPos) -> Option<&mut BlockEntity> {
        unreachable!("read-only routing view")
    }
    fn set_block_entity(&mut self, _: BlockPos, _: BlockEntity) {
        unreachable!("read-only routing view")
    }
    fn piston_state(&self) -> &PistonState {
        &self.piston_state
    }
    fn piston_state_mut(&mut self) -> &mut PistonState {
        unreachable!("read-only routing view")
    }
    fn get_chunk(&self, _: i32, _: i32) -> Option<&Chunk> {
        None
    }
    fn get_chunk_mut(&mut self, _: i32, _: i32) -> Option<&mut Chunk> {
        unreachable!("read-only routing view")
    }
    fn schedule_tick(&mut self, _: BlockPos, _: u32, _: TickPriority) {
        unreachable!("read-only routing view")
    }
    fn schedule_half_tick(&mut self, _: BlockPos, _: u32, _: TickPriority) {
        unreachable!("read-only routing view")
    }
    fn pending_tick_at(&mut self, _: BlockPos) -> bool {
        unreachable!("read-only routing view")
    }
    fn block_action(&mut self, _: BlockPos, _: crate::world::BlockAction) {
        unreachable!("read-only routing view")
    }
    fn play_sound(&mut self, _: BlockPos, _: i32, _: i32, _: f32, _: f32) {
        unreachable!("read-only routing view")
    }
}

fn plain_wire() -> Block {
    Block::RedstoneWire {
        wire: RedstoneWire::default(),
    }
}

/// Directed dust adjacency follows the interpreter's power walk, including
/// transparent supports (which can make an apparently valid stair one-way).
fn connects(view: &View<'_>, from: BlockPos, to: BlockPos) -> bool {
    let d = to - from;
    if d.x.abs() + d.z.abs() != 1 || d.y.abs() > 1 {
        return false;
    }
    let neighbor = BlockPos::new(to.x, from.y, to.z);
    match d.y {
        0 => true,
        1 => {
            !view.get_block(from.offset(BlockFace::Top)).is_solid()
                && !view.get_block(neighbor).is_transparent()
        }
        -1 => !view.get_block(neighbor).is_solid(),
        _ => false,
    }
}

fn failure(view: &View<'_>, pos: BlockPos, is_dust: bool) -> Option<String> {
    // Future piston motion must not start pushing new supports or destroy dust.
    for face in BlockFace::values() {
        let mut p = pos;
        for distance in 1..=MOTION_REACH {
            let next = p.offset(face);
            // Payload walks stop at the plot edge; no outside block can move in.
            if next == p || !contains(view.snapshot.data.world_bounds, next) {
                break;
            }
            p = next;
            let block = view.original(p);
            match block {
                Block::Air {} => break,
                Block::Piston { .. } => {
                    return Some(format!("Piston movement can reach the route at {pos}."))
                }
                Block::MovingPiston { .. } | Block::PistonHead { .. } => {
                    return Some(format!(
                        "Moving geometry near {pos} cannot be certified safe."
                    ))
                }
                _ => {}
            }
            if mchprs_blocks::block_entities::ContainerType::from_block(block).is_some() {
                break;
            }
            // ponytail: interpreted payload lines have no length cap; certify
            // longer occupied rays only when bounded mechanical analysis exists.
            if distance == MOTION_REACH {
                return Some(format!(
                    "An occupied piston-motion ray at {pos} exceeds the safety context."
                ));
            }
        }
    }
    for face in BlockFace::values() {
        let p = pos.offset(face);
        if let Block::Observer { observer } = view.original(p) {
            if BlockFace::from(observer.facing) == face.opposite() {
                return Some(format!("An observer watches the changed block at {pos}."));
            }
        }
    }
    if is_dust {
        for p in crate::world::wire_cache::positions(pos) {
            if matches!(
                view.original(p),
                Block::Piston { .. } | Block::PistonHead { .. } | Block::MovingPiston { .. }
            ) {
                return Some(format!(
                    "Dust placement would notify piston geometry at {p} (including BUD/QC)."
                ));
            }
        }
    } else {
        for face in BlockFace::values() {
            let p = pos.offset(face);
            if matches!(
                view.original(p),
                Block::Piston { .. } | Block::PistonHead { .. } | Block::MovingPiston { .. }
            ) {
                return Some(format!(
                    "Support placement would notify piston geometry at {p}."
                ));
            }
            for vertical in [BlockFace::Top, BlockFace::Bottom] {
                let p = p.offset(vertical);
                if matches!(
                    view.original(p),
                    Block::PistonHead { .. } | Block::MovingPiston { .. }
                ) {
                    return Some(format!("Support placement touches moving geometry at {p}."));
                }
            }
        }
    }
    None
}

fn potential_input(view: &View<'_>, pos: BlockPos) -> bool {
    for face in BlockFace::values() {
        let p = pos.offset(face);
        let block = view.get_block(p);
        if block.is_solid() {
            for strong_face in BlockFace::values() {
                let source = p.offset(strong_face);
                if power::emits_strong_power(
                    view.get_block(source),
                    view,
                    source,
                    strong_face,
                    false,
                ) {
                    return true;
                }
            }
        } else if power::emits_weak_power(block, view, p, face, false) {
            return true;
        }
    }
    false
}

fn receives(
    view: &View<'_>,
    consumer: Block,
    at: BlockPos,
    source: BlockPos,
    any_heading: bool,
) -> bool {
    let mut roots = consumer_roots(consumer, at);
    if matches!(consumer, Block::Hopper { .. }) {
        roots.extend(BlockFace::values().map(|face| (at.offset(face), face, ConsumerInput::Main)));
    }
    if let Block::Piston { piston } = consumer {
        for face in BlockFace::values() {
            if face != BlockFace::from(piston.facing) {
                roots.push((at.offset(face), face, ConsumerInput::Main));
            }
            roots.push((
                at.offset(BlockFace::Top).offset(face),
                face,
                ConsumerInput::Main,
            ));
        }
    }
    for (root, face, input) in roots {
        if root == source {
            // Every possible intermediate dust heading must be harmless, too.
            if redstone::is_diode(consumer)
                || if any_heading {
                    face != BlockFace::Bottom
                } else {
                    power::emits_weak_power(view.get_block(source), view, source, face, true)
                }
            {
                return true;
            }
        } else if input == ConsumerInput::Main && view.get_block(root).is_solid() {
            for side in BlockFace::values() {
                if root.offset(side) == source
                    && if any_heading {
                        side != BlockFace::Bottom
                    } else {
                        power::emits_strong_power(view.get_block(source), view, source, side, true)
                    }
                {
                    return true;
                }
            }
        }
    }
    false
}

fn recipients(pos: BlockPos) -> impl Iterator<Item = BlockPos> {
    (-3i32..=3).flat_map(move |x| {
        (-3i32..=3).flat_map(move |y| {
            (-3i32..=3).filter_map(move |z| {
                (x.abs() + y.abs() + z.abs() <= 3).then_some(pos + BlockPos::new(x, y, z))
            })
        })
    })
}

struct Proposed<'a> {
    view: View<'a>,
    supports: Vec<(BlockPos, Block)>,
    dust: Vec<BlockPos>,
    positions: FxHashSet<BlockPos>,
}

/// Prefixes and completed routes use the same block assignments. A staircase's
/// higher support must be opaque for dust to transmit in both directions.
fn proposed<'a>(
    snapshot: &'a Snapshot,
    path: &[BlockPos],
    budget: &'a Budget<'a>,
) -> Result<Proposed<'a>, String> {
    let start = path[0];
    let end = *path.last().unwrap();
    let positions: FxHashSet<_> = path.iter().copied().collect();
    if positions.len() != path.len() {
        return Err("The route crosses itself.".into());
    }
    let raised: FxHashSet<_> = path
        .windows(2)
        .filter_map(|pair| {
            (pair[0].y != pair[1].y).then(|| {
                if pair[0].y > pair[1].y {
                    pair[0]
                } else {
                    pair[1]
                }
                .offset(BlockFace::Bottom)
            })
        })
        .collect();
    let mut view = View {
        snapshot,
        edits: FxHashMap::default(),
        budget,
        piston_state: PistonState::default(),
    };
    let mut supports = Vec::new();
    let mut dust = Vec::new();
    for &pos in path {
        if !contains(snapshot.data.route_bounds, pos) {
            return Err("The route leaves the search corridor.".into());
        }
        let old = view.original(pos);
        if !matches!(old, Block::Air {})
            && !(matches!(old, Block::RedstoneWire { .. }) && (pos == start || pos == end))
        {
            return Err(format!("The route would overwrite a block at {pos}."));
        }
        let below = pos.offset(BlockFace::Bottom);
        if positions.contains(&below) {
            return Err(format!("A dust cell overlaps required support at {below}."));
        }
        if matches!(view.get_block(below), Block::Air {}) {
            let block = if raised.contains(&below) {
                Block::Wool {
                    color: mchprs_blocks::BlockColorVariant::White,
                }
            } else {
                Block::Glass {}
            };
            view.edits.insert(below, block);
            supports.push((below, block));
        }
        if matches!(old, Block::Air {}) {
            view.edits.insert(pos, plain_wire());
            dust.push(pos);
        }
    }
    Ok(Proposed {
        view,
        supports,
        dust,
        positions,
    })
}

/// Check a complete overlay. Invalid candidate geometry never blacklists cells
/// for sibling paths: planned supports and approach direction are part of state.
fn make_plan(
    snapshot: &Snapshot,
    path: &[BlockPos],
    budget: &Budget<'_>,
) -> Result<Result<Plan, String>, SearchResult> {
    budget.check()?;
    let Proposed {
        view,
        supports,
        dust,
        positions,
    } = match proposed(snapshot, path, budget) {
        Ok(proposed) => proposed,
        Err(reason) => return Ok(Err(reason)),
    };
    if supports.len() + dust.len() > MAX_PLACEMENTS {
        return Err(SearchResult::BudgetExceeded);
    }
    let before = View {
        snapshot,
        edits: FxHashMap::default(),
        budget,
        piston_state: PistonState::default(),
    };
    let support_view = View {
        snapshot,
        edits: supports.iter().copied().collect(),
        budget,
        piston_state: PistonState::default(),
    };
    for &(pos, support) in &supports {
        budget.check()?;
        if let Some(reason) = failure(&view, pos, false) {
            return Ok(Err(reason));
        }
        if support.is_solid() {
            for face in BlockFace::values() {
                let source = pos.offset(face);
                if !positions.contains(&source)
                    && (power::emits_strong_power(
                        view.original(source),
                        &before,
                        source,
                        face,
                        true,
                    ) || power::emits_strong_power(
                        view.original(source),
                        &view,
                        source,
                        face,
                        true,
                    ))
                {
                    return Ok(Err(format!(
                        "The staircase support at {pos} would conduct an unselected source at {source}."
                    )));
                }
            }
            for p in crate::world::wire_cache::positions(pos) {
                if positions.contains(&p) {
                    continue;
                }
                if let Block::RedstoneWire { wire: original } = view.original(p) {
                    let original =
                        wire::get_raw_sides_from(original, p, |pos| before.get_block(pos));
                    if original
                        != wire::get_raw_sides_from(original, p, |pos| support_view.get_block(pos))
                        || original
                            != wire::get_raw_sides_from(original, p, |pos| view.get_block(pos))
                    {
                        return Ok(Err(format!(
                            "The staircase support at {pos} would change unselected dust at {p}."
                        )));
                    }
                }
            }
        }
        for face in BlockFace::values() {
            let neighbor = pos.offset(face);
            let before = view.original(neighbor);
            if !positions.contains(&neighbor)
                && interaction::is_valid_position(before, &view, neighbor) == false
            {
                return Ok(Err(format!(
                    "Support placement invalidates an attachment at {neighbor}."
                )));
            }
            if matches!(before, Block::NoteBlock { .. }) && face == BlockFace::Top {
                return Ok(Err(format!(
                    "Support placement changes a note block instrument at {neighbor}."
                )));
            }
            if matches!(before, Block::NoteBlock { .. }) && face == BlockFace::Bottom {
                return Ok(Err(format!(
                    "Support placement would mute the note block at {neighbor}."
                )));
            }
        }
    }
    for (index, &pos) in path.iter().enumerate() {
        budget.check()?;
        if !interaction::is_valid_position(plain_wire(), &view, pos) {
            return Ok(Err(format!("Invalid dust support at {pos}.")));
        }
        if index > 0
            && (!connects(&view, path[index - 1], pos) || !connects(&view, pos, path[index - 1]))
        {
            return Ok(Err(format!(
                "Dust cannot connect across the step at {pos}."
            )));
        }
        for (other_index, &other) in path.iter().enumerate() {
            if index.abs_diff(other_index) > 1
                && (connects(&view, pos, other) || connects(&view, other, pos))
            {
                return Ok(Err(format!(
                    "The route makes an unintended junction at {pos}."
                )));
            }
        }
        if !matches!(view.original(pos), Block::Air {}) {
            continue;
        }
        if matches!(
            view.original(pos.offset(BlockFace::Bottom)),
            Block::NoteBlock { .. }
        ) {
            return Ok(Err(format!(
                "Dust placement would mute the note block below {pos}."
            )));
        }
        if let Some(reason) = failure(&view, pos, true) {
            return Ok(Err(reason));
        }
        if potential_input(&view, pos) {
            return Ok(Err(format!(
                "An existing power source can inject into the wire at {pos}."
            )));
        }
        for p in crate::world::wire_cache::positions(pos) {
            if positions.contains(&p) {
                continue;
            }
            if matches!(view.original(p), Block::RedstoneWire { .. })
                && (connects(&view, pos, p) || connects(&view, p, pos))
            {
                return Ok(Err(format!(
                    "The route would merge with an unselected wire at {p}."
                )));
            }
        }
        // A QC receiver can be three faces away through an above-base conductor.
        for p in recipients(pos) {
            if receives(&view, view.original(p), p, pos, true) {
                return Ok(Err(format!(
                    "The route adds an unintended input to the circuit at {p}."
                )));
            }
        }
    }
    // Existing dust shape changes can redirect power to unrelated consumers.
    for &pos in path {
        if let Block::RedstoneWire { wire: old } = view.original(pos) {
            let original = wire::get_regulated_sides(old, &before, pos);
            let new = wire::get_regulated_sides(old, &view, pos);
            let intermediate = wire::get_regulated_sides(old, &support_view, pos);
            if geometry(Block::RedstoneWire { wire: new })
                != geometry(Block::RedstoneWire { wire: original })
                || geometry(Block::RedstoneWire { wire: intermediate })
                    != geometry(Block::RedstoneWire { wire: original })
            {
                if let Some(reason) = failure(&view, pos, true) {
                    return Ok(Err(reason));
                }
            }
            // A new conductor can expose an endpoint even when its dust shape
            // stays unchanged, including before the first new dust is placed.
            for p in recipients(pos) {
                let consumer = view.original(p);
                let original = receives(&before, consumer, p, pos, false);
                if original != receives(&view, consumer, p, pos, false)
                    || original != receives(&support_view, consumer, p, pos, false)
                {
                    return Ok(Err(format!(
                        "Connecting at {pos} would redirect an existing input at {p}."
                    )));
                }
            }
        }
    }
    let mut placements = supports;
    for pos in dust {
        budget.check()?;
        placements.push((
            pos,
            Block::RedstoneWire {
                wire: wire::get_state_for_placement(&view, pos),
            },
        ));
    }
    budget.check()?;
    Ok(Ok(Plan {
        path: path.to_vec(),
        placements,
        reads: snapshot.clone(),
    }))
}

struct Node {
    pos: BlockPos,
    parent: Option<usize>,
    steps: usize,
    bends: usize,
    direction: Option<BlockDirection>,
}

fn lower_bound(start: BlockPos, end: BlockPos) -> usize {
    let d = end - start;
    (d.x.abs() + d.z.abs()).max(d.y.abs()) as usize
}

pub(super) fn search(
    snapshot: Snapshot,
    start: BlockPos,
    end: BlockPos,
    prefer_x: bool,
    cancel: &AtomicBool,
) -> SearchResult {
    let budget = Budget {
        cancel,
        started: Instant::now(),
        visits: Cell::new(0),
        missing: Cell::new(false),
    };
    if start == end {
        return SearchResult::Invalid("Choose a different endpoint.".into());
    }
    // Existing endpoint dust consumes no placement budget.
    if lower_bound(start, end) > MAX_PLACEMENTS + 1 {
        return SearchResult::BudgetExceeded;
    }
    // Try the two common bend orders before allocating a frontier.
    let mut reason = None;
    if start.y == end.y {
        for x_first in [prefer_x, !prefer_x] {
            let mut path = vec![start];
            let mut p = start;
            for axis_x in [x_first, !x_first] {
                while if axis_x { p.x != end.x } else { p.z != end.z } {
                    if let Err(result) = budget.check() {
                        return result;
                    }
                    if axis_x {
                        p.x += (end.x - p.x).signum()
                    } else {
                        p.z += (end.z - p.z).signum()
                    }
                    path.push(p);
                }
            }
            match make_plan(&snapshot, &path, &budget) {
                Ok(Ok(plan)) => return SearchResult::Found(plan),
                Ok(Err(error)) => reason = Some(error),
                Err(result) => return result,
            }
        }
    }
    let mut nodes = vec![Node {
        pos: start,
        parent: None,
        steps: 0,
        bends: 0,
        direction: None,
    }];
    let mut frontier = BinaryHeap::from([Reverse((lower_bound(start, end), 0usize, 0usize))]);
    let directions = if prefer_x {
        [
            BlockDirection::East,
            BlockDirection::West,
            BlockDirection::South,
            BlockDirection::North,
        ]
    } else {
        [
            BlockDirection::South,
            BlockDirection::North,
            BlockDirection::East,
            BlockDirection::West,
        ]
    };
    let mut expanded = 0;
    while let Some(Reverse((_, _, id))) = frontier.pop() {
        if let Err(result) = budget.check() {
            return result;
        }
        expanded += 1;
        if expanded > MAX_STATES {
            return SearchResult::BudgetExceeded;
        }
        let mut path = Vec::new();
        let mut cursor = Some(id);
        while let Some(i) = cursor {
            path.push(nodes[i].pos);
            cursor = nodes[i].parent;
        }
        path.reverse();
        let node = &nodes[id];
        if node.pos == end {
            match make_plan(&snapshot, &path, &budget) {
                Ok(Ok(plan)) => return SearchResult::Found(plan),
                Ok(Err(error)) => reason = Some(error),
                Err(result) => return result,
            }
            continue;
        }
        let (last, steps, bends, previous) = (node.pos, node.steps, node.bends, node.direction);
        if steps + lower_bound(last, end) > MAX_PLACEMENTS + 1 {
            continue;
        }
        for direction in directions {
            for dy in [0, 1, -1] {
                let mut pos = last.offset(direction.block_face());
                pos.y += dy;
                if !contains(snapshot.data.route_bounds, pos) || path.contains(&pos) {
                    continue;
                }
                let old = snapshot
                    .data
                    .index(pos)
                    .map(|i| Block::from_id(snapshot.data.blocks[i]))
                    .unwrap();
                if !matches!(old, Block::Air {})
                    && !(pos == end && matches!(old, Block::RedstoneWire { .. }))
                {
                    continue;
                }
                // Dust/support conflicts and self contacts can never be repaired
                // by extending this prefix, so they are safe to prune early.
                if path
                    .iter()
                    .any(|p| p.x == pos.x && p.z == pos.z && p.y.abs_diff(pos.y) == 1)
                {
                    continue;
                }
                let mut prefix = path.clone();
                prefix.push(pos);
                let Proposed { view, .. } = match proposed(&snapshot, &prefix, &budget) {
                    Ok(proposed) => proposed,
                    Err(error) => {
                        reason = Some(error);
                        continue;
                    }
                };
                if !interaction::is_valid_position(plain_wire(), &view, pos)
                    || prefix.windows(2).any(|pair| {
                        !connects(&view, pair[0], pair[1]) || !connects(&view, pair[1], pair[0])
                    })
                {
                    continue;
                }
                if path[..path.len().saturating_sub(1)]
                    .iter()
                    .any(|&p| p.y == pos.y && (connects(&view, p, pos) || connects(&view, pos, p)))
                {
                    continue;
                }
                if matches!(old, Block::Air {}) {
                    if let Some(error) = failure(&view, pos, true) {
                        reason = Some(error);
                        continue;
                    }
                    if potential_input(&view, pos) {
                        reason = Some(format!(
                            "An existing power source can inject into the wire at {pos}."
                        ));
                        continue;
                    }
                }
                if let Err(result) = budget.check() {
                    return result;
                }
                if nodes.len() >= MAX_STATES {
                    return SearchResult::BudgetExceeded;
                }
                let bends = bends + usize::from(previous.is_some_and(|old| old != direction));
                let new_id = nodes.len();
                nodes.push(Node {
                    pos,
                    parent: Some(id),
                    steps: steps + 1,
                    bends,
                    direction: Some(direction),
                });
                frontier.push(Reverse((steps + 1 + lower_bound(pos, end), bends, new_id)));
            }
        }
    }
    reason.map_or(SearchResult::NoPath, SearchResult::Invalid)
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
