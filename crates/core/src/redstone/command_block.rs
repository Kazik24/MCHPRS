//! Minimal command-block lifecycle, sharing ordinary typed scheduled ticks.
use crate::messages;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::TickPriority;

fn facing(world: &impl World, pos: BlockPos) -> BlockFace {
    match world.get_block(pos).property("facing") {
        Some("up") => BlockFace::Top,
        Some("down") => BlockFace::Bottom,
        Some("east") => BlockFace::East,
        Some("west") => BlockFace::West,
        Some("south") => BlockFace::South,
        _ => BlockFace::North,
    }
}

fn condition_met(world: &impl World, pos: BlockPos) -> bool {
    if world.get_block(pos).property("conditional") != Some("true") {
        return true;
    }
    let previous = pos.offset(facing(world, pos).opposite());
    world.get_block(previous).is_command_block()
        && matches!(world.get_block_entity(previous), Some(BlockEntity::CommandBlock(entity)) if entity.success_count > 0)
}

fn plain_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(values) => values.iter().map(plain_text).collect(),
        serde_json::Value::Object(object) => {
            let text = object
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned();
            let extra = object.get("extra").map(plain_text).unwrap_or_default();

            text + &extra
        }
        _ => String::new(),
    }
}

pub(crate) fn update(world: &mut impl World, pos: BlockPos) {
    let block = world.get_block(pos);
    if !block.is_command_block() {
        return;
    }
    if !matches!(
        world.get_block_entity(pos),
        Some(BlockEntity::CommandBlock(_))
    ) {
        world.set_block_entity(pos, BlockEntity::CommandBlock(Box::default()));
    }
    let powered = BlockFace::values().into_iter().any(|face| {
        let neighbor = pos.offset(face);
        super::has_redstone_power(world.get_block(neighbor), world, neighbor, face)
    });
    let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity_mut(pos) else {
        return;
    };
    let rising = powered && !entity.powered;
    entity.powered = powered;
    let start = rising || (entity.automatic && entity.last_execution < 0);
    let repeating = block.get_name() == "repeating_command_block" && (powered || entity.automatic);
    if block.get_name() != "chain_command_block"
        && (start || repeating)
        && !world.pending_tick_at(pos)
    {
        let met = condition_met(world, pos);
        if let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity_mut(pos) {
            entity.condition_met = met;
        }
        // Command blocks wait one game tick, rather than one two-game-tick redstone delay.
        world.schedule_half_tick(pos, 1, TickPriority::Normal);
    }
}

fn execute(world: &mut impl World, pos: BlockPos, notify: bool) -> bool {
    let Some(BlockEntity::CommandBlock(stored)) = world.get_block_entity(pos) else {
        return false;
    };
    let mut entity: CommandBlockEntity = (**stored).clone();
    let tick = world.piston_state().logical_tick.min(i64::MAX as u64) as i64;
    if entity.update_last_execution && entity.last_execution == tick {
        return false;
    }
    if world.get_block(pos).get_name() == "chain_command_block" {
        entity.condition_met = condition_met(world, pos);
    }
    entity.success_count = 0;
    if entity.condition_met && !entity.command.trim().is_empty() {
        let source = serde_json::from_str(&entity.custom_name)
            .map(|value| plain_text(&value))
            .unwrap_or_else(|_| "@".into());
        match world.execute_command_block(&entity.command, &source) {
            Ok(()) => {
                entity.success_count = 1;
                if entity.track_output {
                    entity.last_output = Some(
                        serde_json::json!({"text":messages::COMMAND_BLOCK_EXECUTED}).to_string(),
                    );
                }
            }
            Err(error) => {
                if entity.track_output {
                    entity.last_output =
                        Some(serde_json::json!({"text":error,"color":"red"}).to_string());
                }
            }
        }
    }
    if entity.update_last_execution {
        entity.last_execution = tick;
    }
    if let Some(BlockEntity::CommandBlock(stored)) = world.get_block_entity_mut(pos) {
        **stored = entity;
    }
    if notify {
        super::update_surrounding_blocks(world, pos);
        super::comparator::update_far_neighbors(world, pos);
    }
    true
}

pub(crate) fn tick(world: &mut impl World, pos: BlockPos) {
    let executed = tick_inner(world, pos, true);
    if !executed.is_empty() && world.get_block(pos).get_name() == "repeating_command_block" {
        update(world, pos);
    }
}

/// Compiled output activation uses the existing command allowlist, chain limits,
/// conditions and entity lifecycle, without restarting physical redstone.
pub(crate) fn tick_output(world: &mut impl World, pos: BlockPos) -> Vec<BlockPos> {
    tick_inner(world, pos, false)
}

pub(crate) fn set_output_power(
    world: &mut impl World,
    pos: BlockPos,
    powered: bool,
    capture_condition: bool,
) {
    if !world.get_block(pos).is_command_block() {
        return;
    }
    if !matches!(
        world.get_block_entity(pos),
        Some(BlockEntity::CommandBlock(_))
    ) {
        world.set_block_entity(pos, BlockEntity::CommandBlock(Box::default()));
    }
    let met = condition_met(world, pos);
    let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity_mut(pos) else {
        return;
    };
    entity.powered = powered;
    if capture_condition {
        entity.condition_met = met;
    }
}

fn tick_inner(world: &mut impl World, pos: BlockPos, notify: bool) -> Vec<BlockPos> {
    let name = world.get_block(pos).get_name();
    if name == "chain_command_block" {
        return Vec::new();
    }
    let Some(BlockEntity::CommandBlock(_)) = world.get_block_entity(pos) else {
        return Vec::new();
    };
    // A queued activation still executes if power disappears before its callback.
    if !execute(world, pos, notify) {
        return Vec::new();
    }
    let mut executed = vec![pos];
    if !matches!(world.get_block_entity(pos), Some(BlockEntity::CommandBlock(entity)) if entity.condition_met)
    {
        return executed;
    }
    let mut current = pos;
    // Bound chains even when UpdateLastExecution is disabled or an imported chain loops.
    for _ in 0..256 {
        let next = current.offset(facing(world, current));
        if world.get_block(next).get_name() != "chain_command_block" {
            break;
        }
        let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity(next) else {
            break;
        };
        if entity.powered || entity.automatic {
            if !execute(world, next, notify) {
                break;
            }
            executed.push(next);
        }
        current = next;
    }
    executed
}
