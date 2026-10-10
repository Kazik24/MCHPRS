//! Historical native-cycle certificates cache addresses, never electrical results.
use super::WireNeighbor;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use rustc_hash::FxHashMap;
use std::cell::Cell;

#[derive(Clone, Copy)]
pub(crate) struct PowerRoute {
    pub source: WireNeighbor,
    pub side: BlockFace,
    pub strong: [WireNeighbor; 6],
}

pub(crate) type PowerRoutes = [PowerRoute; 11];
const MAX_ENTRIES: usize = 65_536;

enum Stage {
    Retracting { ids: [u64; 2], finished: u8 },
    AwaitingReset,
    Resetting { ids: [u64; 2], finished: u8 },
    Proven,
}

struct Entry {
    face: BlockFace,
    stage: Stage,
    routes: Option<PowerRoutes>,
    proven: bool,
}

#[derive(Default)]
pub struct InstantPistonCache {
    entries: FxHashMap<BlockPos, Entry>,
    power_route_hits: Cell<u64>,
}

impl InstantPistonCache {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn stats(&self) -> (usize, usize, u64) {
        (
            self.entries.len(),
            self.entries.values().filter(|entry| entry.proven).count(),
            self.power_route_hits.get(),
        )
    }

    pub(crate) fn tracks(
        &self,
        base: BlockPos,
        face: BlockFace,
        slot: usize,
        id: u64,
        extending: bool,
    ) -> bool {
        let Some(entry) = self.entries.get(&base).filter(|entry| entry.face == face) else {
            return false;
        };
        if slot > 1 {
            return false;
        }
        match entry.stage {
            Stage::Retracting { ids, finished } if !extending => {
                ids[slot] == id && finished & (1 << slot) == 0
            }
            Stage::Resetting { ids, finished } if extending => {
                ids[slot] == id && finished & (1 << slot) == 0
            }
            _ => false,
        }
    }

    pub(crate) fn begin_retract(&mut self, base: BlockPos, face: BlockFace, ids: [u64; 2]) {
        if ids[0] == ids[1] {
            self.invalidate(base);
            return;
        }
        if let Some(entry) = self.entries.get_mut(&base) {
            if entry.face != face || !matches!(entry.stage, Stage::Proven) {
                self.invalidate(base);
                return;
            }
            entry.stage = Stage::Retracting { ids, finished: 0 };
            return;
        }
        if self.entries.len() < MAX_ENTRIES {
            self.entries.insert(
                base,
                Entry {
                    face,
                    stage: Stage::Retracting { ids, finished: 0 },
                    routes: None,
                    proven: false,
                },
            );
        }
    }

    pub(crate) fn awaiting_reset(&self, base: BlockPos, face: BlockFace) -> bool {
        self.entries
            .get(&base)
            .is_some_and(|entry| entry.face == face && matches!(entry.stage, Stage::AwaitingReset))
    }

    pub(crate) fn has_routes(&self, base: BlockPos, face: BlockFace) -> bool {
        self.entries
            .get(&base)
            .is_some_and(|entry| entry.face == face && entry.routes.is_some())
    }

    pub(crate) fn begin_reset(
        &mut self,
        base: BlockPos,
        face: BlockFace,
        ids: [u64; 2],
        routes: Option<PowerRoutes>,
    ) {
        let Some(entry) = self.entries.get_mut(&base) else {
            return;
        };
        if entry.face != face || !matches!(entry.stage, Stage::AwaitingReset) || ids[0] == ids[1] {
            self.invalidate(base);
            return;
        }
        if entry.routes.is_none() {
            entry.routes = routes;
        }
        if entry.routes.is_none() {
            self.invalidate(base);
            return;
        }
        entry.stage = Stage::Resetting { ids, finished: 0 };
    }

    pub(crate) fn completed(
        &mut self,
        base: BlockPos,
        face: BlockFace,
        slot: usize,
        id: u64,
        extending: bool,
        footprint_ok: bool,
    ) {
        let Some(entry) = self.entries.get_mut(&base) else {
            return;
        };
        if entry.face != face || slot > 1 || !footprint_ok {
            self.invalidate(base);
            return;
        }
        let (ids, finished) = match &mut entry.stage {
            Stage::Retracting { ids, finished } if !extending => (ids, finished),
            Stage::Resetting { ids, finished } if extending => (ids, finished),
            _ => {
                self.invalidate(base);
                return;
            }
        };
        if ids[slot] != id || *finished & (1 << slot) != 0 {
            self.invalidate(base);
            return;
        }
        *finished |= 1 << slot;
        if *finished == 3 {
            entry.stage = if extending {
                entry.proven = true;
                Stage::Proven
            } else {
                Stage::AwaitingReset
            };
        }
    }

    pub(crate) fn power_routes(&self, base: BlockPos, face: BlockFace) -> Option<&PowerRoutes> {
        let routes = self
            .entries
            .get(&base)
            .filter(|entry| entry.face == face && entry.proven)?
            .routes
            .as_ref()?;
        self.power_route_hits
            .set(self.power_route_hits.get().saturating_add(1));
        Some(routes)
    }

    pub(crate) fn invalidate(&mut self, base: BlockPos) {
        self.entries.remove(&base);
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.power_route_hits.set(0);
    }

    pub(crate) fn block_changed(&mut self, pos: BlockPos, old: Block, new: Block) {
        if old == new {
            return;
        }
        if let Some(entry) = self.entries.get(&pos) {
            let matches = match new {
                Block::Piston { piston } => {
                    piston.sticky && BlockFace::from(piston.facing) == entry.face
                }
                Block::MovingPiston { moving } => {
                    moving.sticky && BlockFace::from(moving.facing) == entry.face
                }
                _ => false,
            };
            if !matches {
                self.invalidate(pos);
            }
        }
        let observer_facing = |block| match block {
            Block::Observer { observer } => Some(observer.facing),
            _ => None,
        };
        if observer_facing(old) != observer_facing(new) {
            self.invalidate(pos.offset(BlockFace::Bottom));
        }
        let cap_base = pos.offset(BlockFace::Bottom).offset(BlockFace::Bottom);
        if self.entries.contains_key(&cap_base)
            && ((!new.is_solid() && old.is_solid()) || old.get_name() != new.get_name())
        {
            self.invalidate(cap_base);
        }
        let transport_block = |block| {
            matches!(
                block,
                Block::Air
                    | Block::RedstoneBlock {}
                    | Block::MovingPiston { .. }
                    | Block::PistonHead { .. }
            )
        };
        let restoring_motion = matches!(old, Block::MovingPiston { .. });
        if restoring_motion && new == Block::RedstoneBlock {
            return;
        }
        if !restoring_motion && (!matches!(old, Block::RedstoneBlock {}) || transport_block(new)) {
            return;
        }
        for face in BlockFace::values() {
            let head_base = pos.offset(face.opposite());
            let far_base = head_base.offset(face.opposite());
            for base in [head_base, far_base] {
                if self.entries.get(&base).is_some_and(|entry| {
                    entry.face == face
                        && (!restoring_motion
                            || match new {
                                Block::RedstoneBlock => false,
                                Block::PistonHead { head } => {
                                    !head.sticky
                                        || head.short
                                        || BlockFace::from(head.facing) != face
                                }
                                Block::MovingPiston { moving } => {
                                    !moving.sticky || BlockFace::from(moving.facing) != face
                                }
                                _ => true,
                            })
                }) {
                    self.invalidate(base);
                }
            }
        }
    }

    pub(crate) fn reject_motion(&mut self, pos: BlockPos, id: u64) {
        if self.is_empty() {
            return;
        }
        let candidates =
            std::iter::once(pos).chain(BlockFace::values().into_iter().flat_map(|face| {
                let head_base = pos.offset(face.opposite());
                [head_base, head_base.offset(face.opposite())]
            }));
        for base in candidates {
            let reject = self.entries.get(&base).is_some_and(|entry| {
                let head = base.offset(entry.face);
                match entry.stage {
                    Stage::Retracting { ids, .. } => {
                        (base == pos && ids[0] == id) || (head == pos && ids[1] == id)
                    }
                    Stage::Resetting { ids, .. } => {
                        (head == pos && ids[0] == id)
                            || (head.offset(entry.face) == pos && ids[1] == id)
                    }
                    _ => false,
                }
            });
            if reject {
                self.invalidate(base);
            }
        }
    }

    pub(crate) fn entity_changed(&mut self, pos: BlockPos) {
        if self.is_empty() {
            return;
        }
        self.invalidate(pos);
        let below = pos.offset(BlockFace::Bottom);
        self.invalidate(below);
        self.invalidate(below.offset(BlockFace::Bottom));
        for face in BlockFace::values() {
            let head_base = pos.offset(face.opposite());
            for base in [head_base, head_base.offset(face.opposite())] {
                if self
                    .entries
                    .get(&base)
                    .is_some_and(|entry| entry.face == face)
                {
                    self.invalidate(base);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_blocks::{
        BlockDirection, BlockFacing,
        blocks::{RedstoneObserver, RedstonePiston},
    };

    fn base() -> BlockPos {
        BlockPos::new(30, 30, 30)
    }
    fn routes(cell: u32) -> PowerRoutes {
        let source = WireNeighbor {
            pos: base(),
            cell: Some(cell),
        };
        [PowerRoute {
            source,
            side: BlockFace::East,
            strong: [source; 6],
        }; 11]
    }
    fn retract(cache: &mut InstantPistonCache) {
        cache.begin_retract(base(), BlockFace::East, [1, 2]);
        cache.completed(base(), BlockFace::East, 1, 2, false, true);
        cache.completed(base(), BlockFace::East, 0, 1, false, true);
    }
    fn proven() -> InstantPistonCache {
        let mut cache = InstantPistonCache::default();
        retract(&mut cache);
        cache.begin_reset(base(), BlockFace::East, [3, 4], Some(routes(5)));
        cache.completed(base(), BlockFace::East, 0, 3, true, true);
        cache.completed(base(), BlockFace::East, 1, 4, true, true);
        cache
    }

    #[test]
    fn needs_both_exact_motion_stages_and_keeps_proven_routes_during_reuse() {
        let mut cache = InstantPistonCache::default();
        assert!(cache.is_empty());
        assert_eq!(cache.stats(), (0, 0, 0));
        cache.begin_retract(base(), BlockFace::East, [1, 2]);
        assert!(cache.tracks(base(), BlockFace::East, 0, 1, false));
        assert!(!cache.tracks(base(), BlockFace::East, 0, 2, false));
        assert!(!cache.tracks(base(), BlockFace::East, 0, 1, true));
        assert!(!cache.tracks(base(), BlockFace::West, 0, 1, false));
        assert!(!cache.tracks(base(), BlockFace::East, 2, 1, false));
        cache.completed(base(), BlockFace::East, 0, 1, false, true);
        assert!(!cache.tracks(base(), BlockFace::East, 0, 1, false));
        assert!(!cache.awaiting_reset(base(), BlockFace::East));
        assert!(cache.power_routes(base(), BlockFace::East).is_none());
        cache.completed(base(), BlockFace::East, 1, 2, false, true);
        assert!(cache.awaiting_reset(base(), BlockFace::East));
        assert!(!cache.has_routes(base(), BlockFace::East));
        cache.begin_reset(base(), BlockFace::East, [3, 4], Some(routes(5)));
        assert!(cache.has_routes(base(), BlockFace::East));
        assert!(!cache.has_routes(base(), BlockFace::West));
        assert_eq!(cache.stats(), (1, 0, 0));
        cache.completed(base(), BlockFace::East, 1, 4, true, true);
        assert!(cache.power_routes(base(), BlockFace::East).is_none());
        cache.completed(base(), BlockFace::East, 0, 3, true, true);
        assert_eq!(cache.stats(), (1, 1, 0));
        assert_eq!(
            cache.power_routes(base(), BlockFace::East).unwrap()[0]
                .source
                .cell,
            Some(5)
        );
        cache.begin_retract(base(), BlockFace::East, [5, 6]);
        assert_eq!(cache.stats(), (1, 1, 1));
        cache.completed(base(), BlockFace::East, 0, 5, false, true);
        cache.completed(base(), BlockFace::East, 1, 6, false, true);
        cache.begin_reset(base(), BlockFace::East, [7, 8], None);
        assert_eq!(
            cache.power_routes(base(), BlockFace::East).unwrap()[0]
                .source
                .cell,
            Some(5)
        );
        cache.completed(base(), BlockFace::East, 0, 7, true, true);
        cache.completed(base(), BlockFace::East, 1, 8, true, true);
        assert_eq!(
            cache.power_routes(base(), BlockFace::East).unwrap()[0]
                .source
                .cell,
            Some(5)
        );
    }

    #[test]
    fn invalid_identity_facing_direction_footprint_or_reset_order_revokes() {
        for (face, slot, id, extending, footprint) in [
            (BlockFace::West, 0, 5, false, true),
            (BlockFace::East, 2, 5, false, true),
            (BlockFace::East, 0, 99, false, true),
            (BlockFace::East, 0, 5, true, true),
            (BlockFace::East, 0, 5, false, false),
        ] {
            let mut cache = proven();
            cache.begin_retract(base(), BlockFace::East, [5, 6]);
            cache.completed(base(), face, slot, id, extending, footprint);
            assert!(cache.entries.is_empty());
        }
        let mut cache = proven();
        cache.begin_reset(base(), BlockFace::East, [5, 6], Some(routes(9)));
        assert!(cache.entries.is_empty());
        cache.begin_retract(base(), BlockFace::East, [1, 1]);
        assert!(cache.entries.is_empty());
        retract(&mut cache);
        cache.begin_reset(base(), BlockFace::East, [3, 4], None);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn ordinary_pose_and_observer_pulses_survive_but_foreign_edits_revoke() {
        let head = base().offset(BlockFace::East);
        let far = head.offset(BlockFace::East);
        let observer = Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        };
        let mut cache = proven();
        cache.block_changed(
            base().offset(BlockFace::Top),
            observer,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: true,
                },
            },
        );
        cache.block_changed(head, Block::Air, Block::RedstoneBlock {});
        cache.block_changed(far, Block::RedstoneBlock {}, Block::Air);
        cache.block_changed(
            base(),
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: true,
                },
            },
            Block::MovingPiston {
                moving: mchprs_blocks::blocks::RedstoneMovingPiston {
                    facing: BlockFacing::East,
                    sticky: true,
                },
            },
        );
        assert!(cache.power_routes(base(), BlockFace::East).is_some());
        for (pos, old, new) in [
            (head, Block::RedstoneBlock {}, Block::Stone {}),
            (far, Block::RedstoneBlock {}, Block::Stone {}),
            (
                base(),
                Block::MovingPiston {
                    moving: mchprs_blocks::blocks::RedstoneMovingPiston {
                        facing: BlockFacing::East,
                        sticky: true,
                    },
                },
                Block::Air,
            ),
            (base().offset(BlockFace::Top), observer, Block::Air),
            (
                base().offset(BlockFace::Top).offset(BlockFace::Top),
                Block::Stone {},
                Block::Air,
            ),
            (
                base().offset(BlockFace::Top).offset(BlockFace::Top),
                Block::Stone {},
                Block::Furnace {
                    facing: BlockDirection::North,
                    lit: false,
                },
            ),
        ] {
            let mut cache = proven();
            cache.block_changed(pos, old, new);
            assert!(cache.entries.is_empty());
        }
    }

    #[test]
    fn interrupted_or_malformed_motion_restoration_revokes_its_owner() {
        let moving = Block::MovingPiston {
            moving: mchprs_blocks::blocks::RedstoneMovingPiston {
                facing: BlockFacing::East,
                sticky: true,
            },
        };
        let piston = RedstonePiston {
            facing: BlockFacing::East,
            sticky: true,
            extended: true,
        };
        let head = base().offset(BlockFace::East);
        let far = head.offset(BlockFace::East);
        for pos in [head, far] {
            for restored in [
                Block::Air,
                Block::Stone {},
                Block::PistonHead {
                    head: RedstonePiston {
                        facing: BlockFacing::West,
                        ..piston
                    }
                    .into(),
                },
                Block::PistonHead {
                    head: RedstonePiston {
                        sticky: false,
                        ..piston
                    }
                    .into(),
                },
                Block::PistonHead {
                    head: mchprs_blocks::blocks::RedstonePistonHead {
                        short: true,
                        ..piston.into()
                    },
                },
            ] {
                let mut cache = proven();
                cache.block_changed(pos, moving, restored);
                assert!(
                    cache.is_empty(),
                    "malformed restoration at {pos:?}: {restored:?}"
                );
            }
        }
        let mut cache = proven();
        cache.block_changed(base(), moving, Block::Piston { piston });
        cache.block_changed(
            head,
            moving,
            Block::PistonHead {
                head: piston.into(),
            },
        );
        cache.block_changed(far, moving, Block::RedstoneBlock);
        assert_eq!(cache.stats().1, 1);
    }

    #[test]
    fn entity_edits_revoke_only_actors_whose_footprint_contains_the_cell() {
        for pos in [
            base(),
            base().offset(BlockFace::East),
            base().offset(BlockFace::East).offset(BlockFace::East),
            base().offset(BlockFace::Top),
            base().offset(BlockFace::Top).offset(BlockFace::Top),
        ] {
            let mut cache = proven();
            cache.entity_changed(pos);
            assert!(cache.is_empty(), "entity edit at {pos:?}");
        }
        let mut cache = proven();
        for pos in [
            base().offset(BlockFace::North),
            base().offset(BlockFace::West).offset(BlockFace::West),
            base()
                .offset(BlockFace::East)
                .offset(BlockFace::East)
                .offset(BlockFace::East),
        ] {
            cache.entity_changed(pos);
            assert_eq!(cache.stats().1, 1, "unrelated entity edit at {pos:?}");
        }
    }

    #[test]
    fn motion_rejection_matches_stage_positions_and_ids_in_every_direction() {
        for face in BlockFace::values() {
            for extending in [false, true] {
                for slot in 0..2 {
                    let mut cache = InstantPistonCache::default();
                    cache.begin_retract(base(), face, [1, 2]);
                    if extending {
                        cache.completed(base(), face, 0, 1, false, true);
                        cache.completed(base(), face, 1, 2, false, true);
                        cache.begin_reset(base(), face, [3, 4], Some(routes(5)));
                    }
                    let head = base().offset(face);
                    let positions = if extending {
                        [head, head.offset(face)]
                    } else {
                        [base(), head]
                    };
                    let ids = if extending { [3, 4] } else { [1, 2] };
                    cache.reject_motion(positions[slot], 99);
                    cache.reject_motion(base().offset(face.opposite()), ids[slot]);
                    assert!(cache.tracks(base(), face, slot, ids[slot], extending));
                    cache.reject_motion(positions[slot], ids[slot]);
                    assert!(
                        cache.is_empty(),
                        "face={face:?} extending={extending} slot={slot}"
                    );
                }
            }
        }
    }

    #[test]
    fn interruption_clear_and_admission_limit_do_not_leave_stale_routes() {
        let mut cache = proven();
        cache.begin_retract(base(), BlockFace::East, [5, 6]);
        cache.reject_motion(base().offset(BlockFace::East), 6);
        assert!(cache.entries.is_empty());
        for x in 0..MAX_ENTRIES + 1 {
            cache.begin_retract(BlockPos::new(x as i32, 0, 0), BlockFace::East, [1, 2]);
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        cache.clear();
        assert!(cache.entries.is_empty());
        assert_eq!(cache.stats(), (0, 0, 0));
    }
}
