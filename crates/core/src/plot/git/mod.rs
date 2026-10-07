//! Persistent, per-plot snapshot repositories. World access stays on the plot thread.
mod diff;
mod repository;
mod resources;
mod snapshot;
#[cfg(test)]
mod tests;
mod visuals;

use self::diff::{Diff, Marker};
use self::repository::{Limits, Repository};
use self::snapshot::Snapshot;
use super::{Plot, PLOT_SCALE, PLOT_WIDTH};
use crate::config::CONFIG;
use crate::messages;
use crate::permissions;
use crate::player::{PacketSender, PlayerPos};
use crate::world::storage::Chunk;
use crate::world::World;
use anyhow::{bail, ensure, Context, Result};
use mchprs_blocks::BlockPos;
use mchprs_network::packets::clientbound::{
    CDestroyEntities, CTabComplete, CTabCompleteMatch, ClientBoundPacket,
};
use mchprs_save_data::plot_data::{PlotData, Tps};
use mchprs_world::AdvancePhase;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const ROOT: &str = "./world/plot-git";
const MAX_WORK_MEMORY_MIB: u64 = 1024;
const MAX_SNAPSHOT_MIB: u64 = 128;

static USED: AtomicUsize = AtomicUsize::new(0);
pub(super) fn memory_usage() -> (usize, usize) {
    (
        USED.load(Ordering::SeqCst),
        mib(CONFIG.git_work_memory_mib.min(MAX_WORK_MEMORY_MIB)),
    )
}
pub(super) struct Reservation(usize);
impl Reservation {
    fn snapshot(bytes: usize) -> Result<Self> {
        // Packed bytes understate decoded maps/collections. Include compression,
        // deserialization, canonical data scratch and fixed chunk storage.
        Self::new(
            bytes
                .checked_mul(10)
                .and_then(|n| n.checked_add(32 * 1048576))
                .context(messages::GIT_CAPTURE_SIZE_OVERFLOW)?,
        )
    }

    fn new(bytes: usize) -> Result<Self> {
        let limit = mib(CONFIG.git_work_memory_mib.min(MAX_WORK_MEMORY_MIB));
        USED.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
            used.checked_add(bytes).filter(|&n| n <= limit)
        })
        .map_err(|_| anyhow::anyhow!(messages::GIT_MEMORY_LIMIT_REACHED))?;
        Ok(Self(bytes))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        USED.fetch_sub(self.0, Ordering::SeqCst);
    }
}

fn mib(value: u64) -> usize {
    value.saturating_mul(1048576).min(usize::MAX as u64) as usize
}
fn limits() -> Limits {
    Limits {
        snapshot: mib(CONFIG.git_snapshot_max_mib.min(MAX_SNAPSHOT_MIB)),
        plot_bytes: CONFIG.git_plot_storage_mib.saturating_mul(1048576),
        total_bytes: CONFIG.git_total_storage_mib.saturating_mul(1048576),
    }
}

fn storage_permission_prefix() -> &'static str {
    if permissions::dedicated_permissions() {
        "mchprs.git.storage."
    } else {
        "git.storage."
    }
}

fn storage_limit_mib(grant: Option<usize>, default: u64, ceiling: u64) -> u64 {
    grant.map_or(default, |limit| limit as u64).min(ceiling)
}

fn plot_storage_mib(grant: Option<usize>) -> u64 {
    storage_limit_mib(
        grant,
        CONFIG.git_default_plot_storage_mib,
        CONFIG.git_plot_storage_mib,
    )
}

fn owner_limits(owner: Option<u128>, cached_storage: Option<u64>) -> Result<Limits> {
    // Run on a Git worker: an offline owner's rank must not trigger a network
    // query on the plot thread or let a higher-ranked member bypass its quota.
    let storage = match (owner, cached_storage) {
        (_, Some(storage)) => storage,
        (Some(owner), None) if CONFIG.luckperms.is_some() => {
            let cache = permissions::load_player_cache(owner)
                .context(messages::GIT_OWNER_STORAGE_LIMIT_UNAVAILABLE)?;
            plot_storage_mib(cache.numeric_limit(storage_permission_prefix()))
        }
        _ => plot_storage_mib(None),
    };
    Ok(Limits {
        plot_bytes: storage.saturating_mul(1048576),
        ..limits()
    })
}

type Task = Box<dyn FnOnce() -> Result<Reply> + Send>;
struct Job {
    task: Task,
    reply: SyncSender<Result<Reply>>,
    span: tracing::Span,
}
static WORKERS: Lazy<SyncSender<Job>> = Lazy::new(|| {
    let (sender, receiver) = mpsc::sync_channel::<Job>(16);
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..2 {
        let receiver = receiver.clone();
        std::thread::Builder::new()
            .name(format!("plot-git-{index}"))
            .spawn(move || loop {
                let job = receiver.lock().unwrap().recv();
                let Ok(job) = job else {
                    break;
                };
                let _entered = job.span.enter();
                let started = Instant::now();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.task))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!(messages::GIT_WORKER_FAILED)));
                match &result {
                    Ok(reply) => {
                        if let Payload::Text(text) = &reply.payload {
                            tracing::info!(result = %text, "Git result");
                        }
                        tracing::info!(elapsed_ms = started.elapsed().as_millis(), "Git worker completed");
                    }
                    Err(error) => tracing::warn!(elapsed_ms = started.elapsed().as_millis(), error = %format_args!("{error:#}"), "Git worker rejected"),
                }
                let _ = job.reply.send(result);
            })
            .expect("Cannot start plot Git worker");
    }
    sender
});

struct Pending {
    receiver: Receiver<Result<Reply>>,
    actor: u128,
    checkout: Option<Tps>,
}
struct Reply {
    payload: Payload,
    names: Vec<String>,
    head: Option<String>,
}
enum Payload {
    Metadata,
    Text(String),
    Chat(Value),
    Diff(Arc<Diff>),
    Checkout(Snapshot, String, Reservation),
    CheckoutFailed(String, bool),
    Markers(Vec<Marker>, PlayerPos),
}

struct Session {
    diff: Arc<Diff>,
    enabled: bool,
    expires: Instant,
    next_update: Instant,
    last_pos: Option<PlayerPos>,
    markers: HashMap<BlockPos, (i32, u8)>,
}

#[derive(Default)]
pub(super) struct State {
    pending: Option<Pending>,
    pub(super) locked: bool,
    pub(super) fatal: bool,
    pub(super) head: Option<String>,
    metadata_retry_at: Option<Instant>,
    sessions: HashMap<u128, Session>,
    names: Vec<String>,
    clicks: HashMap<u128, Instant>,
}

impl State {
    pub(super) fn diagnostics(&self) -> String {
        let (used, limit) = memory_usage();
        format!("Git head {:?}; pending {}; locked {}; recovery required {}; diff sessions {}; RAM reserved {}/{} MiB",
            self.head, self.pending.is_some(), self.locked, self.fatal, self.sessions.len(),
            used / 1048576, limit / 1048576)
    }
}

pub(super) fn recover_pending(path: &Path) -> Result<()> {
    let Some(name) = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix('p'))
    else {
        return Ok(());
    };
    let Some((x, z)) = name.split_once(',') else {
        return Ok(());
    };
    let plot = (x.parse::<i32>()?, z.parse::<i32>()?);
    if !Path::new(ROOT)
        .join(format!("p{},{}", plot.0, plot.1))
        .join("repository.sqlite")
        .exists()
    {
        return Ok(());
    }
    let mut repo = Repository::open(Path::new(ROOT), plot, limits())?;
    if repo.has_pending()? {
        let _reservation = Reservation::snapshot(repo.pending_size()?)?;
        repo.finish_checkout(path)?;
    }
    Ok(())
}

impl Plot {
    pub(super) fn git_checkout_locked(&self) -> bool {
        self.git.locked
    }

    pub(super) fn git_lock_message(&self) -> &'static str {
        if self.git.fatal {
            messages::GIT_STARTUP_RECOVERY_REQUIRED
        } else {
            messages::GIT_CHECKOUT_IN_PROGRESS
        }
    }

    pub(super) fn git_access(&self, player: usize, action: &str) -> bool {
        let actor = &self.players[player];
        let admin = actor.has_explicit_permission("plots.admin.git");
        let member = self.owner == Some(actor.uuid)
            || super::database::is_plot_member(self.world.x, self.world.z, actor.uuid)
            || (self.owner.is_none() && actor.has_permission("plots.admin.interact.unowned"));
        if !admin && !member {
            return false;
        }
        if !actor.can_use_commands() || !actor.has_permission("commands.git") {
            return false;
        }
        match action {
            "checkout" => {
                admin
                    || (self.owner == Some(actor.uuid)
                        && actor.can_edit_plot(self.owner, (self.world.x, self.world.z)))
            }
            "commit" | "branch" => {
                admin || actor.can_edit_plot(self.owner, (self.world.x, self.world.z))
            }
            _ => true,
        }
    }

    fn capture_git(&mut self) -> Result<(Snapshot, Reservation)> {
        let started = Instant::now();
        ensure!(
            self.world.piston_state.phase == AdvancePhase::BetweenTicks,
            messages::GIT_FINISH_PARTIAL_TICK
        );
        self.reset_redpiler(); // Exports compiled block state and its scheduled ticks.
        for chunk in &mut self.world.chunks {
            chunk.prepare_history();
            for entity in chunk.block_entities.values() {
                resources::check_entity(entity)?;
            }
        }
        for motion in &self.world.piston_state.motions {
            if let Some(entity) = &motion.carried_entity {
                resources::check_entity(entity)?;
            }
        }
        let chunks_size = self.world.chunks.iter().try_fold(0u64, |total, c| {
            Ok::<_, anyhow::Error>(total + bincode::serialized_size(&c.history_view())?)
        })?;
        let ticks_size = self
            .world
            .to_be_ticked
            .iter_entries()
            .try_fold(0u64, |total, t| {
                Ok::<_, anyhow::Error>(total + bincode::serialized_size(&t)?)
            })?;
        let estimate = usize::try_from(
            chunks_size + ticks_size + bincode::serialized_size(&self.world.piston_state)? + 128,
        )?;
        ensure!(
            estimate < limits().snapshot,
            messages::GIT_SNAPSHOT_SIZE_LIMIT
        );
        let reservation = Reservation::snapshot(estimate)?;
        let snapshot = Snapshot {
            version: 1,
            data_version: mchprs_save_data::plot_data::MC_DATA_VERSION,
            plot: (self.world.x, self.world.z),
            data: PlotData {
                tps: self.tps,
                world_send_rate: self.world_send_rate,
                piston_animation: self.piston_animation,
                chunk_data: self.world.chunks.iter_mut().map(Chunk::save).collect(),
                pending_ticks: self.world.to_be_ticked.iter_entries().collect(),
                piston_state: self.world.piston_state.clone(),
            },
        };
        tracing::debug!(
            plot_x = self.world.x,
            plot_z = self.world.z,
            bytes = estimate,
            elapsed_ms = started.elapsed().as_millis(),
            "Captured plot Git snapshot"
        );
        Ok((snapshot, reservation))
    }

    fn start_git(&mut self, actor: u128, checkout: Option<Tps>, task: Task) -> Result<()> {
        ensure!(self.git.pending.is_none(), messages::GIT_OPERATION_RUNNING);
        let (reply, receiver) = mpsc::sync_channel(1);
        WORKERS
            .try_send(Job {
                task,
                reply,
                span: tracing::info_span!("git",
                plot_x = self.world.x, plot_z = self.world.z,
                player = self.players.iter().find(|p| p.uuid == actor).map_or("system", |p| p.username.as_str()),
                actor = %format_args!("{actor:032x}")),
            })
            .map_err(|_| anyhow::anyhow!(messages::GIT_WORKER_QUEUE_FULL))?;
        self.git.pending = Some(Pending {
            receiver,
            actor,
            checkout,
        });
        Ok(())
    }

    pub(super) fn handle_git_command(&mut self, player: usize, args: &[&str]) {
        if let Err(error) = self.git_command(player, args) {
            tracing::warn!(error = %format_args!("{error:#}"), "Git command rejected");
            self.players[player]
                .send_error_message(&messages::git_error(format_args!("{error:#}")));
        }
    }

    fn git_command(&mut self, player: usize, args: &[&str]) -> Result<()> {
        let action = match args.first().copied() {
            Some("commit") => "commit",
            Some("branch") if args.len() > 1 => "branch",
            Some("checkout" | "recover" | "rebase") => "checkout",
            Some("diff") if args.get(1) == Some(&"show") => "visual",
            _ => "read",
        };
        ensure!(
            self.git_access(player, action),
            messages::GIT_PERMISSION_DENIED
        );
        let actor = self.players[player].uuid;
        if args.is_empty() || args == ["help"] {
            self.players[player].send_system_message(messages::HELP_GIT);
            return Ok(());
        }
        if args == ["diff", "hide"] {
            self.hide_git(player, false);
            self.players[player].send_system_message(messages::GIT_DIFF_GLOW_HIDDEN);
            return Ok(());
        }
        if args == ["diff", "show"] {
            let session = self
                .git
                .sessions
                .get_mut(&actor)
                .context(messages::GIT_PREPARE_COMPARISON)?;
            session.enabled = true;
            session.last_pos = None;
            session.next_update = Instant::now();
            session.expires =
                Instant::now() + Duration::from_secs(CONFIG.git_session_seconds.clamp(10, 3600));
            self.players[player].send_system_message(messages::GIT_DIFF_INSPECT_HINT);
            return Ok(());
        }
        ensure!(!self.git.locked, messages::GIT_CHECKOUT_LOCKED);
        ensure!(self.git.pending.is_none(), messages::GIT_OPERATION_RUNNING);
        if args.first() == Some(&"diff") && args.get(1) == Some(&"inspect") {
            ensure!(
                args.len() == 5 || args.len() == 6,
                messages::USAGE_GIT_DIFF_INSPECT
            );
            let pos = BlockPos::new(args[2].parse()?, args[3].parse()?, args[4].parse()?);
            let side = args.get(5).map(|s| s.to_string());
            let diff = self
                .git
                .sessions
                .get(&actor)
                .context(messages::GIT_NO_COMPARISON)?
                .diff
                .clone();
            return self.start_git(
                actor,
                None,
                Box::new(move || {
                    Ok(Reply {
                        payload: Payload::Chat(diff.inspect(pos, side.as_deref())?),
                        names: Vec::new(),
                        head: None,
                    })
                }),
            );
        }
        let plot = (self.world.x, self.world.z);
        let owner = self.owner;
        let cached_storage = owner.and_then(|uuid| {
            self.players
                .iter()
                .find(|p| p.uuid == uuid)
                .map(|p| plot_storage_mib(p.numeric_permission_limit(storage_permission_prefix())))
        });
        let name = self.players[player].username.clone();
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let checkout = matches!(args.first(), Some(&"checkout" | &"recover" | &"rebase"));
        let captured = if matches!(
            args.first(),
            Some(&"commit" | &"status" | &"checkout" | &"recover" | &"rebase")
        ) {
            Some(self.capture_git()?)
        } else {
            None
        };
        let previous = checkout.then_some(self.tps);
        let task: Task = Box::new(move || {
            let mut repo =
                Repository::open(Path::new(ROOT), plot, owner_limits(owner, cached_storage)?)?;
            let words: Vec<&str> = owned.iter().map(String::as_str).collect();
            let payload = match words.as_slice() {
                ["status"] => Payload::Text(repo.status(&captured.as_ref().unwrap().0)?),
                ["commit", message @ ..] if !message.is_empty() => Payload::Text(repo.commit(
                    &captured.as_ref().unwrap().0,
                    actor,
                    &name,
                    &message.join(" "),
                )?),
                ["branch"] => Payload::Text(repo.branches()?),
                ["branch", branch] => Payload::Text(repo.branch(branch, "HEAD")?),
                ["branch", branch, reference] => Payload::Text(repo.branch(branch, reference)?),
                ["show", reference] => Payload::Text(repo.show(reference)?),
                ["recoveries"] => Payload::Chat(repo.recoveries(1)?),
                ["recoveries", page] => Payload::Chat(repo.recoveries(page.parse()?)?),
                ["log", rest @ ..] => {
                    let (all, page) = parse_log(rest)?;
                    Payload::Chat(repo.log(all, None, page)?)
                }
                ["search", rest @ ..] => {
                    let (all, page, query) = parse_search(rest)?;
                    Payload::Chat(repo.log(all, Some(&query), page)?)
                }
                ["diff", a, b] => {
                    let (from_label, to_label) = ((*a).to_owned(), (*b).to_owned());
                    let a = repo.resolve(a)?;
                    let b = repo.resolve(b)?;
                    let size = repo.raw_size(&a)?.saturating_add(repo.raw_size(&b)?);
                    let reservation = Reservation::snapshot(size)?;
                    let mut diff = Diff::new(
                        a.clone(),
                        b.clone(),
                        repo.load(&a)?,
                        repo.load(&b)?,
                        reservation,
                    )?;
                    diff.from_label = from_label;
                    diff.to_label = to_label;
                    Payload::Diff(Arc::new(diff))
                }
                ["checkout", branch] => checkout_payload(
                    &mut repo,
                    branch,
                    &captured.as_ref().unwrap().0,
                    actor,
                    &name,
                ),
                ["rebase", reference] => {
                    let save = PathBuf::from(format!("./world/plots/p{},{}", plot.0, plot.1));
                    match repo.rebase(
                        reference,
                        &captured.as_ref().unwrap().0,
                        actor,
                        &name,
                        &save,
                    ) {
                        Ok((snapshot, message, reservation)) => {
                            Payload::Checkout(snapshot, message, reservation)
                        }
                        Err(error) => Payload::CheckoutFailed(
                            format!("{error:#}"),
                            repo.has_pending().unwrap_or(true),
                        ),
                    }
                }
                ["rebase", ..] => bail!(messages::USAGE_GIT_REBASE),
                ["recover", id, branch] => {
                    repo.recover_branch(id, branch, actor, &name)?;
                    checkout_payload(
                        &mut repo,
                        branch,
                        &captured.as_ref().unwrap().0,
                        actor,
                        &name,
                    )
                }
                _ => bail!(messages::git_unknown_arguments(messages::HELP_GIT)),
            };
            // Completion caching must never discard an already durable checkout result.
            Ok(Reply {
                payload,
                names: repo.names().unwrap_or_default(),
                head: repo.sidebar_head().ok(),
            })
        });
        self.start_git(actor, previous, task)?;
        if checkout {
            self.git.locked = true;
            self.close_all_containers();
            self.set_git_tps(Tps::Limited(0));
        }
        self.players[player].send_system_message(messages::GIT_OPERATION_STARTED);
        Ok(())
    }

    fn set_git_tps(&mut self, tps: Tps) {
        self.tps = tps;
        self.sleep_time = super::data::sleep_time_for_tps(tps);
        self.timings.set_tps(tps);
        self.reset_timings();
    }

    fn accept_git(&mut self, pending: Pending, result: Result<Reply>) {
        let actor = self.players.iter().position(|p| p.uuid == pending.actor);
        if let Some(previous) = pending.checkout {
            self.git.locked = false;
            self.set_git_tps(previous);
        }
        match result {
            Ok(reply) => {
                if let Some(head) = reply.head {
                    self.git.head = Some(head);
                }
                if !reply.names.is_empty() {
                    self.git.names = reply.names;
                }
                match reply.payload {
                    Payload::Metadata => {}
                    Payload::Checkout(snapshot, message, _reservation) => {
                        tracing::info!(actor = %format_args!("{:032x}", pending.actor),
                            plot_x = self.world.x, plot_z = self.world.z, %message, "Git restore completed");
                        self.apply_git(snapshot);
                        self.broadcast_plot_chat_message(&message);
                    }
                    Payload::CheckoutFailed(error, needs_recovery) => {
                        tracing::warn!(actor = %format_args!("{:032x}", pending.actor),
                            plot_x = self.world.x, plot_z = self.world.z, %error, needs_recovery,
                            "Git restore rejected");
                        if needs_recovery {
                            self.git.locked = true;
                            self.git.fatal = true;
                            self.set_git_tps(Tps::Limited(0));
                        }
                        if let Some(player) = actor {
                            self.players[player]
                                .send_error_message(&messages::git_checkout_failed(error));
                        }
                    }
                    payload => {
                        if let Some(player) = actor {
                            if !self.git_access(player, "read") {
                                self.hide_git(player, true);
                                return;
                            }
                            match payload {
                                Payload::Text(text) => {
                                    self.players[player].send_system_message(&text)
                                }
                                Payload::Chat(json) => {
                                    self.players[player].send_raw_system_message(json.to_string())
                                }
                                Payload::Diff(diff) => {
                                    self.hide_git(player, true);
                                    self.players[player]
                                        .send_raw_system_message(diff.summary().to_string());
                                    let now = Instant::now();
                                    self.git.sessions.insert(
                                        pending.actor,
                                        Session {
                                            diff,
                                            enabled: false,
                                            expires: now
                                                + Duration::from_secs(
                                                    CONFIG.git_session_seconds.clamp(10, 3600),
                                                ),
                                            next_update: now,
                                            last_pos: None,
                                            markers: HashMap::new(),
                                        },
                                    );
                                }
                                Payload::Markers(markers, center)
                                    if self.git_access(player, "visual") =>
                                {
                                    self.replace_git_markers(player, markers, center);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Err(error) => {
                // A worker failure during checkout cannot prove the save was untouched.
                if pending.checkout.is_some() {
                    let path = Path::new(ROOT)
                        .join(format!("p{},{}", self.world.x, self.world.z))
                        .join("repository.sqlite");
                    if path.exists() {
                        let unresolved = Repository::open(
                            Path::new(ROOT),
                            (self.world.x, self.world.z),
                            limits(),
                        )
                        .and_then(|r| r.has_pending())
                        .unwrap_or(true);
                        if unresolved {
                            self.git.locked = true;
                            self.git.fatal = true;
                            self.set_git_tps(Tps::Limited(0));
                        }
                    }
                }
                if let Some(player) = actor {
                    self.players[player]
                        .send_error_message(&messages::git_error(format_args!("{error:#}")));
                }
            }
        }
    }

    fn load_git_sidebar(&mut self) {
        if self.players.is_empty()
            || self.git.head.is_some()
            || self.git.pending.is_some()
            || self
                .git
                .metadata_retry_at
                .is_some_and(|at| at > Instant::now())
        {
            return;
        }
        let plot = (self.world.x, self.world.z);
        let path = Path::new(ROOT)
            .join(format!("p{},{}", plot.0, plot.1))
            .join("repository.sqlite");
        if !path.exists() {
            // Visiting a plot must not create a Git repository.
            self.git.head = Some(String::new());
            return;
        }
        self.git.metadata_retry_at = Some(Instant::now() + Duration::from_secs(30));
        // Metadata has no requesting player. Database reads stay on a worker;
        // subsequent sidebar refreshes only use the cached head.
        let _ = self.start_git(
            0,
            None,
            Box::new(move || {
                let repo = Repository::open(Path::new(ROOT), plot, limits())?;
                Ok(Reply {
                    payload: Payload::Metadata,
                    names: repo.names().unwrap_or_default(),
                    head: Some(repo.sidebar_head()?),
                })
            }),
        );
    }

    pub(super) fn update_git(&mut self) {
        self.load_git_sidebar();
        if let Some(pending) = self.git.pending.take() {
            match pending.receiver.try_recv() {
                Ok(result) => self.accept_git(pending, result),
                Err(TryRecvError::Empty) => self.git.pending = Some(pending),
                Err(TryRecvError::Disconnected) => self.accept_git(
                    pending,
                    Err(anyhow::anyhow!(messages::GIT_WORKER_DISCONNECTED)),
                ),
            }
        }
        let now = Instant::now();
        let expired: Vec<_> = self
            .git
            .sessions
            .iter()
            .filter(|(uuid, s)| {
                s.expires <= now
                    || !self
                        .players
                        .iter()
                        .position(|p| p.uuid == **uuid)
                        .is_some_and(|p| {
                            self.git_access(p, "read")
                                && (!s.enabled || self.git_access(p, "visual"))
                        })
            })
            .map(|(uuid, _)| *uuid)
            .collect();
        for uuid in expired {
            self.remove_git_session(uuid);
        }
        self.git.clicks.retain(|uuid, time| {
            time.elapsed() < Duration::from_secs(1) && self.players.iter().any(|p| p.uuid == *uuid)
        });
        if self.git.pending.is_some() || self.git.locked {
            return;
        }
        for player in 0..self.players.len() {
            let uuid = self.players[player].uuid;
            let Some(session) = self.git.sessions.get_mut(&uuid) else {
                continue;
            };
            let center = self.players[player].pos;
            if !session.enabled || session.next_update > now {
                continue;
            }
            session.next_update = now + Duration::from_secs(1);
            if session.last_pos.is_some_and(|old| {
                (old.x - center.x).powi(2) + (old.y - center.y).powi(2) + (old.z - center.z).powi(2)
                    < 16.0
            }) {
                continue;
            }
            let diff = session.diff.clone();
            let radius = CONFIG.git_marker_radius.clamp(1, 128) as f64;
            let limit = CONFIG.git_marker_limit.min(512) as usize;
            let view = (CONFIG.view_distance.max(0) * 16) as f64;
            let task: Task = Box::new(move || {
                Ok(Reply {
                    payload: Payload::Markers(diff.near(center, radius.min(view), limit)?, center),
                    names: Vec::new(),
                    head: None,
                })
            });
            let _ = self.start_git(uuid, None, task);
            break;
        }
    }

    pub(super) fn finish_git(&mut self) {
        if let Some(pending) = self.git.pending.take() {
            let result = pending.receiver.recv().unwrap_or_else(|_| {
                Err(anyhow::anyhow!(messages::GIT_WORKER_DISCONNECTED_SHUTDOWN))
            });
            self.accept_git(pending, result);
        }
    }

    fn apply_git(&mut self, snapshot: Snapshot) {
        self.close_all_containers();
        self.reset_redpiler();
        let coords = snapshot.plot;
        self.world.chunks = snapshot
            .data
            .chunk_data
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                Chunk::load(
                    (coords.0 << PLOT_SCALE) + i as i32 / PLOT_WIDTH,
                    (coords.1 << PLOT_SCALE) + i as i32 % PLOT_WIDTH,
                    c,
                )
            })
            .collect();
        self.world.to_be_ticked = snapshot.data.pending_ticks.into_iter().collect();
        self.world.piston_state = snapshot.data.piston_state;
        self.world.history.disable();
        self.world.clear_interpreter_caches();
        self.world.reset_screen_tracking();
        self.world.command_messages.clear();
        self.world.sounds.clear();
        self.world.open_chests.clear();
        self.set_git_tps(Tps::Limited(0));
        for player in 0..self.players.len() {
            self.hide_git(player, true);
            self.players[player].worldedit_undo.clear();
            self.players[player].worldedit_redo.clear();
            let pos = self.players[player].pos;
            if !self.git_player_clear(pos) {
                if let Some(safe) = self.git_safe_position(pos) {
                    self.players[player].teleport(safe);
                    self.players[player].on_ground = false;
                    let packet = self.players[player].entity_teleport_packet();
                    self.broadcast_player_packets(player, &[&packet]);
                }
            }
            self.update_view_pos_for_player(player, true);
        }
    }

    fn git_player_clear(&self, pos: PlayerPos) -> bool {
        super::compass::body_clear(pos, &|p| {
            if !Plot::in_plot_bounds(self.world.x, self.world.z, p.x, p.z) {
                None
            } else if !(0..super::PLOT_BLOCK_HEIGHT).contains(&p.y) {
                Some(mchprs_blocks::blocks::Block::Air)
            } else {
                Some(self.world.get_block(p))
            }
        })
    }
    fn git_safe_position(&self, pos: PlayerPos) -> Option<PlayerPos> {
        // Above the build is always available: creative/spectator plots permit flight.
        let safe = PlayerPos::new(
            pos.x.floor() + 0.5,
            super::PLOT_BLOCK_HEIGHT as f64 + 1.0,
            pos.z.floor() + 0.5,
        );
        safe.is_valid().then_some(safe)
    }

    pub(super) fn sword_git(&mut self, player: usize, hand: i32, yaw: f32, pitch: f32) -> bool {
        let uuid = self.players[player].uuid;
        if self
            .git
            .clicks
            .get(&uuid)
            .is_some_and(|last| last.elapsed() < Duration::from_millis(200))
        {
            return true;
        }
        if !self.git_access(player, "visual") {
            return false;
        }
        let slot = match hand {
            0 => 36 + self.players[player].selected_slot as usize,
            1 => 45,
            _ => return false,
        };
        if !self.players[player]
            .inventory
            .get(slot)
            .and_then(Option::as_ref)
            .is_some_and(|i| i.item_type.get_name().ends_with("_sword"))
        {
            return false;
        }
        let Some(session) = self.git.sessions.get(&uuid).filter(|s| s.enabled) else {
            return false;
        };
        let data = &self.players[player];
        let eye = PlayerPos::new(
            data.pos.x,
            data.pos.y + if data.crouching { 1.27 } else { 1.62 },
            data.pos.z,
        );
        let Some(pos) = diff::aimed(
            eye,
            yaw,
            pitch,
            session
                .markers
                .iter()
                .map(|(&pos, &(_, kind))| Marker { pos, kind }),
            CONFIG.git_marker_radius.clamp(1, 128) as f64,
        ) else {
            return false;
        };
        let diff = session.diff.clone();
        self.git.clicks.insert(uuid, Instant::now());
        if self.git.pending.is_none() {
            let _ = self.start_git(
                uuid,
                None,
                Box::new(move || {
                    Ok(Reply {
                        payload: Payload::Chat(diff.inspect(pos, None)?),
                        names: Vec::new(),
                        head: None,
                    })
                }),
            );
        } else {
            self.players[player].send_system_message(messages::GIT_INSPECTION_BUSY);
        }
        true
    }

    pub(super) fn complete_git(&self, player: usize, id: i32, text: &str) -> Option<CTabComplete> {
        if !text.starts_with("/git ") {
            return None;
        }
        let start = text.rfind(' ')? + 1;
        let prefix = &text[start..];
        let words: Vec<_> = text[..start].split_whitespace().collect();
        let values: Vec<&str> = if words.len() == 1 {
            vec![
                "status",
                "commit",
                "log",
                "search",
                "branch",
                "checkout",
                "rebase",
                "diff",
                "recoveries",
                "recover",
                "help",
            ]
        } else if words.get(1) == Some(&"diff") && words.len() == 2 {
            vec!["show", "hide", "inspect", "HEAD"]
        } else if words.get(1) == Some(&"rebase") {
            vec![]
        } else {
            vec!["HEAD"]
        };
        let matches = if self.git_access(player, "read") {
            values
                .into_iter()
                .chain(self.git.names.iter().map(String::as_str))
                .filter(|s| s.starts_with(prefix))
                .take(100)
                .map(|s| CTabCompleteMatch {
                    match_: s.to_owned(),
                    tooltip: None,
                })
                .collect()
        } else {
            Vec::new()
        };
        Some(CTabComplete {
            id,
            start: text[..start].encode_utf16().count() as i32,
            length: prefix.encode_utf16().count() as i32,
            matches,
        })
    }

    fn remove_git_session(&mut self, uuid: u128) {
        if let Some(player) = self.players.iter().position(|p| p.uuid == uuid) {
            self.hide_git(player, true);
        } else {
            self.git.sessions.remove(&uuid);
        }
    }
    pub(super) fn hide_git(&mut self, player: usize, remove: bool) {
        let uuid = self.players[player].uuid;
        if let Some(session) = self.git.sessions.get_mut(&uuid) {
            let ids = session.markers.drain().map(|(_, v)| v.0).collect();
            self.players[player].send_packet(&CDestroyEntities { entity_ids: ids }.encode());
            session.enabled = false;
            session.last_pos = None;
        }
        if remove {
            self.git.sessions.remove(&uuid);
        }
    }
}

fn checkout_payload(
    repo: &mut Repository,
    branch: &str,
    before: &Snapshot,
    actor: u128,
    name: &str,
) -> Payload {
    let save = PathBuf::from(format!("./world/plots/p{},{}", repo.plot.0, repo.plot.1));
    match repo.checkout(branch, before, actor, name, &save) {
        Ok((snapshot, message, reservation)) => Payload::Checkout(snapshot, message, reservation),
        Err(error) => {
            Payload::CheckoutFailed(format!("{error:#}"), repo.has_pending().unwrap_or(true))
        }
    }
}

fn parse_log(args: &[&str]) -> Result<(bool, usize)> {
    match args {
        [] => Ok((false, 1)),
        ["--all"] => Ok((true, 1)),
        [page] => Ok((false, page.parse()?)),
        ["--all", page] => Ok((true, page.parse()?)),
        _ => bail!(messages::USAGE_GIT_LOG),
    }
}
fn parse_search(args: &[&str]) -> Result<(bool, usize, String)> {
    let mut index = 0;
    let mut all = false;
    let mut page = 1;
    while let Some(flag) = args.get(index) {
        match *flag {
            "--all" => {
                all = true;
                index += 1;
            }
            "--page" => {
                page = args
                    .get(index + 1)
                    .context(messages::GIT_MISSING_PAGE)?
                    .parse()?;
                index += 2;
            }
            _ => break,
        }
    }
    let text = args[index..].join(" ");
    ensure!(
        !text.is_empty() && text.chars().count() <= 128,
        messages::GIT_SEARCH_TEXT_LIMIT
    );
    Ok((all, page, text))
}
