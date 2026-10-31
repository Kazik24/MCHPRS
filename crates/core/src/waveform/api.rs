use super::*;
use hyper::{
    Body, Request, Response, Server, StatusCode,
    body::Bytes,
    service::{make_service_fn, service_fn},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    convert::Infallible,
    io::Write,
    net::{SocketAddr, TcpListener},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, oneshot};

type Coord = (i32, i32);
type Sources = HashMap<Coord, (u128, mpsc::SyncSender<Read>)>;
static SOURCES: Lazy<Mutex<Sources>> = Lazy::new(|| Mutex::new(HashMap::new()));
const INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(crate) struct Error {
    pub status: u16,
    pub code: &'static str,
}
impl Error {
    pub fn new(status: u16, code: &'static str) -> Self {
        Self { status, code }
    }
    fn value(&self) -> Value {
        json!({"error":self.code,"resnapshot":self.status==409})
    }
}
#[derive(Clone)]
pub(crate) struct Cursor {
    pub plot: Coord,
    pub source: u128,
    pub capture: u128,
    pub after: u64,
    pub through: Option<u64>,
    pub first: u64,
    pub observed: u64,
    pub completed: u64,
    pub partial: bool,
}
impl Cursor {
    pub fn encode(&self) -> String {
        format!(
            "1:{}:{}:{:x}:{:x}:{:x}:{}:{:x}:{:x}:{:x}:{}",
            self.plot.0,
            self.plot.1,
            self.source,
            self.capture,
            self.after,
            self.through.map_or_else(|| "-".into(), |s| format!("{s:x}")),
            self.first,
            self.observed,
            self.completed,
            u8::from(self.partial)
        )
    }
    pub(super) fn parse(input: &str) -> Result<Self, Error> {
        let invalid = || Error::new(400, "invalid_cursor");
        if input.len() > 512 {
            return Err(invalid());
        }
        let p: Vec<_> = input.split(':').collect();
        if p.len() != 11 || p[0] != "1" {
            return Err(invalid());
        }
        let result = Self::from_fields(&p)?;
        if result.first > result.observed || result.completed > result.observed {
            return Err(invalid());
        }
        Ok(result)
    }
    fn from_fields(p: &[&str]) -> Result<Self, Error> {
        let invalid = || Error::new(400, "invalid_cursor");
        let u64_at = |i| u64::from_str_radix(p[i], 16).map_err(|_| invalid());
        Ok(Self {
            plot: (p[1].parse().map_err(|_| invalid())?, p[2].parse().map_err(|_| invalid())?),
            source: u128::from_str_radix(p[3], 16).map_err(|_| invalid())?,
            capture: u128::from_str_radix(p[4], 16).map_err(|_| invalid())?,
            after: u64_at(5)?,
            through: if p[6] == "-" { None } else { Some(u64_at(6)?) },
            first: u64_at(7)?,
            observed: u64_at(8)?,
            completed: u64_at(9)?,
            partial: match p[10] {
                "0" => false,
                "1" => true,
                _ => return Err(invalid()),
            },
        })
    }
}
pub(crate) struct Query {
    pub cursor: Option<Cursor>,
    pub limit: usize,
    pub metadata: bool,
}
struct Page {
    value: Value,
    _work: Reservation,
}
struct Read {
    query: Query,
    deadline: Instant,
    reply: oneshot::Sender<Result<Page, Error>>,
    work: Reservation,
}
pub(crate) struct Source {
    plot: Coord,
    generation: u128,
    requests: mpsc::Receiver<Read>,
    owner: Option<u128>,
    owner_name: Option<String>,
}
impl Source {
    pub fn register(plot: Coord) -> Result<Self, String> {
        let mut sources = SOURCES.lock().unwrap();
        if sources.len() >= 4096 {
            return Err("Waveform source registry full".into());
        }
        let generation = rand::random();
        let (send, requests) = mpsc::sync_channel(8);
        sources.insert(plot, (generation, send));
        Ok(Self {
            plot,
            generation,
            requests,
            owner: None,
            owner_name: None,
        })
    }
    pub fn poll(&mut self, recorder: &Recorder, unsupported: bool, owner: Option<u128>) {
        if owner != self.owner {
            self.owner = owner;
            self.owner_name =
                owner.map(|uuid| crate::plot::database::get_cached_username(format!("{uuid:032x}")).unwrap_or_else(|| "Unknown owner".into()));
        }
        let started = Instant::now();
        for _ in 0..2 {
            if started.elapsed() >= Duration::from_millis(2) {
                break;
            }
            let Ok(read) = self.requests.try_recv() else {
                break;
            };
            if read.deadline < Instant::now() || read.reply.is_closed() {
                continue;
            }
            let result = if read.query.metadata {
                Ok(recorder.metadata(self.generation, unsupported))
            } else {
                recorder.page(self.plot, self.generation, &read.query, unsupported)
            };
            let _ = read.reply.send(result.map(|mut value| {
                value["owner"] = json!(self.owner_name);
                Page { value, _work: read.work }
            }));
        }
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let mut sources = SOURCES.lock().unwrap();
        if sources.get(&self.plot).is_some_and(|(id, _)| *id == self.generation) {
            sources.remove(&self.plot);
        }
    }
}
fn registered(plot: Coord, generation: Option<u128>) -> bool {
    SOURCES
        .lock()
        .unwrap()
        .get(&plot)
        .is_some_and(|(id, _)| generation.is_none_or(|expected| expected == *id))
}
async fn read(plot: Coord, query: Query) -> Result<Page, Error> {
    let reply = enqueue_read(plot, query)?;
    await_read(plot, reply).await
}

fn enqueue_read(plot: Coord, query: Query) -> Result<oneshot::Receiver<Result<Page, Error>>, Error> {
    let sender = SOURCES
        .lock()
        .unwrap()
        .get(&plot)
        .map(|(_, send)| send.clone())
        .ok_or_else(|| Error::new(404, "plot_not_loaded"))?;
    let work = reserve(&WORK, WORK_BYTES).map_err(|_| Error::new(503, "busy"))?;
    let (send, reply) = oneshot::channel();
    sender
        .try_send(Read {
            query,
            deadline: Instant::now() + Duration::from_secs(2),
            reply: send,
            work,
        })
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => Error::new(503, "busy"),
            _ => Error::new(404, "plot_not_loaded"),
        })?;
    Ok(reply)
}

async fn await_read(plot: Coord, reply: oneshot::Receiver<Result<Page, Error>>) -> Result<Page, Error> {
    let page = tokio::time::timeout(Duration::from_secs(2), reply)
        .await
        .map_err(|_| Error::new(503, "source_timeout"))?
        .map_err(|_| Error::new(404, "plot_not_loaded"))??;
    let generation = u128::from_str_radix(page.value["source"].as_str().unwrap_or(""), 16).ok();
    if !registered(plot, generation) {
        return Err(Error::new(404, "plot_not_loaded"));
    }
    Ok(page)
}

#[derive(PartialEq)]
enum Mode {
    Metadata,
    Data,
    Stream,
}
fn route(request: &Request<Body>) -> Result<(Coord, Mode, Query), Error> {
    let bad = || Error::new(400, "invalid_query");
    if request.method() != hyper::Method::GET {
        return Err(Error::new(405, "method_not_allowed"));
    }
    if request.uri().to_string().len() > 2048 {
        return Err(bad());
    }
    let path: Vec<_> = request.uri().path().split('/').collect();
    if !(path.len() == 7 || path.len() == 8) || path[1..4] != ["api", "v1", "plots"] || path[6] != "waveform" {
        return Err(Error::new(404, "not_found"));
    }
    let coord = |text: &str| -> Result<i32, Error> {
        let n = text.parse::<i32>().map_err(|_| Error::new(400, "invalid_coordinates"))?;
        if n.to_string() != text {
            return Err(Error::new(400, "invalid_coordinates"));
        }
        Ok(n)
    };
    let plot = (coord(path[4])?, coord(path[5])?);
    let mode = match path.get(7).copied() {
        None => Mode::Metadata,
        Some("data") => Mode::Data,
        Some("stream") => Mode::Stream,
        _ => return Err(Error::new(404, "not_found")),
    };
    let mut cursor = None;
    let mut limit = 4096;
    let mut seen_cursor = false;
    let mut seen_limit = false;
    let url = reqwest::Url::parse(&format!("http://waveform{}", request.uri())).map_err(|_| bad())?;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "cursor" if !seen_cursor && mode != Mode::Metadata => {
                seen_cursor = true;
                cursor = Some(Cursor::parse(&value)?);
            }
            "limit" if !seen_limit && mode != Mode::Metadata => {
                seen_limit = true;
                limit = value.parse::<usize>().map_err(|_| Error::new(400, "invalid_limit"))?;
            }
            _ => return Err(bad()),
        }
    }
    if limit == 0 || limit > 8192 {
        return Err(Error::new(400, "invalid_limit"));
    }
    if mode == Mode::Stream {
        if let Some(id) = request.headers().get("last-event-id") {
            cursor = Some(Cursor::parse(id.to_str().map_err(|_| bad())?)?);
        }
        if cursor.as_ref().is_none_or(|c| c.through.is_some()) {
            return Err(Error::new(400, "snapshot_required"));
        }
    }
    if cursor.as_ref().is_some_and(|c| c.plot != plot) {
        return Err(Error::new(400, "invalid_cursor"));
    }
    let metadata = mode == Mode::Metadata;
    Ok((plot, mode, Query { cursor, limit, metadata }))
}
fn gzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    writer.write_all(bytes)?;
    writer.finish()
}
fn response(status: u16, value: &Value, compress: bool) -> Response<Body> {
    let bytes = serde_json::to_vec(value).unwrap();
    if bytes.len() > PAGE_BYTES {
        return response(503, &Error::new(503, "response_too_large").value(), false);
    }
    let mut builder = Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store");
    if status == 429 || status == 503 {
        builder = builder.header("Retry-After", "1");
    }
    let bytes = if compress {
        match gzip(&bytes) {
            Ok(data) => {
                builder = builder.header("Content-Encoding", "gzip").header("Vary", "Accept-Encoding");
                data
            }
            Err(_) => bytes,
        }
    } else {
        bytes
    };
    builder.body(Body::from(bytes)).unwrap()
}
struct StreamWriter {
    sender: hyper::body::Sender,
    gzip: Option<flate2::write::GzEncoder<Vec<u8>>>,
}
impl StreamWriter {
    async fn send(&mut self, bytes: &[u8]) -> bool {
        let bytes = if let Some(gzip) = &mut self.gzip {
            if gzip.write_all(bytes).and_then(|_| gzip.flush()).is_err() {
                return false;
            }
            std::mem::take(gzip.get_mut())
        } else {
            bytes.to_vec()
        };
        matches!(
            tokio::time::timeout(Duration::from_secs(2), self.sender.send_data(Bytes::from(bytes))).await,
            Ok(Ok(()))
        )
    }
    async fn finish(mut self) {
        if let Some(gzip) = &mut self.gzip {
            if gzip.try_finish().is_ok() {
                let bytes = std::mem::take(gzip.get_mut());
                let _ = tokio::time::timeout(Duration::from_secs(2), self.sender.send_data(Bytes::from(bytes))).await;
            }
        }
    }
}
async fn stream(
    mut writer: StreamWriter,
    plot: Coord,
    mut query: Query,
    first: Page,
    stop: Arc<AtomicBool>,
    _permit: tokio::sync::OwnedSemaphorePermit,
) {
    let mut page = Some(first);
    let mut signature = None;
    let mut heartbeat = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let result = match page.take() {
            Some(page) => Ok(page),
            None => {
                read(
                    plot,
                    Query {
                        cursor: query.cursor.clone(),
                        limit: query.limit,
                        metadata: false,
                    },
                )
                .await
            }
        };
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                let packet = format!("event: reset\ndata: {}\n\n", error.value());
                let _ = writer.send(packet.as_bytes()).await;
                break;
            }
        };
        let Some(cursor) = page.value["cursor"].as_str() else {
            let _ = writer
                .send(b"event: reset\ndata: {\"error\":\"capture_changed\",\"resnapshot\":true}\n\n")
                .await;
            break;
        };
        let next = (
            cursor.to_owned(),
            page.value["window"].clone(),
            page.value["state"].clone(),
            page.value["reason"].clone(),
            page.value["owner"].clone(),
        );
        if signature.as_ref() != Some(&next) {
            let data = serde_json::to_string(&page.value).unwrap();
            if data.len() > PAGE_BYTES {
                break;
            }
            let packet = format!("id: {cursor}\nevent: batch\ndata: {data}\n\n");
            if !writer.send(packet.as_bytes()).await {
                break;
            }
            query.cursor = Cursor::parse(cursor).ok();
            signature = Some(next);
            heartbeat = Instant::now();
        } else if heartbeat.elapsed() >= Duration::from_secs(15) {
            if !writer.send(b": heartbeat\n\n").await {
                break;
            }
            heartbeat = Instant::now();
        }
        drop(page);
        tokio::time::sleep(INTERVAL).await;
    }
    writer.finish().await;
}

async fn handle(request: Request<Body>, stop: Arc<AtomicBool>, slots: Arc<Semaphore>) -> Response<Body> {
    let compress = request
        .headers()
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.split(',').any(|part| part.trim() == "gzip"));
    let (plot, mode, query) = match route(&request) {
        Ok(route) => route,
        Err(error) => return response(error.status, &error.value(), false),
    };
    let permit = match slots.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return response(429, &Error::new(429, "rate_limited").value(), false),
    };
    let page = match read(
        plot,
        Query {
            cursor: query.cursor.clone(),
            limit: query.limit,
            metadata: query.metadata,
        },
    )
    .await
    {
        Ok(page) => page,
        Err(error) => return response(error.status, &error.value(), false),
    };
    if mode != Mode::Stream {
        return response(200, &page.value, compress);
    }
    let (sender, body) = Body::channel();
    tokio::spawn(stream(
        StreamWriter {
            sender,
            gzip: compress.then(|| flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast())),
        },
        plot,
        query,
        page,
        stop,
        permit,
    ));
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-store")
        .header("X-Accel-Buffering", "no");
    if compress {
        response = response.header("Content-Encoding", "gzip").header("Vary", "Accept-Encoding");
    }
    response.body(body).unwrap()
}
pub(crate) struct ApiServer {
    shutdown: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    pub address: SocketAddr,
}
impl ApiServer {
    pub fn start(address: &str) -> Result<Self, String> {
        super::validate_config()?;
        let listener = TcpListener::bind(address).map_err(|e| format!("Cannot bind waveform API: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let (shutdown, closed) = oneshot::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::Builder::new()
            .name("waveform-api".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let slots = Arc::new(Semaphore::new(64));
                    let signal = stop.clone();
                    let connections = Arc::new(Semaphore::new(64));
                    let service = make_service_fn(move |_| {
                        let connection = connections.clone().try_acquire_owned();
                        let stop = stop.clone();
                        let slots = slots.clone();
                        async move {
                            let connection = connection.map_err(|_| std::io::Error::other("Waveform connection limit"))?;
                            Ok::<_, std::io::Error>(service_fn(move |request| {
                                let _keep_connection = &connection;
                                let stop = stop.clone();
                                let slots = slots.clone();
                                async move { Ok::<_, Infallible>(handle(request, stop, slots).await) }
                            }))
                        }
                    });
                    let server = Server::from_tcp(listener)
                        .expect("validated API listener")
                        .http1_only(true)
                        .http1_header_read_timeout(Duration::from_secs(5))
                        .serve(service);
                    let (drain, draining) = oneshot::channel();
                    let server = tokio::spawn(async move {
                        if let Err(error) = server
                            .with_graceful_shutdown(async {
                                let _ = draining.await;
                            })
                            .await
                        {
                            tracing::error!(%error,"Waveform API failed");
                        }
                    });
                    let _ = closed.await;
                    signal.store(true, Ordering::Relaxed);
                    let _ = drain.send(());
                    if tokio::time::timeout(Duration::from_secs(5), server).await.is_err() {
                        tracing::warn!("Waveform API forced closed after shutdown timeout");
                    }
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shutdown: Some(shutdown),
            worker: Some(worker),
            address,
        })
    }
}
impl Drop for ApiServer {
    fn drop(&mut self) {
        if let Some(send) = self.shutdown.take() {
            let _ = send.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
