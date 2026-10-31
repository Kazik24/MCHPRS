use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::{
    BlockFace, BlockPos,
    blocks::{Block, ButtonFace},
};
use mchprs_core::{
    plot::{
        PLOT_BLOCK_WIDTH, PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    redstone,
    world::{World, storage::Chunk},
};
use mchprs_world::TickPriority;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use std::{fs::File, path::Path};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Cpu {
    pub name: &'static str,
    pub schematic: &'static str,
    pub sha256: &'static str,
    pub start: BlockPos,
    pub stop: Option<BlockPos>,
    pub origin: BlockPos,
    pub active_ticks: u32,
    pub screen: Option<Screen>,
}

#[derive(Clone, Copy)]
pub struct Screen {
    pub x: i32,
    pub top_y: i32,
    pub bottom_y: i32,
    pub left_z: i32,
    pub right_z: i32,
    pub sparse: bool,
}

pub const CPUS: [Cpu; 4] = [
    Cpu {
        name: "pm1_sort",
        schematic: "instant-pistons/PM1_SORT.schem",
        sha256: "e338028d50a4400056e25037d1f43d37c08baed079edede6298f78b0a6341654",
        start: BlockPos::new(187, 35, 72),
        stop: Some(BlockPos::new(187, 32, 72)),
        origin: BlockPos::new(8, 8, 8),
        active_ticks: 12_051,
        screen: None,
    },
    Cpu {
        name: "anpu_pong",
        schematic: "Q2CK@Q2CK_Anpu1_Pong_KBTV.schem",
        sha256: "f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec",
        start: BlockPos::new(122, 68, 56),
        stop: None,
        origin: BlockPos::new(8, 8, 8),
        active_ticks: 5_000,
        screen: Some(Screen {
            x: 148,
            top_y: 97,
            bottom_y: 66,
            left_z: 46,
            right_z: 77,
            sparse: false,
        }),
    },
    Cpu {
        name: "cpu_bubblesort",
        schematic: "piston-research/cpu-bubblesort/CPU_BubbleSort.schem",
        sha256: "803dbe5d6751b152bea9b134d6c8571cfafa3e840e372f82e41a535b658f1611",
        start: BlockPos::new(166, 7, 156),
        stop: None,
        origin: BlockPos::new(2, 8, 2),
        active_ticks: 10_035,
        screen: None,
    },
    Cpu {
        name: "zcore12b_sierpinski",
        schematic: "ZCore12B_FIX.schem",
        sha256: "7f074b967a043f36915f5980cbd4da54f62e45832cc385c1ce53fb21dbe9bac8",
        start: BlockPos::new(194, 16, 120),
        stop: None,
        origin: BlockPos::new(8, 8, 8),
        active_ticks: 50_000,
        screen: Some(Screen {
            x: 219,
            top_y: 88,
            bottom_y: 50,
            left_z: 10,
            right_z: 71,
            sparse: true,
        }),
    },
];

pub const CHECKPOINTS: [u32; 9] = [0, 10, 100, 1000, 5000, 10_000, 20_000, 30_000, 50_000];

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    pub tick: u32,
    pub blocks: String,
    pub entities: String,
    pub scheduler: String,
    pub pistons: String,
    pub chat: String,
    pub messages: usize,
    pub lit_lamps: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reference {
    pub schema: u32,
    pub schematic_sha256: String,
    pub checkpoints: Vec<Checkpoint>,
    pub chat_trace: Vec<(u32, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ram_trace: Vec<(u32, Vec<Option<u16>>)>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenFrame {
    pub tick: u32,
    /// Row-major lamp pixels. The upper row is first; sparse screens omit gaps.
    pub pixels: String,
}

pub fn screen(world: &PlotWorld, cpu: Cpu) -> String {
    let screen = cpu.screen.expect("CPU has no screen");
    let mut pixels = String::new();
    for y in (screen.bottom_y..=screen.top_y).rev() {
        for z in screen.left_z..=screen.right_z {
            let pos = BlockPos::new(screen.x, y, z);
            match world.get_block(pos) {
                Block::RedstoneLamp { lit } => pixels.push(if lit { '1' } else { '0' }),
                _ if screen.sparse => {},
                _ => panic!("screen pixel missing at {pos}"),
            }
        }
    }
    pixels
}

pub fn collect_screen(world: &PlotWorld, cpu: Cpu, tick: u32, trace: &mut Vec<ScreenFrame>) {
    let pixels = screen(world, cpu);
    if trace.last().is_none_or(|frame| frame.pixels != pixels) {
        trace.push(ScreenFrame { tick, pixels });
    }
}

pub fn checkpoint(world: &PlotWorld, tick: u32, chat: &[(u32, String)]) -> Checkpoint {
    let mut blocks = Sha256::new();
    let mut entities = Sha256::new();
    let mut lit_lamps = 0;
    // Include section coordinates and empty sections: no dependency on palette layout.
    for chunk in world.get_chunks() {
        blocks.update(chunk.x.to_le_bytes());
        blocks.update(chunk.z.to_le_bytes());
        for (section_y, section) in chunk.sections.iter().enumerate() {
            blocks.update(section.block_count().to_le_bytes());
            if section.block_count() == 0 {
                continue;
            }
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let id = chunk.get_block(x, section_y as u32 * 16 + y, z);
                        blocks.update(id.to_le_bytes());
                        lit_lamps += usize::from(matches!(Block::from_id(id), Block::RedstoneLamp { lit: true }));
                    }
                }
            }
        }
        let mut entries: Vec<_> = chunk
            .block_entities
            .iter()
            .map(|(pos, entity)| {
                let mut entity = entity.clone();
                if let BlockEntity::Container { inventory, .. } = &mut entity {
                    for item in inventory {
                        if let Some(bytes) = &mut item.nbt {
                            let blob = nbt::Blob::from_reader(&mut std::io::Cursor::new(&*bytes)).unwrap();
                            // Compound field order has no meaning; import reserializes HashMaps.
                            *bytes = serde_json::to_vec(&canonical_nbt(&nbt::Value::Compound(blob.content))).unwrap();
                        }
                    }
                }
                (pos, entity)
            })
            .collect();
        entries.sort_by_key(|(p, _)| (p.x, p.y, p.z));
        entities.update(bincode::serialize(&(chunk.x, chunk.z, entries)).unwrap());
    }
    Checkpoint {
        tick,
        blocks: format!("{:x}", blocks.finalize()),
        entities: format!("{:x}", entities.finalize()),
        scheduler: digest(&world.scheduler().iter_entries().collect::<Vec<_>>()),
        pistons: digest(world.piston_state()),
        chat: digest(chat),
        messages: chat.len(),
        lit_lamps,
    }
}

fn canonical_nbt(value: &nbt::Value) -> serde_json::Value {
    use nbt::Value;
    use serde_json::json;
    match value {
        Value::Compound(values) => {
            let values: std::collections::BTreeMap<_, _> = values.iter().map(|(name, value)| (name, canonical_nbt(value))).collect();
            json!(["compound", values])
        }
        Value::List(values) => {
            json!(["list", values.iter().map(canonical_nbt).collect::<Vec<_>>()])
        }
        Value::Byte(v) => json!(["byte", v]),
        Value::Short(v) => json!(["short", v]),
        Value::Int(v) => json!(["int", v]),
        Value::Long(v) => json!(["long", v]),
        Value::Float(v) => json!(["float", v.to_bits()]),
        Value::Double(v) => json!(["double", v.to_bits()]),
        Value::ByteArray(v) => json!(["byte_array", v]),
        Value::String(v) => json!(["string", v]),
        Value::IntArray(v) => json!(["int_array", v]),
        Value::LongArray(v) => json!(["long_array", v]),
    }
}

fn digest(value: &(impl Serialize + ?Sized)) -> String {
    format!("{:x}", Sha256::digest(bincode::serialize(value).unwrap()))
}

pub fn load_cpu(cpu: Cpu) -> PlotWorld {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(path).unwrap())),
        cpu.sha256,
        "frozen schematic changed"
    );
    let mut world = load_at(cpu.schematic, cpu.origin);
    // Frozen traces measure simulated execution, independent of wall-clock speed
    // and the live client's bounded chat queue. Retain every emitted message.
    world.disable_command_output_limits_for_replay();
    if cpu.name == "cpu_bubblesort" {
        let pos = cpu.origin + BlockPos::new(150, 17, 119);
        let Block::Lever { mut lever } = world.get_block(pos) else {
            panic!("missing load lever")
        };
        assert!(!lever.powered);
        lever.powered = true;
        world.set_block(pos, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut world, pos);
        redstone::update_surrounding_blocks(&mut world, pos.offset(BlockFace::Bottom));
        wait_quiet(&mut world);
        click_cpu(&mut world, cpu, BlockPos::new(166, 7, 158));
        wait_quiet(&mut world);
        assert_eq!(world.piston_state().logical_tick, 364, "frozen load/reset protocol");
    }
    world
}

fn quiet(world: &PlotWorld) -> bool {
    world.scheduler().iter_entries().next().is_none() && world.piston_state().events.is_empty() && world.piston_state().motions.is_empty()
}

fn wait_quiet(world: &mut PlotWorld) {
    let mut consecutive = 0;
    for _ in 0..4096 {
        world.tick_interpreted();
        consecutive = if quiet(world) { consecutive + 1 } else { 0 };
        if consecutive == 20 {
            return;
        }
    }
    panic!("CPU preparation did not settle within 4096 ticks");
}

pub fn click_cpu(world: &mut PlotWorld, cpu: Cpu, local: BlockPos) {
    click(world, local + cpu.origin - BlockPos::new(8, 8, 8));
}

pub fn collect_ram(world: &PlotWorld, cpu: Cpu, tick: u32, trace: &mut Vec<(u32, Vec<Option<u16>>)>) {
    if cpu.name != "cpu_bubblesort" {
        return;
    }
    let words = [7, 17, 27, 37]
        .into_iter()
        .flat_map(|y| (0..16).map(move |row| (y, row)))
        .map(|(y, row)| {
            (0..12).try_fold(0, |word, bit| {
                match world.get_block(cpu.origin + BlockPos::new(175 + 4 * bit, y, 62 - 4 * row)) {
                    Block::Piston { piston } => Some(word | (u16::from(!piston.extended) << bit)),
                    _ => None,
                }
            })
        })
        .collect::<Vec<_>>();
    if trace.last().is_none_or(|(_, previous)| *previous != words) {
        trace.push((tick, words));
    }
}

pub fn collect_chat(world: &PlotWorld, tick: u32, trace: &mut Vec<(u32, String)>) {
    let new: Vec<_> = world.command_output().skip(trace.len()).map(|text| (tick, text.to_owned())).collect();
    trace.extend(new);
}

pub fn reference(cpu: Cpu) -> Reference {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/cpu-references")
        .join(format!("{}.json", cpu.name));
    let mut reference: Reference = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(reference.schema, 1);
    assert_eq!(reference.schematic_sha256, cpu.sha256);
    assert_eq!(
        reference
            .checkpoints
            .iter()
            .take(CHECKPOINTS.len())
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        CHECKPOINTS
    );
    // Preserve the original references and apply only the block-state changes
    // captured from the interpreter before this performance work. Every other
    // checkpoint field and output assertion remains the original reference.
    #[derive(Deserialize)]
    struct BlockPatch {
        tick: u32,
        before: String,
        after: String,
    }
    #[derive(Deserialize)]
    struct Migration {
        schema: u32,
        cpus: std::collections::BTreeMap<String, Vec<BlockPatch>>,
    }
    let migration: Migration = serde_json::from_str(include_str!("../../../test_data/cpu-references/piston-shape-checkpoints.json")).unwrap();
    assert_eq!(migration.schema, 1);
    for patch in migration.cpus.get(cpu.name).into_iter().flatten() {
        let checkpoint = reference
            .checkpoints
            .iter_mut()
            .find(|checkpoint| checkpoint.tick == patch.tick)
            .expect("baseline migration checkpoint missing");
        assert_eq!(checkpoint.blocks, patch.before);
        checkpoint.blocks.clone_from(&patch.after);
    }
    reference
}

pub fn compile_cpu(world: PlotWorld, cpu: Cpu) -> (PlotWorld, Compiler, Duration) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
    let clip = load_schematic(File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let ticks = world.scheduler().iter_entries().collect();
    let timeout = Duration::from_secs(
        std::env::var("MCHPRS_REDPILER_COMPILE_TIMEOUT_SECS")
            .ok()
            .and_then(|seconds| seconds.parse().ok())
            .unwrap_or(300),
    );
    let monitor = Arc::new(TaskMonitor::default());
    let worker_monitor = monitor.clone();
    let compile_started = Instant::now();
    let (world, compiler, result, timed_out) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let mut compiler = Compiler::default();
            let result = compiler.compile(&world, bounds, CompilerOptions::default(), ticks, worker_monitor);
            (world, compiler, result)
        });
        let mut timed_out = false;
        while !worker.is_finished() {
            if compile_started.elapsed() >= timeout && !monitor.cancelled() {
                timed_out = true;
                monitor.cancel();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let (world, compiler, result) = worker.join().unwrap();
        (world, compiler, result, timed_out)
    });
    assert!(!timed_out, "Redpiler compile timed out after {timeout:?}");
    result.unwrap_or_else(|error| panic!("Redpiler rejected {}: {error}", cpu.name));
    (world, compiler, compile_started.elapsed())
}

#[derive(Clone, Copy)]
pub enum ReplayStep {
    Start,
    Tick(u32),
    Stop,
    StopTick,
}

pub fn replay(
    world: &mut PlotWorld,
    cpu: Cpu,
    expected: &Reference,
    screen_only: bool,
    flush_every: u32,
    redpiler: bool,
    mut step: impl FnMut(&mut PlotWorld, ReplayStep),
) -> (u64, u64, u64) {
    world.set_screen_only(screen_only);
    let initial_visual_counts = world.visual_update_counts();
    let mut trace = Vec::new();
    let mut screen_trace = Vec::new();
    let mut ram_trace = Vec::new();
    collect_ram(world, cpu, 0, &mut ram_trace);
    if cpu.screen.is_some() {
        collect_screen(world, cpu, 0, &mut screen_trace);
    }
    assert_eq!(checkpoint(world, 0, &trace), expected.checkpoints[0], "{} initial state", cpu.name);
    step(world, ReplayStep::Start);
    let active_ticks = cpu.active_ticks;
    let mut next_checkpoint = 1;
    for tick in 1..=50_000 {
        step(world, ReplayStep::Tick(tick));
        if flush_every != 0 && tick % flush_every == 0 {
            world.flush_block_changes();
        }
        collect_chat(world, tick, &mut trace);
        collect_ram(world, cpu, tick, &mut ram_trace);
        if cpu.name == "cpu_bubblesort" && !redpiler && tick >= active_ticks {
            assert_eq!(quiet(world), tick > active_ticks, "BubbleSort stop boundary at {tick}");
        }
        if cpu.screen.is_some() {
            collect_screen(world, cpu, tick, &mut screen_trace);
        }
        if CHECKPOINTS.contains(&tick) {
            assert_eq!(
                checkpoint(world, tick, &trace),
                expected.checkpoints[next_checkpoint],
                "{} at tick {tick}",
                cpu.name
            );
            next_checkpoint += 1;
        }
    }
    if cpu.stop.is_some() {
        step(world, ReplayStep::Stop);
        for tick in 50_001..=50_100 {
            step(world, ReplayStep::StopTick);
            collect_chat(world, tick, &mut trace);
        }
        assert_eq!(
            checkpoint(world, 50_100, &trace),
            expected.checkpoints[next_checkpoint],
            "{} stop control",
            cpu.name
        );
        next_checkpoint += 1;
    }
    assert_eq!(next_checkpoint, expected.checkpoints.len());
    assert_eq!(ram_trace, expected.ram_trace, "{} per-tick RAM transitions", cpu.name);
    if cpu.name == "cpu_bubblesort" {
        let initial = &ram_trace[0].1;
        let final_words = &ram_trace.last().unwrap().1;
        assert_eq!(&initial[..48], &final_words[..48], "program and unused banks");
        assert_eq!(&final_words[48..], (0..16).map(Some).collect::<Vec<_>>());
    }
    assert_eq!(trace, expected.chat_trace, "{} ordered, tick-stamped command output", cpu.name);
    if cpu.name == "pm1_sort" {
        assert_eq!(trace.len(), 1535); // Includes the manual stop message after SORT halted.
        assert!(trace.iter().any(|(tick, text)| *tick == 12_051 && text.contains("shut")));
    }
    if cpu.screen.is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/cpu-references").join(format!("{}_screen.json", cpu.name));
        let frames: Vec<ScreenFrame> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(!frames.is_empty(), "CPU must have a captured screen frame");
        assert_eq!(screen_trace, frames, "{} exact per-game-tick screen output", cpu.name);
    }
    // A disconnected start button must not pass merely by reaching an idle final state.
    assert_ne!(expected.checkpoints[0].blocks, expected.checkpoints[3].blocks);
    assert_ne!(expected.checkpoints[2].blocks, expected.checkpoints[3].blocks);
    let final_counts = world.visual_update_counts();
    (
        final_counts.0 - initial_visual_counts.0,
        final_counts.1 - initial_visual_counts.1,
        final_counts.2 - initial_visual_counts.2,
    )
}
fn load_at(name: &str, origin: BlockPos) -> PlotWorld {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(name);
    let mut clip = load_schematic(File::open(path).unwrap()).unwrap();
    // Place the entire selection independent of WorldEdit's player origin.
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    assert!(origin.x >= 0 && origin.y >= 0 && origin.z >= 0);
    assert!(
        clip.size_x as i32 + origin.x <= PLOT_BLOCK_WIDTH
            && clip.size_z as i32 + origin.z <= PLOT_BLOCK_WIDTH
            && clip.size_y as i32 + origin.y <= 256
    );
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(&mut world, &clip, origin, false);
    world
}

pub fn click(world: &mut PlotWorld, local: BlockPos) {
    let pos = BlockPos::new(local.x + 8, local.y + 8, local.z + 8);
    let Block::StoneButton { mut button } = world.get_block(pos) else {
        panic!("missing button at {pos}")
    };
    assert!(!button.powered);
    button.powered = true;
    world.set_block(pos, Block::StoneButton { button });
    world.schedule_tick(pos, 10, TickPriority::Normal);
    redstone::update_surrounding_blocks(world, pos);
    let face = match button.face {
        ButtonFace::Floor => BlockFace::Bottom,
        ButtonFace::Ceiling => BlockFace::Top,
        ButtonFace::Wall => button.facing.opposite().block_face(),
    };
    redstone::update_surrounding_blocks(world, pos.offset(face));
}
