#[path = "../benches/support/cpus.rs"]
mod cpus;

#[test]
fn frozen_cpu_inputs_and_initial_states() {
    for cpu in cpus::CPUS {
        let expected = cpus::reference(cpu);
        let world = cpus::load_cpu(cpu);
        assert_eq!(cpus::checkpoint(&world, 0, &[]), expected.checkpoints[0]);
    }
}

#[test]
#[ignore = "50,000-tick CPU regression; run with --release -- --ignored"]
fn pm1_sort_frozen_reference() {
    cpus::replay(cpus::CPUS[0], &cpus::reference(cpus::CPUS[0]));
}

#[test]
#[ignore = "50,000-tick CPU regression; run with --release -- --ignored"]
fn anpu_pong_frozen_reference() {
    cpus::replay(cpus::CPUS[1], &cpus::reference(cpus::CPUS[1]));
}
