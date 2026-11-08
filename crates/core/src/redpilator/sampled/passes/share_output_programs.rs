//! Share identical immutable rise/fall programs without merging component state.
use super::{CompilePhases, check_cancelled};
use crate::redpilator::sampled::{Operation, Program, ScratchError};

pub(in super::super) fn run(program: &mut Program, phases: &mut CompilePhases<'_>) -> Result<(), ScratchError> {
    check_cancelled(phases.monitor)?;
    phases.next("share output programs");
    let original_operations = program.operations.len();
    program.compact_output_programs(true);

    if phases.enabled {
        eprintln!(
            "Redpilator output storage: before={} after={} capacity={} bytes_per_operation={}",
            original_operations,
            program.operations.len(),
            program.operations.capacity(),
            std::mem::size_of::<Operation>()
        );
    }

    program.validate()?;
    check_cancelled(phases.monitor)
}
