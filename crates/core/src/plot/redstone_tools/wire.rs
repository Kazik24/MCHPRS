//! Live wire placement. Only the plot thread reads or changes the live world.
mod routing;
#[cfg(test)]
mod session_tests;

use super::{Plot, PlotWorld, ToolCommand};
use crate::messages;
use crate::player::{Gamemode, PacketSender, Player, PlayerPos};
use crate::plot::{preview, worldedit};
use crate::world::World;
use anyhow::{bail, Result};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::packets::clientbound::{CChatMessage, CHeldItemChange, ClientBoundPacket};
use mchprs_network::packets::components;
use mchprs_network::packets::serverbound::{SHeldItemChange, ServerBoundPacketHandler};
use once_cell::sync::Lazy;
use routing::{Capture, GeometryCheck, Plan, SearchResult, Snapshot};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const UPDATE_INTERVAL: Duration = Duration::from_millis(50);
const UI_WORK: Duration = Duration::from_millis(1);
const CLICK_INTERVAL: Duration = Duration::from_millis(200);
const NOTICE_INTERVAL: Duration = Duration::from_secs(1);
const AIM_DISTANCE: f64 = 64.0;

struct Job {
    snapshot: Snapshot,
    start: BlockPos,
    end: BlockPos,
    prefer_x: bool,
    generation: u64,
    cancel: Arc<AtomicBool>,
    reply: mpsc::SyncSender<(u64, SearchResult)>,
}

static WORKERS: Lazy<mpsc::SyncSender<Job>> = Lazy::new(|| {
    let (sender, receiver) = mpsc::sync_channel::<Job>(2);
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..2 {
        let receiver = receiver.clone();
        std::thread::Builder::new()
            .name(format!("wire-router-{index}"))
            .spawn(move || loop {
                let Ok(job) = receiver.lock().unwrap().recv() else {
                    break;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    routing::search(job.snapshot, job.start, job.end, job.prefer_x, &job.cancel)
                }))
                .unwrap_or_else(|_| SearchResult::Invalid(messages::WIRE_UNSUPPORTED.into()));
                let _ = job.reply.try_send((job.generation, result));
            })
            .expect("Cannot start wire routing worker");
    }
    sender
});

struct Pending {
    cancel: Arc<AtomicBool>,
    reply: mpsc::Receiver<(u64, SearchResult)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plane {
    Horizontal,
    VerticalX,
    VerticalZ,
    Free,
}

impl Plane {
    fn next(self) -> Self {
        match self {
            Self::Horizontal => Self::VerticalX,
            Self::VerticalX => Self::VerticalZ,
            Self::VerticalZ => Self::Free,
            Self::Free => Self::Horizontal,
        }
    }

    fn axis(self) -> Option<usize> {
        match self {
            Self::Horizontal => Some(1),
            Self::VerticalX => Some(2),
            Self::VerticalZ => Some(0),
            Self::Free => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Horizontal => messages::WIRE_PLANE_HORIZONTAL,
            Self::VerticalX => messages::WIRE_PLANE_VERTICAL_X,
            Self::VerticalZ => messages::WIRE_PLANE_VERTICAL_Z,
            Self::Free => messages::WIRE_PLANE_FREE,
        }
    }
}

pub(crate) struct Session {
    slot: u32,
    plane: Plane,
    start: Option<BlockPos>,
    target: Option<BlockPos>,
    prefer_x: bool,
    generation: u64,
    pending: Option<Pending>,
    capture: Option<Capture>,
    snapshot: Option<Snapshot>,
    check: Option<GeometryCheck>,
    received: Option<SearchResult>,
    plan: Option<Plan>,
    wanted: HashMap<BlockPos, u8>,
    markers: HashMap<BlockPos, (i32, u8)>,
    displayed: bool,
    next_update: Instant,
    next_capture: Instant,
    needs_search: bool,
    last_click: Option<Instant>,
    notice: Option<String>,
    shown_notice: Option<String>,
    next_notice: Instant,
}

impl Session {
    fn new(slot: u32) -> Self {
        Self {
            slot,
            plane: Plane::Horizontal,
            start: None,
            target: None,
            prefer_x: true,
            generation: 0,
            pending: None,
            capture: None,
            snapshot: None,
            check: None,
            received: None,
            plan: None,
            wanted: HashMap::new(),
            markers: HashMap::new(),
            displayed: false,
            next_update: Instant::now(),
            next_capture: Instant::now(),
            needs_search: false,
            last_click: None,
            notice: None,
            shown_notice: None,
            next_notice: Instant::now(),
        }
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
        self.capture = None;
        self.snapshot = None;
        self.check = None;
        self.received = None;
        self.plan = None;
        self.needs_search = true;
        self.displayed = false;
        for kind in self.wanted.values_mut() {
            *kind = 2;
        }
        if let Some(start) = self.start {
            self.wanted.insert(start, 4);
        }
        if let Some(target) = self.target {
            self.wanted.insert(target, 4);
        }
        self.next_capture = Instant::now();
    }

    fn retarget(&mut self, target: Option<BlockPos>) {
        if let Some(previous) = self.target {
            if self
                .wanted
                .get(&previous)
                .is_some_and(|kind| matches!(kind, 1 | 4))
            {
                self.wanted.remove(&previous);
            }
        }
        let snapshot = self.snapshot.clone().filter(|snapshot| {
            self.start
                .zip(target)
                .is_some_and(|(start, end)| snapshot.contains_target(start, end))
        });
        self.target = target;
        self.invalidate();
        self.snapshot = snapshot;
    }

    fn set_status(&mut self, message: &str) {
        self.notice = Some(format!("{} | {message}", self.plane.label()));
    }

    fn send_status(&mut self, viewer: &impl PacketSender, now: Instant) {
        if now < self.next_notice || self.notice == self.shown_notice {
            return;
        }
        if let Some(message) = &self.notice {
            action_bar(viewer, message);
            self.shown_notice = self.notice.clone();
            self.next_notice = now + NOTICE_INTERVAL;
        }
    }

    fn submit(&mut self, sender: &mpsc::SyncSender<Job>, start: BlockPos, end: BlockPos) -> bool {
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (reply, receiver) = mpsc::sync_channel(1);
        let job = Job {
            snapshot: snapshot.clone(),
            start,
            end,
            prefer_x: self.prefer_x,
            generation: self.generation,
            cancel: cancel.clone(),
            reply,
        };
        if sender.try_send(job).is_err() {
            return false;
        }
        self.needs_search = false;
        self.pending = Some(Pending {
            cancel,
            reply: receiver,
        });
        true
    }

    fn publish(&mut self, result: SearchResult) {
        match result {
            SearchResult::Found(plan) => {
                self.wanted = plan.path.iter().map(|&pos| (pos, 0)).collect();
                for &(pos, block) in &plan.placements {
                    self.wanted.insert(
                        pos,
                        if matches!(block, Block::RedstoneWire { .. }) {
                            0
                        } else {
                            2
                        },
                    );
                }
                self.set_status(&messages::wire_status(plan.placements.len()));
                self.plan = Some(plan);
            }
            SearchResult::NoPath => self.set_status(messages::WIRE_NO_PATH),
            SearchResult::BudgetExceeded => self.set_status(messages::WIRE_BUDGET),
            SearchResult::Invalid(reason) => self.set_status(&reason),
            SearchResult::Cancelled => {}
        }
        if let Some(start) = self.start {
            self.wanted.insert(start, 4);
        }
        if let Some(target) = self.target {
            self.wanted
                .insert(target, if self.plan.is_some() { 4 } else { 1 });
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn tool_item() -> Item {
    Item::from_name("carrot_on_a_stick").expect("Minecraft tool item")
}

fn holds_pen(player: &Player) -> bool {
    player.inventory[36 + player.selected_slot as usize]
        .as_ref()
        .is_some_and(|item| item.item_type == tool_item())
}

impl Plot {
    pub(super) fn wire_tool(&mut self, player: usize, args: &[&str]) -> Result<()> {
        let plane = match args {
            [] => None,
            ["free"] => Some(Plane::Free),
            ["plane"] => Some(Plane::Horizontal),
            _ => bail!(messages::WIRE_USAGE),
        };
        if !matches!(self.players[player].gamemode, Gamemode::Creative) {
            bail!(messages::SWITCH_CREATIVE_MODE_FIRST);
        }
        if !self.players[player].can_build_action("place", self.owner, (self.world.x, self.world.z))
        {
            bail!(messages::TOOL_PERMISSION_DENIED);
        }
        if let Some(plane) = plane.filter(|_| holds_pen(&self.players[player])) {
            let data = &mut self.players[player];
            let mut session = data
                .redstone_tools
                .wire
                .take()
                .unwrap_or_else(|| Session::new(data.selected_slot));
            session.plane = plane;
            let target = session
                .start
                .and_then(|start| aim(&self.world, data, data.yaw, data.pitch, Some(start), plane));
            session.retarget(target);
            session.set_status(if session.start.is_none() {
                messages::WIRE_ENABLED
            } else if target.is_none() {
                messages::WIRE_NO_TARGET
            } else {
                messages::WIRE_PENDING
            });
            data.redstone_tools.wire_disabled = false;
            data.redstone_tools.wire = Some(session);
            return Ok(());
        }
        let data = &self.players[player];
        let selected = data.selected_slot;
        let slot = if holds_pen(data) {
            selected
        } else {
            (0..9)
                .find(|&slot| data.inventory[36 + slot as usize].is_none())
                .ok_or_else(|| anyhow::anyhow!(messages::WIRE_FULL_HOTBAR))?
        };
        self.clear_wire_tool(player);
        if self.players[player].inventory[36 + slot as usize].is_none() {
            let mut blob = nbt::Blob::new();
            components::set_tool_display(
                tool_item().get_id() as i32,
                &mut blob,
                "Wire Pen",
                "L start | R commit | F mode | Q bend",
            )
            .map_err(|error| anyhow::anyhow!(messages::container_components_failed(error)))?;
            self.players[player].set_inventory_slot(
                36 + slot,
                Some(ItemStack {
                    item_type: tool_item(),
                    count: 1,
                    nbt: Some(blob),
                }),
            );
        }
        ServerBoundPacketHandler::handle_held_item_change(
            self,
            SHeldItemChange { slot: slot as i16 },
            player,
        );
        self.players[player].send_packet(&CHeldItemChange { slot: slot as i8 }.encode());
        let mut session = Session::new(slot);
        if let Some(plane) = plane {
            session.plane = plane;
            session.set_status(messages::WIRE_ENABLED);
        }
        self.players[player].redstone_tools.wire_disabled = false;
        self.players[player].redstone_tools.wire = Some(session);
        self.players[player].send_system_message(messages::WIRE_ENABLED);
        Ok(())
    }

    pub(in crate::plot) fn clear_wire_tool(&mut self, player: usize) {
        if let Some(mut session) = self.players[player].redstone_tools.wire.take() {
            preview::clear_markers(&self.players[player], &mut session.markers);
            if session.shown_notice.is_some() {
                action_bar(&self.players[player], "");
            }
        }
    }

    pub(in crate::plot) fn wire_tools_active(&self) -> bool {
        self.players.iter().any(|player| {
            !player.redstone_tools.wire_disabled
                && (player.redstone_tools.wire.is_some()
                    || (matches!(player.gamemode, Gamemode::Creative) && holds_pen(player)))
        })
    }

    pub(in crate::plot) fn wire_held(&self, player: usize) -> bool {
        let data = &self.players[player];
        !data.redstone_tools.wire_disabled && holds_pen(data)
    }

    pub(in crate::plot) fn unload_wire_chunk(&mut self, player: usize, x: i32, z: i32) {
        let data = &mut self.players[player];
        if let Some(mut session) = data.redstone_tools.wire.take() {
            if preview::unload_markers(data, &mut session.markers, x, z) {
                session.displayed = false;
            }
            data.redstone_tools.wire = Some(session);
        }
    }

    pub(in crate::plot) fn flip_wire_route(&mut self, player: usize, bend: bool) -> bool {
        if !self.wire_held(player) {
            return false;
        }
        if !matches!(self.players[player].gamemode, Gamemode::Creative)
            || self.check_tool_access(player, ToolCommand::Wire).is_err()
            || !self.players[player].can_build_action(
                "place",
                self.owner,
                (self.world.x, self.world.z),
            )
        {
            return true;
        }
        let slot = self.players[player].selected_slot;
        let session = self.players[player]
            .redstone_tools
            .wire
            .get_or_insert_with(|| Session::new(slot));
        if bend {
            session.prefer_x = !session.prefer_x;
        } else {
            session.plane = session.plane.next();
        }
        session.retarget(session.target);
        session.set_status(if session.start.is_some() {
            messages::WIRE_PENDING
        } else {
            messages::WIRE_ENABLED
        });
        true
    }

    pub(in crate::plot) fn use_wire_tool(
        &mut self,
        player: usize,
        hand: i32,
        yaw: f32,
        pitch: f32,
    ) -> bool {
        if !self.wire_held(player) {
            return false;
        }
        if hand != 0
            || !yaw.is_finite()
            || !pitch.is_finite()
            || self.players[player].awaiting_teleport()
            || !matches!(self.players[player].gamemode, Gamemode::Creative)
        {
            return true;
        }
        if self.check_tool_access(player, ToolCommand::Wire).is_err()
            || !self.players[player].can_build_action(
                "place",
                self.owner,
                (self.world.x, self.world.z),
            )
        {
            self.clear_wire_tool(player);
            self.players[player].send_no_permission_message();
            return true;
        }
        let slot = self.players[player].selected_slot;
        let mut session = self.players[player]
            .redstone_tools
            .wire
            .take()
            .unwrap_or_else(|| Session::new(slot));
        if session
            .last_click
            .is_some_and(|last| last.elapsed() < CLICK_INTERVAL)
        {
            self.players[player].redstone_tools.wire = Some(session);
            return true;
        }
        session.last_click = Some(Instant::now());
        if self.players[player].crouching {
            session.start = None;
            session.target = None;
            session.invalidate();
            session.wanted.clear();
            session.set_status(messages::WIRE_ENABLED);
        } else if session.start.is_none() {
            session.set_status(messages::WIRE_SELECT_START_FIRST);
        } else if let Some(target) = aim(
            &self.world,
            &self.players[player],
            yaw,
            pitch,
            session.start,
            session.plane,
        ) {
            if session.target != Some(target) {
                session.retarget(Some(target));
                session.set_status(messages::WIRE_PENDING);
            } else if !session.displayed {
                session.set_status(messages::WIRE_ROUTE_NOT_DISPLAYED);
            } else if let Some(plan) = session.plan.take() {
                if self.git_checkout_locked()
                    || self
                        .redpiler
                        .current_flags()
                        .is_some_and(|flags| flags.io_only)
                {
                    session.plan = Some(plan);
                    session.set_status(messages::WIRE_UNSUPPORTED);
                } else if !plan.reads.is_current(&self.world) {
                    session.invalidate();
                    session.set_status(messages::WIRE_STALE);
                } else {
                    self.reset_redpiler();
                    if !plan.reads.is_current(&self.world) {
                        session.invalidate();
                        session.set_status(messages::WIRE_STALE);
                    } else {
                        let positions = undo_positions(&self.world, &plan);
                        let undo = worldedit::capture_positions(&mut self.world, positions);
                        let previous = self.world.set_authoritative_updates(true);
                        for &(pos, block) in &plan.placements {
                            let block = if matches!(block, Block::RedstoneWire { .. }) {
                                Block::RedstoneWire {
                                    wire: crate::redstone::wire::get_state_for_placement(
                                        &self.world,
                                        pos,
                                    ),
                                }
                            } else {
                                block
                            };
                            crate::interaction::place_in_world(block, &mut self.world, pos, &None);
                        }
                        self.world.flush_block_changes();
                        self.world.set_authoritative_updates(previous);
                        let data = &mut self.players[player];
                        data.worldedit_undo.push(undo);
                        data.worldedit_redo.clear();
                        worldedit::trim_history(data);
                        session.set_status(&messages::wire_built(plan.placements.len()));
                        session.start = Some(target);
                        session.target = Some(target);
                        session.invalidate();
                        session.wanted.clear();
                        session.wanted.insert(target, 4);
                    }
                }
            } else if session.start == Some(target) {
                session.set_status(messages::WIRE_START_SELECTED);
            } else if session.pending.is_some() || session.needs_search {
                session.set_status(messages::WIRE_PENDING);
            }
        } else {
            session.set_status(messages::WIRE_NO_TARGET);
        }
        self.players[player].redstone_tools.wire = Some(session);
        true
    }

    pub(in crate::plot) fn start_wire_route(&mut self, player: usize, clicked: BlockPos) -> bool {
        if !self.wire_held(player) {
            return false;
        }
        if self.players[player].awaiting_teleport()
            || !matches!(self.players[player].gamemode, Gamemode::Creative)
        {
            return true;
        }
        if self.check_tool_access(player, ToolCommand::Wire).is_err()
            || !self.players[player].can_build_action(
                "place",
                self.owner,
                (self.world.x, self.world.z),
            )
        {
            self.clear_wire_tool(player);
            self.players[player].send_no_permission_message();
            return true;
        }
        if !Plot::in_plot_bounds(self.world.x, self.world.z, clicked.x, clicked.z)
            || !self.container_in_reach(player, clicked)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&clicked.y)
        {
            return true;
        }
        let clicked_block = self.world.get_block(clicked);
        let start = match clicked_block {
            Block::RedstoneWire { .. } => clicked,
            Block::Air {} => return true,
            _ => clicked.offset(BlockFace::Top),
        };
        if !self.world.contains_position(start) {
            return true;
        }

        let slot = self.players[player].selected_slot;
        let session = self.players[player]
            .redstone_tools
            .wire
            .get_or_insert_with(|| Session::new(slot));
        session.start = Some(start);
        session.target = Some(start);
        session.invalidate();
        session.wanted.clear();
        session.wanted.insert(start, 4);
        session.set_status(messages::WIRE_START_SELECTED);
        true
    }

    pub(in crate::plot) fn update_wire_tools(&mut self) {
        let started = Instant::now();
        let count = self.players.len();
        if count == 0 {
            return;
        }
        let first = self.wire_cursor % count;
        for offset in 0..count {
            let player = (first + offset) % count;
            if self.players[player].redstone_tools.wire.is_none() {
                continue;
            }
            self.wire_cursor = (player + 1) % count;
            if !self.wire_held(player)
                || self.players[player]
                    .redstone_tools
                    .wire
                    .as_ref()
                    .unwrap()
                    .slot
                    != self.players[player].selected_slot
                || !matches!(self.players[player].gamemode, Gamemode::Creative)
                || self.players[player].awaiting_teleport()
                || self.check_tool_access(player, ToolCommand::Wire).is_err()
                || !self.players[player].can_build_action(
                    "place",
                    self.owner,
                    (self.world.x, self.world.z),
                )
            {
                self.clear_wire_tool(player);
                continue;
            }
            let mut session = self.players[player].redstone_tools.wire.take().unwrap();
            let now = Instant::now();
            if now >= session.next_update {
                session.next_update = now + UPDATE_INTERVAL;
                if session.start.is_some() {
                    let target = aim(
                        &self.world,
                        &self.players[player],
                        self.players[player].yaw,
                        self.players[player].pitch,
                        session.start,
                        session.plane,
                    );
                    if session.target != target {
                        session.retarget(target);
                    }
                    if session.check.is_none() {
                        session.check = session.snapshot.clone().map(GeometryCheck::new);
                    }
                }
            }
            if let Some(pending) = &session.pending {
                match pending.reply.try_recv() {
                    Ok((generation, result)) => {
                        session.pending = None;
                        if generation == session.generation {
                            session.received = Some(result);
                            session.check = session.snapshot.clone().map(GeometryCheck::new);
                        }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        session.pending = None;
                        session.set_status(messages::WIRE_UNSUPPORTED);
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
            if let Some(check) = &mut session.check {
                if let Some(unchanged) = check.step(&self.world) {
                    let checked = unchanged.then(|| check.checked_snapshot());
                    session.check = None;
                    if !unchanged {
                        session.invalidate();
                        session.set_status(messages::WIRE_STALE);
                    } else {
                        let checked = checked.unwrap();
                        session.snapshot = Some(checked.clone());
                        if let Some(plan) = &mut session.plan {
                            plan.reads = checked.clone();
                        }
                        if let Some(mut result) = session.received.take() {
                            if let SearchResult::Found(plan) = &mut result {
                                plan.reads = checked;
                            }
                            session.publish(result);
                        }
                    }
                }
            }
            if session.pending.is_none() && session.needs_search {
                if let (Some(start), Some(end)) = (session.start, session.target) {
                    if start != end {
                        if session.capture.is_none()
                            && session.snapshot.is_none()
                            && now >= session.next_capture
                        {
                            match Capture::new(&self.world, start, end) {
                                Ok(capture) => session.capture = Some(capture),
                                Err(reason) => {
                                    session.set_status(&reason);
                                    session.next_capture = now + UPDATE_INTERVAL;
                                }
                            }
                        }
                        while session.capture.is_some() && started.elapsed() < UI_WORK {
                            match session.capture.as_mut().unwrap().step(&self.world) {
                                Ok(Some(snapshot)) => {
                                    session.snapshot = Some(snapshot);
                                    session.capture = None;
                                    session.check =
                                        session.snapshot.clone().map(GeometryCheck::new);
                                }
                                Ok(None) => {}
                                Err(reason) => {
                                    session.capture = None;
                                    session.next_capture = now + UPDATE_INTERVAL;
                                    session.set_status(&reason);
                                }
                            }
                        }
                        if session.submit(&WORKERS, start, end) {
                            session.set_status(messages::WIRE_PENDING);
                        }
                    }
                }
            }
            let wanted: HashMap<_, _> = session
                .wanted
                .iter()
                .filter(|(pos, _)| {
                    Self::get_chunk_distance(
                        pos.x >> 4,
                        pos.z >> 4,
                        self.players[player].last_chunk_x,
                        self.players[player].last_chunk_z,
                    ) <= crate::config::CONFIG.view_distance.max(0) as u32
                })
                .map(|(&pos, &kind)| (pos, kind))
                .collect();
            let reconciled = preview::reconcile_markers(
                &self.players[player],
                &mut session.markers,
                &wanted,
                128,
            );
            session.displayed = wanted.len() == session.wanted.len() && reconciled;
            session.send_status(&self.players[player], now);
            self.players[player].redstone_tools.wire = Some(session);
            if started.elapsed() >= UI_WORK {
                break;
            }
        }
    }
}

fn action_bar(viewer: &impl PacketSender, message: &str) {
    viewer.send_packet(
        &CChatMessage {
            message: serde_json::json!({"text": message, "color": "gray"}).to_string(),
            position: 2,
            sender: 0,
        }
        .encode(),
    );
}

fn aim(
    world: &PlotWorld,
    player: &Player,
    yaw: f32,
    pitch: f32,
    start: Option<BlockPos>,
    plane: Plane,
) -> Option<BlockPos> {
    if !player.pos.is_valid() || !yaw.is_finite() || !pitch.is_finite() {
        return None;
    }
    let eye_y = player.pos.y + if player.crouching { 1.27 } else { 1.62 };
    let radians_pitch = f64::from(pitch).to_radians();
    let radians_yaw = f64::from(yaw).to_radians();
    let direction = [
        -radians_yaw.sin() * radians_pitch.cos(),
        -radians_pitch.sin(),
        radians_yaw.cos() * radians_pitch.cos(),
    ];
    let origin = [player.pos.x, eye_y, player.pos.z];
    let axis = plane.axis();
    let read = |pos: BlockPos| {
        Plot::in_plot_bounds(world.x, world.z, pos.x, pos.z).then(|| world.get_block(pos))
    };
    let bounds = |block| match block {
        Block::Air {} => None,
        Block::RedstoneWire { .. } => Some(crate::plot::compass::Bounds {
            min: [0.0; 3],
            max: [1.0, 1.0 / 16.0, 1.0],
        }),
        _ => Some(crate::plot::compass::Bounds {
            min: [0.0; 3],
            max: [1.0; 3],
        }),
    };
    if let Some(hit) = crate::plot::compass::ray_trace(
        PlayerPos::new(origin[0], origin[1], origin[2]),
        yaw,
        pitch,
        AIM_DISTANCE,
        &read,
        &bounds,
    ) {
        let hit = hit.block;
        let target = if matches!(world.get_block(hit), Block::RedstoneWire { .. }) {
            hit
        } else {
            hit.offset(BlockFace::Top)
        };
        let coordinates = [target.x, target.y, target.z];
        if world.contains_position(target)
            && start
                .zip(axis)
                .is_none_or(|(start, axis)| coordinates[axis] == [start.x, start.y, start.z][axis])
        {
            return Some(target);
        }
    }
    let axis = axis?;
    let start = start?;
    let support = [
        f64::from(start.x) + 0.5,
        f64::from(start.y) - 0.5,
        f64::from(start.z) + 0.5,
    ];
    let reach = origin
        .iter()
        .zip(support)
        .map(|(eye, support)| (eye - support).powi(2))
        .sum::<f64>()
        .sqrt()
        .clamp(2.0, AIM_DISTANCE);
    let intersection = (support[axis] - origin[axis]) / direction[axis];
    // At an edge-on view, use the start's reach instead of an unbounded cursor.
    let distance = if intersection.is_finite() && (0.0..=AIM_DISTANCE).contains(&intersection) {
        intersection
    } else {
        reach
    };
    let mut point =
        std::array::from_fn::<_, 3, _>(|axis| origin[axis] + direction[axis] * distance);
    point[axis] = support[axis];
    let target = BlockPos::new(
        point[0].floor() as i32,
        point[1].floor() as i32 + 1,
        point[2].floor() as i32,
    );
    world.contains_position(target).then_some(target)
}

fn undo_positions(world: &PlotWorld, plan: &Plan) -> Vec<BlockPos> {
    let mut positions: HashSet<_> = plan.placements.iter().map(|&(pos, _)| pos).collect();
    // Placement changes surrounding stored wire shapes; raw WorldEdit undo must restore them too.
    for &(pos, _) in &plan.placements {
        for side in BlockFace::values() {
            let neighbor = pos.offset(side);
            for cell in [
                neighbor,
                neighbor.offset(BlockFace::Top),
                neighbor.offset(BlockFace::Bottom),
            ] {
                if world.contains_position(cell)
                    && matches!(world.get_block(cell), Block::RedstoneWire { .. })
                {
                    positions.insert(cell);
                }
            }
        }
    }
    let mut positions: Vec<_> = positions.into_iter().collect();
    positions.sort_by_key(|pos| (pos.y, pos.z, pos.x));
    positions
}
