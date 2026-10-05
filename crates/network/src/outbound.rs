//! One ordered writer per connection. Only adjacent visual updates may coalesce.
use crate::packets::clientbound::{
    C3BMultiBlockChangeRecord, CMultiBlockChange, ClientBoundPacket,
};
use crate::packets::PacketEncoder;
use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MAX_QUEUED_BYTES: usize = 128 * 1024 * 1024;
const MAX_QUEUED_ITEMS: usize = 16_384;
type Section = (i32, u32, i32);
type Blocks = BTreeMap<Section, BTreeMap<u16, u32>>;

#[derive(Debug, Default, Clone, Copy)]
pub struct SendStats {
    pub packets: u64,
    pub bytes: u64,
    pub encode_ns: u64,
    pub compress_ns: u64,
    pub write_ns: u64,
    pub coalesced_blocks: u64,
    pub queued_bytes: usize,
    pub failures: u64,
}

#[derive(Debug, Default)]
struct Counters {
    packets: AtomicU64,
    bytes: AtomicU64,
    encode_ns: AtomicU64,
    compress_ns: AtomicU64,
    write_ns: AtomicU64,
    coalesced_blocks: AtomicU64,
    failures: AtomicU64,
}

#[derive(Debug)]
enum Payload {
    Packet(PacketEncoder),
    Blocks(Blocks),
}

#[derive(Debug)]
struct Item {
    compressed: bool,
    payload: Payload,
    bytes: usize,
}

#[derive(Debug, Default)]
struct Pending {
    items: VecDeque<Item>,
    bytes: usize,
    closed: bool,
}

#[derive(Debug, Default)]
struct Shared {
    pending: Mutex<Pending>,
    ready: Condvar,
    counters: Counters,
}

#[derive(Debug)]
struct Handle {
    shared: Arc<Shared>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        // The worker owns Shared, but never Handle: the last sender wakes it
        // to drain and exit rather than keeping a waiting thread alive forever.
        self.shared.pending.lock().unwrap().closed = true;
        self.shared.ready.notify_one();
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Outbound {
    handle: Arc<Handle>,
}

impl Outbound {
    pub(crate) fn new(stream: TcpStream) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        std::thread::spawn(move || {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            Self::run(&worker, &stream);
            let _ = stream.shutdown(Shutdown::Both);
        });
        Self {
            handle: Arc::new(Handle { shared }),
        }
    }

    pub(crate) fn packet(&self, packet: &PacketEncoder, compressed: bool) {
        let mut pending = self.handle.shared.pending.lock().unwrap();
        if pending.closed {
            return;
        }
        let bytes = packet.buffer.len() + 32;
        pending.bytes += bytes;
        pending.items.push_back(Item {
            compressed,
            payload: Payload::Packet(packet.clone()),
            bytes,
        });
        self.finish_enqueue(&mut pending);
    }

    pub(crate) fn blocks(&self, packet: &CMultiBlockChange, compressed: bool) {
        if packet.records.is_empty() {
            return;
        }
        let mut pending = self.handle.shared.pending.lock().unwrap();
        if pending.closed {
            return;
        }
        let merge = pending.items.back().is_some_and(|item| {
            item.compressed == compressed && matches!(item.payload, Payload::Blocks(_))
        });
        if !merge {
            pending.items.push_back(Item {
                compressed,
                payload: Payload::Blocks(Blocks::new()),
                bytes: 64,
            });
            pending.bytes += 64;
        }
        let item = pending.items.back_mut().unwrap();
        let Payload::Blocks(sections) = &mut item.payload else {
            unreachable!()
        };
        let records = sections
            .entry((packet.chunk_x, packet.chunk_y, packet.chunk_z))
            .or_default();
        let mut added = 0;
        for record in &packet.records {
            let position =
                (u16::from(record.x) << 8) | (u16::from(record.z) << 4) | u16::from(record.y);
            if records.insert(position, record.block_id).is_some() {
                self.handle
                    .shared
                    .counters
                    .coalesced_blocks
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                added += 32;
            }
        }
        item.bytes += added;
        pending.bytes += added;
        self.finish_enqueue(&mut pending);
    }

    fn finish_enqueue(&self, pending: &mut Pending) {
        if pending.bytes > MAX_QUEUED_BYTES || pending.items.len() > MAX_QUEUED_ITEMS {
            // Never block the simulator or silently leave a connected client
            // stale. Terminate a connection whose reliable queue cannot keep up.
            pending.closed = true;
            pending.items.clear();
            pending.bytes = 0;
            self.handle
                .shared
                .counters
                .failures
                .fetch_add(1, Ordering::Relaxed);
            tracing::warn!("Closing client: outbound queue exceeded its limit");
        }
        self.handle.shared.ready.notify_one();
    }

    pub(crate) fn close(&self) {
        self.handle.shared.pending.lock().unwrap().closed = true;
        self.handle.shared.ready.notify_one();
    }

    pub(crate) fn stats(&self) -> SendStats {
        let c = &self.handle.shared.counters;
        SendStats {
            packets: c.packets.load(Ordering::Relaxed),
            bytes: c.bytes.load(Ordering::Relaxed),
            encode_ns: c.encode_ns.load(Ordering::Relaxed),
            compress_ns: c.compress_ns.load(Ordering::Relaxed),
            write_ns: c.write_ns.load(Ordering::Relaxed),
            coalesced_blocks: c.coalesced_blocks.load(Ordering::Relaxed),
            failures: c.failures.load(Ordering::Relaxed),
            queued_bytes: self.handle.shared.pending.lock().unwrap().bytes,
        }
    }

    fn run(shared: &Shared, mut stream: impl Write) {
        loop {
            let item = {
                let mut pending = shared.pending.lock().unwrap();
                while pending.items.is_empty() && !pending.closed {
                    pending = shared.ready.wait(pending).unwrap();
                }
                let Some(item) = pending.items.pop_front() else {
                    return;
                };
                pending.bytes -= item.bytes;
                item
            };
            let packets = match item.payload {
                Payload::Packet(packet) => vec![packet],
                Payload::Blocks(sections) => {
                    let now = Instant::now();
                    let packets = sections
                        .into_iter()
                        .map(|((chunk_x, chunk_y, chunk_z), records)| {
                            CMultiBlockChange {
                                chunk_x,
                                chunk_y,
                                chunk_z,
                                records: records
                                    .into_iter()
                                    .map(|(pos, block_id)| C3BMultiBlockChangeRecord {
                                        x: (pos >> 8) as u8,
                                        y: (pos & 15) as u8,
                                        z: ((pos >> 4) & 15) as u8,
                                        block_id,
                                    })
                                    .collect(),
                            }
                            .encode()
                        })
                        .collect();
                    shared
                        .counters
                        .encode_ns
                        .fetch_add(now.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    packets
                }
            };
            for packet in packets {
                let now = Instant::now();
                let mut bytes = Vec::new();
                let result = if item.compressed {
                    packet.write_compressed(&mut bytes)
                } else {
                    packet.write_uncompressed(&mut bytes)
                };
                shared
                    .counters
                    .compress_ns
                    .fetch_add(now.elapsed().as_nanos() as u64, Ordering::Relaxed);
                let now = Instant::now();
                let result = result.and_then(|()| stream.write_all(&bytes));
                shared
                    .counters
                    .write_ns
                    .fetch_add(now.elapsed().as_nanos() as u64, Ordering::Relaxed);
                if result.is_err() {
                    shared.counters.failures.fetch_add(1, Ordering::Relaxed);
                    let mut pending = shared.pending.lock().unwrap();
                    pending.closed = true;
                    pending.items.clear();
                    pending.bytes = 0;
                    return;
                }
                shared.counters.packets.fetch_add(1, Ordering::Relaxed);
                shared
                    .counters
                    .bytes
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests;
