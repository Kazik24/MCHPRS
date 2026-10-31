#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_core::plot::PlotWorld;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct CountingAllocator;

struct Counts {
    alloc_calls: AtomicU64,
    alloc_bytes: AtomicU64,
    realloc_calls: AtomicU64,
    realloc_old_bytes: AtomicU64,
    realloc_new_bytes: AtomicU64,
}

static COUNTS: Counts = Counts {
    alloc_calls: AtomicU64::new(0),
    alloc_bytes: AtomicU64::new(0),
    realloc_calls: AtomicU64::new(0),
    realloc_old_bytes: AtomicU64::new(0),
    realloc_new_bytes: AtomicU64::new(0),
};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// This wrapper exists only in this isolated example so the benchmark can count System allocator calls.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        COUNTS.alloc_calls.fetch_add(1, Ordering::Relaxed);
        COUNTS.alloc_bytes.fetch_add(layout.size() as u64, Ordering::Relaxed);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        COUNTS.realloc_calls.fetch_add(1, Ordering::Relaxed);
        COUNTS.realloc_old_bytes.fetch_add(layout.size() as u64, Ordering::Relaxed);
        COUNTS.realloc_new_bytes.fetch_add(new_size as u64, Ordering::Relaxed);
        new_ptr
    }
}

#[derive(Clone, Copy, Default)]
struct Snapshot {
    alloc_calls: u64,
    alloc_bytes: u64,
    realloc_calls: u64,
    realloc_old_bytes: u64,
    realloc_new_bytes: u64,
}

impl Snapshot {
    fn read() -> Self {
        Self {
            alloc_calls: COUNTS.alloc_calls.load(Ordering::Relaxed),
            alloc_bytes: COUNTS.alloc_bytes.load(Ordering::Relaxed),
            realloc_calls: COUNTS.realloc_calls.load(Ordering::Relaxed),
            realloc_old_bytes: COUNTS.realloc_old_bytes.load(Ordering::Relaxed),
            realloc_new_bytes: COUNTS.realloc_new_bytes.load(Ordering::Relaxed),
        }
    }

    fn delta(self, previous: Self) -> Value {
        json!({
            "alloc_calls":self.alloc_calls - previous.alloc_calls,
            "alloc_bytes":self.alloc_bytes - previous.alloc_bytes,
            "realloc_calls":self.realloc_calls - previous.realloc_calls,
            "realloc_old_bytes":self.realloc_old_bytes - previous.realloc_old_bytes,
            "realloc_new_bytes":self.realloc_new_bytes - previous.realloc_new_bytes,
        })
    }
}

fn calibrate() {
    let before = Snapshot::read();
    let mut allocation = Vec::<u8>::new();
    allocation.reserve_exact(257);
    black_box(allocation.as_mut_ptr());
    let measured = Snapshot::read().delta(before);
    assert!(measured["alloc_calls"].as_u64().unwrap() > 0, "allocator counter did not observe calibration allocation");
    assert!(measured["alloc_bytes"].as_u64().unwrap() >= 257, "allocator counter reported too few calibration bytes");
    drop(allocation);
}

fn ram(world: &PlotWorld, cpu: cpus::Cpu) -> Vec<Option<u16>> {
    let mut trace = Vec::new();
    cpus::collect_ram(world, cpu, 0, &mut trace);
    trace.pop().unwrap().1
}

#[cfg(feature = "plot-bench")]
fn staged_sections(world: &PlotWorld) -> usize {
    world.get_chunks().iter().flat_map(|chunk| &chunk.sections).filter(|section| section.has_pending_block_changes()).count()
}

fn main() {
    const TICKS: u32 = 256;
    let cpu = cpus::CPUS[2];
    calibrate();

    let mut native = cpus::load_cpu(cpu);
    let (mut compiled, mut compiler, compile_wall) = cpus::compile_cpu(cpus::load_cpu(cpu), cpu);
    #[cfg(feature = "plot-bench")]
    let work_before = compiler.diagnostic_work().unwrap();
    let run = cpu.start;
    cpus::click_cpu(&mut native, cpu, run);
    let control_before = Snapshot::read();
    compiler.try_on_use_block(cpu.origin + run).unwrap();
    let control_after = Snapshot::read();

    let mut native_ticks = Duration::ZERO;
    for _ in 0..TICKS {
        let started = Instant::now();
        native.tick_interpreted();
        native_ticks += started.elapsed();
    }

    let mut kernel_time = Duration::ZERO;
    let mut publication_time = Duration::ZERO;
    #[cfg(feature = "plot-bench")]
    let staged_before = staged_sections(&compiled);
    let mut kernel_samples = Vec::with_capacity(TICKS as usize);
    let mut publication_samples = Vec::with_capacity(TICKS as usize);
    for tick in 1..=TICKS {
        let before = Snapshot::read();
        let started = Instant::now();
        compiler.tick().unwrap();
        kernel_time += started.elapsed();
        let after = Snapshot::read();
        kernel_samples.push(json!({"tick":tick,"allocations":after.delta(before)}));

        let before = Snapshot::read();
        let started = Instant::now();
        compiler.try_flush(&mut compiled).unwrap();
        publication_time += started.elapsed();
        let after = Snapshot::read();
        publication_samples.push(json!({"tick":tick,"allocations":after.delta(before)}));
    }
    assert!(compiler.is_active(), "compiled run must not fall back to native");
    #[cfg(feature = "plot-bench")]
    let staged_after = staged_sections(&compiled);
    #[cfg(feature = "plot-bench")]
    let work = {
        let after = compiler.diagnostic_work().unwrap();
        json!({"operations":after[0]-work_before[0],"deliveries":after[1]-work_before[1],
            "accepted_piston_actions":after[2]-work_before[2],"token_movements":after[3]-work_before[3],
            "motion_completions":after[4]-work_before[4],"scheduled_repeater_ticks":after[5]-work_before[5],
            "extension_checks":after[6]-work_before[6],"empty_extensions":after[7]-work_before[7],
            "mate_checks":after[8]-work_before[8],"onward_pushes":after[9]-work_before[9],
            "token_queries":after[10]-work_before[10],"token_candidates":after[11]-work_before[11],
            "movement_flips":after[12]-work_before[12],"movement_steps":after[13]-work_before[13]})
    };
    #[cfg(not(feature = "plot-bench"))]
    let work = Value::Null;
    #[cfg(feature = "plot-bench")]
    let staged = json!({"before":staged_before,"after":staged_after});
    #[cfg(not(feature = "plot-bench"))]
    let staged = Value::Null;

    // try_reset always performs full publication before deactivating the sampled runtime.
    assert!(!compiler.current_flags().unwrap().update, "reset requires the full fixture bounds when updates are enabled");
    let reset_before = Snapshot::read();
    let reset_started = Instant::now();
    compiler.try_reset(&mut compiled, (cpu.origin, cpu.origin)).unwrap();
    let reset_publication_time = reset_started.elapsed();
    let reset_after = Snapshot::read();
    let native_ram = ram(&native, cpu);
    let compiled_ram = ram(&compiled, cpu);
    assert_eq!(compiled_ram, native_ram, "BubbleSort RAM after full reset at tick {TICKS}");

    let executable = std::env::current_exe().unwrap();
    let executable_sha256 = format!("{:x}", Sha256::digest(std::fs::read(executable).unwrap()));
    let visibility = if cfg!(feature = "redpiler-full-publication") { "complete" } else { "redstone-only" };
    let artifact = json!({
        "fixture":cpu.schematic,"fixture_sha256":cpu.sha256,"ticks":TICKS,
        "publication_visibility":visibility,"output_validation":"final RAM compared after complete reset",
        "allocation_scope":"process-wide allocations observed synchronously inside each call; not attributed per subsystem",
        "kernel_scope":"compiler.tick call; engine path plus any synchronous helper allocations",
        "publication_scope":"compiler.try_flush call; includes host PlotWorld setters and publication work",
        "native_ms":native_ticks.as_secs_f64()*1000.0,
        "compile_wall_ms":compile_wall.as_secs_f64()*1000.0,
        "kernel_ms":kernel_time.as_secs_f64()*1000.0,
        "publication_ms":publication_time.as_secs_f64()*1000.0,
        "full_reset_publication_ms":reset_publication_time.as_secs_f64()*1000.0,
        "control_allocations":control_after.delta(control_before),"full_reset_allocations":reset_after.delta(reset_before),
        "timing_note":"allocator-instrumented diagnostic, not production throughput",
        "staged_sections":staged,
        "runtime_work":work,
        "native_ram":native_ram,"compiled_ram_after_reset":compiled_ram,
        "kernel_samples":kernel_samples,"publication_samples":publication_samples,
        "executable_sha256":executable_sha256,
        "features":{"redpiler_full_publication":cfg!(feature = "redpiler-full-publication"),"redpiler_trace":cfg!(feature = "redpiler-trace")},
        "source_provenance":"external build manifest required: record git HEAD, SHA-256 of the dirty patch including untracked source files, rustc -Vv, and the exact cargo build command"
    });
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mair-six-build-bench");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("bubblesort-allocations.json"), serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    eprintln!("BubbleSort {TICKS} ticks: kernel={kernel_time:?}, publication={publication_time:?}, full reset={reset_publication_time:?}");
}
