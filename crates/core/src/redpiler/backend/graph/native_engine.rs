//! Experimental whole-graph C execution; Rust owns loading, projection and verification.
mod emit;

use super::*;
use sha2::{Digest, Sha256};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const NONE: u32 = u32::MAX;
static NEXT_MODULE: AtomicU64 = AtomicU64::new(0);
static COMPILED_MODULES: Mutex<Vec<([u8; 32], PathBuf, PathBuf)>> = Mutex::new(Vec::new());

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Position {
    x: i32,
    y: i32,
    z: i32,
}
impl From<BlockPos> for Position {
    fn from(p: BlockPos) -> Self {
        Self { x: p.x, y: p.y, z: p.z }
    }
}
impl From<Position> for BlockPos {
    fn from(p: Position) -> Self {
        Self::new(p.x, p.y, p.z)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Component {
    powered: u8,
    locked: u8,
    strength: u8,
    pending: u8,
    north: u8,
    south: u8,
    east: u8,
    west: u8,
    power: u8,
    reserved: [u8; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Piston {
    extended: u8,
    head: u8,
    availability: u8,
    reserved: [u8; 5],
    revision: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Resource {
    at: u32,
    enabled: u8,
    reserved: [u8; 3],
    revision: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct StateView {
    components: *mut Component,
    pistons: *mut Piston,
    resources: *mut Resource,
    classic_signals: *mut u8,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InitialTick {
    node: u32,
    delay: u32,
    priority: u32,
    registry: u32,
    current_registry: u32,
    block_type: u32,
    has_block_type: u8,
    reserved: [u8; 3],
    position: Position,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FutureTick {
    node: u32,
    priority: u32,
    delay: u32,
    registry: u32,
    current_registry: u32,
    block_type: u32,
    has_block_type: u8,
    reserved: [u8; 3],
    position: Position,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Request {
    piston: u32,
    action: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Deferred {
    piston: u32,
    resource: u32,
    contact: u32,
    revision: u64,
    resource_revision: u64,
    due_tick: u64,
    has_resource: u8,
    extending: u8,
    reserved: [u8; 6],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Effect {
    kind: u32,
    position: Position,
    action: u32,
    sound_id: u32,
    sound_category: u32,
    facing: u8,
    sticky: u8,
    extended: u8,
    reserved: u8,
    volume: f32,
    pitch: f32,
}
#[repr(C)]
struct Future {
    logical_tick: u64,
    scheduler_position: u32,
    tick_count: u32,
    request_count: u32,
    deferred_count: u32,
    ticks: *mut FutureTick,
    tick_capacity: u32,
    requests: *mut Request,
    request_capacity: u32,
    deferred: *mut Deferred,
    deferred_capacity: u32,
}
impl Default for Future {
    fn default() -> Self {
        Self {
            logical_tick: 0,
            scheduler_position: 0,
            tick_count: 0,
            request_count: 0,
            deferred_count: 0,
            ticks: std::ptr::null_mut(),
            tick_capacity: 0,
            requests: std::ptr::null_mut(),
            request_capacity: 0,
            deferred: std::ptr::null_mut(),
            deferred_capacity: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Counters {
    ticks: u64,
    scheduled: u64,
    callbacks: u64,
    walks: u64,
    samples: u64,
    actions: u64,
    publications: u64,
    wire_query_fact_evaluations: u64,
    wire_dust_path_evaluations: u64,
    wire_isolated_replays: u64,
    wire_instant_reset_replays: u64,
    instant_route_invocations: u64,
    instant_route_steps: u64,
    classic_net_invocations: u64,
    classic_receiver_ops: u64,
    compiled_power_queries: u64,
    compiled_guard_evaluations: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Allocator {
    context: *mut c_void,
    allocate: unsafe extern "C" fn(*mut c_void, usize, usize) -> *mut c_void,
    deallocate: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, usize),
}
unsafe extern "C" fn allocate(_: *mut c_void, size: usize, align: usize) -> *mut c_void {
    let Ok(layout) = Layout::from_size_align(size, align) else {
        return std::ptr::null_mut();
    };
    if size == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: the C runtime supplies this same size/alignment when releasing the allocation.
    unsafe { alloc_zeroed(layout).cast() }
}
unsafe extern "C" fn release(_: *mut c_void, ptr: *mut c_void, size: usize, align: usize) {
    if !ptr.is_null() {
        if let Ok(layout) = Layout::from_size_align(size, align) {
            // SAFETY: this pointer was allocated by `allocate` with the supplied layout.
            unsafe { dealloc(ptr.cast(), layout) };
        }
    }
}

type Create = unsafe extern "C" fn(StateView, Allocator, u64, *const InitialTick, u32) -> *mut c_void;
type Destroy = unsafe extern "C" fn(*mut c_void);
type Tick = unsafe extern "C" fn(*mut c_void, StateView) -> u32;
type Use = unsafe extern "C" fn(*mut c_void, StateView, u32) -> u32;
type Plate = unsafe extern "C" fn(*mut c_void, StateView, u32, u8) -> u32;
type Snapshot = unsafe extern "C" fn(*const c_void, *mut Future) -> u32;
type Count = unsafe extern "C" fn(*const c_void) -> *const Counters;
type ErrorNode = unsafe extern "C" fn(*const c_void) -> u32;
type Dirty = unsafe extern "C" fn(*mut c_void, *mut u8, u32) -> u32;
type Effects = unsafe extern "C" fn(*mut c_void, *mut Effect, u32) -> u32;
type EffectCount = unsafe extern "C" fn(*const c_void) -> u32;
type VerifyQueries = unsafe extern "C" fn(*mut c_void, u8);

#[derive(Clone, Debug, Default)]
pub struct NativeEngineStats {
    pub compile_time: Duration,
    pub cache_hit: bool,
    pub nodes: usize,
    pub source: PathBuf,
    pub library: PathBuf,
    pub ticks: u64,
    pub compiled_query_targets: usize,
    pub total_query_targets: usize,
    pub compiled_guard_targets: usize,
    pub total_guard_targets: usize,
    pub classic_nets: usize,
    pub wire_query_fact_evaluations: u64,
    pub wire_dust_path_evaluations: u64,
    pub wire_isolated_replays: u64,
    pub wire_instant_reset_replays: u64,
    pub instant_route_invocations: u64,
    pub instant_route_steps: u64,
    pub classic_net_invocations: u64,
    pub classic_receiver_ops: u64,
    pub compiled_power_queries: u64,
    pub compiled_guard_evaluations: u64,
}

pub(super) struct NativeEngine {
    _library: libloading::Library,
    engine: *mut c_void,
    destroy: Destroy,
    tick: Tick,
    use_block: Use,
    plate: Plate,
    snapshot: Snapshot,
    counters: Count,
    error_node: ErrorNode,
    dirty: Dirty,
    effects: Effects,
    effect_count: EffectCount,
    verify_queries: VerifyQueries,
    components: Box<[Component]>,
    pistons: Box<[Piston]>,
    resources: Box<[Resource]>,
    classic_signals: Box<[u8]>,
    classic_wire_by_node: Box<[Option<(u32, u8)>]>,
    dirty_buffer: Vec<u8>,
    stats: NativeEngineStats,
    initial_tick: u64,
}

// SAFETY: the instance and its private allocations move together; execution requires &mut self.
// No C worker thread, TLS state or retained pointer into the Rust wrapper is used.
unsafe impl Send for NativeEngine {}

impl Drop for NativeEngine {
    fn drop(&mut self) {
        // SAFETY: the loaded module remains alive through this destructor and owns this handle.
        unsafe { (self.destroy)(self.engine) };
    }
}

impl NativeEngine {
    pub(super) fn compile(backend: &GraphBackend, clang: &Path, linker: Option<&Path>, artifacts: &Path) -> Result<Self, NativeQueryError> {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return Err(error("whole-graph DLL demo currently targets Windows x86_64"));
        }
        if backend.future.clock.phase != GraphPhase::BetweenTicks
            || !backend.future.piston_requests.is_empty()
            || !backend.future.deferred_pistons.is_empty()
        {
            return Err(NativeQueryError::new(
                "enable whole-graph execution between ticks before active piston work",
            ));
        }
        let started = Instant::now();
        let source_text = emit::program(&backend.program)?;
        let (query_source, compiled_query_targets, total_query_targets, compiled_guard_targets) =
            super::native_query::compact_power_source(&backend.program, &backend.state)?;
        let mut text = source_text;
        for (name, size) in [
            ("NEComponentState", std::mem::size_of::<Component>()),
            ("NEPistonState", std::mem::size_of::<Piston>()),
            ("NEResourceState", std::mem::size_of::<Resource>()),
            ("NEStateView", std::mem::size_of::<StateView>()),
            ("NEInitialTick", std::mem::size_of::<InitialTick>()),
            ("NEFutureTick", std::mem::size_of::<FutureTick>()),
            ("NEFutureDeferredPiston", std::mem::size_of::<Deferred>()),
            ("NEEffect", std::mem::size_of::<Effect>()),
            ("NEFutureSnapshot", std::mem::size_of::<Future>()),
            ("NECounters", std::mem::size_of::<Counters>()),
        ] {
            text.push_str(&format!("_Static_assert(sizeof({name}) == {size}, \"Rust/C ABI: {name}\");\n"));
        }
        text.push_str("#define NE_COMPILED_POWER 1\n#define NE_COMPILED_GUARDS 1\n#include \"native_engine.c\"\n");
        text.push_str(&query_source);
        text.push_str("__declspec(dllexport) NEEngine *ne_create_compiled(NEStateView s, NEAllocator a, uint64_t t, const NEInitialTick *q, uint32_t n) { return ne_create(&ne_program,s,a,t,0,q,n); }\n");
        let linker = linker.ok_or_else(|| NativeQueryError::new("whole-graph Windows demo requires lld-link"))?;
        let mut digest = Sha256::new();
        for part in [
            text.as_bytes(),
            include_bytes!("native_engine.h"),
            include_bytes!("native_engine.c"),
            clang.as_os_str().as_encoded_bytes(),
            linker.as_os_str().as_encoded_bytes(),
        ] {
            digest.update((part.len() as u64).to_le_bytes());
            digest.update(part);
        }
        let key: [u8; 32] = digest.finalize().into();
        // Repeated benchmark instances share immutable machine code, never engine state.
        let mut modules = COMPILED_MODULES.lock().map_err(error)?;
        let cached = modules.iter().find(|entry| entry.0 == key);
        let cache_hit = cached.is_some();
        let (source, library) = if let Some((_, source, library)) = cached {
            (source.clone(), library.clone())
        } else {
            let unique = NEXT_MODULE.fetch_add(1, Ordering::Relaxed);
            let directory = artifacts.join(format!("whole-graph-{}-{unique}", std::process::id()));
            std::fs::create_dir_all(&directory).map_err(error)?;
            let source = directory.join("graph.c");
            std::fs::write(directory.join("native_engine.h"), include_str!("native_engine.h")).map_err(error)?;
            std::fs::write(directory.join("native_engine.c"), include_str!("native_engine.c")).map_err(error)?;
            std::fs::write(&source, &text).map_err(error)?;
            let library = directory.join("graph.dll");
            let object = directory.join("graph.obj");
            run(Command::new(clang)
                .args([
                    "--target=x86_64-pc-windows-msvc",
                    "-O3",
                    "-ffreestanding",
                    "-fno-builtin",
                    "-Werror",
                    "-c",
                ])
                .arg(&source)
                .arg("-o")
                .arg(&object))?;
            let mut link = Command::new(linker);
            link.args(["/dll", "/noentry", "/nodefaultlib", "/machine:x64"])
                .arg(format!("/out:{}", library.display()))
                .arg(&object);
            for symbol in [
                "ne_destroy",
                "ne_tick",
                "ne_use_block",
                "ne_set_pressure_plate",
                "ne_snapshot_future",
                "ne_counters",
                "ne_error_node",
                "ne_copy_dirty",
                "ne_copy_effects",
                "ne_pending_effect_count",
                "ne_verify_compiled_power",
            ] {
                link.arg(format!("/export:{symbol}"));
            }
            run(&mut link)?;
            modules.push((key, source.clone(), library.clone()));
            (source, library)
        };
        drop(modules);
        // SAFETY: the module is generated locally from the validated graph and embedded runtime.
        let loaded = unsafe { libloading::Library::new(&library) }.map_err(error)?;
        unsafe fn symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Result<T, NativeQueryError> {
            // SAFETY: signatures are checked against the embedded C header at this call site.
            Ok(unsafe { *library.get::<T>(name).map_err(error)? })
        }
        let create: Create = unsafe { symbol(&loaded, b"ne_create_compiled\0")? };
        let mut result = Self {
            destroy: unsafe { symbol(&loaded, b"ne_destroy\0")? },
            tick: unsafe { symbol(&loaded, b"ne_tick\0")? },
            use_block: unsafe { symbol(&loaded, b"ne_use_block\0")? },
            plate: unsafe { symbol(&loaded, b"ne_set_pressure_plate\0")? },
            snapshot: unsafe { symbol(&loaded, b"ne_snapshot_future\0")? },
            counters: unsafe { symbol(&loaded, b"ne_counters\0")? },
            error_node: unsafe { symbol(&loaded, b"ne_error_node\0")? },
            dirty: unsafe { symbol(&loaded, b"ne_copy_dirty\0")? },
            effects: unsafe { symbol(&loaded, b"ne_copy_effects\0")? },
            effect_count: unsafe { symbol(&loaded, b"ne_pending_effect_count\0")? },
            verify_queries: unsafe { symbol(&loaded, b"ne_verify_compiled_power\0")? },
            engine: std::ptr::null_mut(),
            components: backend.state.components.iter().map(pack_component).collect(),
            pistons: backend
                .state
                .pistons
                .iter()
                .map(|p| Piston {
                    extended: p.extended.into(),
                    head: p.head_present.into(),
                    availability: availability(p.availability),
                    revision: p.revision,
                    ..Default::default()
                })
                .collect(),
            resources: backend
                .state
                .resources
                .iter()
                .map(|r| Resource {
                    at: r.at.index() as u32,
                    enabled: r.enabled.into(),
                    revision: r.revision,
                    ..Default::default()
                })
                .collect(),
            classic_signals: backend.state.classic_signals.clone(),
            classic_wire_by_node: backend.program.classic().wire_by_node.clone(),
            dirty_buffer: vec![0; backend.state.components.len()],
            initial_tick: backend.future.clock.logical_tick,
            stats: NativeEngineStats {
                compile_time: started.elapsed(),
                cache_hit,
                nodes: backend.program.nodes().len(),
                source,
                library,
                ticks: 0,
                compiled_query_targets,
                total_query_targets,
                compiled_guard_targets,
                total_guard_targets: backend.program.guards().len(),
                classic_nets: backend.program.classic().nets.len(),
                ..Default::default()
            },
            _library: loaded,
        };
        let ticks: Vec<_> = backend
            .future
            .scheduler
            .iter()
            .map(|(t, d, p)| InitialTick {
                node: t.node.map_or(NONE, |n| n.index() as u32),
                delay: d as u32,
                priority: p as u32,
                registry: t.registry_type,
                current_registry: t.current_registry_type,
                block_type: t.block_type.unwrap_or(0),
                has_block_type: t.block_type.is_some().into(),
                position: t.position.into(),
                ..Default::default()
            })
            .collect();
        let view = result.view();
        // SAFETY: all input arrays outlive the handle; imported entries are copied by ne_create.
        result.engine = unsafe {
            create(
                view,
                Allocator {
                    context: std::ptr::null_mut(),
                    allocate,
                    deallocate: release,
                },
                result.initial_tick,
                ticks.as_ptr(),
                ticks.len() as u32,
            )
        };
        if result.engine.is_null() {
            return Err(NativeQueryError::new("C engine rejected its program or allocation failed"));
        }
        Ok(result)
    }

    fn view(&mut self) -> StateView {
        StateView {
            components: self.components.as_mut_ptr(),
            pistons: self.pistons.as_mut_ptr(),
            resources: self.resources.as_mut_ptr(),
            classic_signals: self.classic_signals.as_mut_ptr(),
        }
    }
    fn status(&self, code: u32) -> Result<(), NativeQueryError> {
        if code == 0 {
            Ok(())
        } else {
            let node = unsafe { (self.error_node)(self.engine) };
            Err(NativeQueryError::new(format!("C inference failed: status={code}, node={node}")))
        }
    }
    pub(super) fn tick(&mut self) -> Result<(), NativeQueryError> {
        let view = self.view();
        let code = unsafe { (self.tick)(self.engine, view) };
        self.status(code)
    }
    pub(super) fn use_block(&mut self, node: NodeId) -> Result<(), NativeQueryError> {
        let view = self.view();
        let code = unsafe { (self.use_block)(self.engine, view, node.index() as u32) };
        self.status(code)
    }
    pub(super) fn plate(&mut self, node: NodeId, powered: bool) -> Result<(), NativeQueryError> {
        let view = self.view();
        let code = unsafe { (self.plate)(self.engine, view, node.index() as u32, powered.into()) };
        self.status(code)
    }
    pub(super) fn counts(&self) -> GraphExecutionCounts {
        let c = unsafe { *(self.counters)(self.engine) };
        GraphExecutionCounts {
            ticks: c.ticks,
            scheduled_ticks: c.scheduled,
            callbacks: c.callbacks,
            wire_walks: c.walks,
            piston_samples: c.samples,
            piston_actions: c.actions,
            deferred_publications: c.publications,
            classic_net_invocations: c.classic_net_invocations,
            classic_receiver_ops: c.classic_receiver_ops,
            ..Default::default()
        }
    }
    pub(super) fn logical_tick(&self) -> u64 {
        self.initial_tick.wrapping_add(self.counts().ticks)
    }
    pub(super) fn verify_power_queries(&mut self) {
        unsafe { (self.verify_queries)(self.engine, 1) };
    }
    pub(super) fn stats(&self) -> NativeEngineStats {
        let mut s = self.stats.clone();
        let c = unsafe { *(self.counters)(self.engine) };
        s.ticks = c.ticks;
        s.wire_query_fact_evaluations = c.wire_query_fact_evaluations;
        s.wire_dust_path_evaluations = c.wire_dust_path_evaluations;
        s.wire_isolated_replays = c.wire_isolated_replays;
        s.wire_instant_reset_replays = c.wire_instant_reset_replays;
        s.instant_route_invocations = c.instant_route_invocations;
        s.instant_route_steps = c.instant_route_steps;
        s.classic_net_invocations = c.classic_net_invocations;
        s.classic_receiver_ops = c.classic_receiver_ops;
        s.compiled_power_queries = c.compiled_power_queries;
        s.compiled_guard_evaluations = c.compiled_guard_evaluations;
        s
    }

    pub(super) fn sync_state(
        &mut self,
        state: &mut CircuitState,
        dirty: &mut [bool],
        effects: &mut Vec<PresentationEffect>,
    ) -> Result<(), NativeQueryError> {
        let changed = unsafe { (self.dirty)(self.engine, self.dirty_buffer.as_mut_ptr(), self.dirty_buffer.len() as u32) };
        if changed == u32::MAX {
            return Err(error("invalid C dirty-state projection"));
        }
        state.classic_signals.copy_from_slice(&self.classic_signals);
        for (index, ((target, source), &changed)) in state.components.iter_mut().zip(&self.components).zip(&self.dirty_buffer).enumerate() {
            if changed == 0 {
                continue;
            }
            dirty[index] = true;
            match target {
                ComponentState::Electrical(e) => {
                    e.powered = source.powered != 0;
                    e.repeater_locked = source.locked != 0;
                    e.output_strength = source.strength;
                    e.pending_tick = source.pending != 0;
                }
                ComponentState::Wire(w) => {
                    w.power = self
                        .classic_wire_by_node
                        .get(index)
                        .copied()
                        .flatten()
                        .map_or(source.power, |(net, attenuation)| {
                            self.classic_signals[net as usize].saturating_sub(attenuation)
                        });
                    w.north = side(source.north)?;
                    w.south = side(source.south)?;
                    w.east = side(source.east)?;
                    w.west = side(source.west)?;
                }
                ComponentState::Specialized => {}
            }
        }
        for (target, source) in state.pistons.iter_mut().zip(&self.pistons) {
            target.extended = source.extended != 0;
            target.head_present = source.head != 0;
            target.availability = match source.availability {
                0 => PistonAvailability::Ready,
                1 => PistonAvailability::HeadPending,
                2 => PistonAvailability::PublishingHeadRemoval,
                3 => PistonAvailability::BasePendingHeadPresent,
                4 => PistonAvailability::BasePending,
                _ => return Err(NativeQueryError::new("C returned invalid piston availability")),
            };
            target.revision = source.revision;
        }
        for (target, source) in state.resources.iter_mut().zip(&self.resources) {
            target.at = ContactId::new(source.at);
            target.enabled = source.enabled != 0;
            target.revision = source.revision;
        }
        let count = unsafe { (self.effect_count)(self.engine) };
        if count == u32::MAX {
            return Err(error("invalid C engine while projecting effects"));
        }
        let mut emitted = vec![Effect::default(); count as usize];
        let copied = unsafe { (self.effects)(self.engine, emitted.as_mut_ptr(), count) };
        if copied != count {
            return Err(NativeQueryError::new("C effect count changed during projection"));
        }
        for effect in emitted {
            effects.push(match effect.kind {
                0 => PresentationEffect::BlockAction {
                    position: effect.position.into(),
                    action: BlockAction::Piston {
                        action: action(effect.action)?,
                        piston: RedstonePiston {
                            facing: BlockFace::try_from_id(effect.facing.into())
                                .ok_or_else(|| NativeQueryError::new("invalid C effect face"))?
                                .into(),
                            sticky: effect.sticky != 0,
                            extended: effect.extended != 0,
                        },
                    },
                },
                1 => PresentationEffect::Sound {
                    position: effect.position.into(),
                    id: effect.sound_id as i32,
                    category: effect.sound_category as i32,
                    volume: effect.volume,
                    pitch: effect.pitch,
                },
                _ => return Err(NativeQueryError::new("C returned an unsupported presentation effect")),
            });
        }
        Ok(())
    }

    pub(super) fn sync_future(&self, future: &mut GraphFuture) -> Result<(), NativeQueryError> {
        let mut snapshot = Future::default();
        let code = unsafe { (self.snapshot)(self.engine, &mut snapshot) };
        if code != 0 && code != 3 {
            return self.status(code);
        }
        let mut ticks = vec![FutureTick::default(); snapshot.tick_count as usize];
        let mut requests = vec![Request::default(); snapshot.request_count as usize];
        let mut deferred = vec![Deferred::default(); snapshot.deferred_count as usize];
        snapshot.ticks = ticks.as_mut_ptr();
        snapshot.tick_capacity = ticks.len() as u32;
        snapshot.requests = requests.as_mut_ptr();
        snapshot.request_capacity = requests.len() as u32;
        snapshot.deferred = deferred.as_mut_ptr();
        snapshot.deferred_capacity = deferred.len() as u32;
        self.status(unsafe { (self.snapshot)(self.engine, &mut snapshot) })?;
        future.scheduler = TickScheduler::default();
        future.pending.clear();
        for t in ticks {
            let tick = ScheduledNodeTick {
                node: (t.node != NONE).then(|| NodeId::new(t.node)),
                position: t.position.into(),
                block_type: (t.has_block_type != 0).then_some(t.block_type),
                registry_type: t.registry,
                current_registry_type: t.current_registry,
            };
            let priority = *TickPriority::ALL
                .get(t.priority as usize)
                .ok_or_else(|| NativeQueryError::new("invalid C tick priority"))?;
            if t.delay >= 32 {
                return Err(NativeQueryError::new("invalid C scheduled delay"));
            }
            future.scheduler.schedule_half_tick(tick, t.delay as usize, priority);
            *future.pending.entry(tick.into()).or_default() += 1;
        }
        future.piston_requests = requests
            .into_iter()
            .map(|r| {
                Ok(PistonRequest {
                    piston: PistonId::new(r.piston),
                    action: action(r.action)?,
                })
            })
            .collect::<Result<_, NativeQueryError>>()?;
        future.deferred_pistons = deferred
            .into_iter()
            .map(|d| DeferredPistonEvent {
                piston: PistonId::new(d.piston),
                revision: d.revision,
                resource: (d.has_resource != 0).then_some(pistons::DeferredResourceEvent {
                    resource: ResourceId::new(d.resource),
                    revision: d.resource_revision,
                    contact: ContactId::new(d.contact),
                }),
                due_tick: d.due_tick,
                extending: d.extending != 0,
            })
            .collect();
        future.clock = Clock {
            logical_tick: snapshot.logical_tick,
            phase: GraphPhase::BetweenTicks,
            scheduled_advanced: false,
        };
        Ok(())
    }
}

pub(super) fn projected_classic_state(program: &CircuitProgram, state: &CircuitState) -> CircuitState {
    let mut result = state.clone();
    for (net_index, net) in program.classic().nets.iter().enumerate() {
        for wire in &program.classic().wires[net.wires.clone()] {
            if let ComponentState::Wire(value) = &mut result.components[wire.node.index()] {
                value.power = state.classic_signals[net_index].saturating_sub(wire.attenuation);
            }
        }
    }
    result
}

fn error(e: impl std::fmt::Display) -> NativeQueryError {
    NativeQueryError::new(e.to_string())
}
fn run(command: &mut Command) -> Result<(), NativeQueryError> {
    let output = command.output().map_err(error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(error(format!(
            "native build failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}
fn action(code: u32) -> Result<mchprs_world::PistonAction, NativeQueryError> {
    match code {
        0 => Ok(mchprs_world::PistonAction::Extend),
        1 => Ok(mchprs_world::PistonAction::Retract),
        2 => Ok(mchprs_world::PistonAction::RetractWithoutPull),
        _ => Err(error("invalid C piston action")),
    }
}
pub(super) fn availability(value: PistonAvailability) -> u8 {
    match value {
        PistonAvailability::Ready => 0,
        PistonAvailability::HeadPending => 1,
        PistonAvailability::PublishingHeadRemoval => 2,
        PistonAvailability::BasePendingHeadPresent => 3,
        PistonAvailability::BasePending => 4,
    }
}
fn side(code: u8) -> Result<RedstoneWireSide, NativeQueryError> {
    match code {
        0 => Ok(RedstoneWireSide::Up),
        1 => Ok(RedstoneWireSide::Side),
        2 => Ok(RedstoneWireSide::None),
        _ => Err(error("invalid C wire side")),
    }
}
fn pack_component(state: &ComponentState) -> Component {
    match state {
        ComponentState::Electrical(s) => Component {
            powered: s.powered.into(),
            locked: s.repeater_locked.into(),
            strength: s.output_strength,
            pending: s.pending_tick.into(),
            ..Default::default()
        },
        ComponentState::Wire(w) => Component {
            north: w.north as u8,
            south: w.south as u8,
            east: w.east as u8,
            west: w.west as u8,
            power: w.power,
            ..Default::default()
        },
        ComponentState::Specialized => Component::default(),
    }
}
