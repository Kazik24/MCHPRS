//! Normal O4/O5 kernels retain live inputs even when controls change through reset.
use super::*;
use crate::redpiler::CompilerOptions;

#[test]
fn production_kernels_preserve_unstable_input_histories_and_native_handoff() {
    for level in [4, 5] {
        for id in ["adder_1bit", "adder_11bits"] {
            let circuit = load_adder(id, ORIGIN, None);
            let mut native = circuit.build();
            let mut projected = circuit.build();
            let monitor = TaskMonitor::default();
            let options = CompilerOptions::parse(&format!("-O{level}")).unwrap();
            assert!(!options.stable_instant_inputs);
            let mut prepared = passes::rich::prepare(&native, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
            passes::run_graph_pipeline(
                &mut prepared.graph,
                &options,
                &prepared.entry,
                &passes::default_graph_pipeline(&options),
                &monitor,
            )
            .unwrap();
            assert_eq!(prepared.graph.plans.instant_events.len(), 1);
            assert_eq!(prepared.graph.plans.cpu_kernels.entries.is_empty(), level == 4);
            let mut candidate = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
            candidate.set_test_recording(true);
            let controls: Vec<_> = circuit
                .cells
                .iter()
                .filter(|(_, block, _)| matches!(block, Block::Lever { .. }))
                .map(|(pos, _, _)| circuit.place(*pos))
                .collect();
            let manifest: Value =
                serde_json::from_slice(&std::fs::read(root().join(format!("test_data/instant-pistons-io/fixtures/{id}.json"))).unwrap()).unwrap();
            let case = manifest["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["expectation"].is_object())
                .unwrap();
            let mut steps = case_steps(&circuit, case, false, true);
            let mut seed = 0x19a4_110bu32;
            for _ in 0..96 {
                // Several control writes per tick deliberately overlap unfinished reset work.
                for _ in 0..3 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    steps.push(Step::SetLever {
                        position: controls[(seed as usize >> 8) % controls.len()],
                        powered: seed >> 31 != 0,
                    });
                }
                steps.push(Step::Tick);
            }
            steps.extend(std::iter::repeat_n(Step::Tick, 16));
            let positions = circuit.positions();
            for (index, step) in steps.into_iter().enumerate() {
                let changed = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position),Block::Lever{lever} if lever.powered!=powered),
                    Step::Tick => false,
                };
                native_step(&mut native, step);
                graph_step(&mut candidate, step, changed).unwrap();
                check_boundary(
                    &candidate,
                    &native,
                    &mut projected,
                    &positions,
                    &format!("production O{level} {id} step {index}"),
                )
                .unwrap();
            }
            assert!(candidate.execution_counts().instant_kernel_invocations > 0);
            candidate.reset(&mut projected);
            for tick in 0..17 {
                assert_eq!(
                    first_difference(&future(&native, &positions), &future(&projected, &positions)),
                    None,
                    "O{level} {id} handoff {tick}"
                );
                native.tick_interpreted();
                projected.tick_interpreted();
            }
        }
    }
}
