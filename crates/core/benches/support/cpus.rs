use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::{
    blocks::{Block, ButtonFace},
    BlockFace, BlockPos,
};
use mchprs_core::{
    plot::{
        worldedit::{load_schematic, paste_clipboard},
        PlotWorld, PLOT_WIDTH,
    },
    redstone,
    world::{storage::Chunk, World},
};
use mchprs_world::TickPriority;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use std::{fs::File, path::Path};

#[derive(Clone, Copy)]
pub struct Cpu {
    pub name: &'static str,
    pub schematic: &'static str,
    pub sha256: &'static str,
    pub start: BlockPos,
    pub stop: Option<BlockPos>,
    pub origin: BlockPos,
    pub active_ticks: u32,
}

pub const CPUS: [Cpu; 3] = [
    Cpu {
        name: "pm1_sort",
        schematic: "PM1_SORT.schem",
        sha256: "e338028d50a4400056e25037d1f43d37c08baed079edede6298f78b0a6341654",
        start: BlockPos::new(187, 35, 72),
        stop: Some(BlockPos::new(187, 32, 72)),
        origin: BlockPos::new(8, 8, 8),
        active_ticks: 12_051,
    },
    Cpu {
        name: "anpu_pong",
        schematic: "Q2CK@Q2CK_Anpu1_Pong_KBTV.schem",
        sha256: "f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec",
        start: BlockPos::new(122, 68, 56),
        stop: None,
        origin: BlockPos::new(8, 8, 8),
        active_ticks: 5_000,
    },
    Cpu {
        name: "cpu_bubblesort",
        schematic: "piston-research/cpu-bubblesort/CPU_BubbleSort.schem",
        sha256: "907e74d6c4882d4c7065d55e0f8ec4cca8885a34f7af90ca51c060953badaf2c",
        start: BlockPos::new(166, 7, 156),
        stop: None,
        origin: BlockPos::new(2, 8, 2),
        active_ticks: 10_035,
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
    /// Row-major 32x32 lamp pixels. The upper row is first.
    pub pixels: String,
}

pub fn screen(world: &PlotWorld) -> String {
    let mut pixels = String::with_capacity(1024);
    for y in (58..=89).rev() {
        for z in 38..=69 {
            let pos = BlockPos::new(148, y + 8, z + 8);
            let Block::RedstoneLamp { lit } = world.get_block(pos) else {
                panic!("screen pixel missing at {pos}");
            };
            pixels.push(if lit { '1' } else { '0' });
        }
    }
    pixels
}

pub fn collect_screen(world: &PlotWorld, tick: u32, trace: &mut Vec<ScreenFrame>) {
    let pixels = screen(world);
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
                        lit_lamps += usize::from(matches!(
                            Block::from_id(id),
                            Block::RedstoneLamp { lit: true }
                        ));
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
                            let blob =
                                nbt::Blob::from_reader(&mut std::io::Cursor::new(&*bytes)).unwrap();
                            // Compound field order has no meaning; import reserializes HashMaps.
                            *bytes = serde_json::to_vec(&canonical_nbt(&nbt::Value::Compound(
                                blob.content,
                            )))
                            .unwrap();
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
            let values: std::collections::BTreeMap<_, _> = values
                .iter()
                .map(|(name, value)| (name, canonical_nbt(value)))
                .collect();
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
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
        assert_eq!(
            world.piston_state().logical_tick,
            364,
            "frozen load/reset protocol"
        );
    }
    world
}

fn quiet(world: &PlotWorld) -> bool {
    world.scheduler().iter_entries().next().is_none()
        && world.piston_state().events.is_empty()
        && world.piston_state().motions.is_empty()
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

pub fn collect_ram(
    world: &PlotWorld,
    cpu: Cpu,
    tick: u32,
    trace: &mut Vec<(u32, Vec<Option<u16>>)>,
) {
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
    let new: Vec<_> = world
        .command_output()
        .skip(trace.len())
        .map(|text| (tick, text.to_owned()))
        .collect();
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
    let migration: Migration = serde_json::from_str(include_str!(
        "../../../../test_data/cpu-references/piston-shape-checkpoints.json"
    ))
    .unwrap();
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

/// Load and validate outside the measured region. Only interpreter tick calls are timed.
pub struct RunTiming {
    pub total: Duration,
    pub active: Duration,
    pub active_ticks: u32,
    pub visual_counts: (u64, u64, u64),
}

pub fn replay(cpu: Cpu, expected: &Reference) -> RunTiming {
    replay_with_visuals(cpu, expected, false, 0)
}

pub fn replay_with_visuals(
    cpu: Cpu,
    expected: &Reference,
    screen_only: bool,
    flush_every: u32,
) -> RunTiming {
    let mut world = load_cpu(cpu);
    if flush_every != 0 {
        // Flush imported initial blocks outside timing in both modes.
        world.flush_block_changes();
    }
    world.set_screen_only(screen_only);
    let initial_visual_counts = world.visual_update_counts();
    let mut trace = Vec::new();
    let mut screen_trace = Vec::new();
    let mut ram_trace = Vec::new();
    collect_ram(&world, cpu, 0, &mut ram_trace);
    if cpu.name == "anpu_pong" {
        collect_screen(&world, 0, &mut screen_trace);
    }
    assert_eq!(
        checkpoint(&world, 0, &trace),
        expected.checkpoints[0],
        "{} initial state",
        cpu.name
    );
    click_cpu(&mut world, cpu, cpu.start);
    let mut elapsed = Duration::ZERO;
    let active_ticks = cpu.active_ticks;
    let mut active = Duration::ZERO;
    let mut next_checkpoint = 1;
    for tick in 1..=50_000 {
        let now = Instant::now();
        world.tick_interpreted();
        if flush_every != 0 && tick % flush_every == 0 {
            world.flush_block_changes();
        }
        let tick_elapsed = now.elapsed();
        elapsed += tick_elapsed;
        if tick <= active_ticks {
            active += tick_elapsed;
        }
        collect_chat(&world, tick, &mut trace);
        collect_ram(&world, cpu, tick, &mut ram_trace);
        if cpu.name == "cpu_bubblesort" && tick >= active_ticks {
            assert_eq!(
                quiet(&world),
                tick > active_ticks,
                "BubbleSort stop boundary at {tick}"
            );
        }
        if cpu.name == "anpu_pong" {
            collect_screen(&world, tick, &mut screen_trace);
        }
        if CHECKPOINTS.contains(&tick) {
            assert_eq!(
                checkpoint(&world, tick, &trace),
                expected.checkpoints[next_checkpoint],
                "{} at tick {tick}",
                cpu.name
            );
            next_checkpoint += 1;
        }
    }
    if let Some(stop) = cpu.stop {
        click_cpu(&mut world, cpu, stop);
        for tick in 50_001..=50_100 {
            world.tick_interpreted();
            collect_chat(&world, tick, &mut trace);
        }
        assert_eq!(
            checkpoint(&world, 50_100, &trace),
            expected.checkpoints[next_checkpoint],
            "{} stop control",
            cpu.name
        );
        next_checkpoint += 1;
    }
    assert_eq!(next_checkpoint, expected.checkpoints.len());
    assert_eq!(
        ram_trace, expected.ram_trace,
        "{} per-tick RAM transitions",
        cpu.name
    );
    if cpu.name == "cpu_bubblesort" {
        let initial = &ram_trace[0].1;
        let final_words = &ram_trace.last().unwrap().1;
        assert_eq!(
            &initial[..48],
            &final_words[..48],
            "program and unused banks"
        );
        assert_eq!(&final_words[48..], (0..16).map(Some).collect::<Vec<_>>());
    }
    assert_eq!(
        trace, expected.chat_trace,
        "{} ordered, tick-stamped command output",
        cpu.name
    );
    if cpu.name == "pm1_sort" {
        assert_eq!(trace.len(), 1535); // Includes the manual stop message after SORT halted.
        assert!(trace
            .iter()
            .any(|(tick, text)| *tick == 12_051 && text.contains("shut")));
    }
    if cpu.name == "anpu_pong" {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/cpu-references/anpu_screen.json");
        let frames: Vec<ScreenFrame> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(frames.len() > 1, "Pong must draw on its screen");
        assert_eq!(
            screen_trace, frames,
            "ANPU exact per-game-tick screen output"
        );
    }
    // A disconnected start button must not pass merely by reaching an idle final state.
    assert_ne!(
        expected.checkpoints[0].blocks,
        expected.checkpoints[3].blocks
    );
    assert_ne!(
        expected.checkpoints[2].blocks,
        expected.checkpoints[3].blocks
    );
    RunTiming {
        total: elapsed,
        active,
        active_ticks,
        visual_counts: {
            let final_counts = world.visual_update_counts();
            (
                final_counts.0 - initial_visual_counts.0,
                final_counts.1 - initial_visual_counts.1,
                final_counts.2 - initial_visual_counts.2,
            )
        },
    }
}

fn load_at(name: &str, origin: BlockPos) -> PlotWorld {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(name);
    let mut clip = load_schematic(File::open(path).unwrap()).unwrap();
    // Place the entire selection independent of WorldEdit's player origin.
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    assert!(origin.x >= 0 && origin.y >= 0 && origin.z >= 0);
    assert!(
        clip.size_x as i32 + origin.x <= 256
            && clip.size_z as i32 + origin.z <= 256
            && clip.size_y as i32 + origin.y <= 256
    );
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
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
