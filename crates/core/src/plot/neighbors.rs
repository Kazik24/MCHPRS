//! Read-only neighbor snapshots. World access stays on the owning plot thread;
//! only unloaded saves are read by the bounded background worker.
use super::{Plot, NUM_CHUNKS, PLOT_SCALE, PLOT_SECTIONS, PLOT_WIDTH};
use crate::config::CONFIG;
use crate::world::storage::Chunk;
use mchprs_network::packets::PacketEncoder;
use mchprs_save_data::plot_data::PlotData;
use once_cell::sync::Lazy;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

type Coord = (i32, i32);
type Snapshot = Arc<PacketEncoder>;
type Response = Vec<(Coord, Snapshot)>;
const LIVE_CACHE_BYTES: usize = 16 * 1024 * 1024;
const DISK_CACHE_BYTES: usize = 64 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn plot_for_chunk((x, z): Coord) -> Coord {
    (x >> PLOT_SCALE, z >> PLOT_SCALE)
}

#[derive(Debug)]
struct Request {
    plot: Coord,
    chunks: Vec<Coord>,
    reply: SyncSender<Response>,
}

struct Service {
    sources: Mutex<HashMap<Coord, (u64, SyncSender<Request>)>>,
    next_source: AtomicU64,
    disk: SyncSender<Request>,
}

static SERVICE: Lazy<Service> = Lazy::new(|| {
    let (disk, requests) = mpsc::sync_channel::<Request>(32);
    std::thread::Builder::new()
        .name("neighbor-snapshots".into())
        .spawn(move || {
            let mut cache = Cache::new(DISK_CACHE_BYTES);
            while let Ok(request) = requests.recv() {
                let Some(request) = SERVICE.route_live(request) else {
                    continue;
                };
                let result = disk_snapshots(Path::new("./world/plots"), &request, &mut cache);
                // A plot may have loaded while its saved snapshot was being read.
                let Some(request) = SERVICE.route_live(request) else {
                    continue;
                };
                match result {
                    Ok(packets) => {
                        let _ = request.reply.try_send(packets);
                    }
                    Err(error) => tracing::warn!(
                        plot_x = request.plot.0,
                        plot_z = request.plot.1,
                        "Cannot read neighboring plot: {error:#}"
                    ),
                }
            }
        })
        .expect("Cannot start neighbor snapshot worker");
    Service {
        sources: Mutex::new(HashMap::new()),
        next_source: AtomicU64::new(1),
        disk,
    }
});

impl Service {
    /// A full live queue is retried later, never replaced by stale disk data.
    fn route_live(&self, request: Request) -> Option<Request> {
        let sources = self.sources.lock().unwrap();
        let Some((_, source)) = sources.get(&request.plot) else {
            return Some(request);
        };
        match source.try_send(request) {
            Ok(()) | Err(TrySendError::Full(_)) => None,
            Err(TrySendError::Disconnected(request)) => Some(request),
        }
    }

    fn request(&self, request: Request) {
        if let Some(request) = self.route_live(request) {
            // Overload drops the reply sender so the viewer can retry next interval.
            let _ = self.disk.try_send(request);
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn file_stamp(path: &Path) -> std::io::Result<Option<FileStamp>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileStamp {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Version {
    Live((u64, u64)),
    Saved(bool, Option<FileStamp>),
}

struct Cached {
    version: Version,
    packet: Snapshot,
    used: u64,
}

struct Cache {
    entries: HashMap<Coord, Cached>,
    bytes: usize,
    limit: usize,
    clock: u64,
}

impl Cache {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            limit,
            clock: 0,
        }
    }

    fn get(&mut self, chunk: Coord, version: &Version) -> Option<Snapshot> {
        let entry = self.entries.get_mut(&chunk)?;
        if &entry.version != version {
            return None;
        }
        self.clock += 1;
        entry.used = self.clock;
        Some(entry.packet.clone())
    }

    fn insert(&mut self, chunk: Coord, version: Version, packet: Snapshot) {
        if let Some(old) = self.entries.remove(&chunk) {
            self.bytes -= old.packet.buffer.len() + 128;
        }
        let bytes = packet.buffer.len() + 128;
        if bytes > self.limit {
            return;
        }
        while self.bytes + bytes > self.limit {
            let oldest = *self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .unwrap()
                .0;
            let old = self.entries.remove(&oldest).unwrap();
            self.bytes -= old.packet.buffer.len() + 128;
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            chunk,
            Cached {
                version,
                packet,
                used: self.clock,
            },
        );
    }
}

fn disk_snapshots(root: &Path, request: &Request, cache: &mut Cache) -> anyhow::Result<Response> {
    let path = root.join(format!("p{},{}", request.plot.0, request.plot.1));
    let stamp = file_stamp(&path)?;
    let template = root.join("pTEMPLATE");
    let template_stamp = if stamp.is_none() {
        file_stamp(&template)?
    } else {
        None
    };
    let version = Version::Saved(stamp.is_some(), stamp.clone().or(template_stamp.clone()));
    let chunks: Vec<_> = request
        .chunks
        .iter()
        .copied()
        .filter(|&chunk| plot_for_chunk(chunk) == request.plot)
        .collect();
    let mut result = Vec::with_capacity(chunks.len());
    let mut loaded = None;
    for coord in chunks {
        if let Some(packet) = cache.get(coord, &version) {
            result.push((coord, packet));
            continue;
        }
        if loaded.is_none() && (stamp.is_some() || template_stamp.is_some()) {
            let source = if stamp.is_some() { &path } else { &template };
            let data = PlotData::<PLOT_SECTIONS>::load_from_file(source, false)?;
            anyhow::ensure!(
                data.chunk_data.len() == NUM_CHUNKS,
                "wrong plot chunk count"
            );
            loaded = Some(data);
        }
        let chunk = if let Some(data) = &loaded {
            let x = coord.0.rem_euclid(PLOT_WIDTH);
            let z = coord.1.rem_euclid(PLOT_WIDTH);
            Chunk::load(
                coord.0,
                coord.1,
                data.chunk_data[(x * PLOT_WIDTH + z) as usize].clone(),
            )
        } else {
            Plot::generate_chunk(8, coord.0, coord.1)
        };
        let packet = Arc::new(chunk.encode_packet_for_client(true));
        cache.insert(coord, version.clone(), packet.clone());
        result.push((coord, packet));
    }
    Ok(result)
}

struct Work {
    request: Request,
    packets: Response,
    next: usize,
}

pub(super) struct LiveSource {
    plot: Coord,
    requests: Receiver<Request>,
    registration: u64,
    work: Option<Work>,
    cache: Cache,
}

impl LiveSource {
    pub fn register(plot: Coord) -> Self {
        let (sender, requests) = mpsc::sync_channel(32);
        let registration = SERVICE.next_source.fetch_add(1, Ordering::Relaxed);
        SERVICE
            .sources
            .lock()
            .unwrap()
            .insert(plot, (registration, sender));
        Self {
            plot,
            requests,
            registration,
            work: None,
            cache: Cache::new(LIVE_CACHE_BYTES),
        }
    }

    pub fn poll(&mut self, chunks: &[Chunk]) {
        if self.work.is_none() {
            let Ok(request) = self.requests.try_recv() else {
                return;
            };
            self.work = Some(Work {
                request,
                packets: Vec::new(),
                next: 0,
            });
        }
        let started = Instant::now();
        let work = self.work.as_mut().unwrap();
        // Keep a large initial view from monopolizing the simulation thread.
        for _ in 0..8 {
            let Some(&coord) = work.request.chunks.get(work.next) else {
                break;
            };
            work.next += 1;
            if plot_for_chunk(coord) != self.plot {
                continue;
            }
            let index =
                coord.0.rem_euclid(PLOT_WIDTH) * PLOT_WIDTH + coord.1.rem_euclid(PLOT_WIDTH);
            let Some(chunk) = chunks.get(index as usize) else {
                continue;
            };
            let version = Version::Live(chunk.snapshot_version());
            let packet = self.cache.get(coord, &version).unwrap_or_else(|| {
                let packet = Arc::new(chunk.encode_packet_for_client(true));
                self.cache.insert(coord, version, packet.clone());
                packet
            });
            work.packets.push((coord, packet));
            if started.elapsed() >= Duration::from_millis(5) {
                break;
            }
        }
        if work.next == work.request.chunks.len() {
            let work = self.work.take().unwrap();
            let _ = work.request.reply.try_send(work.packets);
        }
    }
}

impl Drop for LiveSource {
    fn drop(&mut self) {
        let mut sources = SERVICE.sources.lock().unwrap();
        // An old plot can finish dropping just after its replacement registers.
        if sources
            .get(&self.plot)
            .is_some_and(|(id, _)| *id == self.registration)
        {
            sources.remove(&self.plot);
        }
    }
}

struct Slot {
    generation: u64,
    requested: bool,
    packet: Option<Snapshot>,
}

struct Pending {
    response: Receiver<Response>,
    recipients: HashMap<Coord, Vec<(u128, u64)>>,
    started: Instant,
}

#[derive(Default)]
pub(super) struct Views {
    players: HashMap<u128, HashMap<Coord, Slot>>,
    pending: HashMap<Coord, Pending>,
    last_request: HashMap<Coord, Instant>,
    generation: u64,
    dirty: bool,
    next_poll: Option<Instant>,
}

impl Views {
    pub fn load(&mut self, player: u128, chunk: Coord) {
        self.dirty = true;
        self.generation += 1;
        self.players.entry(player).or_default().insert(
            chunk,
            Slot {
                generation: self.generation,
                requested: false,
                packet: None,
            },
        );
    }

    pub fn unload(&mut self, player: u128, chunk: Coord) {
        self.dirty = true;
        if let Some(chunks) = self.players.get_mut(&player) {
            chunks.remove(&chunk);
        }
    }

    pub fn remove_player(&mut self, player: u128) {
        self.dirty = true;
        self.players.remove(&player);
    }

    pub fn packet(&self, player: u128, chunk: Coord) -> Option<Snapshot> {
        self.players.get(&player)?.get(&chunk)?.packet.clone()
    }

    fn update(
        &mut self,
        now: Instant,
        interval: Duration,
        mut request: impl FnMut(Request),
    ) -> Vec<(u128, Snapshot)> {
        self.dirty = false;
        let mut deliveries = Vec::new();
        self.pending
            .retain(|plot, pending| match pending.response.try_recv() {
                Ok(packets) => {
                    self.last_request.insert(*plot, now);
                    for (coord, packet) in packets {
                        for &(player, generation) in
                            pending.recipients.get(&coord).into_iter().flatten()
                        {
                            let Some(slot) = self
                                .players
                                .get_mut(&player)
                                .and_then(|chunks| chunks.get_mut(&coord))
                            else {
                                continue;
                            };
                            if slot.generation != generation {
                                continue;
                            }
                            let changed = slot.packet.as_ref().is_none_or(|old| {
                                old.packet_id != packet.packet_id || old.buffer != packet.buffer
                            });
                            if changed {
                                slot.packet = Some(packet.clone());
                                deliveries.push((player, packet.clone()));
                            }
                        }
                    }
                    false
                }
                Err(TryRecvError::Disconnected) => false,
                Err(TryRecvError::Empty) => now.duration_since(pending.started) < REQUEST_TIMEOUT,
            });
        let mut interests: BTreeMap<Coord, HashMap<Coord, Vec<(u128, u64)>>> = BTreeMap::new();
        for (&player, chunks) in &self.players {
            for (&coord, slot) in chunks {
                interests
                    .entry(plot_for_chunk(coord))
                    .or_default()
                    .entry(coord)
                    .or_default()
                    .push((player, slot.generation));
            }
        }
        self.last_request
            .retain(|plot, _| interests.contains_key(plot));
        self.pending.retain(|plot, _| interests.contains_key(plot));
        for (plot, recipients) in interests {
            if self.pending.contains_key(&plot) {
                continue;
            }
            let initial = recipients.iter().any(|(coord, viewers)| {
                viewers
                    .iter()
                    .any(|(player, _)| !self.players[player][coord].requested)
            });
            if !initial
                && self
                    .last_request
                    .get(&plot)
                    .is_some_and(|&last| now.duration_since(last) < interval)
            {
                continue;
            }
            for (coord, viewers) in &recipients {
                for (player, _) in viewers {
                    self.players
                        .get_mut(player)
                        .unwrap()
                        .get_mut(coord)
                        .unwrap()
                        .requested = true;
                }
            }
            let (reply, response) = mpsc::sync_channel(1);
            let chunks = recipients.keys().copied().collect();
            self.pending.insert(
                plot,
                Pending {
                    response,
                    recipients,
                    started: now,
                },
            );
            self.last_request.insert(plot, now);
            request(Request {
                plot,
                chunks,
                reply,
            });
        }
        deliveries
    }
}

impl Plot {
    pub(super) fn update_neighbor_views(&mut self) {
        if let Some(source) = &mut self.neighbor_source {
            source.poll(&self.world.chunks);
        }
        let now = Instant::now();
        // Unlimited-TPS plots can execute this loop millions of times per second.
        // Poll viewers at 20 Hz, or immediately when visible chunks change.
        if !self.neighbor_views.dirty
            && self.neighbor_views.next_poll.is_some_and(|next| now < next)
        {
            return;
        }
        self.neighbor_views.next_poll = Some(now + Duration::from_millis(50));
        self.neighbor_views.players.retain(|uuid, _| {
            self.players
                .iter()
                .any(|player| player.uuid == *uuid && player.client.alive())
        });
        if CONFIG.neighbor_update_interval_ms == 0 {
            return;
        }
        let deliveries = self.neighbor_views.update(
            now,
            Duration::from_millis(CONFIG.neighbor_update_interval_ms),
            |request| SERVICE.request(request),
        );
        for (uuid, packet) in deliveries {
            if let Some(player) = self.players.iter().find(|player| player.uuid == uuid) {
                player.client.send_packet(&packet);
            }
        }
    }

    pub(super) fn restore_neighbor_chunk(&mut self, player: usize, pos: mchprs_blocks::BlockPos) {
        if let Some(packet) = self
            .neighbor_views
            .packet(self.players[player].uuid, (pos.x >> 4, pos.z >> 4))
        {
            self.players[player].client.send_packet(&packet);
        }
    }
}

#[cfg(test)]
mod tests;
