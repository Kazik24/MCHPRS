use super::*;

fn lamp_program() -> Program {
    Program::new(
        vec![BlockPos::new(10, 10, 10)],
        vec![Block::RedstoneLamp { lit: true }],
        vec![],
        vec![[vec![Operation::UpdateLamp(0)], vec![Operation::UpdateLamp(0)]]],
        vec![],
        vec![],
        vec![],
    )
    .unwrap()
}

#[test]
fn cancelled_optimization_keeps_instruction_storage_unchanged() {
    let mut program = lamp_program();
    let before = program.operations.clone();
    let ranges = program.output_programs.clone();
    let monitor = TaskMonitor::default();
    monitor.cancel();

    let error = share_output_programs::run(&mut program, Some(&monitor), &mut CompilePhases::new()).unwrap_err();

    assert_eq!(error.reason, "compilation cancelled");
    assert_eq!(program.operations, before);
    assert_eq!(program.output_programs, ranges);
}

#[test]
fn output_sharing_preserves_programs_pending_work_and_later_execution() {
    let mut program = lamp_program();
    let vectors = program.output_vectors();
    let mut original = program.start();

    share_output_programs::run(&mut program, None, &mut CompilePhases::new()).unwrap();

    assert_eq!(program.output_vectors(), vectors);
    assert_eq!(program.operations.len(), 1);
    assert_eq!(program.output_programs[0][0], program.output_programs[0][1]);
    let mut optimized = program.start();
    original.execute_output_program(0, false);
    optimized.execute_output_program(0, false);
    for _ in 0..4 {
        assert_eq!(original.blocks, optimized.blocks);
        assert_eq!(original.pending_entries(), optimized.pending_entries());
        original.tick().unwrap();
        optimized.tick().unwrap();
    }
}
