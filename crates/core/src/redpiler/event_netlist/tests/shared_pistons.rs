#[test]
fn shared_route_keeps_piston_recipient_and_occurrence_identity() {
    use super::super::physical_signal_network::SignalEvent;
    use super::super::program_graph::{GuardId, PistonId};

    let events = [
        SignalEvent {
            piston: PistonId::new(0),
            occurrence: 41,
            route: 7,
            action: mchprs_world::PistonAction::Extend,
            piston_input: 15,
            captures: vec![],
            piston_guards: vec![(GuardId::new(0), true)],
        },
        SignalEvent {
            piston: PistonId::new(1),
            occurrence: 42,
            route: 7,
            action: mchprs_world::PistonAction::Extend,
            piston_input: 15,
            captures: vec![],
            piston_guards: vec![(GuardId::new(0), true)],
        },
    ];
    assert!(belongs_to_piston(&events[0], PistonId::new(0), &[7]));
    assert!(!belongs_to_piston(&events[1], PistonId::new(0), &[7]));
    assert!(belongs_to_piston(&events[1], PistonId::new(1), &[7]));
    assert_ne!(events[0].occurrence, events[1].occurrence);
}
