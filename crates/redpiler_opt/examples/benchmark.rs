//! Synthetic serial DAG timings; these are not server, BDD, or physical timings.
//! Serial DAG execution may be faster or slower than the server's BDD evaluator.
use mchprs_redpiler_opt::{optimize, reference, Circuit, Limits, Op, Plan};
use std::hint::black_box;
use std::time::Instant;

fn add(ops: &mut Vec<Op>, op: Op) -> usize {
    let id = ops.len();
    ops.push(op);
    id
}

fn inputs(count: usize) -> (Vec<usize>, Vec<Op>) {
    let ids: Vec<_> = (1..=count).rev().collect();
    let ops = ids.iter().copied().map(Op::Input).collect();
    (ids, ops)
}

fn cases() -> Vec<(&'static str, Circuit)> {
    let (ids, mut ops) = inputs(8);
    let yes = add(&mut ops, Op::Const(true));
    let mut roots = Vec::new();
    for input in 0..8 {
        let buffer = add(&mut ops, Op::Buffer(input));
        let inverse = add(&mut ops, Op::Not(buffer));
        let original = add(&mut ops, Op::Not(inverse));
        let same = add(&mut ops, Op::And(original, yes));
        let zero = add(&mut ops, Op::Xor(same, same));
        let identity = add(&mut ops, Op::Or(same, zero));
        roots.push(add(&mut ops, Op::Select((input + 1) % 8, identity, buffer)));
    }
    roots.push(roots[0]);
    for _ in 0..256 {
        add(&mut ops, Op::And(0, 1));
    }
    let redundant = Circuit::new(ids, ops, roots);

    let (ids, mut ops) = inputs(8);
    let shared = add(&mut ops, Op::Xor(0, 1));
    let duplicate = add(&mut ops, Op::Xor(1, 0));
    let selected = add(&mut ops, Op::Select(2, shared, 3));
    let selected_again = add(&mut ops, Op::Select(2, duplicate, 3));
    let mut roots = Vec::new();
    for input in 3..8 {
        let left = add(&mut ops, Op::Xor(selected, input));
        let right = add(&mut ops, Op::Xor(input, selected_again));
        roots.push(add(&mut ops, Op::Select(4, left, right)));
    }
    roots.extend([selected, selected_again, roots[0]]);
    let fanout = Circuit::new(ids, ops, roots);

    // Inputs 4/5 are current state. Roots 1/2 are next-state writes, even
    // though neither is a displayed output. Root 3 repeats a distinct consumer.
    let (ids, mut ops) = inputs(6);
    let displayed = add(&mut ops, Op::Xor(0, 1));
    let write_a = add(&mut ops, Op::Select(2, 3, 4));
    let next_b = add(&mut ops, Op::Xor(5, displayed));
    let write_b = add(&mut ops, Op::Select(2, next_b, 5));
    add(&mut ops, Op::And(0, 5));
    let state = Circuit::new(ids, ops, vec![displayed, write_a, write_b, write_a]);

    let (ids, mut ops) = inputs(256);
    let mut previous: Vec<_> = (0..256).collect();
    for round in 0..8 {
        let mut next = Vec::with_capacity(256);
        for input in 0..256 {
            let mixed = add(
                &mut ops,
                Op::Xor(previous[input], previous[(input + 17) % 256]),
            );
            next.push(add(
                &mut ops,
                Op::Select(
                    (input + round * 3) % 256,
                    mixed,
                    previous[(input + 31) % 256],
                ),
            ));
        }
        previous = next;
    }
    let mut roots: Vec<_> = previous.iter().step_by(8).copied().collect();
    roots.extend([previous[0], previous[1]]);
    let large = Circuit::new(ids, ops, roots);
    vec![
        ("redundant_8", redundant),
        ("xor_mux_fanout_8", fanout),
        ("state_writes_6", state),
        ("domain_256", large),
    ]
}

fn vectors(count: usize) -> Vec<Vec<bool>> {
    let samples = if count <= 8 { 1 << count } else { 256 };
    let mut seed = 0x5eed_u64;
    (0..samples)
        .map(|sample| {
            (0..count)
                .map(|input| {
                    if count <= 8 {
                        sample & (1 << input) != 0
                    } else {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        seed >> 63 != 0
                    }
                })
                .collect()
        })
        .collect()
}

fn check(
    reference: &Plan,
    optimized: &Plan,
    inputs: &[Vec<bool>],
    reference_scratch: &mut [bool],
    optimized_scratch: &mut [bool],
    reference_roots: &mut [bool],
    optimized_roots: &mut [bool],
) {
    assert_eq!(reference.inputs(), optimized.inputs());
    assert_eq!(reference.roots().len(), optimized.roots().len());
    for values in inputs {
        reference
            .evaluate(values, reference_scratch, reference_roots)
            .unwrap();
        optimized
            .evaluate(values, optimized_scratch, optimized_roots)
            .unwrap();
        assert_eq!(reference_roots, optimized_roots);
        for (net, slot) in optimized.original_to_slot().iter().enumerate() {
            if let Some(slot) = slot {
                let original = reference.original_to_slot()[net].unwrap();
                assert_eq!(
                    reference_scratch[original], optimized_scratch[*slot],
                    "net {net}"
                );
            }
        }
    }
}

fn evaluate_many(
    plan: &Plan,
    inputs: &[Vec<bool>],
    scratch: &mut [bool],
    roots: &mut [bool],
    evaluations: usize,
) {
    for sample in 0..evaluations {
        black_box(plan)
            .evaluate(
                black_box(inputs[sample % inputs.len()].as_slice()),
                black_box(&mut *scratch),
                black_box(&mut *roots),
            )
            .unwrap();
        black_box(&*roots);
    }
}

fn measure(
    plan: &Plan,
    inputs: &[Vec<bool>],
    scratch: &mut [bool],
    roots: &mut [bool],
    evaluations: usize,
) -> f64 {
    let start = Instant::now();
    evaluate_many(plan, inputs, scratch, roots, evaluations);
    start.elapsed().as_secs_f64()
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    (sorted[(sorted.len() - 1) / 2] + sorted[sorted.len() / 2]) / 2.0
}

fn plan_data_bytes(plan: &Plan) -> usize {
    // Logical arrays plus compiled input ordinals; excludes Vec headers,
    // spare capacity and allocator metadata.
    std::mem::size_of_val(plan.inputs())
        + std::mem::size_of_val(plan.ops())
        + std::mem::size_of_val(plan.roots())
        + std::mem::size_of_val(plan.original_to_slot())
        + plan.ops().len() * std::mem::size_of::<usize>()
}

fn argument(args: &[String], name: &str, default: usize) -> usize {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map_or(default, |pair| {
            pair[1].parse().expect("argument needs a positive integer")
        })
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let check_only = args.iter().any(|arg| arg == "--check");
    let samples = argument(&args, "--samples", 5);
    let evaluations = argument(&args, "--evaluations", 100_000);
    assert!(samples > 0 && evaluations > 0);
    println!("{{\"scope\":\"synthetic serial DAG; no server, BDD or physical claims\",\"plan_data_estimate_excludes\":\"Vec headers, capacity slack, allocator metadata\",\"cases\":[");
    for (case_index, (name, circuit)) in cases().into_iter().enumerate() {
        if case_index != 0 {
            println!(",");
        }
        let baseline = reference(&circuit).unwrap();
        let optimized = optimize(&circuit, Limits::default(), || false).unwrap();
        assert!(!optimized.used_fallback());
        assert_eq!(baseline.ops().len(), circuit.ops().len());
        assert_eq!(baseline.original_to_slot().len(), circuit.ops().len());
        assert!(baseline.original_to_slot().iter().all(Option::is_some));
        let input_vectors = vectors(circuit.inputs().len());
        let mut baseline_scratch = vec![false; baseline.scratch_len()];
        let mut optimized_scratch = vec![false; optimized.scratch_len()];
        let mut baseline_roots = vec![false; circuit.roots().len()];
        let mut optimized_roots = vec![false; circuit.roots().len()];
        check(
            &baseline,
            &optimized,
            &input_vectors,
            &mut baseline_scratch,
            &mut optimized_scratch,
            &mut baseline_roots,
            &mut optimized_roots,
        );
        print!("{{\"case\":\"{name}\",\"inputs\":{},\"ordered_roots\":{},\"reference_ops\":{},\"optimized_ops\":{},\"reference_scratch_bytes\":{},\"optimized_scratch_bytes\":{},\"checked_input_vectors\":{}", circuit.inputs().len(), circuit.roots().len(), baseline.ops().len(), optimized.ops().len(), baseline.scratch_len() * std::mem::size_of::<bool>(), optimized.scratch_len() * std::mem::size_of::<bool>(), input_vectors.len());
        print!(",\"reference_plan_data_estimated_bytes\":{},\"optimized_plan_data_estimated_bytes\":{}", plan_data_bytes(&baseline), plan_data_bytes(&optimized));
        if check_only {
            print!("}}");
            continue;
        }
        let mut baseline_compile = Vec::new();
        let mut optimized_compile = Vec::new();
        for sample in 0..samples {
            // Alternate order to reduce a systematic warm-cache advantage.
            for optimized_first in [sample % 2 == 0, sample % 2 != 0] {
                let start = Instant::now();
                let plan = if optimized_first {
                    optimize(black_box(&circuit), Limits::default(), || false).unwrap()
                } else {
                    reference(black_box(&circuit)).unwrap()
                };
                let seconds = start.elapsed().as_secs_f64();
                if optimized_first {
                    optimized_compile.push(seconds);
                } else {
                    baseline_compile.push(seconds);
                }
                black_box(plan);
            }
        }
        evaluate_many(
            &baseline,
            &input_vectors,
            &mut baseline_scratch,
            &mut baseline_roots,
            1_000,
        );
        evaluate_many(
            &optimized,
            &input_vectors,
            &mut optimized_scratch,
            &mut optimized_roots,
            1_000,
        );
        let mut baseline_seconds = Vec::new();
        let mut optimized_seconds = Vec::new();
        for sample in 0..samples {
            for optimized_first in [sample % 2 == 0, sample % 2 != 0] {
                let seconds = if optimized_first {
                    measure(
                        &optimized,
                        &input_vectors,
                        &mut optimized_scratch,
                        &mut optimized_roots,
                        evaluations,
                    )
                } else {
                    measure(
                        &baseline,
                        &input_vectors,
                        &mut baseline_scratch,
                        &mut baseline_roots,
                        evaluations,
                    )
                };
                if optimized_first {
                    optimized_seconds.push(seconds);
                } else {
                    baseline_seconds.push(seconds);
                }
            }
        }
        assert_eq!(baseline_roots, optimized_roots);
        print!(",\"samples\":{samples},\"evaluations_per_sample\":{evaluations},\"reference_compile_seconds\":{baseline_compile:?},\"optimized_compile_seconds\":{optimized_compile:?},\"reference_compile_median_seconds\":{},\"optimized_compile_median_seconds\":{},\"reference_evaluation_seconds\":{baseline_seconds:?},\"optimized_evaluation_seconds\":{optimized_seconds:?},\"reference_evaluation_median_seconds\":{},\"optimized_evaluation_median_seconds\":{},\"reference_evaluations_per_second\":{},\"optimized_evaluations_per_second\":{},\"speedup\":{} }}", median(&baseline_compile), median(&optimized_compile), median(&baseline_seconds), median(&optimized_seconds), evaluations as f64 / median(&baseline_seconds), evaluations as f64 / median(&optimized_seconds), median(&baseline_seconds) / median(&optimized_seconds));
    }
    println!("\n]}}");
}
