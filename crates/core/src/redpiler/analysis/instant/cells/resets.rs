//! Monotone admission through paired power/update restoration edges; this is not action ordering.
use super::*;
use crate::redpiler::passes::utils::instant_mechanisms::{RestorationSource, restoration_inputs};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
#[cfg(test)]
mod tests;

pub(super) fn propagate(graph: &CompileGraph, index: &CellIndex, resets: &mut [Option<CellReset>], monitor: &TaskMonitor) -> Result<(), String> {
    let sources = restoration_sources(graph, index, monitor)?;
    let mut waiting = vec![Vec::new(); graph.pistons.len()];
    let mut retained = 0usize;
    for (actor, spec) in graph.pistons.iter().enumerate() {
        check_cancelled(monitor)?;
        if resets[actor].is_some() || !spec.sticky || !matches!(spec.lane, PistonLane::SingleResource { .. }) {
            continue;
        }
        for (driver, reset) in restoration_inputs(graph, &index.power, PistonId::new(actor as u32), &sources, monitor)? {
            retained += std::mem::size_of::<(usize, CellReset)>();
            if retained > MAX_CELL_TABLE_BYTES {
                return Err("reset dependencies exceed the cell preparation budget".into());
            }
            waiting[driver.index()].push((actor, reset));
        }
    }
    propagate_waiting(resets, waiting, monitor)
}

fn propagate_waiting(resets: &mut [Option<CellReset>], mut waiting: Vec<Vec<(usize, CellReset)>>, monitor: &TaskMonitor) -> Result<(), String> {
    let mut ready: VecDeque<_> = resets
        .iter()
        .enumerate()
        .filter_map(|(actor, reset)| reset.is_some().then_some(actor))
        .collect();
    while let Some(driver) = ready.pop_front() {
        check_cancelled(monitor)?;
        for (actor, reset) in std::mem::take(&mut waiting[driver]) {
            check_cancelled(monitor)?;
            if resets[actor].is_none() {
                resets[actor] = Some(reset);
                ready.push_back(actor);
            }
        }
    }
    Ok(())
}

fn restoration_sources(
    graph: &CompileGraph,
    index: &CellIndex,
    monitor: &TaskMonitor,
) -> Result<FxHashMap<ContactId, Vec<RestorationSource>>, String> {
    let mut sources = FxHashMap::<_, Vec<_>>::default();
    for (actor, spec) in graph.pistons.iter().enumerate() {
        check_cancelled(monitor)?;
        let PistonLane::SingleResource { far } = spec.lane else { continue };
        if !spec.sticky {
            continue;
        }
        for &resource in &index.resources[spec.near.index()] {
            let token = &graph.resources[resource.index()];
            if !token.contacts.contains(&far) || graph.profiles[token.material.index()].electrical != ProfileElectrical::RedstoneSource {
                continue;
            }
            for (contact, route) in [(spec.near, spec.routes.resource_restored_near), (far, spec.routes.resource_restored_far)] {
                if let Some(route) = route {
                    sources.entry(contact).or_default().push(RestorationSource {
                        driver: PistonId::new(actor as u32),
                        resource,
                        route,
                    });
                }
            }
        }
    }
    Ok(sources)
}
