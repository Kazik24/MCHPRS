//! Minimal command-block lifecycle, sharing ordinary typed scheduled ticks.
use crate::messages;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::TickPriority;

pub(crate) trait CommandBlockHost {
    fn block(&self, pos: BlockPos) -> mchprs_blocks::blocks::Block;
    fn entity(&self, pos: BlockPos) -> Option<&CommandBlockEntity>;
    fn entity_mut(&mut self, pos: BlockPos) -> Option<&mut CommandBlockEntity>;
    fn create_entity(&mut self, pos: BlockPos);
    fn powered(&self, pos: BlockPos) -> bool;
    fn pending(&mut self, pos: BlockPos) -> bool;
    fn schedule(&mut self, pos: BlockPos);
    fn logical_tick(&self) -> u64;
    fn execute(&mut self, pos: BlockPos, command: &str, source: &str) -> Result<(), String>;
    fn notify(&mut self, pos: BlockPos);
}

impl<W: World> CommandBlockHost for W {
    fn block(&self, pos: BlockPos) -> mchprs_blocks::blocks::Block {
        self.get_block(pos)
    }
    fn entity(&self, pos: BlockPos) -> Option<&CommandBlockEntity> {
        match self.get_block_entity(pos) {
            Some(BlockEntity::CommandBlock(entity)) => Some(entity),
            _ => None,
        }
    }
    fn entity_mut(&mut self, pos: BlockPos) -> Option<&mut CommandBlockEntity> {
        match self.get_block_entity_mut(pos) {
            Some(BlockEntity::CommandBlock(entity)) => Some(entity),
            _ => None,
        }
    }
    fn create_entity(&mut self, pos: BlockPos) {
        self.set_block_entity(pos, BlockEntity::CommandBlock(Box::default()));
    }
    fn powered(&self, pos: BlockPos) -> bool {
        BlockFace::values().into_iter().any(|face| {
            let neighbor = pos.offset(face);
            super::has_redstone_power(self.get_block(neighbor), self, neighbor, face)
        })
    }
    fn pending(&mut self, pos: BlockPos) -> bool {
        self.pending_tick_at(pos)
    }
    fn schedule(&mut self, pos: BlockPos) {
        self.schedule_half_tick(pos, 1, TickPriority::Normal);
    }
    fn logical_tick(&self) -> u64 {
        self.piston_state().logical_tick
    }
    fn execute(&mut self, pos: BlockPos, command: &str, source: &str) -> Result<(), String> {
        self.execute_command_block_at(pos, command, source)
    }
    fn notify(&mut self, pos: BlockPos) {
        super::update_surrounding_blocks(self, pos);
        super::comparator::update_far_neighbors(self, pos);
    }
}

fn facing(world: &impl CommandBlockHost, pos: BlockPos) -> BlockFace {
    world.block(pos).facing().unwrap_or(BlockFace::North)
}

fn condition_met(world: &impl CommandBlockHost, pos: BlockPos) -> bool {
    if world.block(pos).conditional() != Some(true) {
        return true;
    }
    let previous = pos.offset(facing(world, pos).opposite());
    world.block(previous).is_command_block() && world.entity(previous).is_some_and(|entity| entity.success_count > 0)
}

pub(crate) fn plain_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(values) => values.iter().map(plain_text).collect(),
        serde_json::Value::Object(object) => {
            let text = object.get("text").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
            let extra = object.get("extra").map(plain_text).unwrap_or_default();

            text + &extra
        }
        _ => String::new(),
    }
}

pub(crate) fn command_source(custom_name: &str) -> String {
    serde_json::from_str(custom_name)
        .map(|value| plain_text(&value))
        .unwrap_or_else(|_| "@".into())
}

pub(crate) fn update(world: &mut impl CommandBlockHost, pos: BlockPos) {
    let block = world.block(pos);
    if !block.is_command_block() {
        return;
    }
    if world.entity(pos).is_none() {
        world.create_entity(pos);
    }
    let powered = world.powered(pos);
    let Some(entity) = world.entity_mut(pos) else {
        return;
    };
    let rising = powered && !entity.powered;
    entity.powered = powered;
    let start = rising || (entity.automatic && entity.last_execution < 0);
    let repeating = block.get_name() == "repeating_command_block" && (powered || entity.automatic);
    if block.get_name() != "chain_command_block" && (start || repeating) && !world.pending(pos) {
        let met = condition_met(world, pos);
        if let Some(entity) = world.entity_mut(pos) {
            entity.condition_met = met;
        }
        // Command blocks wait one game tick, rather than one two-game-tick redstone delay.
        world.schedule(pos);
    }
}

fn execute(world: &mut impl CommandBlockHost, pos: BlockPos, notify: bool) -> bool {
    let Some(stored) = world.entity(pos) else {
        return false;
    };
    let mut entity: CommandBlockEntity = stored.clone();
    let tick = world.logical_tick().min(i64::MAX as u64) as i64;
    if entity.update_last_execution && entity.last_execution == tick {
        return false;
    }
    if world.block(pos).get_name() == "chain_command_block" {
        entity.condition_met = condition_met(world, pos);
    }
    entity.success_count = 0;
    if entity.condition_met && !entity.command.trim().is_empty() {
        let source = command_source(&entity.custom_name);
        match world.execute(pos, &entity.command, &source) {
            Ok(()) => {
                entity.success_count = 1;
                if entity.track_output {
                    entity.last_output = Some(serde_json::json!({"text":messages::COMMAND_BLOCK_EXECUTED}).to_string());
                }
            }
            Err(error) => {
                if entity.track_output {
                    entity.last_output = Some(serde_json::json!({"text":error,"color":"red"}).to_string());
                }
            }
        }
    }
    if entity.update_last_execution {
        entity.last_execution = tick;
    }
    if let Some(stored) = world.entity_mut(pos) {
        *stored = entity;
    }
    if notify {
        world.notify(pos);
    }
    true
}

pub(crate) fn tick(world: &mut impl CommandBlockHost, pos: BlockPos) {
    let executed = tick_inner(world, pos, true);
    if !executed.is_empty() && world.block(pos).get_name() == "repeating_command_block" {
        update(world, pos);
    }
}

fn tick_inner(world: &mut impl CommandBlockHost, pos: BlockPos, notify: bool) -> Vec<BlockPos> {
    let name = world.block(pos).get_name();
    if name == "chain_command_block" {
        return Vec::new();
    }
    let Some(_) = world.entity(pos) else {
        return Vec::new();
    };
    // A queued activation still executes if power disappears before its callback.
    if !execute(world, pos, notify) {
        return Vec::new();
    }
    let mut executed = vec![pos];
    if !world.entity(pos).is_some_and(|entity| entity.condition_met) {
        return executed;
    }
    let mut current = pos;
    // Bound chains even when UpdateLastExecution is disabled or an imported chain loops.
    for _ in 0..256 {
        let next = current.offset(facing(world, current));
        if world.block(next).get_name() != "chain_command_block" {
            break;
        }
        let Some(entity) = world.entity(next) else {
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
