use super::*;
use crate::{Plan, optimize};

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
