//! Encode the shared sampled shape programs, including their ordered callbacks.
use super::{AUX_TABLES, Program, ScratchError, overflow};
use crate::redpiler::sampled::routing::Axis;
use std::collections::HashMap;

fn id(n: usize) -> Result<u32, ScratchError> {
    u32::try_from(n).map_err(|_| overflow())
}

pub(super) fn encode(p: &Program, t: &mut [Vec<u32>; AUX_TABLES]) -> Result<(), ScratchError> {
    let mut maps = HashMap::new();
    for range in &p.cell_shape_steps {
        let steps = range.as_ref().map_or(&[][..], |range| &p.shape_steps[range.clone()]);
        t[53].extend([id(t[54].len() / 7)?, id(steps.len())?]);
        for step in steps {
            t[54].extend([
                id(step.node)?,
                id(t[55].len() / 2)?,
                id(step.states.axes.len())?,
                id(t[56].len())?,
                id(step.states.ports.len())?,
                id(t[58].len())?,
                id(step.callbacks.len())?,
            ]);
            for &Axis::Token(token, cells) in &step.states.axes {
                t[55].extend([id(token)?, id(cells + 1)?]);
            }
            for state in &step.states.ports {
                let index = if let Some(map) = state {
                    if let Some(&index) = maps.get(map) {
                        index
                    } else {
                        let index = id(t[57].len() / 256)?;
                        t[57].extend(map.iter().map(|&v| u32::from(v)));
                        maps.insert(*map, index);
                        index
                    }
                } else {
                    u32::MAX
                };
                t[56].push(index);
            }
            for &node in &step.callbacks {
                t[58].push(id(node)?);
            }
        }
    }
    Ok(())
}
