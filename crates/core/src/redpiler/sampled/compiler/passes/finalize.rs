//! Validate discovered records and build the indexes required by runtime activation.
use super::*;

pub(in super::super) fn run(program: &mut Program, monitor: Option<&TaskMonitor>, phases: &mut CompilePhases) -> Result<(), ScratchError> {
    phases.next("program finalization");
    check_cancelled(monitor)?;
    program.validate()?;
    program.prepare_projection();
    if phases.enabled {
        let analog = program
            .initial
            .iter()
            .filter(|block| matches!(block, Block::RedstoneComparator { .. } | Block::RedstoneWire { .. }))
            .count();
        let comparators = program
            .initial
            .iter()
            .filter(|block| matches!(block, Block::RedstoneComparator { .. }))
            .count();
        let identical = program
            .output_programs
            .iter()
            .filter(|ranges| !ranges[0].is_empty() && program.operations[ranges[0].clone()] == program.operations[ranges[1].clone()])
            .count();
        let shared_operations: usize = program
            .output_programs
            .iter()
            .filter(|ranges| !ranges[0].is_empty() && program.operations[ranges[0].clone()] == program.operations[ranges[1].clone()])
            .map(|ranges| ranges[0].len())
            .sum();
        eprintln!(
            "Redpiler census: components={} analog_readers={} comparator_readers={} arrangement_links={} avoided_nonanalog_global_link_visits={} reactive={} operations={} identical_nonempty_edges={} shareable_operations={} operation_bytes={} input_state_bytes={} histogram_bytes={} flip_records={} flip_bytes={}",
            program.initial.len(),
            analog,
            comparators,
            program.arrangement_links.len(),
            (program.initial.len() - analog).saturating_mul(program.arrangement_links.len()),
            program.has_reactive,
            program.operations.len(),
            identical,
            shared_operations,
            std::mem::size_of::<Operation>(),
            std::mem::size_of::<crate::redpiler::sampled::input::InputState>(),
            std::mem::size_of::<crate::redpiler::sampled::input::Histogram>(),
            program.movement_flips.len(),
            std::mem::size_of::<crate::redpiler::sampled::program::ArrangementFlip>()
        );
    }
    Ok(())
}
