use super::*;

#[test]
fn local_propagation_proves_hidden_path_contradictions_without_enumeration() {
    let mut diagram = Diagram::new_structural(Limits::default()).unwrap();
    let a = diagram.input(0).unwrap();
    let b = diagram.input(1).unwrap();
    let not_b = diagram.not(b).unwrap();
    let ab = diagram.and(a, b).unwrap();
    let a_not_b = diagram.and(a, not_b).unwrap();
    let partition = diagram.or(ab, a_not_b).unwrap();
    let not_partition = diagram.not(partition).unwrap();
    let impossible = diagram.and(a, not_partition).unwrap();
    assert_ne!(
        impossible,
        Formula::FALSE,
        "the structural identities alone do not prove this contradiction"
    );
    let budget = Limits {
        max_nodes: 64,
        max_work: 4096,
    };
    assert!(diagram.prove_false(impossible, budget).unwrap());
    assert!(!diagram.prove_false(impossible, Limits { max_nodes: 1, ..budget }).unwrap());
    assert!(!diagram.prove_false(impossible, Limits { max_work: 0, ..budget }).unwrap());
    assert!(!diagram.prove_false(partition, budget).unwrap());
    assert!(!diagram.prove_false(a, budget).unwrap());
}

#[test]
fn local_propagation_never_rejects_a_satisfiable_generated_guard_even_with_a_cut_cone() {
    let mut diagram = Diagram::new_structural(Limits::default()).unwrap();
    let mut values = vec![(Formula::FALSE, 0u8), (Formula::TRUE, 255u8)];
    for bit in 0..3 {
        let mask = (0..8).fold(0, |mask, input| mask | (u8::from(input & (1 << bit) != 0) << input));
        values.push((diagram.input(bit).unwrap(), mask));
    }
    let mut random = 7u32;
    for index in 0..192 {
        let mut operand = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            values[random as usize % values.len()]
        };
        let (a, a_mask) = operand();
        let (b, b_mask) = operand();
        let (c, c_mask) = operand();
        let result = match index % 5 {
            0 => (diagram.and(a, b).unwrap(), a_mask & b_mask),
            1 => (diagram.or(a, b).unwrap(), a_mask | b_mask),
            2 => (diagram.xor(a, b).unwrap(), a_mask ^ b_mask),
            3 => (diagram.not(a).unwrap(), !a_mask),
            _ => (diagram.select(a, b, c).unwrap(), (a_mask & b_mask) | (!a_mask & c_mask)),
        };
        values.push(result);
    }
    for &(formula, truth_mask) in &values {
        for max_nodes in [0, 1, 8, 32, 128] {
            for max_work in [0, 64, 4096] {
                if diagram.prove_false(formula, Limits { max_nodes, max_work }).unwrap() {
                    assert_eq!(truth_mask, 0, "a truncated or exhausted proof must never reject a feasible path");
                }
            }
        }
    }
    let roots: Vec<_> = values.iter().map(|&(formula, _)| formula).collect();
    let plan = plan(&diagram, &roots);
    for input in 0..8 {
        let actual = evaluate(&plan, &std::array::from_fn::<_, 3, _>(|bit| input & (1 << bit) != 0));
        for (&actual, &(_, truth_mask)) in actual.iter().zip(&values) {
            assert_eq!(actual, truth_mask & (1 << input) != 0);
        }
    }
}

#[test]
fn local_propagation_validates_roots_and_keeps_canonical_proofs_trivial() {
    for structural in [false, true] {
        let mut diagram = Diagram::with_representation(Limits::default(), structural).unwrap();
        let a = diagram.input(0).unwrap();
        let empty = Limits { max_nodes: 0, max_work: 0 };
        assert!(diagram.prove_false(Formula::FALSE, empty).unwrap());
        assert!(!diagram.prove_false(Formula::TRUE, empty).unwrap());
        assert!(!diagram.prove_false(a, Limits::default()).unwrap());
        assert_eq!(diagram.prove_false(Formula(u32::MAX), empty), Err(SymbolicError::InvalidFormula));
    }
}
use crate::{Plan, optimize};

#[test]
fn binary_implications_prove_exactly_the_forced_bits_of_all_two_variable_relations() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let a = diagram.input(2).unwrap();
    let b = diagram.input(5).unwrap();
    let functions: Vec<_> = (0u16..16)
        .map(|mask| {
            let bit = |index: u32| if mask & (1u16 << index) != 0u16 { Formula::TRUE } else { Formula::FALSE };
            let low = diagram.select(b, bit(2), bit(0)).unwrap();
            let high = diagram.select(b, bit(3), bit(1)).unwrap();
            diagram.select(a, high, low).unwrap()
        })
        .collect();
    for first in 0u16..16 {
        for second in 0u16..16 {
            let models: Vec<_> = (0..4).filter(|input| first & second & (1 << input) != 0).collect();
            let bindings = diagram.forced_inputs(&[functions[first as usize], functions[second as usize]]).unwrap();
            if models.is_empty() {
                assert!(bindings.is_none());
            } else {
                let expected: Vec<_> = [(2, 1), (5, 2)]
                    .into_iter()
                    .filter_map(|(input, mask)| {
                        let first = models[0] & mask != 0;
                        models.iter().all(|model| (model & mask != 0) == first).then_some((input, first))
                    })
                    .collect();
                assert_eq!(bindings.unwrap(), expected);
            }
        }
    }
    assert_eq!(
        diagram.forced_inputs(&[Formula::FALSE, Formula(u32::MAX)]),
        Err(SymbolicError::InvalidFormula)
    );
}

#[test]
fn higher_arity_conditions_never_produce_an_unproven_constant() {
    let mut diagram = Diagram::new(Limits {
        max_work: 1_000_000,
        ..Limits::default()
    })
    .unwrap();
    let variables: Vec<_> = (0..3).map(|input| diagram.input(input).unwrap()).collect();
    let mut functions = Vec::new();
    for mask in 0u16..256 {
        let bit = |index: u32| if mask & (1u16 << index) != 0u16 { Formula::TRUE } else { Formula::FALSE };
        let low_zero = diagram.select(variables[2], bit(4), bit(0)).unwrap();
        let low_one = diagram.select(variables[2], bit(6), bit(2)).unwrap();
        let low = diagram.select(variables[1], low_one, low_zero).unwrap();
        let high_zero = diagram.select(variables[2], bit(5), bit(1)).unwrap();
        let high_one = diagram.select(variables[2], bit(7), bit(3)).unwrap();
        let high = diagram.select(variables[1], high_one, high_zero).unwrap();
        functions.push(diagram.select(variables[0], high, low).unwrap());
    }
    let mut random = 0x91f3_7ad5u32;
    for _ in 0..1024 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let first = random as usize & 255;
        let second = (random >> 16) as usize & 255;
        let models: Vec<_> = (0..8).filter(|input| first & second & (1 << input) != 0).collect();
        match diagram.forced_inputs(&[functions[first], functions[second]]).unwrap() {
            None => assert!(models.is_empty()),
            Some(bindings) => {
                for (input, value) in bindings {
                    assert!(models.iter().all(|model| (model & (1 << input) != 0) == value));
                }
            }
        }
    }
}

#[test]
fn binding_proven_inputs_preserves_functions_and_root_layout() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let a = diagram.input(0).unwrap();
    let b = diagram.input(1).unwrap();
    let root = diagram.xor(a, b).unwrap();
    for bindings in [
        vec![],
        vec![(0, false)],
        vec![(0, true)],
        vec![(1, false)],
        vec![(1, true)],
        vec![(0, true), (1, false)],
    ] {
        let roots = diagram.bind_roots(&[root, root, a, Formula::TRUE], &bindings).unwrap();
        assert_eq!(roots[0], roots[1]);
        let plan = plan(&diagram, &roots);
        for original in [[false, false], [true, false], [false, true], [true, true]] {
            let mut bound = original;
            for &(input, value) in &bindings {
                bound[input] = value;
            }
            assert_eq!(evaluate(&plan, &original), [bound[0] ^ bound[1], bound[0] ^ bound[1], bound[0], true]);
        }
    }
    assert_eq!(diagram.bind_roots(&[root], &[(0, true), (0, false)]), Err(SymbolicError::InvalidInput));
    assert_eq!(diagram.bind_roots(&[root], &[(99, true)]), Err(SymbolicError::InvalidInput));
    assert_eq!(diagram.bind_roots(&[Formula(u32::MAX)], &[]), Err(SymbolicError::InvalidFormula));
    diagram.limits.max_work = diagram.work();
    assert_eq!(diagram.forced_inputs(&[a]), Err(SymbolicError::BudgetExceeded));
    assert_eq!(diagram.bind_roots(&[root], &[(0, true)]), Err(SymbolicError::BudgetExceeded));
}

#[test]
fn neutral_boolean_and_signal_inputs_fold_without_spending_the_proof_budget() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let input = diagram.input(0).unwrap();
    let signal = Signal::from_bits([input; 4]);
    let zero = Signal::literal(0).unwrap();
    let before = diagram.work();
    for _ in 0..4096 {
        for (value, expected) in [(Formula::FALSE, Formula::FALSE), (Formula::TRUE, input), (input, input)] {
            assert_eq!(diagram.and(input, value).unwrap(), expected);
            assert_eq!(diagram.and(value, input).unwrap(), expected);
        }
        assert_eq!(diagram.or(input, Formula::FALSE).unwrap(), input);
        assert_eq!(diagram.or(Formula::FALSE, input).unwrap(), input);
        assert_eq!(diagram.or(input, Formula::TRUE).unwrap(), Formula::TRUE);
        assert_eq!(diagram.or(Formula::TRUE, input).unwrap(), Formula::TRUE);
        assert_eq!(diagram.or(input, input).unwrap(), input);
        assert_eq!(diagram.xor(input, input).unwrap(), Formula::FALSE);
        assert_eq!(diagram.xor(input, Formula::FALSE).unwrap(), input);
        assert_eq!(diagram.xor(Formula::FALSE, input).unwrap(), input);
        assert_eq!(diagram.maximum(signal, signal).unwrap(), signal);
        assert_eq!(diagram.maximum(signal, zero).unwrap(), signal);
        assert_eq!(diagram.maximum(zero, signal).unwrap(), signal);
    }
    assert_eq!(diagram.work(), before);
    let invalid = Formula(u32::MAX);
    assert_eq!(diagram.and(Formula::FALSE, invalid), Err(SymbolicError::InvalidFormula));
    assert_eq!(diagram.or(Formula::TRUE, invalid), Err(SymbolicError::InvalidFormula));
    assert_eq!(diagram.xor(invalid, invalid), Err(SymbolicError::InvalidFormula));
    assert_eq!(diagram.maximum(zero, Signal::from_bits([invalid; 4])), Err(SymbolicError::InvalidFormula));
}

#[test]
fn lowering_discards_unreachable_proof_work_before_building_the_plan_layout() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let variables: Vec<_> = (0..16).map(|input| diagram.input(input).unwrap()).collect();
    for &a in &variables {
        for &b in &variables {
            diagram.xor(a, b).unwrap();
        }
    }
    assert!(diagram.node_count() > 128);
    let root = diagram.xor(variables[0], variables[1]).unwrap();
    let circuit = diagram.circuit(&[root, root]).unwrap();
    assert_eq!(circuit.inputs(), &(0..16).collect::<Vec<_>>());
    assert!(
        circuit.ops().len() < 32,
        "dead proof work must not allocate retained original-net entries"
    );
    let plan = optimize(&circuit, Limits::default(), || false).unwrap();
    assert_eq!(plan.original_to_slot().len(), circuit.ops().len());
    for mask in 0..256 {
        let inputs: Vec<_> = (0..16).map(|bit| mask & (1 << bit) != 0).collect();
        assert_eq!(evaluate(&plan, &inputs), [inputs[0] ^ inputs[1]; 2]);
    }
}

#[test]
fn live_input_lowering_preserves_sparse_identifiers_and_all_selected_outputs() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let a = diagram.input(2).unwrap();
    let unused = diagram.input(9).unwrap();
    let b = diagram.input(100).unwrap();
    diagram.and(a, unused).unwrap();
    let root = diagram.xor(a, b).unwrap();
    let roots = [root, Formula::TRUE, root, Formula::FALSE];
    let original = optimize(&diagram.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    let compact = optimize(&diagram.circuit_live_inputs(&roots).unwrap(), Limits::default(), || false).unwrap();
    assert_eq!(original.inputs(), &[2, 9, 100], "the existing declared-input API is unchanged");
    assert_eq!(
        compact.inputs(),
        &[2, 100],
        "runtime bindings omit unreachable inputs without renumbering them"
    );
    for mask in 0..8 {
        let all = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
        assert_eq!(evaluate(&compact, &[all[0], all[2]]), evaluate(&original, &all));
    }
}

#[test]
fn live_input_lowering_keeps_constant_empty_and_invalid_root_contracts() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    diagram.input(500).unwrap();
    for roots in [vec![], vec![Formula::TRUE, Formula::FALSE, Formula::TRUE]] {
        let circuit = diagram.circuit_live_inputs(&roots).unwrap();
        assert!(circuit.inputs().is_empty());
        let compact = optimize(&circuit, Limits::default(), || false).unwrap();
        let expected: Vec<_> = roots.iter().map(|root| root.constant().unwrap()).collect();
        assert_eq!(evaluate(&compact, &[]), expected);
    }
    assert_eq!(diagram.circuit_live_inputs(&[Formula(u32::MAX)]), Err(SymbolicError::InvalidFormula));
}

#[test]
fn structural_boolean_traces_match_canonical_functions_for_every_input() {
    let mut canonical = Diagram::new(Limits::default()).unwrap();
    let mut structural = Diagram::new_structural(Limits::default()).unwrap();
    let mut ordered: Vec<_> = (0..3).map(|input| canonical.input(input).unwrap()).collect();
    let mut shared: Vec<_> = (0..3).map(|input| structural.input(input).unwrap()).collect();
    let mut random = 0x81ac_047du32;
    for step in 0..128 {
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            random as usize % ordered.len()
        };
        let [a, b, c] = [next(), next(), next()];
        let operation = |diagram: &mut Diagram, values: &[Formula]| match step % 5 {
            0 => diagram.not(values[a]),
            1 => diagram.and(values[a], values[b]),
            2 => diagram.or(values[a], values[b]),
            3 => diagram.xor(values[a], values[b]),
            _ => diagram.select(values[a], values[b], values[c]),
        };
        ordered.push(operation(&mut canonical, &ordered).unwrap());
        shared.push(operation(&mut structural, &shared).unwrap());
    }
    ordered.extend([ordered[3], Formula::TRUE, ordered[3]]);
    shared.extend([shared[3], Formula::TRUE, shared[3]]);
    let reference = plan(&canonical, &ordered);
    let compiled = optimize(&structural.circuit_live_inputs(&shared).unwrap(), Limits::default(), || false).unwrap();
    for mask in 0..8 {
        let inputs = [mask & 1 != 0, mask & 2 != 0, mask & 4 != 0];
        assert_eq!(evaluate(&compiled, &inputs), evaluate(&reference, &inputs));
    }
}

#[test]
fn structural_four_bit_operations_preserve_all_strengths_and_attenuations() {
    let mut diagram = Diagram::new_structural(Limits::default()).unwrap();
    let mut bits = [Formula::FALSE; 8];
    for (input, bit) in bits.iter_mut().enumerate() {
        *bit = diagram.input(input).unwrap();
    }
    let left = Signal::from_bits(bits[..4].try_into().unwrap());
    let right = Signal::from_bits(bits[4..].try_into().unwrap());
    let mut roots = diagram.maximum(left, right).unwrap().bits().to_vec();
    roots.push(diagram.signal_equal(left, right).unwrap());
    roots.push(diagram.powered(left).unwrap());
    for attenuation in 0..=16 {
        roots.extend(diagram.attenuate(left, attenuation).unwrap().bits());
    }
    let compiled = optimize(&diagram.circuit_live_inputs(&roots).unwrap(), Limits::default(), || false).unwrap();
    for a in 0u8..16 {
        for b in 0u8..16 {
            let inputs: Vec<_> = (0..8)
                .map(|bit| if bit < 4 { a & (1 << bit) != 0 } else { b & (1 << (bit - 4)) != 0 })
                .collect();
            let values = evaluate(&compiled, &inputs);
            let signal = |values: &[bool]| values.iter().enumerate().fold(0u8, |value, (bit, &set)| value | (u8::from(set) << bit));
            assert_eq!(signal(&values[..4]), a.max(b));
            assert_eq!(values[4], a == b);
            assert_eq!(values[5], a != 0);
            for attenuation in 0u8..=16 {
                let start = 6 + usize::from(attenuation) * 4;
                assert_eq!(signal(&values[start..start + 4]), a.saturating_sub(attenuation));
            }
        }
    }
}

#[test]
fn structural_sharing_substitution_and_proof_boundaries_are_explicit() {
    let mut diagram = Diagram::new_structural(Limits::default()).unwrap();
    let a = diagram.input(2).unwrap();
    let b = diagram.input(7).unwrap();
    let inverse = diagram.not(a).unwrap();
    assert_eq!(diagram.not(inverse).unwrap(), a);
    assert_eq!(diagram.and(a, inverse).unwrap(), Formula::FALSE);
    assert_eq!(diagram.or(inverse, a).unwrap(), Formula::TRUE);
    assert_eq!(diagram.xor(inverse, a).unwrap(), Formula::TRUE);
    assert_eq!(diagram.and(a, b).unwrap(), diagram.and(b, a).unwrap());
    let difference = diagram.xor(a, b).unwrap();
    let roots = diagram.bind_roots(&[difference, a, difference], &[(2, true)]).unwrap();
    let compiled = optimize(&diagram.circuit_live_inputs(&roots).unwrap(), Limits::default(), || false).unwrap();
    assert_eq!(compiled.inputs(), &[7]);
    for b in [false, true] {
        assert_eq!(evaluate(&compiled, &[b]), [!b, true, !b]);
    }
    assert_eq!(diagram.forced_inputs(&[a]), Err(SymbolicError::RequiresCanonicalDiagram));
    let invalid = Formula(u32::MAX);
    assert_eq!(diagram.forced_inputs(&[invalid]), Err(SymbolicError::InvalidFormula));
    assert_eq!(diagram.and(Formula::FALSE, invalid), Err(SymbolicError::InvalidFormula));
    let mut bounded = Diagram::new_structural(Limits {
        max_nodes: 3,
        ..Limits::default()
    })
    .unwrap();
    let input = bounded.input(0).unwrap();
    assert_eq!(bounded.not(input), Err(SymbolicError::BudgetExceeded));
    assert_eq!(bounded.and(input, Formula::FALSE).unwrap(), Formula::FALSE);
}

#[test]
fn structural_late_data_selection_has_linear_storage_instead_of_a_truth_relation() {
    const WIDTH: usize = 24;
    let mut diagram = Diagram::new_structural(Limits {
        max_nodes: 256,
        ..Limits::default()
    })
    .unwrap();
    let controls: Vec<_> = (0..WIDTH).map(|input| diagram.input(input).unwrap()).collect();
    let data: Vec<_> = (WIDTH..WIDTH * 2).map(|input| diagram.input(input).unwrap()).collect();
    let mut result = Formula::FALSE;
    for (&control, &value) in controls.iter().zip(&data) {
        let selected = diagram.and(control, value).unwrap();
        result = diagram.or(result, selected).unwrap();
    }
    assert!(diagram.node_count() < WIDTH * 5);
    let compiled = optimize(&diagram.circuit_live_inputs(&[result]).unwrap(), Limits::default(), || false).unwrap();
    let mut random = 0x9de4_7ab1u32;
    for _ in 0..128 {
        let inputs: Vec<_> = (0..WIDTH * 2)
            .map(|_| {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                random & (1 << 30) != 0
            })
            .collect();
        let expected = inputs[..WIDTH].iter().zip(&inputs[WIDTH..]).any(|(&control, &value)| control && value);
        assert_eq!(evaluate(&compiled, &inputs), [expected]);
    }
}

#[test]
fn bounded_structural_implication_never_proves_a_false_two_variable_consequence() {
    let mut diagram = Diagram::new_structural(Limits::default()).unwrap();
    let a = diagram.input(0).unwrap();
    let b = diagram.input(1).unwrap();
    let mut functions = vec![(a, 0b1010u8), (b, 0b1100u8), (Formula::FALSE, 0), (Formula::TRUE, 15)];
    for mask in 0u8..16 {
        let bit = |index: u32| if mask & (1u8 << index) != 0 { Formula::TRUE } else { Formula::FALSE };
        let low = diagram.select(b, bit(2), bit(0)).unwrap();
        let high = diagram.select(b, bit(3), bit(1)).unwrap();
        functions.push((diagram.select(a, high, low).unwrap(), mask));
    }
    let original = functions.clone();
    for &(function, mask) in &original {
        functions.push((diagram.not(function).unwrap(), !mask & 15));
        for &(other, other_mask) in &original[..4] {
            functions.push((diagram.and(function, other).unwrap(), mask & other_mask));
            functions.push((diagram.or(function, other).unwrap(), mask | other_mask));
        }
    }
    for &(left, left_mask) in &functions {
        for &(right, right_mask) in &functions {
            if diagram.structurally_implies(left, right) {
                assert_eq!(left_mask & !right_mask, 0, "a structural proof must be valid for every valuation");
            }
        }
    }
    let roots: Vec<_> = functions.iter().map(|&(function, _)| function).collect();
    let compiled = optimize(&diagram.circuit_live_inputs(&roots).unwrap(), Limits::default(), || false).unwrap();
    for mask in 0..4 {
        let expected: Vec<_> = functions.iter().map(|&(_, table)| table & (1 << mask) != 0).collect();
        assert_eq!(evaluate(&compiled, &[mask & 1 != 0, mask & 2 != 0]), expected);
    }
    let both = diagram.and(a, b).unwrap();
    let either = diagram.or(a, b).unwrap();
    let not_either = diagram.not(either).unwrap();
    assert_eq!(
        diagram.and(both, not_either).unwrap(),
        Formula::FALSE,
        "a visited superset must prune the next path"
    );
    assert_eq!(diagram.or(both, a).unwrap(), a, "visited subsets need not accumulate indefinitely");
    assert!(
        !diagram.implies_with_budget(a, b, &mut 0),
        "an exhausted proof returns unknown, not a guessed result"
    );
}

#[test]
fn bounded_memo_eviction_preserves_every_two_variable_function_and_the_work_budget() {
    let limits = Limits {
        max_nodes: 64,
        ..Limits::default()
    };
    let mut diagram = Diagram::new(limits).unwrap();
    let a = diagram.input(0).unwrap();
    let b = diagram.input(1).unwrap();
    let mut functions = Vec::new();
    for mask in 0..16 {
        let value = |bit| if mask & (1 << bit) == 0 { Formula::FALSE } else { Formula::TRUE };
        let low = diagram.select(b, value(2), value(0)).unwrap();
        let high = diagram.select(b, value(3), value(1)).unwrap();
        functions.push(diagram.select(a, high, low).unwrap());
    }
    let mut evicted = false;
    for condition in 0..16 {
        for yes in 0..16 {
            for no in 0..16 {
                let before = diagram.selections.len();
                let selected = diagram.select(functions[condition], functions[yes], functions[no]).unwrap();
                evicted |= diagram.selections.len() < before;
                let expected = (condition & yes) | ((!condition & no) & 15);
                assert_eq!(selected, functions[expected]);
                assert!(diagram.selections.len() < limits.max_nodes);
            }
        }
    }
    assert!(evicted);
    assert!(diagram.node_count() < limits.max_nodes);
    let work = diagram.work();
    assert!(work > 4096, "cache eviction must not reset charged construction work");
}

#[test]
fn selected_counts_preserve_multiplicity_and_normalize_equivalent_revision_histories() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let variables: Vec<_> = (0..8).map(|input| diagram.input(input).unwrap()).collect();
    let inverse = diagram.not(variables[0]).unwrap();
    assert_eq!(diagram.count(&[]).unwrap(), []);
    assert_eq!(diagram.count(&[variables[0], inverse]).unwrap(), [Formula::TRUE]);
    let counted = diagram.count(&variables).unwrap();
    let duplicate = diagram.count(&[variables[0], variables[0], inverse]).unwrap();
    let roots: Vec<_> = counted.iter().chain(&duplicate).copied().collect();
    let plan = plan(&diagram, &roots);
    for mask in 0u32..256 {
        let inputs = std::array::from_fn::<_, 8, _>(|bit| mask & (1 << bit) != 0);
        let output = evaluate(&plan, &inputs);
        let number = |bits: &[bool]| bits.iter().enumerate().fold(0, |value, (bit, &set)| value | (usize::from(set) << bit));
        assert_eq!(number(&output[..counted.len()]), mask.count_ones() as usize);
        assert_eq!(number(&output[counted.len()..]), 1 + usize::from(inputs[0]));
    }
    assert_eq!(diagram.count(&[Formula::TRUE; 255]).unwrap(), [Formula::TRUE; 8]);
}

fn plan(diagram: &Diagram, roots: &[Formula]) -> Plan {
    optimize(&diagram.circuit(roots).unwrap(), Limits::default(), || false).unwrap()
}

fn evaluate(plan: &Plan, inputs: &[bool]) -> Vec<bool> {
    let mut roots = vec![false; plan.roots().len()];
    plan.evaluate(inputs, &mut vec![false; plan.scratch_len()], &mut roots).unwrap();
    roots
}

#[test]
fn conditionals_lower_to_the_existing_plan_with_order_and_duplicate_roots() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    // Declaration order intentionally differs from the decision-variable order.
    let c = diagram.input(7).unwrap();
    let a = diagram.input(2).unwrap();
    let b = diagram.input(5).unwrap();
    let selected = diagram.select(a, b, c).unwrap();
    let xor = diagram.xor(b, c).unwrap();
    let and = diagram.and(a, b).unwrap();
    let roots = [selected, xor, and, selected, Formula::TRUE];
    let plan = plan(&diagram, &roots);
    assert_eq!(plan.inputs(), [2, 5, 7]);
    for mask in 0..8 {
        let [a, b, c] = std::array::from_fn(|bit| mask & (1 << bit) != 0);
        assert_eq!(
            evaluate(&plan, &[a, b, c]),
            [if a { b } else { c }, b ^ c, a && b, if a { b } else { c }, true]
        );
    }
}

#[test]
fn functional_equality_is_canonical_even_when_construction_order_differs() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let a = diagram.input(0).unwrap();
    let b = diagram.input(1).unwrap();
    let c = diagram.input(2).unwrap();
    let not_a = diagram.not(a).unwrap();
    assert_eq!(diagram.and(a, not_a).unwrap(), Formula::FALSE);
    assert_eq!(diagram.or(a, not_a).unwrap(), Formula::TRUE);
    let b_or_c = diagram.or(b, c).unwrap();
    let left = diagram.and(a, b_or_c).unwrap();
    let ab = diagram.and(a, b).unwrap();
    let ac = diagram.and(a, c).unwrap();
    let right = diagram.or(ac, ab).unwrap();
    assert_eq!(left, right, "distributivity must not require concrete operand enumeration");
    let not_b = diagram.not(b).unwrap();
    let neither = diagram.and(not_a, not_b).unwrap();
    let either = diagram.or(a, b).unwrap();
    assert_eq!(diagram.not(either).unwrap(), neither);
}

#[test]
fn twenty_two_inputs_are_reduced_without_an_operand_mask_table() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let inputs: Vec<_> = (0..22).map(|input| diagram.input(input).unwrap()).collect();
    let mut conjunction = Formula::TRUE;
    let mut any_inverse = Formula::FALSE;
    for &input in inputs.iter().rev() {
        conjunction = diagram.and(input, conjunction).unwrap();
        let inverse = diagram.not(input).unwrap();
        any_inverse = diagram.or(inverse, any_inverse).unwrap();
    }
    assert_eq!(diagram.not(any_inverse).unwrap(), conjunction);
    assert!(diagram.node_count() < 256);
    assert!(diagram.work() < 2_000);
    let plan = plan(&diagram, &[conjunction]);
    assert_eq!(evaluate(&plan, &[true; 22]), [true]);
    let mut sample = [true; 22];
    sample[17] = false;
    assert_eq!(evaluate(&plan, &sample), [false]);
}

#[test]
fn deep_conditionals_use_a_heap_work_stack() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let mut all = Formula::TRUE;
    for id in (0..4096).rev() {
        let input = diagram.input(id).unwrap();
        all = diagram.and(input, all).unwrap();
    }
    let inverted = diagram.not(all).unwrap();
    assert_eq!(diagram.and(all, inverted).unwrap(), Formula::FALSE);
    assert!(diagram.node_count() < 20_000);
}

#[test]
fn four_bit_power_operations_match_every_strength_and_attenuation() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let a = Signal::from_bits(std::array::from_fn(|bit| diagram.input(bit).unwrap()));
    let b = Signal::from_bits(std::array::from_fn(|bit| diagram.input(bit + 4).unwrap()));
    let mut roots = Vec::new();
    roots.extend(diagram.maximum(a, b).unwrap().bits());
    roots.push(diagram.powered(a).unwrap());
    roots.push(diagram.signal_equal(a, b).unwrap());
    for amount in 0..=17 {
        roots.extend(diagram.attenuate(a, amount).unwrap().bits());
    }
    let plan = plan(&diagram, &roots);
    let read = |bits: &[bool]| bits.iter().enumerate().fold(0u8, |value, (bit, &set)| value | (u8::from(set) << bit));
    for a in 0u8..16 {
        for b in 0u8..16 {
            let values: Vec<_> = (0..4)
                .map(|bit| a & (1 << bit) != 0)
                .chain((0..4).map(|bit| b & (1 << bit) != 0))
                .collect();
            let actual = evaluate(&plan, &values);
            assert_eq!(read(&actual[..4]), a.max(b));
            assert_eq!(actual[4], a != 0);
            assert_eq!(actual[5], a == b);
            for amount in 0..=17u8 {
                let start = 6 + usize::from(amount) * 4;
                assert_eq!(
                    read(&actual[start..start + 4]),
                    a.saturating_sub(amount),
                    "strength={a}, attenuation={amount}"
                );
            }
        }
    }
}

#[test]
fn sequential_guarded_assignments_use_the_preceding_symbolic_write() {
    let mut diagram = Diagram::new(Limits::default()).unwrap();
    let data = diagram.input(0).unwrap();
    let enable = diagram.input(1).unwrap();
    let old_state = diagram.input(2).unwrap();
    let first = diagram.select(enable, data, old_state).unwrap();
    let inverse = diagram.not(first).unwrap();
    let second = diagram.select(data, inverse, first).unwrap();
    let plan = plan(&diagram, &[first, second]);
    for mask in 0..8 {
        let [data, enable, old] = std::array::from_fn(|bit| mask & (1 << bit) != 0);
        let first = if enable { data } else { old };
        assert_eq!(evaluate(&plan, &[data, enable, old]), [first, if data { !first } else { first }]);
    }
}

#[test]
fn budgets_and_invalid_values_decline_instead_of_inventing_a_formula() {
    assert!(matches!(
        Diagram::new(Limits { max_work: 10, max_nodes: 1 }),
        Err(SymbolicError::BudgetExceeded)
    ));
    let mut diagram = Diagram::new(Limits { max_work: 10, max_nodes: 3 }).unwrap();
    let a = diagram.input(0).unwrap();
    assert_eq!(diagram.input(1), Err(SymbolicError::BudgetExceeded));
    assert_eq!(diagram.select(a, Formula(u32::MAX), Formula::FALSE), Err(SymbolicError::InvalidFormula));
    assert_eq!(diagram.input(InputId::MAX), Err(SymbolicError::InvalidInput));
    assert_eq!(Signal::literal(16), Err(SymbolicError::InvalidSignal));
    let invalid = Signal::from_bits([Formula(u32::MAX); 4]);
    assert_eq!(diagram.attenuate(invalid, 15), Err(SymbolicError::InvalidFormula));
    let mut exhausted = Diagram::new(Limits { max_work: 0, max_nodes: 100 }).unwrap();
    assert_eq!(exhausted.input(0), Err(SymbolicError::BudgetExceeded));
}
