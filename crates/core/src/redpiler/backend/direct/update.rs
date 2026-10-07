use mchprs_world::TickPriority;

use super::node::{NodeId, NodeType};
use super::*;

/// Only block-state properties are observable; comparator and command-block
/// entity output strengths do not themselves change the watched block state.
pub(super) fn observed_state(node: &Node) -> (bool, bool, u8) {
    (
        !matches!(node.ty, NodeType::CommandBlock { .. } | NodeType::Constant | NodeType::InstantSource)
            && node.powered,
        matches!(node.ty, NodeType::Repeater { .. }) && node.locked,
        if matches!(node.ty, NodeType::Wire) { node.output_power } else { 0 },
    )
}

pub(super) fn notify_observers(
    scheduler: &mut TickScheduler<NodeId>,
    nodes: &mut Nodes,
    observers: &[NodeId],
) {
    for &id in observers {
        let node = &mut nodes[id];
        if !node.powered && !node.pending_tick {
            schedule_tick(scheduler, id, node, 1, TickPriority::Normal);
        }
    }
}

#[inline(always)]
pub(super) fn update_node(
    scheduler: &mut TickScheduler<NodeId>,
    events: &mut Vec<Event>,
    nodes: &mut Nodes,
    node_id: NodeId,
) -> bool {
    let before = observed_state(&nodes[node_id]);
    update_node_inner(scheduler, events, nodes, node_id);
    observed_state(&nodes[node_id]) != before
}

fn update_node_inner(
    scheduler: &mut TickScheduler<NodeId>,
    events: &mut Vec<Event>,
    nodes: &mut Nodes,
    node_id: NodeId,
) {
    let node = &mut nodes[node_id];

    match node.ty {
        NodeType::CommandBlock {
            repeating,
            chain,
            automatic,
        } => {
            let powered = has_main_input(node);
            let rising = powered && !node.powered;
            let schedule =
                !chain && !node.pending_tick && (rising || (repeating && (powered || automatic)));
            if powered != node.powered || schedule {
                set_node(node, powered);
                events.push(Event::CommandBlockPower {
                    node_id,
                    powered,
                    capture_condition: schedule,
                });
            }
            if schedule {
                node.pending_tick = true;
                scheduler.schedule_half_tick(node_id, 1, TickPriority::Normal);
            }
        }
        NodeType::Repeater {
            delay,
            facing_diode,
        } => {
            let should_be_locked = has_side_input(node);
            if should_be_locked != node.locked {
                set_node_locked(node, should_be_locked);
            }
            if node.locked || node.pending_tick {
                return;
            }

            let should_be_powered = has_main_input(node);
            if should_be_powered != node.powered {
                let priority = if facing_diode {
                    TickPriority::Highest
                } else if !should_be_powered {
                    TickPriority::Higher
                } else {
                    TickPriority::High
                };
                schedule_tick(scheduler, node_id, node, delay as usize, priority);
            }
        }
        NodeType::Torch => {
            if node.pending_tick {
                return;
            }
            let should_be_powered = !has_main_input(node);
            if node.powered != should_be_powered {
                schedule_tick(scheduler, node_id, node, 1, TickPriority::Normal);
            }
        }
        NodeType::Comparator {
            mode,
            far_input,
            facing_diode,
        } => {
            if node.pending_tick {
                return;
            }
            let (mut input_power, side_input_power) = input_strengths(node);
            if let Some(far_override) = far_input {
                if input_power < 15 {
                    input_power = far_override.get();
                }
            }
            let old_strength = node.output_power;
            let output_power = calculate_comparator_output(mode, input_power, side_input_power);
            if output_power != old_strength {
                let priority = if facing_diode {
                    TickPriority::High
                } else {
                    TickPriority::Normal
                };
                schedule_tick(scheduler, node_id, node, 1, priority);
            }
        }
        NodeType::Lamp => {
            let should_be_lit = has_main_input(node);
            let lit = node.powered;
            if lit && !should_be_lit {
                schedule_tick(scheduler, node_id, node, 2, TickPriority::Normal);
            } else if !lit && should_be_lit {
                set_node(node, true);
            }
        }
        NodeType::Trapdoor => {
            let should_be_powered = has_main_input(node);
            if node.powered != should_be_powered {
                set_node(node, should_be_powered);
            }
        }
        NodeType::Wire => {
            let (input_power, _) = input_strengths(node);
            if node.output_power != input_power {
                node.output_power = input_power;
                node.changed = true;
            }
        }
        NodeType::NoteBlock { noteblock_id } => {
            let should_be_powered = has_main_input(node);
            if node.powered != should_be_powered {
                set_node(node, should_be_powered);
                if should_be_powered {
                    events.push(Event::NoteBlockPlay { noteblock_id });
                }
            }
        }
        _ => {}
    }
}
