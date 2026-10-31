//! Discover joint token-dependent inputs, lower flips and reject unsafe motion effects.
use super::*;

pub(in super::super) fn run(
    context: &Context<'_, impl World>,
    program: &mut Program,
    probe_indexes: &ProbeIndexes<'_>,
    phases: &mut CompilePhases,
) -> Result<(), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let entry = context.entry;
    let monitor = context.monitor;
    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    phases.next("piston arrangement discovery");
    check_cancelled(monitor)?;
    program.input_arrangements = crate::redpiler::sampled::group_probe::run(world, bounds, &entry, &program, &probe_indexes, monitor)?;
    if phases.enabled {
        eprintln!("Redpiler inventory: piston_arrangement_groups={}", program.input_arrangements.len());
    }
    phases.next("movement program validation");
    check_cancelled(monitor)?;
    program.compile_movement_flips()?;
    for (component, block) in program.initial.iter().enumerate() {
        if !block.is_command_block() {
            continue;
        }
        let can_be_powered_by_motion = program.input_groups_by_target[component]
            .iter()
            .any(|&group| program.input_arrangements[group].inputs.iter().any(|inputs| !inputs.is_empty()));
        if can_be_powered_by_motion {
            return Err(error(program.positions[component], "command block execution is unsupported"));
        }
    }
    Ok(())
}
