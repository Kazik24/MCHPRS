use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::headless::{CompilerOptions, Input, Session};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

const MAX_MESSAGE: u64 = 1_048_576;
const MAX_CELLS: i32 = 30_000;
const MAX_CLIENTS: usize = 4;

struct Connection {
    session: Option<Session>,
    size: [i32; 3],
    origin: BlockPos,
    hello: bool,
    compiled: bool,
}

fn pos(value: &Value, size: [i32; 3], origin: BlockPos) -> Result<BlockPos, String> {
    let a = value.as_array().filter(|a| a.len() == 3).ok_or("invalid position")?;
    let mut p = [0i32; 3];
    for i in 0..3 {
        p[i] = a[i]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .filter(|n| *n >= 0 && *n < size[i])
            .ok_or("position outside region")?;
    }
    Ok(origin + BlockPos::new(p[0], p[1], p[2]))
}

fn block_string(block: Block) -> String {
    let name = block.get_name();
    let mut props: Vec<_> = mchprs_blocks::generated::STATE_PROPERTIES[block.get_id() as usize].iter().copied().collect();
    props.sort_by(|a, b| a.0.cmp(b.0));
    if props.is_empty() {
        return format!("minecraft:{name}");
    }
    format!(
        "minecraft:{name}[{}]",
        props.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")
    )
}

fn changes(items: Vec<(BlockPos, u32)>, origin: BlockPos) -> Value {
    let mut positions = BTreeMap::new();
    for (p, state) in items {
        let p = p - origin;
        positions.insert((p.x, p.y, p.z), state);
    }
    let mut states = Vec::<String>::new();
    let mut indices = std::collections::HashMap::new();
    let changes: Vec<_> = positions
        .into_iter()
        .map(|((x, y, z), state)| {
            let i = *indices.entry(state).or_insert_with(|| {
                let i = states.len();
                states.push(block_string(Block::from_id(state)));
                i
            });
            json!([x, y, z, i])
        })
        .collect();
    json!({"states":states,"changes":changes})
}

impl Connection {
    fn handle(&mut self, request: &Value) -> Result<Value, String> {
        let op = request["op"].as_str().ok_or("missing operation")?;
        if op == "rawr" {
            if self.hello || request["protocol"].as_u64() != Some(3) {
                return Err("unsupported protocol or repeated handshake".into());
            }
            self.hello = true;
            return Ok(
                json!({"protocol":3,"engine":"mroww","version":env!("CARGO_PKG_VERSION"),"revision":option_env!("MROWW_API_REVISION").unwrap_or("unversioned"),"registry":"1.21.5","data_version":4325,"capabilities":{"schematic_load":true,"modes":["interpreter","compiled"]},"limits":{"message_bytes":MAX_MESSAGE,"cells":MAX_CELLS,"axis":64,"inputs":64,"ticks":64}}),
            );
        }
        if !self.hello {
            return Err("handshake required".into());
        }
        match op {
            "load_schematic" => {
                if self.session.is_some() {
                    return Err("session already loaded".into());
                }
                if request["format"].as_str() != Some("sponge-v3") { return Err("format must be sponge-v3".into()); }
                let encoded = request["schematic"].as_str().ok_or("missing schematic")?;
                if encoded.len() > 900_000 { return Err("encoded schematic exceeds limit".into()); }
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| "invalid schematic base64")?;
                if bytes.len() > 675_000 { return Err("compressed schematic exceeds limit".into()); }
                let (mut session, dims) = Session::from_sponge_schematic(&bytes, MAX_CELLS as u32).map_err(|e| format!("invalid schematic: {e:#}"))?;
                let origin = BlockPos::new(4, 4, 4);
                let mut baseline = Vec::new();
                for x in 0..dims[0] as i32 { for y in 0..dims[1] as i32 { for z in 0..dims[2] as i32 {
                    let p = origin + BlockPos::new(x, y, z);
                    baseline.push((p, Block::get_id(session.get(p).map_err(|e| e.to_string())?)));
                }}}
                if serde_json::to_vec(&changes(baseline, origin)).map_err(|e| e.to_string())?.len() + 64 > MAX_MESSAGE as usize {
                    return Err("full baseline exceeds response limit".into());
                }
                session.drain_changes().map_err(|e| e.to_string())?;
                self.session = Some(session);
                self.size = dims.map(|n| n as i32);
                self.origin = origin;
                Ok(json!({"size":dims}))
            }
            "compile" => {
                if self.compiled {
                    return Err("session already compiled".into());
                }
                let s = self.session.as_mut().ok_or("no session")?;
                match request["mode"].as_str() {
                    Some("interpreter") => {
                        self.compiled = true;
                        Ok(json!({"mode":"interpreter"}))
                    }
                    Some("compiled") => {
                        let level = request
                            .get("optimization")
                            .map(|value| value.as_u64().filter(|level| *level <= 6).ok_or("invalid optimization"))
                            .transpose()?
                            .unwrap_or(0);
                        s.compile(CompilerOptions {
                            optimization_level: Some(level as usize),
                            ..Default::default()
                        })
                        .map_err(|e| e.to_string())?;
                        self.compiled = true;
                        Ok(json!({"mode":"compiled"}))
                    }
                    _ => Err("unsupported mode".into()),
                }
            }
            "advance" => {
                if !self.compiled {
                    return Err("session is not compiled".into());
                }
                let ticks = request["ticks"].as_u64().filter(|n| *n <= 64).ok_or("invalid tick count")?;
                let inputs = request["inputs"].as_array().filter(|a| a.len() <= 64).ok_or("invalid inputs")?;
                let mut parsed = Vec::new();
                for input in inputs {
                    let p = pos(&input["pos"], self.size, self.origin)?;
                    match input["kind"].as_str() {
                        Some("use") => parsed.push(Input::Use(p)),
                        Some("plate") => parsed.push(Input::Plate(p, input["powered"].as_bool().ok_or("missing plate state")?)),
                        _ => return Err("unsupported input".into()),
                    }
                }
                let projection = match request.get("projection") {
                    None => false,
                    Some(value) => value.as_bool().ok_or("invalid projection flag")?,
                };
                let s = self.session.as_mut().ok_or("no session")?;
                s.advance_with_inputs(ticks, &parsed).map_err(|e| format!("session failed: {e}"))?;
                let mut result = json!({"tick":s.ticks()});
                if projection {
                    let c = changes(s.drain_changes().map_err(|e| e.to_string())?, self.origin);
                    result["states"] = c["states"].clone();
                    result["changes"] = c["changes"].clone();
                }
                Ok(result)
            }
            "project" => {
                let s = self.session.as_mut().ok_or("no session")?;
                let mut items = Vec::new();
                match request["kind"].as_str() {
                    Some("changes") => items = s.drain_changes().map_err(|e| e.to_string())?,
                    Some("baseline") => {
                        // Baseline owns the change stream through this point.
                        s.drain_changes().map_err(|e| e.to_string())?;
                        for x in 0..self.size[0] {
                            for y in 0..self.size[1] {
                                for z in 0..self.size[2] {
                                    let p = self.origin + BlockPos::new(x, y, z);
                                    let block = s.get(p).map_err(|e| e.to_string())?;
                                    items.push((p, Block::get_id(block)));
                                }
                            }
                        }
                    }
                    _ => return Err("invalid projection kind".into()),
                }
                let mut result = changes(items, self.origin);
                result["tick"] = json!(s.ticks());
                Ok(result)
            }
            "inspect" => {
                let p = pos(&request["pos"], self.size, self.origin)?;
                let s = self.session.as_ref().ok_or("no session")?;
                Ok(json!({"state":block_string(s.get(p).map_err(|e|e.to_string())?),"tick":s.ticks()}))
            }
            "close" => {
                self.session = None;
                Ok(json!({}))
            }
            _ => Err("unknown operation".into()),
        }
    }
}

fn serve(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut connection = Connection {
        session: None,
        size: [0; 3],
        origin: BlockPos::new(0, 0, 0),
        hello: false,
        compiled: false,
    };
    loop {
        let mut line = Vec::new();
        let count = reader.by_ref().take(MAX_MESSAGE + 1).read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if count as u64 > MAX_MESSAGE || line.last() != Some(&b'\n') {
            break;
        }
        let request: Value = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => break,
        };
        let Some(id) = request["id"].as_u64() else { break };
        let close = request["op"] == "close";
        let finished = Arc::new(AtomicBool::new(false));
        let watchdog_done = finished.clone();
        let watchdog_stream = stream.try_clone()?;
        let watchdog = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            while !watchdog_done.load(Ordering::Acquire) {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    let _ = watchdog_stream.shutdown(std::net::Shutdown::Both);
                    break;
                }
                std::thread::park_timeout(remaining);
            }
        });
        let result = connection.handle(&request);
        finished.store(true, Ordering::Release);
        watchdog.thread().unpark();
        let _ = watchdog.join();
        let (mut response, fatal) = match result {
            Ok(value) => (value, false),
            Err(error) => {
                let fatal = error.starts_with("session failed:");
                (json!({"error":error,"fatal":fatal}), fatal)
            }
        };
        response["id"] = json!(id);
        response["ok"] = json!(!response.get("error").is_some());
        let bytes = serde_json::to_vec(&response)?;
        if bytes.len() as u64 + 1 > MAX_MESSAGE {
            break;
        }
        stream.write_all(&bytes)?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        if close || fatal {
            break;
        }
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let address = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8765".into());
    let listener = TcpListener::bind(address)?;
    if !matches!(listener.local_addr()?.ip(), IpAddr::V4(ip) if ip.is_loopback()) {
        return Err(std::io::Error::other("loopback IPv4 required"));
    }
    eprintln!("MROWW RedGear API listening on {}", listener.local_addr()?);
    let active = Arc::new(AtomicUsize::new(0));
    for incoming in listener.incoming() {
        let stream = incoming?;
        if active
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < MAX_CLIENTS).then_some(n + 1))
            .is_err()
        {
            drop(stream);
            continue;
        }
        let active = active.clone();
        std::thread::spawn(move || {
            struct Lease(Arc<AtomicUsize>);
            impl Drop for Lease {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _lease = Lease(active);
            let _ = serve(stream);
        });
    }
    Ok(())
}
