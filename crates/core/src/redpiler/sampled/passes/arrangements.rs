//! Discover joint token-dependent inputs, lower flips and reject unsafe motion effects.
use super::*;

pub(in super::super) fn run(
    context: &Context<'_, impl World>,
    program: &mut Program,
    probe_indexes: &ProbeIndexes<'_>,
    phases: &mut CompilePhases<'_>,
) -> Result<(), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let entry = context.entry;
    let monitor = context.monitor;

    phases.next("piston arrangement discovery");
    check_cancelled(monitor)?;
    let discovery = crate::redpiler::sampled::group_probe::run(world, bounds, &entry, &program, &probe_indexes, monitor)?;
    program.input_arrangements = discovery.groups;
    let mut routing = discovery.routing;

    if phases.enabled {
        eprintln!("Redpiler inventory: piston_arrangement_groups={}", program.input_arrangements.len());
    }

    phases.next("movement program validation");
    check_cancelled(monitor)?;
    program.compile_movement_flips()?;
    if let Some(graph) = &mut routing { graph.bind_links(&program.arrangement_links); }
    program.routing = routing.map(std::sync::Arc::new);
    Ok(())
}
