use base64::Engine;
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::headless::{CompilerOptions, Input, Session};
use mchprs_core::redpilator::TaskMonitor;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

const MAX_MESSAGE: u64 = 16_777_216;
const MAX_CELLS: u32 = 30_000;
const MAX_CLIENTS: usize = 4;
const MAX_SESSIONS: usize = 4;
const MAX_TICKS: u64 = 1_000_000;
const MAX_INPUTS: usize = 64;
const MAX_AXIS: u32 = 64;
const BACKENDS: [(&str, &str); 3] = [("mroww-redpilator", "rust"), ("mroww-llvm", "llvm"), ("mroww-interpreter", "interpreter")];

fn executor_available(executor: &str) -> bool {
    match executor {
        "rust" | "interpreter" => true,
        // Matches the platform-specific native loader in redpilator/sampled/llvm/library.rs.
        "llvm" => cfg!(any(windows, target_os = "linux")),
        _ => false,
    }
}

fn capabilities() -> Value {
    json!({"recompile":false})
}

fn default_backend() -> &'static str {
    if executor_available("llvm") { "mroww-llvm" } else { "mroww-redpilator" }
}

fn describe(request: &Value) -> ApiResult<Value> {
    let protocol = request["protocol"]
        .as_u64()
        .ok_or_else(|| error("invalid_request", "protocol must be an unsigned integer"))?;
    if protocol != 4 {
        return Err(error("unsupported_protocol", "Protocol 4 is required"));
    }
    let backends: Vec<_> = BACKENDS
        .iter()
        .filter(|(_, executor)| executor_available(executor))
        .map(|(id, executor)| json!({"id":id,"executors":[executor],"capabilities":capabilities()}))
        .collect();
    let mut manifest = json!({
        "type":"manifest", "protocol":4, "engine":"mroww", "default_backend":default_backend(),
        "backends":backends,
        "limits":{"axis":MAX_AXIS,"cells":MAX_CELLS,"ticks":MAX_TICKS,"inputs":MAX_INPUTS,"message_bytes":MAX_MESSAGE}
    });
    identify(&mut manifest, request, None);
    Ok(manifest)
}

struct ApiError {
    code: &'static str,
    message: String,
}
type ApiResult<T> = Result<T, ApiError>;
fn error(code: &'static str, message: impl ToString) -> ApiError {
    ApiError {
        code,
        message: message.to_string(),
    }
}

struct Output {
    stream: Mutex<TcpStream>,
    outstanding: Mutex<HashSet<u64>>,
}
impl Output {
    fn send(&self, value: &Value, terminal: bool) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(value)?;
        let mut stream = self.stream.lock().unwrap();
        let mut outstanding = terminal.then(|| self.outstanding.lock().unwrap());
        let result = if bytes.len() as u64 + 1 > MAX_MESSAGE {
            Err(std::io::Error::other("API response exceeds limit"))
        } else {
            stream
                .write_all(&bytes)
                .and_then(|_| stream.write_all(b"\n"))
                .and_then(|_| stream.flush())
        };
        if result.is_err() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(outstanding) = outstanding.as_mut() {
            if let Some(id) = value["id"].as_u64() {
                outstanding.remove(&id);
            }
        }
        result
    }
    fn failure(&self, request: &Value, session: Option<&str>, e: ApiError, terminal: bool) {
        let mut value = json!({"type":"error", "code":e.code, "message":e.message});
        identify(&mut value, request, session);
        let _ = self.send(&value, terminal);
    }
}
fn identify(value: &mut Value, request: &Value, session: Option<&str>) {
    if let Some(id) = request.get("id") {
        value["id"] = id.clone();
    }
    if let Some(session) = session {
        value["session_id"] = json!(session);
    }
}
fn position(value: &Value, size: [u32; 3], origin: BlockPos) -> ApiResult<BlockPos> {
    let a = value
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or_else(|| error("invalid_input", "Position must have three coordinates"))?;
    let mut p = [0; 3];
    for i in 0..3 {
        p[i] = a[i]
            .as_u64()
            .filter(|n| *n < size[i] as u64)
            .ok_or_else(|| error("invalid_input", "Position is outside the schematic"))? as i32;
    }
    Ok(origin + BlockPos::new(p[0], p[1], p[2]))
}
fn block_string(block: Block) -> String {
    let name = block.get_name();
    let mut props: Vec<_> = mchprs_blocks::generated::STATE_PROPERTIES[block.get_id() as usize]
        .iter()
        .copied()
        .collect();
    props.sort_by(|a, b| a.0.cmp(b.0));
    if props.is_empty() {
        return format!("minecraft:{name}");
    }
    let properties: Vec<_> = props.iter().map(|(key, value)| format!("{key}={value}")).collect();
    format!("minecraft:{name}[{}]", properties.join(","))
}
fn changes(items: Vec<(BlockPos, u32)>, origin: BlockPos) -> Value {
    let mut positions = BTreeMap::new();
    for (p, state) in items {
        let p = p - origin;
        positions.insert((p.x, p.y, p.z), state);
    }
    let mut states = Vec::<String>::new();
    let mut indices = HashMap::new();
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

struct Release {
    position: BlockPos,
    tick: u64,
}
struct Simulation {
    session: Session,
    size: [u32; 3],
    origin: BlockPos,
    original: HashMap<BlockPos, u32>,
    reference: HashMap<BlockPos, u32>,
    releases: Vec<Release>,
}
impl Simulation {
    fn projection(&mut self, baseline: bool) -> ApiResult<(Value, HashMap<BlockPos, u32>)> {
        let reference = if baseline { &self.original } else { &self.reference };
        let mut current = HashMap::new();
        let mut changed = Vec::new();
        for x in 0..self.size[0] as i32 {
            for y in 0..self.size[1] as i32 {
                for z in 0..self.size[2] as i32 {
                    let p = self.origin + BlockPos::new(x, y, z);
                    let state = self.session.get(p).map_err(|e| error("execution_failed", e))?.get_id();
                    if state != reference.get(&p).copied().unwrap_or(Block::Air.get_id()) {
                        changed.push((p, state));
                    }
                    if state != Block::Air.get_id() {
                        current.insert(p, state);
                    }
                }
            }
        }
        self.session.drain_changes().map_err(|e| error("execution_failed", e))?;
        Ok((changes(changed, self.origin), current))
    }
    fn advance(&mut self, request: &Value, monitor: &TaskMonitor) -> ApiResult<()> {
        let ticks = request["ticks"]
            .as_u64()
            .filter(|n| *n <= MAX_TICKS)
            .ok_or_else(|| error("invalid_request", "Invalid tick count or request limit exceeded"))?;
        let end = self
            .session
            .ticks()
            .checked_add(ticks)
            .ok_or_else(|| error("invalid_request", "Tick count overflow"))?;
        let inputs = request["inputs"]
            .as_array()
            .filter(|a| a.len() <= MAX_INPUTS)
            .ok_or_else(|| error("invalid_input", "Inputs must be an array of at most 64 actions"))?;
        let mut parsed = Vec::new();
        let mut actions = Vec::new();
        for input in inputs {
            let p = position(&input["pos"], self.size, self.origin)?;
            let mut release = None;
            let action = match input.get("kind") {
                Some(kind) => {
                    if input.get("power").is_some() || input.get("duration_ticks").is_some() {
                        return Err(error("invalid_input", "Do not mix kind with power or duration_ticks"));
                    }
                    match kind.as_str() {
                        Some("use") => Input::Use(p),
                        Some("plate") => Input::Plate(
                            p,
                            input["powered"]
                                .as_bool()
                                .ok_or_else(|| error("invalid_input", "Missing Boolean plate state"))?,
                        ),
                        _ => return Err(error("invalid_input", "Unsupported input kind")),
                    }
                }
                None => {
                    let power = match input.get("power") {
                        None => 15,
                        Some(v) => v
                            .as_u64()
                            .filter(|n| *n <= 15)
                            .ok_or_else(|| error("invalid_input", "Power must be an integer from 0 to 15"))?,
                    };
                    if let Some(duration) = input.get("duration_ticks") {
                        let duration = duration
                            .as_u64()
                            .filter(|n| *n > 0)
                            .ok_or_else(|| error("invalid_input", "Duration must be a positive integer"))?;
                        let due = self
                            .session
                            .ticks()
                            .checked_add(duration)
                            .ok_or_else(|| error("invalid_input", "Release tick overflow"))?;
                        if power > 0 {
                            release = Some(due);
                        }
                    }
                    Input::Power(p, power > 0)
                }
            };
            parsed.push(action);
            actions.push((p, release));
        }
        self.session.validate_inputs(&parsed).map_err(|e| error("invalid_input", e))?;
        for (action, (p, release)) in parsed.into_iter().zip(actions) {
            self.session.advance_with_inputs(0, &[action]).map_err(|e| error("execution_failed", e))?;
            self.releases.retain(|r| r.position != p);
            if let Some(tick) = release {
                self.releases.push(Release { position: p, tick });
            }
        }
        while self.session.ticks() < end {
            if monitor.cancelled() {
                return Err(error("execution_failed", "Session was closed"));
            }
            let boundary = self.releases.iter().map(|r| r.tick).min().unwrap_or(end).min(end);
            self.session
                .advance((boundary - self.session.ticks()).min(16))
                .map_err(|e| error("execution_failed", e))?;
            let now = self.session.ticks();
            let mut remaining = Vec::new();
            for release in self.releases.drain(..) {
                if release.tick <= now {
                    self.session
                        .set_power(release.position, false)
                        .map_err(|e| error("execution_failed", e))?;
                } else {
                    remaining.push(release);
                }
            }
            self.releases = remaining;
        }
        Ok(())
    }
    fn handle(&mut self, request: &Value, monitor: &TaskMonitor) -> ApiResult<(Value, Option<HashMap<BlockPos, u32>>)> {
        let mut reference = None;
        let mut value = match request["op"].as_str() {
            Some("advance") => {
                let projection = match request.get("projection") {
                    None => false,
                    Some(v) => v.as_bool().ok_or_else(|| error("invalid_request", "Invalid projection flag"))?,
                };
                self.advance(request, monitor)?;
                if projection {
                    let (v, r) = self.projection(false)?;
                    reference = Some(r);
                    v
                } else {
                    json!({})
                }
            }
            Some("project") => {
                let baseline = match request["kind"].as_str() {
                    Some("baseline") => true,
                    Some("changes") => false,
                    _ => return Err(error("invalid_request", "Invalid projection kind")),
                };
                let (v, r) = self.projection(baseline)?;
                reference = Some(r);
                v
            }
            // shortcut: advertise recompilation only once pending runtime state can be imported atomically.
            Some("recompile") => return Err(error("unsupported_operation", "This backend does not support atomic recompilation")),
            _ => return Err(error("invalid_request", "Unknown operation")),
        };
        value["type"] = json!("result");
        value["tick"] = json!(self.session.ticks());
        Ok((value, reference))
    }
}

struct Begin {
    encoded: String,
    options: CompilerOptions,
    executor: &'static str,
}
fn parse_begin(request: &Value) -> ApiResult<Begin> {
    if request["protocol"].as_u64() != Some(4) {
        return Err(error("unsupported_protocol", "Protocol 4 is required"));
    }
    if request["format"].as_str() != Some("sponge-v3") {
        return Err(error("invalid_request", "Format must be sponge-v3"));
    }
    let encoded = request["schematic"]
        .as_str()
        .ok_or_else(|| error("invalid_request", "Missing Base64 schematic"))?
        .to_owned();
    let settings = request["settings"]
        .as_str()
        .ok_or_else(|| error("invalid_settings", "Settings must be a string"))?;
    let mut options = CompilerOptions::parse(settings).map_err(|e| error("invalid_settings", e))?;
    let executor = match request["backend"].as_str() {
        Some("redpilator") => {
            if options.llvm {
                "llvm"
            } else {
                "rust"
            }
        }
        Some("mroww-redpilator") => {
            if options.llvm {
                return Err(error("invalid_settings", "mroww-redpilator conflicts with --llvm"));
            }
            "rust"
        }
        Some("mroww-llvm") => {
            options.llvm = true;
            "llvm"
        }
        Some("mroww-interpreter" | "mroww-interpteter") => {
            if !settings.trim().is_empty() {
                return Err(error("invalid_settings", "The interpreter does not accept compiler settings"));
            }
            "interpreter"
        }
        _ => return Err(error("invalid_request", "Unknown backend selector")),
    };
    if !executor_available(executor) {
        return Err(error("unsupported_backend", "Selected backend is unavailable in this build"));
    }
    Ok(Begin { encoded, options, executor })
}
enum Command {
    Request(Value),
    Close(Value),
    Disconnect,
}
struct Worker {
    sender: mpsc::SyncSender<Command>,
    monitor: Arc<TaskMonitor>,
    compiling: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    closing: AtomicBool,
    thread: std::thread::JoinHandle<()>,
}
fn compile(begin: Begin, monitor: Arc<TaskMonitor>) -> ApiResult<Simulation> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&begin.encoded)
        .map_err(|_| error("invalid_schematic", "Invalid schematic Base64"))?;
    let (mut session, size) = Session::from_sponge_schematic(&bytes, MAX_CELLS).map_err(|e| error("invalid_schematic", format!("{e:#}")))?;
    if size.iter().any(|axis| *axis > MAX_AXIS) {
        return Err(error("invalid_schematic", "Schematic exceeds the axis limit"));
    }
    let original: HashMap<_, _> = session.original_blocks().iter().copied().collect();
    if monitor.cancelled() {
        return Err(error("cancelled", "Compilation cancelled"));
    }
    if begin.executor != "interpreter" {
        session
            .compile_with_monitor(begin.options, monitor.clone())
            .map_err(|e| error(if monitor.cancelled() { "cancelled" } else { "compile_failed" }, e))?;
    }
    Ok(Simulation {
        session,
        size,
        origin: BlockPos::new(4, 4, 4),
        reference: original.clone(),
        original,
        releases: Vec::new(),
    })
}
fn spawn_worker(session: String, request: Value, begin: Begin, output: Arc<Output>) -> Worker {
    let (sender, receiver) = mpsc::sync_channel(64);
    let monitor = Arc::new(TaskMonitor::default());
    let compiling = Arc::new(AtomicBool::new(true));
    let alive = Arc::new(AtomicBool::new(true));
    let worker_monitor = monitor.clone();
    let worker_compiling = compiling.clone();
    let worker_alive = alive.clone();
    let thread = std::thread::spawn(move || {
        struct Lifetime(Arc<AtomicBool>);
        impl Drop for Lifetime {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _lifetime = Lifetime(worker_alive.clone());
        let executor = begin.executor;
        let (stop, progress_stopped) = mpsc::channel();
        let progress_output = output.clone();
        let progress_session = session.clone();
        let progress_monitor = worker_monitor.clone();
        let progress = std::thread::spawn(move || {
            loop {
                if progress_output.send(&json!({"type":"progress", "session_id":progress_session, "phase":"compiling", "description":"Preparing simulation", "details":progress_monitor.status()}), false).is_err() { progress_monitor.cancel(); break; }
                if progress_stopped.recv_timeout(Duration::from_secs(1)) != Err(mpsc::RecvTimeoutError::Timeout) {
                    break;
                }
            }
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compile(begin, worker_monitor.clone())))
            .unwrap_or_else(|_| Err(error("compile_failed", "Compilation worker panicked")));
        let _ = stop.send(());
        let _ = progress.join();
        let mut simulation = match result {
            Ok(simulation) if !worker_monitor.cancelled() => {
                let mut finished = json!({"type":"finished", "tick":0, "backend":executor});
                identify(&mut finished, &request, Some(&session));
                worker_compiling.store(false, Ordering::Release);
                if output.send(&finished, true).is_err() {
                    return;
                }
                Some(simulation)
            }
            result => {
                let e = if worker_monitor.cancelled() {
                    error("cancelled", "Compilation cancelled")
                } else {
                    result
                        .err()
                        .unwrap_or_else(|| error("compile_failed", "Compilation did not produce a session"))
                };
                worker_alive.store(false, Ordering::Release);
                output.failure(&request, Some(&session), e, true);
                None
            }
        };
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Disconnect => break,
                Command::Close(request) => {
                    drop(simulation.take());
                    worker_alive.store(false, Ordering::Release);
                    let mut result = json!({"type":"result", "state":"closed"});
                    identify(&mut result, &request, Some(&session));
                    let _ = output.send(&result, true);
                    break;
                }
                Command::Request(request) => {
                    let Some(current) = simulation.as_mut() else {
                        output.failure(&request, Some(&session), error("unknown_session", "Session has ended"), true);
                        continue;
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| current.handle(&request, &worker_monitor)))
                        .unwrap_or_else(|_| Err(error("execution_failed", "Simulation worker panicked")));
                    match result {
                        Ok((mut value, reference)) => {
                            identify(&mut value, &request, Some(&session));
                            if output.send(&value, true).is_err() {
                                break;
                            }
                            if let Some(reference) = reference {
                                current.reference = reference;
                            }
                        }
                        Err(e) => {
                            let fatal = e.code == "execution_failed";
                            output.failure(&request, Some(&session), e, true);
                            if fatal {
                                simulation = None;
                                worker_alive.store(false, Ordering::Release);
                            }
                        }
                    }
                }
            }
        }
    });
    Worker {
        sender,
        monitor,
        compiling,
        alive,
        closing: AtomicBool::new(false),
        thread,
    }
}
fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<Value>> {
    let mut line = Vec::new();
    let count = reader.by_ref().take(MAX_MESSAGE + 1).read_until(b'\n', &mut line)?;
    if count == 0 {
        return Ok(None);
    }
    if count as u64 > MAX_MESSAGE || line.last() != Some(&b'\n') {
        return Err(std::io::Error::other("Oversized or incomplete API message"));
    }
    let request: Value = serde_json::from_slice(&line)?;
    if !request.is_object() {
        return Err(std::io::Error::other("Expected a JSON object"));
    }
    Ok(Some(request))
}
fn serve(stream: TcpStream) -> std::io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let output = Arc::new(Output {
        stream: Mutex::new(stream),
        outstanding: Mutex::new(HashSet::new()),
    });
    let mut workers: HashMap<String, Worker> = HashMap::new();
    let mut next_session = 1u64;
    let mut begun = false;
    let result = (|| -> std::io::Result<()> {
        while let Some(request) = read_request(&mut reader)? {
            workers.retain(|_, worker| worker.alive.load(Ordering::Acquire));
            if request.get("id").is_some_and(|v| v.as_u64().is_none()) {
                output.failure(&json!({}), None, error("invalid_request", "id must be an unsigned integer"), false);
                continue;
            }
            if let Some(id) = request["id"].as_u64() {
                let inserted = output.outstanding.lock().unwrap().insert(id);
                if !inserted {
                    output.failure(&request, None, error("invalid_request", "Request id is already outstanding"), false);
                    continue;
                }
            }
            if request["op"] == "describe" {
                match describe(&request) {
                    Ok(manifest) => output.send(&manifest, true)?,
                    Err(e) => output.failure(&request, None, e, true),
                }
                continue;
            }
            if request["op"] == "begin" {
                let begin = match parse_begin(&request) {
                    Ok(v) => v,
                    Err(e) => {
                        output.failure(&request, None, e, true);
                        continue;
                    }
                };
                if workers.len() >= MAX_SESSIONS {
                    output.failure(&request, None, error("resource_limit", "Connection session limit reached"), true);
                    continue;
                }
                let session = format!("s{next_session}");
                next_session += 1;
                let mut accepted = json!({"type":"accepted", "protocol":4, "engine":"mroww", "capabilities":capabilities()});
                identify(&mut accepted, &request, Some(&session));
                output.send(&accepted, false)?;
                begun = true;
                workers.insert(session.clone(), spawn_worker(session, request, begin, output.clone()));
                continue;
            }
            let session = request["session_id"].as_str();
            if !begun {
                output.failure(&request, None, error("invalid_request", "First request must be describe or begin"), true);
                continue;
            }
            let Some(worker) = session
                .and_then(|s| workers.get(s))
                .filter(|w| w.alive.load(Ordering::Acquire) && !w.closing.load(Ordering::Acquire))
            else {
                output.failure(&request, session, error("unknown_session", "Unknown session"), true);
                continue;
            };
            let close = request["op"] == "close";
            let command = if close {
                Command::Close(request.clone())
            } else if worker.compiling.load(Ordering::Acquire) {
                output.failure(&request, session, error("session_busy", "Session is compiling"), true);
                continue;
            } else {
                Command::Request(request.clone())
            };
            match worker.sender.try_send(command) {
                Ok(()) => {
                    if close {
                        worker.closing.store(true, Ordering::Release);
                        worker.monitor.cancel();
                    }
                }
                Err(mpsc::TrySendError::Full(_)) => output.failure(&request, session, error("resource_limit", "Session request queue is full"), true),
                Err(mpsc::TrySendError::Disconnected(_)) => output.failure(&request, session, error("unknown_session", "Session has ended"), true),
            }
        }
        Ok(())
    })();
    for worker in workers.values() {
        worker.monitor.cancel();
    }
    for (_, worker) in workers {
        let _ = worker.sender.send(Command::Disconnect);
        let _ = worker.thread.join();
    }
    let _ = output.stream.lock().unwrap().shutdown(Shutdown::Both);
    result
}
pub fn run(address: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(address)?;
    serve_listener(listener)
}

pub fn serve_listener(listener: TcpListener) -> std::io::Result<()> {
    eprintln!("MROWW RedGear API v4 listening on {}", listener.local_addr()?);
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
