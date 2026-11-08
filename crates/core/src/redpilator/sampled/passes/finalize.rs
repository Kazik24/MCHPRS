//! Validate discovered records and build the indexes required by runtime activation.
use super::{CompilePhases, check_cancelled};
use crate::redpilator::sampled::{Operation, Program, ScratchError};
use mchprs_blocks::blocks::Block;

pub(in super::super) fn run(program: &mut Program, phases: &mut CompilePhases<'_>) -> Result<(), ScratchError> {
    phases.next("program finalization");
    check_cancelled(phases.monitor)?;
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
        let identical_outputs = program
            .output_programs
            .iter()
            .filter(|ranges| !ranges[0].is_empty() && program.operations[ranges[0].clone()] == program.operations[ranges[1].clone()]);
        let identical = identical_outputs.clone().count();
        let shared_operations: usize = identical_outputs.map(|ranges| ranges[0].len()).sum();
        eprintln!(
            "Redpilator census: components={} analog_readers={} comparator_readers={} arrangement_links={} avoided_nonanalog_global_link_visits={} reactive={} operations={} identical_nonempty_edges={} shareable_operations={} operation_bytes={} input_state_bytes={} histogram_bytes={} flip_records={} flip_bytes={}",
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
            std::mem::size_of::<crate::redpilator::sampled::input::InputState>(),
            std::mem::size_of::<crate::redpilator::sampled::input::Histogram>(),
            program.movement_flips.len(),
            std::mem::size_of::<crate::redpilator::sampled::program::ArrangementFlip>()
        );
    }
    Ok(())
}
