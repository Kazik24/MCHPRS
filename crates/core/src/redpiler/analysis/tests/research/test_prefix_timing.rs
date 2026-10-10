//! Opt-in timing inventory and interpreter/Redpiler output comparison for TEST_ fixtures.
use super::*;
use crate::redstone::instant_piston_tests::capture_at;
use crate::redstone::piston::trace;
use mchprs_blocks::blocks::Block;
use sha2::{Digest, Sha256};

const SOURCE: &str = "test_data/piston-research/test-prefix-20261008";

fn fixture(
    name: &str,
) -> (
    PlotWorld,
    Vec<BlockPos>,
    Vec<BlockPos>,
    (BlockPos, BlockPos),
) {
    let bytes = std::fs::read(root().join(SOURCE).join(name)).unwrap();
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let first = BASE;
    let last = first
        + BlockPos::new(
            clipboard.size_x as i32 - 1,
            clipboard.size_y as i32 - 1,
            clipboard.size_z as i32 - 1,
        );
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        first + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let mut levers = Vec::new();
    let mut traced = Vec::new();
    for y in first.y..=last.y {
        for z in first.z..=last.z {
            for x in first.x..=last.x {
                let pos = BlockPos::new(x, y, z);
                let block = world.get_block(pos);
                if matches!(block, Block::Lever { .. }) {
                    levers.push(pos);
                }
                if matches!(
                    block,
                    Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::StonePressurePlate { .. }
                        | Block::RedstoneWire { .. }
                        | Block::Piston { .. }
                        | Block::PistonHead { .. }
                        | Block::Observer { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::RedstoneLamp { .. }
                ) || block.is_copper_bulb()
                {
                    traced.push(pos);
                }
            }
        }
    }
    (world, levers, traced, (first, last))
}

fn event(entry: &trace::Entry) -> Value {
    let mut value = serde_json::to_value(entry).unwrap();
    let operation = value.get_mut("operation").unwrap();
    if let Some(sample) = operation.get_mut("Sample") {
        let pos: BlockPos = serde_json::from_value(sample["pos"].clone()).unwrap();
        sample["pos"] = json!(pos - BASE);
    } else if let Some(applied) = operation.get_mut("Applied") {
        let pos: BlockPos = serde_json::from_value(applied["pos"].clone()).unwrap();
        applied["pos"] = json!(pos - BASE);
    }
    value
}

fn state(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Vec<Value> {
    let mut blocks = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                let block = world.get_block(pos);
                if matches!(
                    block,
                    Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::StonePressurePlate { .. }
                        | Block::RedstoneWire { .. }
                        | Block::Piston { .. }
                        | Block::PistonHead { .. }
                        | Block::Observer { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::RedstoneLamp { .. }
                ) || block.is_copper_bulb()
                {
                    blocks.push(json!({"pos":pos-BASE,"name":block.get_name(),"properties":block.properties(),
                        "power":if let Block::Piston { piston }=block { Some(crate::redstone::piston::should_piston_extend(world,piston.facing,pos)) } else { None }}));
                }
            }
        }
    }
    blocks
}

fn ticks(world: &mut PlotWorld, bounds: (BlockPos, BlockPos), count: usize) -> Vec<Value> {
    (1..=count)
        .map(|step| {
            let operations = trace::capture(|| world.tick_interpreted());
            json!({"step":step,"operations":operations.iter().map(event).collect::<Vec<_>>(),
                "state":state(world,bounds),"pending_ticks":world.scheduler().iter_entries().count(),
                "piston_events":world.piston_state().events.len(),"motions":world.piston_state().motions.len()})
        })
        .collect()
}

fn baseline(name: &str, bounds: (BlockPos, BlockPos)) -> Value {
    let (mut world, _, traced, _) = fixture(name);
    let initial = state(&world, bounds);
    let mut idle = Vec::new();
    let callbacks = capture_at(&traced, || idle = ticks(&mut world, bounds, 12));
    json!({"initial":initial,"idle_callbacks":callbacks,"idle_ticks":idle,
        "idle_state":state(&world,bounds),"idle_pending_ticks":world.scheduler().iter_entries().count(),
        "idle_piston_events":world.piston_state().events.len(),"idle_motions":world.piston_state().motions.len()})
}

#[test]
#[ignore = "author TEST_ schematic native timing report; requires explicit output path"]
fn test_prefixed_schematics_have_native_timing_report() {
    let output =
        std::path::PathBuf::from(std::env::var("MCHPRS_TEST_PREFIX_TIMING_OUTPUT").unwrap());
    assert!(!output.exists(), "choose a new report output");
    let directory = root().join(SOURCE);
    let mut names: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("TEST_") && name.ends_with(".schem"))
        .collect();
    names.sort();
    let mut reports = Vec::new();
    for name in names {
        let bytes = std::fs::read(directory.join(&name)).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let (world, levers, traced, bounds) = fixture(&name);
        let lever_states: Vec<_> = levers
            .iter()
            .map(|&pos| matches!(world.get_block(pos), Block::Lever { lever } if lever.powered))
            .collect();
        let mechanisms: Vec<_> = traced
            .iter()
            .map(|&pos| {
                let block = world.get_block(pos);
                json!({"pos":pos-BASE,"name":block.get_name(),"properties":block.properties()})
            })
            .collect();
        let idle = baseline(&name, bounds);
        let mut compilation = Vec::new();
        for budget_multiplier in [1, 8] {
            for optimize in [false, true] {
                {
                    let (world, _, _, _) = fixture(&name);
                    let mut compiler = Compiler::default();
                    let started = std::time::Instant::now();
                    let result = compiler.compile(
                        &world,
                        world.get_corners(),
                        CompilerOptions {
                            budget_multiplier,
                            optimize,
                            ..Default::default()
                        },
                        vec![],
                        Default::default(),
                    );
                    compilation.push(json!({"budget_multiplier":budget_multiplier,"optimize":optimize,
                        "elapsed_seconds":started.elapsed().as_secs_f64(),
                        "result":result.map(|_| "compiled".to_owned()).unwrap_or_else(|error| error.to_string())}));
                }
            }
        }
        let mut combinations = Vec::new();
        let count = 1usize << levers.len();
        for mask in 0..count {
            let (mut world, _, traced, _) = fixture(&name);
            let mut action_edges = Vec::new();
            for (index, &pos) in levers.iter().enumerate() {
                let desired = mask & (1 << index) != 0;
                if desired != lever_states[index] {
                    action_edges.push(trace::capture(|| lever_action(&mut world, pos, desired)));
                }
            }
            let mut frames = Vec::new();
            let callbacks = capture_at(&traced, || frames = ticks(&mut world, bounds, 12));
            combinations.push(json!({"lever_mask":mask,"target_levers":levers.iter().enumerate().map(|(i,p)| json!({"pos":*p-BASE,"powered":mask&(1<<i)!=0})).collect::<Vec<_>>(),
                "action_edges":action_edges.iter().map(|x|x.iter().map(event).collect::<Vec<_>>()).collect::<Vec<_>>(),
                "callbacks":callbacks,"ticks":frames}));
        }
        let mut pulses = Vec::new();
        for (index, &pos) in levers.iter().enumerate() {
            let (mut world, _, traced, _) = fixture(&name);
            let start = lever_states[index];
            let mut idle_ticks = Vec::new();
            let idle_callbacks = capture_at(&traced, || idle_ticks = ticks(&mut world, bounds, 4));
            let rising = trace::capture(|| lever_action(&mut world, pos, !start));
            let first_tick = trace::capture(|| world.tick_interpreted());
            let cancelled = trace::capture(|| lever_action(&mut world, pos, start));
            let mut settle = Vec::new();
            let callbacks = capture_at(&traced, || settle = ticks(&mut world, bounds, 12));
            pulses.push(json!({"lever":pos-BASE,"saved_powered":start,"idle_ticks":idle_ticks,
                "idle_callbacks":idle_callbacks,"first_edge":rising.iter().map(event).collect::<Vec<_>>(),
                "one_tick_later":first_tick.iter().map(event).collect::<Vec<_>>(),
                "return_edge":cancelled.iter().map(event).collect::<Vec<_>>(),"callbacks":callbacks,"settle":settle}));
        }
        reports.push(json!({"name":name,"sha256":hash,"bounds":{"first":bounds.0-BASE,"last":bounds.1-BASE},
            "levers":levers.iter().enumerate().map(|(i,p)|json!({"pos":*p-BASE,"powered":lever_states[i]})).collect::<Vec<_>>(),
            "actuators_and_observers":mechanisms,"compilation":compilation,"idle":idle,
            "input_combinations":combinations,"one_game_tick_pulses":pulses}));
    }
    std::fs::write(output, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}
