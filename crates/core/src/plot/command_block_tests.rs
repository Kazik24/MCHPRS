use super::*;
use mchprs_blocks::block_entities::CommandBlockEntity;

fn world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}
#[test]
fn command_blocks_cannot_dispatch_privileged_commands_or_flood_output() {
    let mut world = world();
    for command in [
        "stop",
        "lp user Lord225 parent set admin",
        "execute run stop",
        "fill 0 0 0 1 1 1 air",
        "function attack",
        "whitelist add Fake",
    ] {
        assert!(
            world.execute_command_block(command, "@").is_err(),
            "{command}"
        );
    }
    for _ in 0..64 {
        world.execute_command_block("say hello", "@").unwrap();
    }
    assert!(world.execute_command_block("say overflow", "@").is_err());
    assert_eq!(world.command_output().count(), 64);
}
fn place(
    world: &mut PlotWorld,
    pos: BlockPos,
    name: &str,
    command: &str,
    auto: bool,
    conditional: bool,
) {
    let mut block = Block::from_name(name).unwrap();
    block.set_properties(std::collections::HashMap::from([
        ("facing", "east"),
        ("conditional", if conditional { "true" } else { "false" }),
    ]));
    world.set_block(pos, block);
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: command.into(),
            automatic: auto,
            ..Default::default()
        })),
    );
}
fn advance(world: &mut PlotWorld, ticks: usize) {
    for _ in 0..ticks {
        world.tick_interpreted();
    }
}
fn success(world: &PlotWorld, pos: BlockPos) -> i32 {
    match world.get_block_entity(pos).unwrap() {
        BlockEntity::CommandBlock(entity) => entity.success_count,
        _ => panic!(),
    }
}

#[test]
fn command_block_impulse_runs_once_per_edge_and_survives_short_pulses() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    place(
        &mut world,
        pos,
        "command_block",
        "say impulse",
        false,
        false,
    );
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    redstone::command_block::update(&mut world, pos);
    redstone::command_block::update(&mut world, pos);
    assert_eq!(world.scheduler().iter_entries().count(), 1);
    assert!(world.command_messages.is_empty());
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 1);
    assert_eq!(world.command_messages.len(), 1);
    assert!(world.command_messages[0].message.contains("[@] impulse"));
    assert_eq!(success(&world, pos), 1);
    advance(&mut world, 5);
    assert_eq!(world.command_messages.len(), 1);
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 2);
    assert_eq!(world.command_messages.len(), 2);
    advance(&mut world, 5);
    assert_eq!(world.command_messages.len(), 2);
}

#[test]
fn command_block_repeating_stops_and_stale_ticks_cannot_execute_replacements() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    place(
        &mut world,
        pos,
        "repeating_command_block",
        "tellraw @a \"repeating\"",
        false,
        false,
    );
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 6);
    assert_eq!(world.command_messages.len(), 6);
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 6);
    assert_eq!(world.command_messages.len(), 7);
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    redstone::command_block::update(&mut world, pos);
    world.delete_block_entity(pos);
    world.set_block(pos, Block::Stone {});
    advance(&mut world, 4);
    assert_eq!(world.command_messages.len(), 7);
}

#[test]
fn command_block_conditional_chains_and_unsupported_commands_are_bounded() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    let chain = pos.offset(BlockFace::East);
    place(&mut world, pos, "command_block", "say first", true, false);
    place(
        &mut world,
        chain,
        "chain_command_block",
        "say second",
        true,
        true,
    );
    place(
        &mut world,
        chain.offset(BlockFace::East),
        "chain_command_block",
        "scoreboard players add x y 1",
        true,
        true,
    );
    place(
        &mut world,
        chain.offset(BlockFace::East).offset(BlockFace::East),
        "chain_command_block",
        "say must not run",
        true,
        true,
    );
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 2);
    assert_eq!(world.command_messages.len(), 2);
    assert_eq!(success(&world, chain), 1);
    assert_eq!(success(&world, chain.offset(BlockFace::East)), 0);
    assert_eq!(
        success(
            &world,
            chain.offset(BlockFace::East).offset(BlockFace::East)
        ),
        0
    );
    advance(&mut world, 4);
    assert_eq!(world.command_messages.len(), 2);
    assert_eq!(
        redstone::comparator::get_override(world.get_block(pos), &world, pos),
        1
    );
    assert!(world.chunks.iter().any(Chunk::requires_interpreter));
}

#[test]
fn command_block_automatic_repeat_and_pending_ticks_survive_restart() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    place(
        &mut world,
        pos,
        "repeating_command_block",
        "say auto",
        true,
        false,
    );
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 2);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let state = world.piston_state.clone();
    let chunks = world
        .chunks
        .iter_mut()
        .enumerate()
        .map(|(i, chunk)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, chunk.save()))
        .collect();
    let mut resumed = PlotWorld::from_chunks(0, 0, chunks, ticks.into_iter().collect());
    resumed.piston_state = state;
    resumed.invalidate_interpreter_caches();
    advance(&mut resumed, 4);
    assert_eq!(resumed.command_messages.len(), 4);
    assert_eq!(success(&resumed, pos), 1);
}

#[test]
fn looping_chain_stops_at_the_command_limit() {
    let mut world = world();
    let start = BlockPos::new(40, 30, 39);
    place(&mut world, start, "command_block", "say start", true, false);
    let mut source = world.get_block(start);
    source.set_properties(std::collections::HashMap::from([("facing", "south")]));
    world.set_block(start, source);
    for (pos, face) in [
        (BlockPos::new(40, 30, 40), "east"),
        (BlockPos::new(41, 30, 40), "south"),
        (BlockPos::new(41, 30, 41), "west"),
        (BlockPos::new(40, 30, 41), "north"),
    ] {
        place(
            &mut world,
            pos,
            "chain_command_block",
            "say loop",
            true,
            false,
        );
        let mut block = world.get_block(pos);
        block.set_properties(std::collections::HashMap::from([("facing", face)]));
        world.set_block(pos, block);
        let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity_mut(pos) else {
            panic!()
        };
        entity.update_last_execution = false;
    }
    redstone::command_block::update(&mut world, start);
    advance(&mut world, 2);
    // The chain still terminates at 256 links; only 64 messages may be queued.
    assert_eq!(world.command_messages.len(), 64);
}

#[test]
fn failed_conditional_source_skips_chain_and_activation_latches_condition() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    let previous = pos.offset(BlockFace::West);
    place(
        &mut world,
        previous,
        "command_block",
        "say previous",
        false,
        false,
    );
    place(
        &mut world,
        pos,
        "command_block",
        "say conditional",
        true,
        true,
    );
    place(
        &mut world,
        pos.offset(BlockFace::East),
        "chain_command_block",
        "say chain",
        true,
        false,
    );
    redstone::command_block::update(&mut world, pos);
    advance(&mut world, 1);
    assert!(world.command_messages.is_empty());
    let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity_mut(previous) else {
        panic!()
    };
    entity.success_count = 1;
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    redstone::command_block::update(&mut world, pos);
    let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity_mut(previous) else {
        panic!()
    };
    entity.success_count = 0;
    advance(&mut world, 1);
    assert_eq!(
        world.command_messages.len(),
        2,
        "activation retains the condition observed at scheduling"
    );
}
