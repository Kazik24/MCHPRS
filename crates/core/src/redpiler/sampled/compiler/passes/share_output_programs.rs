//! Share identical immutable rise/fall programs without merging component state.
use super::*;

pub(super) fn run(program: &mut Program, timings: bool) {
    let original_operations = program.operations.len();
    program.compact_output_programs(true);
    if timings {
        eprintln!(
            "Redpiler output storage: before={} after={} capacity={} bytes_per_operation={}",
            original_operations,
            program.operations.len(),
            program.operations.capacity(),
            std::mem::size_of::<Operation>()
        );
    }
}
