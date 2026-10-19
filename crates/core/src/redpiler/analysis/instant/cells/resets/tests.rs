use super::*;

fn witness() -> CellReset {
    let occurrence = RouteOccurrence {
        route: RouteId::new(0),
        ordinal: 0,
    };
    CellReset::InputRestoration {
        resource: ResourceId::new(0),
        contact: ContactId::new(0),
        notification: occurrence,
        sample: occurrence,
    }
}

#[test]
fn reset_worklist_cannot_certify_an_unseeded_feedback_loop() {
    let monitor = TaskMonitor::default();
    let mut resets = vec![None, None, None];
    propagate_waiting(
        &mut resets,
        vec![vec![(1, witness())], vec![(0, witness()), (2, witness())], vec![]],
        &monitor,
    )
    .unwrap();
    assert_eq!(resets, vec![None, None, None]);
    resets[0] = Some(witness());
    propagate_waiting(
        &mut resets,
        vec![vec![(1, witness())], vec![(0, witness()), (2, witness())], vec![]],
        &monitor,
    )
    .unwrap();
    assert!(
        resets.iter().all(Option::is_some),
        "one real reset anchor closes the dependency chain once"
    );
    monitor.cancel();
    assert!(propagate_waiting(&mut resets, vec![vec![], vec![], vec![]], &monitor).is_err());
}
