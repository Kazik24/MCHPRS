use super::*;
use mchprs_blocks::{BlockPos, blocks::Block};
use rustc_hash::FxHashMap;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::mem::size_of;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Probe {
    pub name: String,
    pub pos: BlockPos,
}
impl Probe {
    pub fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 32
            && name
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
    }
}
pub(crate) fn signal_value(world: &impl crate::world::World, pos: BlockPos) -> Option<bool> {
    let block = world.get_block(pos);
    match block {
        Block::Air {} => None,
        Block::RedstoneWire { wire } => Some(wire.power > 0),
        Block::RedstoneComparator { .. } => Some(crate::redstone::source_strength(block, world, pos) > 0),
        Block::RedstoneBlock => Some(true),
        Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } | Block::RedstoneLamp { lit } => Some(lit),
        _ => Some(if let Some(powered) = block.property("powered") {
            powered == "true"
        } else if let Some(power) = block.property("power") {
            power != "0"
        } else {
            crate::redstone::redstone_lamp_should_be_lit(world, pos)
        }),
    }
}
pub(crate) enum Kind {
    Signal(u16, Option<bool>),
    Text(BlockPos, String, usize),
}
pub(crate) struct Record {
    pub seq: u64,
    pub tick: u64,
    pub kind: Kind,
}
pub(crate) struct Capture {
    pub id: u128,
    pub ticks: u64,
    pub first: u64,
    pub observed: u64,
    pub completed: u64,
    pub partial: bool,
    pub state: &'static str,
    pub reason: Option<&'static str>,
    pub baseline_seq: u64,
    pub seq: u64,
    pub baseline: Vec<Option<bool>>,
    values: Vec<Option<bool>>,
    pub records: VecDeque<Record>,
    pub reserved: usize,
    text_bytes: usize,
    text_count: usize,
    overflow_notified: bool,
    _reservation: Reservation,
}
#[derive(Default)]
pub(crate) struct Recorder {
    pub probes: Vec<Probe>,
    index: FxHashMap<BlockPos, u16>,
    pub(crate) capture: Option<Capture>,
    pub admitted_signals: usize,
}
impl Recorder {
    pub fn enabled(&self) -> bool {
        self.capture.as_ref().is_some_and(|c| c.state == "recording")
    }
    pub fn off(&mut self) {
        self.capture = None;
    }
    pub fn discontinuity(&mut self, tick: u64, partial: bool, values: Vec<Option<bool>>) {
        if let Some(c) = &mut self.capture {
            c.id = rand::random();
            c.records.clear();
            c.text_bytes = 0;
            c.text_count = 0;
            c.seq = 0;
            c.baseline_seq = 0;
            c.first = tick;
            c.observed = tick;
            c.completed = if partial { tick.saturating_sub(1) } else { tick };
            c.partial = partial;
            c.baseline = values.clone();
            c.values = values;
            c.reason = Some("world_replaced");
        }
    }
    pub fn take_overflow_notice(&mut self) -> bool {
        let Some(c) = self.capture.as_mut().filter(|c| c.state == "overflow" && !c.overflow_notified) else {
            return false;
        };
        c.overflow_notified = true;
        true
    }
    pub fn stop(&mut self, reason: &'static str) {
        if let Some(c) = self.capture.as_mut().filter(|c| c.state == "recording") {
            c.state = "stopped";
            c.reason = Some(reason);
        }
    }
    pub fn replace(&mut self, probes: Vec<Probe>) {
        self.index = probes.iter().enumerate().map(|(i, p)| (p.pos, i as u16)).collect();
        self.probes = probes;
    }
    pub fn on(&mut self, ticks: u64, tick: u64, values: Vec<Option<bool>>, limit: usize) -> Result<(), String> {
        if ticks == 0 || ticks > CONFIG.waveform_max_ticks as u64 || values.len() != self.probes.len() || self.probes.len() > limit {
            return Err("Capture exceeds your limits; use /waveform status.".into());
        }
        let bytes = mib(CONFIG.waveform_plot_memory_mib);
        let reservation = reserve(&STORED, bytes)?;
        let overhead = mib(CONFIG.waveform_text_memory_mib) + CONFIG.waveform_max_signals as usize * 256;
        let capacity = (CONFIG.waveform_max_records as usize).min(bytes.saturating_sub(overhead) / size_of::<Record>());
        let mut records = VecDeque::new();
        records
            .try_reserve_exact(capacity)
            .map_err(|_| "Cannot allocate the capture; try again later.")?;
        if records.capacity() * size_of::<Record>() + overhead > bytes || capacity == 0 {
            return Err("Capture exceeds the memory budget; ask staff.".into());
        }
        self.capture = Some(Capture {
            id: rand::random(),
            ticks,
            first: tick,
            observed: tick,
            completed: tick,
            partial: false,
            state: "recording",
            reason: None,
            baseline_seq: 0,
            seq: 0,
            baseline: values.clone(),
            values,
            records,
            reserved: bytes,
            text_bytes: 0,
            text_count: 0,
            overflow_notified: false,
            _reservation: reservation,
        });
        self.admitted_signals = limit;
        Ok(())
    }
    pub fn tick(&mut self, tick: u64, partial: bool) {
        let Some(c) = self.capture.as_mut().filter(|c| c.state == "recording") else {
            return;
        };
        c.observed = tick;
        c.partial = partial;
        c.completed = if partial { tick.saturating_sub(1) } else { tick };
        let first = c.first.max(tick.saturating_sub(c.ticks - 1));
        while c.records.front().is_some_and(|r| r.tick < first) {
            let record = c.records.pop_front().unwrap();
            c.baseline_seq = record.seq;
            match record.kind {
                Kind::Signal(id, value) => c.baseline[id as usize] = value,
                Kind::Text(_, text, _) => {
                    c.text_bytes -= text.capacity();
                    c.text_count -= 1;
                }
            }
        }
        c.first = first;
    }
    pub fn affected(&self, pos: BlockPos) -> smallvec::SmallVec<[BlockPos; 8]> {
        if !self.enabled() {
            return smallvec::SmallVec::new();
        }
        // Incoming power can travel through one neighboring solid block.
        static OFFSETS: Lazy<Vec<BlockPos>> = Lazy::new(|| {
            (-2i32..=2)
                .flat_map(|x| {
                    (-2i32..=2).flat_map(move |y| {
                        (-2i32..=2)
                            .filter(move |z| x.abs() + y.abs() + z.abs() <= 2)
                            .map(move |z| BlockPos::new(x, y, z))
                    })
                })
                .collect()
        });
        OFFSETS
            .iter()
            .map(|offset| BlockPos::new(pos.x + offset.x, pos.y + offset.y, pos.z + offset.z))
            .filter(|candidate| self.index.contains_key(candidate))
            .collect()
    }
    pub fn changed(&mut self, pos: BlockPos, value: Option<bool>, tick: u64) {
        if !self.enabled() {
            return;
        }
        let Some(&id) = self.index.get(&pos) else {
            return;
        };
        if self.capture.as_ref().unwrap().values[id as usize] == value {
            return;
        }
        if self.push(tick, Kind::Signal(id, value)) {
            self.capture.as_mut().unwrap().values[id as usize] = value;
        }
    }
    pub fn text(&mut self, pos: BlockPos, message: &str, tick: u64) {
        if !self.enabled() {
            return;
        }
        let Ok(component) = serde_json::from_str(message) else {
            return;
        };
        let plain = crate::redstone::command_block::plain_text(&component);
        static FORMAT: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new("(?i)§[0-9a-fk-orx]").unwrap());
        let plain = FORMAT.replace_all(&plain, "");
        let original = plain.len();
        let mut end = original.min(TEXT_BYTES);
        while !plain.is_char_boundary(end) {
            end -= 1;
        }
        let text = plain[..end].to_owned();
        self.push(tick, Kind::Text(pos, text, if end < original { original } else { 0 }));
    }
    fn push(&mut self, tick: u64, kind: Kind) -> bool {
        let c = self.capture.as_mut().unwrap();
        let (bytes, count) = match &kind {
            Kind::Text(_, text, _) => (text.capacity(), 1),
            _ => (0, 0),
        };
        if c.records.len() >= CONFIG.waveform_max_records as usize
            || c.records.len() == c.records.capacity()
            || c.seq == u64::MAX
            || c.text_count + count > CONFIG.waveform_max_text_events as usize
            || c.text_bytes + bytes > mib(CONFIG.waveform_text_memory_mib)
        {
            c.state = "overflow";
            c.reason = Some("overflow");
            tracing::warn!("Waveform capture stopped: buffer full");
            return false;
        }
        c.seq += 1;
        c.text_bytes += bytes;
        c.text_count += count;
        c.records.push_back(Record { seq: c.seq, tick, kind });
        true
    }
    pub fn metadata(&self, source: u128, unsupported: bool) -> Value {
        let c = self.capture.as_ref();
        json!({"source":format!("{source:x}"),"capture":c.map(|c| format!("{:x}",c.id)),
            "state":if unsupported {"unsupported_backend"} else {c.map_or(if self.probes.is_empty() {"empty"}else{"disabled"},|c|c.state)},
            "reason":c.and_then(|c|c.reason),"signals":self.schema(),
            "limits":{"signals":self.admitted_signals,"ticks":c.map(|c|c.ticks)},
            "usage":{"reserved_bytes":c.map_or(0,|c|c.reserved),"records":c.map_or(0,|c|c.records.len()),"text_events":c.map_or(0,|c|c.text_count)},
            "window":c.map(|c|json!({"first_tick":c.first.to_string(),"observed_tick":c.observed.to_string(),"completed_tick":c.completed.to_string(),"partial_tick":c.partial}))})
    }
    fn schema(&self) -> Vec<Value> {
        self.probes.iter().map(|p| json!([p.name, [p.pos.x, p.pos.y, p.pos.z]])).collect()
    }

    pub(crate) fn page(&self, plot: (i32, i32), source: u128, request: &api::Query, unsupported: bool) -> Result<Value, api::Error> {
        let Some(c) = &self.capture else {
            if request.cursor.is_some() {
                return Err(api::Error::new(409, "capture_changed"));
            }
            let mut value = self.metadata(source, unsupported);
            value["ticks"] = json!([]);
            return Ok(value);
        };
        let mut cursor = match &request.cursor {
            Some(cursor) => cursor.clone(),
            None => api::Cursor {
                plot,
                source,
                capture: c.id,
                after: c.baseline_seq,
                through: Some(c.seq),
                first: c.first,
                observed: c.observed,
                completed: c.completed,
                partial: c.partial,
            },
        };
        if cursor.plot != plot {
            return Err(api::Error::new(400, "invalid_cursor"));
        }
        if cursor.source != source {
            return Err(api::Error::new(409, "source_changed"));
        }
        if cursor.capture != c.id {
            return Err(api::Error::new(409, "capture_changed"));
        }
        if cursor.after < c.baseline_seq || cursor.through.is_some() && cursor.first < c.first {
            return Err(api::Error::new(409, "history_expired"));
        }
        if cursor.after > c.seq || cursor.through.is_some_and(|s| s < cursor.after || s > c.seq) {
            return Err(api::Error::new(400, "invalid_cursor"));
        }
        let through = cursor.through.unwrap_or(c.seq);
        let base = cursor.after;
        let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
        let mut bytes = if request.cursor.is_none() { 128 * 1024 } else { 4096 };
        let start = usize::try_from(base - c.baseline_seq).map_err(|_| api::Error::new(400, "invalid_cursor"))?;
        for record in c.records.iter().skip(start).take(request.limit) {
            if record.seq > through {
                break;
            }
            let cost = match &record.kind {
                Kind::Text(_, t, _) => t.len() * 6 + 256,
                _ => 128,
            };
            if bytes + cost > PAGE_BYTES {
                break;
            }
            bytes += cost;
            let entry = match &record.kind {
                Kind::Signal(id, value) => json!([record.seq - base, 0, id, value.map(u8::from)]),
                Kind::Text(pos, text, original) => json!([record.seq - base, 1, pos.x, pos.y, pos.z, text, original]),
            };
            if groups.last().is_none_or(|(tick, _)| *tick != record.tick.to_string()) {
                groups.push((record.tick.to_string(), Vec::new()));
            }
            groups.last_mut().unwrap().1.push(entry);
            cursor.after = record.seq;
        }
        let more = cursor.after < through;
        let window = if cursor.through.is_some() {
            json!({"first_tick":cursor.first.to_string(),"observed_tick":cursor.observed.to_string(),"completed_tick":cursor.completed.to_string(),"partial_tick":cursor.partial})
        } else {
            json!({"first_tick":c.first.to_string(),"observed_tick":c.observed.to_string(),"completed_tick":c.completed.to_string(),"partial_tick":c.partial})
        };
        let snapshot = request.cursor.is_none().then(|| {
            json!({"signals":self.schema(),"baseline":c.baseline.iter().map(|v|v.map(u8::from)).collect::<Vec<_>>(),
            "baseline_tick":c.first.to_string(),"baseline_seq":c.baseline_seq.to_string(),"through_seq":through.to_string()})
        });
        if !more {
            cursor.through = None;
        }
        Ok(
            json!({"source":format!("{source:x}"),"capture":format!("{:x}",c.id),"state":if unsupported {"unsupported_backend"} else {c.state},
            "reason":c.reason,"window":window,"snapshot":snapshot,"baseline_seq":c.baseline_seq.to_string(),"seq_base":base.to_string(),"ticks":groups,"more":more,"cursor":cursor.encode()}),
        )
    }
}
