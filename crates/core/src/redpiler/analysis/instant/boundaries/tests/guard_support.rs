use super::*;

fn graph() -> CompileGraph {
    CompileGraph {
        nodes: vec![],
        links: vec![],
        power_links: vec![],
        comparator_inputs: vec![],
        profiles: vec![],
        contacts: vec![],
        routes: vec![],
        wires: vec![],
        wire_relations: vec![],
        sites: vec![],
        pistons: vec![],
        resources: vec![],
        initial_pistons: vec![],
        initial_resources: vec![],
        plans: Default::default(),
        guards: vec![
            Guard::Always,
            Guard::State(StateTest::PistonExtended(PistonId::new(0))),
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Captured,
                test: SiteTest::Solid,
            },
            Guard::All(vec![GuardId::new(1), GuardId::new(2), GuardId::new(1)].into()),
            Guard::Not(GuardId::new(3)),
            Guard::State(StateTest::ResourceAt {
                resource: ResourceId::new(0),
                contact: ContactId::new(0),
            }),
        ],
    }
}

#[test]
fn atomic_and_shared_dag_guard_leaves_match_the_full_walk_in_original_order() {
    let graph = graph();
    let monitor = TaskMonitor::default();
    let mut cache = GuardSupport::default();
    for root in 0..graph.guards.len() {
        let root = GuardId::new(root as u32);
        let mut reference_budget = Budget::unbounded(&monitor);
        let expected = walk_guard_leaves(&graph, &mut reference_budget, root).unwrap();
        let mut budget = Budget::unbounded(&monitor);
        let actual = guard_leaves(&graph, &mut cache, &mut budget, root).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(guard_leaves(&graph, &mut cache, &mut budget, root).unwrap(), expected);
    }
    assert_eq!(cache.leaves.len(), graph.guards.len());
    assert_eq!(cache.leaves[&GuardId::new(3)].len(), 2, "shared atoms occur once in first-visit order");
}

#[test]
fn missing_guard_roots_are_never_remembered_as_successful() {
    let graph = graph();
    let monitor = TaskMonitor::default();
    let mut budget = Budget::unbounded(&monitor);
    let mut cache = GuardSupport::default();
    for _ in 0..2 {
        assert!(guard_leaves(&graph, &mut cache, &mut budget, GuardId::new(99)).is_err());
        assert!(cache.leaves.is_empty());
    }
}
