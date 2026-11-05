use crate::{
    plot::{PLOT_WIDTH, PlotWorld},
    redpilator::{Compiler, CompilerOptions, PublicationMode, TaskMonitor},
    world::{World, storage::Chunk},
};
use mchprs_blocks::{BlockPos, block_entities::BlockEntity, blocks::Block};
use serde_json::{Value, json};
use std::{cell::RefCell, path::Path, sync::Arc};

thread_local! {
    static TRACE: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}
pub(super) fn enabled() -> bool {
    TRACE.with(|v| v.borrow().is_some())
}
pub(super) fn record(tick: u64, phase: u64, message: String) {
    TRACE.with(|v| {
        if let Some(v) = v.borrow_mut().as_mut() {
            v.push(json!({"tick":tick,"phase":phase,"message":message}));
        }
    });
}
pub(super) fn trace(f: impl FnOnce()) -> Vec<Value> {
    TRACE.with(|v| {
        assert!(v.borrow().is_none());
        *v.borrow_mut() = Some(vec![])
    });
    f();
    TRACE.with(|v| v.borrow_mut().take().unwrap())
}
#[derive(serde::Deserialize)]
struct Fixture {
    min: BlockPos,
    max: BlockPos,
    start: BlockPos,
    blocks: Vec<(BlockPos, u32)>,
    entities: Vec<(BlockPos, BlockEntity)>,
    pending: Vec<mchprs_world::TickEntry>,
}
fn write(root: &Path, name: &str, value: &impl serde::Serialize) {
    std::fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn region(p: BlockPos) -> bool {
    (40..=100).contains(&p.x) && (30..=54).contains(&p.y) && (90..=125).contains(&p.z)
}

#[test]
#[ignore = "PM1 load/counter causal trace, diagnostic artifacts only"]
fn pm1_load_counter_causal_trace() {
    let full = std::env::var_os("PM1_CAUSAL_FULL").is_some();
    let root = std::env::var_os("PM1_CAUSAL_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/pm1-causal-first"));
    assert!(!root.exists(), "fresh output directory required");
    std::fs::create_dir_all(&root).unwrap();
    let mut f: Fixture = serde_json::from_reader(flate2::read::GzDecoder::new(
        std::fs::File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scraps/redpilator-regressions/2026-11-04/pm1-piston-slice.json.gz"))
            .unwrap(),
    ))
    .unwrap();
    if full {
        let clip = crate::plot::worldedit::load_schematic(
            std::fs::File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/PM1_SORT_FIXED.schem")).unwrap(),
        )
        .unwrap();
        f.min = BlockPos::new(8, 8, 8);
        f.max = f.min + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1);
        f.blocks.clear();
        for y in 0..clip.size_y {
            for z in 0..clip.size_z {
                for x in 0..clip.size_x {
                    let id = clip.data.get_entry(((y * clip.size_z + z) * clip.size_x + x) as usize);
                    if id != 0 {
                        f.blocks.push((f.min + BlockPos::new(x as i32, y as i32, z as i32), id));
                    }
                }
            }
        }
        f.entities = clip.block_entities.into_iter().map(|(p, e)| (f.min + p, e)).collect();
    }
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    world.disable_command_output_limits_for_replay();
    for &(p, id) in &f.blocks {
        world.set_block(p, Block::from_id(id));
    }
    for (p, e) in &f.entities {
        world.set_block_entity(*p, e.clone());
    }
    for t in &f.pending {
        world.restore_scheduled_tick(t.clone());
    }
    let actors: Vec<_> = f
        .blocks
        .iter()
        .filter_map(|(p, id)| (region(*p) && matches!(Block::from_id(*id), Block::Piston { .. })).then_some(*p))
        .collect();
    let initial: Vec<_> = f
        .blocks
        .iter()
        .filter(|(p, _)| region(*p))
        .map(|(p, id)| json!({"pos":p,"id":id,"block":format!("{:?}",Block::from_id(*id))}))
        .collect();
    write(&root, "initial-region.json", &initial);
    let mut projected = world.activation_snapshot();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &projected,
            (f.min, f.max),
            CompilerOptions {
                publication: Some(PublicationMode::All),
                ..Default::default()
            },
            f.pending,
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    let initial_world = world.activation_snapshot();
    let mut tick_states = vec![];
    let mut record_state = |tick: u32, world: &PlotWorld, projected: &PlotWorld, compiler: &Compiler| {
        let runtime = compiler.runtime.as_ref().unwrap();
        let native_pending: Vec<_> = world.scheduler().iter_entries().collect();
        let rust_pending = runtime.pending_entries();
        let mut diffs = vec![];
        let mut entity_diffs = vec![];
        let mut initial_diffs = vec![];
        for y in f.min.y..=f.max.y {
            for z in f.min.z..=f.max.z {
                for x in f.min.x..=f.max.x {
                    let p = BlockPos::new(x, y, z);
                    let a = world.get_block(p);
                    let original = initial_world.get_block(p);
                    if original != a {
                        initial_diffs.push(json!({"pos":p,"initial":format!("{original:?}"),"current":format!("{a:?}")}));
                    }
                    let b = projected.get_block(p);
                    if a != b {
                        diffs.push(json!({"pos":p,"interpreter":format!("{a:?}"),"compiled":format!("{b:?}")}));
                    }
                    let a = world.get_block_entity(p).map(|e| serde_json::to_value(e).unwrap());
                    let b = projected.get_block_entity(p).map(|e| serde_json::to_value(e).unwrap());
                    if a != b {
                        entity_diffs.push(json!({"pos":p,"interpreter":a,"compiled":b}));
                    }
                }
            }
        }
        let row = json!({"tick":tick,"native_pending":native_pending,"compiled_pending":rust_pending,"differences":diffs,"entity_differences":entity_diffs,"initial_differences":initial_diffs});
        println!(
            "PM1 tick {tick}: blocks={} entities={} pending native={} rust={}",
            diffs.len(),
            entity_diffs.len(),
            native_pending.len(),
            rust_pending.len()
        );
        tick_states.push(row);
    };
    let input_native = crate::redstone::instant_piston_tests::capture_all(|| {
        assert!(crate::interaction::use_redstone(world.get_block(f.start), &mut world, f.start, None).is_some());
    });
    let input_rust = trace(|| compiler.try_on_use_block(f.start).unwrap());
    compiler.try_flush(&mut projected).unwrap();
    write(&root, "native-input.json", &input_native);
    write(&root, "rust-input.json", &input_rust);
    record_state(0, &world, &projected, &compiler);
    let ticks = std::env::var("PM1_CAUSAL_TICKS").ok().map(|v| v.parse::<u32>().unwrap()).unwrap_or(10);
    assert!((10..=20).contains(&ticks));
    for tick in 1..=ticks {
        let mut detail = vec![];
        let native = crate::redstone::instant_piston_tests::capture_all(|| {
            detail = crate::redstone::piston::trace::capture_detailed(&actors, || world.tick_interpreted());
        });
        let rust = trace(|| compiler.tick().unwrap());
        compiler.try_flush(&mut projected).unwrap();
        write(&root, &format!("native-{tick}.json"), &native);
        write(&root, &format!("native-pistons-{tick}.json"), &detail);
        write(&root, &format!("rust-{tick}.json"), &rust);
        record_state(tick, &world, &projected, &compiler);
    }
    write(&root, "states.json", &tick_states);
}

#[test]
#[ignore = "full PM1 interpreter/compiled instruction, halt and final-state diagnostic"]
fn pm1_full_replay_effects() {
    use super::cpu_trace::cpus;
    let root = std::env::var_os("PM1_REPLAY_OUTPUT")
        .map(std::path::PathBuf::from)
        .expect("PM1_REPLAY_OUTPUT must name a fresh directory");
    assert!(!root.exists(), "fresh output directory required");
    std::fs::create_dir_all(&root).unwrap();
    let cpu = cpus::Cpu {
        schematic: "PM1_SORT_FIXED.schem",
        sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
        ..cpus::CPUS[0]
    };
    let mut native = cpus::load_cpu(cpu);
    let (mut projected, mut compiler, elapsed) = cpus::compile_cpu_with_options(
        native.activation_snapshot(),
        cpu,
        CompilerOptions { publication: Some(PublicationMode::All), ..Default::default() },
    );
    eprintln!("PM1 replay compile: {elapsed:?}");
    let positions = compiler.runtime.as_ref().unwrap().program.projection_positions.clone();
    let mut native_chat = Vec::new();
    let mut compiled_chat = Vec::new();
    let mut emission_cursor = 0;
    let mut checkpoints = Vec::new();
    let mut native_quiet = None;
    let mut compiled_quiet = None;
    cpus::click_cpu(&mut native, cpu, cpu.start);
    compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
    for tick in 1..=50_000 {
        native.tick_interpreted();
        compiler.tick().unwrap();
        cpus::collect_chat(&native, tick, &mut native_chat);
        let runtime = compiler.runtime.as_ref().unwrap();
        compiled_chat.extend(runtime.command_emissions[emission_cursor..].iter().map(|event| {
            (event.tick as u32, event.message.message.clone())
        }));
        emission_cursor = runtime.command_emissions.len();
        if tick >= cpu.active_ticks {
            if native_quiet.is_none() && native.scheduler().iter_entries().next().is_none()
                && native.piston_state().events.is_empty() && native.piston_state().motions.is_empty()
            {
                native_quiet = Some(tick);
            }
            if compiled_quiet.is_none() && runtime.pending_entries().is_empty()
                && runtime.pending_piston_events().is_empty() && !runtime.has_motions()
            {
                compiled_quiet = Some(tick);
            }
        }
        if [15, 16, 20, 100, 300, 500, 1000, 5000, 10_000, 12_051, 12_055, 20_000, 50_000].contains(&tick) {
            compiler.try_flush(&mut projected).unwrap();
            emission_cursor = 0;
            let runtime = compiler.runtime.as_ref().unwrap();
            let mut differences = Vec::new();
            let mut entity_differences = Vec::new();
            for &pos in &positions {
                let a = native.get_block(pos);
                let b = projected.get_block(pos);
                if a != b {
                    differences.push(json!({"pos":pos,"interpreter":format!("{a:?}"),"compiled":format!("{b:?}")}));
                }
                let a = native.get_block_entity(pos).map(|e| serde_json::to_value(e).unwrap());
                let b = projected.get_block_entity(pos).map(|e| serde_json::to_value(e).unwrap());
                if a != b {
                    entity_differences.push(json!({"pos":pos,"interpreter":a,"compiled":b}));
                }
            }
            eprintln!("PM1 replay tick {tick}: messages={}/{} blocks={} entities={}", native_chat.len(), compiled_chat.len(), differences.len(), entity_differences.len());
            checkpoints.push(json!({"tick":tick,"differences":differences,"entity_differences":entity_differences,
                "native_messages":native_chat.len(),"compiled_messages":compiled_chat.len(),
                "native_pending":native.scheduler().iter_entries().collect::<Vec<_>>(),"compiled_pending":runtime.pending_entries(),
                "native_piston_events":native.piston_state().events,"compiled_piston_events":runtime.pending_piston_events()}));
        }
        if tick % 1000 == 0 {
            eprintln!("PM1 replay reached {tick}: messages={}/{}", native_chat.len(), compiled_chat.len());
        }
    }
    // Keep emissions intact until their tick-stamped trace is collected.
    let bounds = projected.get_corners();
    compiler.try_reset(&mut projected, bounds).unwrap();
    let native_final = cpus::checkpoint(&native, 50_000, &native_chat);
    let compiled_final = cpus::checkpoint(&projected, 50_000, &compiled_chat);
    write(&root, "native-chat.json", &native_chat);
    write(&root, "compiled-chat.json", &compiled_chat);
    write(&root, "checkpoints.json", &checkpoints);
    write(&root, "result.json", &json!({
        "schematic_sha256":cpu.sha256,"ticks":50_000,"compile_ms":elapsed.as_millis(),
        "native_first_quiet_after_halt":native_quiet,"compiled_first_quiet_after_halt":compiled_quiet,
        "native_final":native_final,"compiled_final":compiled_final,
        "exact_chat_equal":native_chat==compiled_chat,
        "message_order_equal":native_chat.iter().map(|(_,s)|s).eq(compiled_chat.iter().map(|(_,s)|s)),
    }));
    let expected = cpus::reference(cpus::CPUS[0]);
    let expected: Vec<_> = expected.chat_trace.into_iter().filter(|(tick, _)| *tick <= 50_000).collect();
    assert_eq!(native_chat, expected, "interpreter instruction trace changed from frozen reference");
    assert_eq!(projected.command_output().collect::<Vec<_>>(), compiled_chat.iter().map(|(_,s)|s.as_str()).collect::<Vec<_>>());
    eprintln!("PM1 replay artifacts: {}", root.display());
}

#[test]
#[ignore = "capture PM1 lamps at completion for independent screen decoding"]
fn pm1_final_screen_capture() {
    use super::cpu_trace::cpus;
    let root = std::path::PathBuf::from(std::env::var_os("PM1_SCREEN_OUTPUT").expect("PM1_SCREEN_OUTPUT required"));
    assert!(!root.exists(), "fresh output directory required");
    std::fs::create_dir_all(&root).unwrap();
    let cpu = cpus::Cpu {
        schematic: "PM1_SORT_FIXED.schem",
        sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
        ..cpus::CPUS[0]
    };
    let clip = crate::plot::worldedit::load_schematic(std::fs::File::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic),
    ).unwrap()).unwrap();
    let mut lamps = Vec::new();
    for y in 0..clip.size_y {
        for z in 0..clip.size_z {
            for x in 0..clip.size_x {
                let id = clip.data.get_entry(((y * clip.size_z + z) * clip.size_x + x) as usize);
                if let Block::RedstoneLamp { lit } = Block::from_id(id) {
                    lamps.push((cpu.origin + BlockPos::new(x as i32, y as i32, z as i32), lit));
                }
            }
        }
    }
    let mut world = cpus::load_cpu(cpu);
    let screen = cpus::Cpu { screen: Some(cpus::Screen {
        x: 230, top_y: 71, bottom_y: 40, left_z: 81, right_z: 112, sparse: false,
    }), ..cpu };
    let mut frames = Vec::new();
    cpus::collect_screen(&world, screen, 0, &mut frames);
    cpus::click_cpu(&mut world, cpu, cpu.start);
    for tick in 1..=12_055 {
        world.tick_interpreted();
        cpus::collect_screen(&world, screen, tick, &mut frames);
        if tick % 1000 == 0 { eprintln!("PM1 screen reached {tick}"); }
    }
    let mut chat = Vec::new();
    cpus::collect_chat(&world, 12_055, &mut chat);
    let final_lamps: Vec<_> = lamps.iter().map(|(pos, _)| {
        let Block::RedstoneLamp { lit } = world.get_block(*pos) else { panic!("lamp removed at {pos:?}"); };
        (*pos, lit)
    }).collect();
    write(&root, "lamps.json", &json!({"schematic_sha256":cpu.sha256,"initial":lamps,"final":final_lamps}));
    write(&root, "screen-frames.json", &frames);
    write(&root, "final-checkpoint.json", &cpus::checkpoint(&world, 12_055, &chat));
    assert_eq!(chat.len(), 1534);
    assert!(chat.last().unwrap().1.contains("shut"));
    assert!(world.scheduler().iter_entries().next().is_none());
    assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
}
