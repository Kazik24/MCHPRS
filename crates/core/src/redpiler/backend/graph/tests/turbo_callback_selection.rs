use super::*;

fn delivery(target: SiteId, guard: u32, view: OperandView, receiver: Option<CallbackReceiver>, sequence: u32) -> RouteDelivery {
    RouteDelivery {
        target,
        kind: DeliveryKind::Update { direction: None },
        guard: GuardId::new(guard),
        view,
        sequence,
        receiver,
    }
}

#[test]
fn indexed_and_linear_selection_keep_the_same_first_passing_row() {
    let receiver = CallbackReceiver::Component(NodeId::new(7));
    let target = SiteId::new(200);
    let other_targets: Vec<_> = (0..10).map(|slot| SiteId::new(slot * 11 + 3)).collect();
    let mut route = other_targets
        .iter()
        .map(|&site| delivery(site, 9, OperandView::Captured, Some(receiver), 17))
        .collect::<Vec<_>>();
    route.extend([
        delivery(target, 9, OperandView::Captured, None, 30),
        delivery(target, 9, OperandView::Live, Some(receiver), 30),
        delivery(target, 8, OperandView::Captured, Some(receiver), 90),
        delivery(target, 9, OperandView::Captured, Some(receiver), 90),
    ]);
    let observed = ObservedSite {
        profile: None,
        receiver: Some(receiver),
        properties: ObservedProperties::Passive,
    };
    let matches_guard = |guard, _, observed: &ObservedSite| guard == GuardId::new(9) && observed.receiver == Some(receiver);
    let linear = first_turbo_delivery(&route, target, &observed, matches_guard);
    let indexed = first_turbo_delivery(&route[10..], target, &observed, matches_guard);
    assert_eq!(linear, indexed);
    assert_eq!(linear, Some(route[13]));

    let miss_guard = |_, _, _: &ObservedSite| false;
    let linear_miss = first_turbo_delivery(&route, target, &observed, miss_guard);
    let indexed_miss = first_turbo_delivery(&route[10..], target, &observed, miss_guard);
    assert_eq!(linear_miss, indexed_miss);
    assert_eq!(linear_miss, None);
}
