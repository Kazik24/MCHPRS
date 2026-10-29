#[test]
fn physical_graph_probe_builds_weighted_runtime() {
    use crate::redpiler::backend::graph::wire_probe::{
        probe,
        tests::{BOUNDS, LEFT, RECEIVER, RIGHT, fixture},
    };
    let world = fixture();
    let table = probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER]).unwrap();
    let mut runtime = EventNetlist::from_probe(&table).unwrap();
    assert_eq!(runtime.power(0), table.outputs()[0], "probe entry state is retained");
    for (mask, expected) in table.samples().iter().enumerate() {
        let inputs = [(mask & 1 != 0) as u8 * 15, (mask & 2 != 0) as u8 * 15];
        assert_eq!(&runtime.frame(&inputs, &[], &[0]).unwrap(), expected, "mask {mask}");
    }
}
#[test]
fn probe_conversion_rejects_interacting_source_tables() {
    let samples = vec![vec![0], vec![13], vec![11], vec![13]];
    let mut net = EventNetlist::from_samples(2, 1, &samples).unwrap();
    for (mask, expected) in samples.iter().enumerate() {
        let inputs = [(mask & 1 != 0) as u8 * 15, (mask & 2 != 0) as u8 * 15];
        assert_eq!(&net.frame(&inputs, &[], &[0]).unwrap(), expected);
    }
    assert!(net.frame(&[7, 0], &[], &[0]).is_err(), "physical probe covers binary sources only");
    let mut nonlinear = samples;
    nonlinear[3][0] = 14;
    assert!(EventNetlist::from_samples(2, 1, &nonlinear).is_err());
}
#[test]
fn contact_event_is_delivered_when_input_strength_does_not_change() {
    let links = vec![
        Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        },
        Link {
            source: 0,
            target: 1,
            attenuation: 0,
            contact: None,
        },
        Link {
            source: 0,
            target: 1,
            attenuation: 1,
            contact: Some((0, 1)),
        },
    ];
    let actions = vec![
        Action {
            input: 0,
            threshold: 0,
            when_powered: true,
            resource: 0,
            from: 0,
            to: 1,
        },
        Action {
            input: 1,
            threshold: 0,
            when_powered: true,
            resource: 1,
            from: 0,
            to: 1,
        },
    ];
    let mut net = EventNetlist::new(vec![15], vec![0, 0], 2, 2, links, actions, &[(0, 1)]).unwrap();
    assert_eq!(net.power(1), 15);
    assert_eq!(net.frame(&[15], &[0], &[1]).unwrap(), [15]);
    assert_eq!(net.contacts, [1, 1], "contact update delivers a request even though power stays 15");
}
#[test]
fn unpowered_bud_needs_a_request_and_revalidates_at_execution() {
    let links = vec![Link {
        source: 0,
        target: 0,
        attenuation: 0,
        contact: None,
    }];
    let actions = vec![Action {
        input: 0,
        threshold: 0,
        when_powered: false,
        resource: 0,
        from: 1,
        to: 0,
    }];
    let mut net = EventNetlist::new(vec![0], vec![1], 2, 1, links, actions, &[]).unwrap();
    assert_eq!(net.frame(&[0], &[], &[0]).unwrap(), [0]);
    assert_eq!(net.contacts, [1], "unpowered state alone cannot queue a retraction");
    assert_eq!(net.frame(&[15], &[0], &[0]).unwrap(), [15]);
    assert_eq!(net.contacts, [1], "live powered state rejects a stale request");
    assert_eq!(net.frame(&[0], &[0], &[0]).unwrap(), [0]);
    assert_eq!(net.contacts, [0], "a notified, unpowered resource moves once");
}
#[test]
fn falling_edge_and_explicit_rearm_are_separate_cycles() {
    let links = vec![Link {
        source: 0,
        target: 0,
        attenuation: 0,
        contact: None,
    }];
    let actions = vec![
        Action {
            input: 0,
            threshold: 0,
            when_powered: false,
            resource: 0,
            from: 1,
            to: 0,
        },
        Action {
            input: 0,
            threshold: 0,
            when_powered: true,
            resource: 0,
            from: 0,
            to: 1,
        },
    ];
    let mut net = EventNetlist::new(vec![15], vec![1], 2, 1, links, actions, &[]).unwrap();
    net.begin_computation_cycle();
    net.frame(&[0], &[0], &[]).unwrap();
    assert_eq!(net.contacts, [0], "falling-edge request pulls the resource");
    net.begin_computation_cycle();
    net.frame(&[15], &[1], &[]).unwrap();
    assert_eq!(net.contacts, [1], "explicit rearm moves it back on a later cycle");
    net.frame(&[0], &[], &[]).unwrap();
    assert_eq!(net.contacts, [1], "unpowered input without an event remains BUD");
    net.begin_computation_cycle();
    net.frame(&[0], &[0], &[]).unwrap();
    assert_eq!(net.contacts, [0]);
}
#[test]
fn fifo_revalidates_and_consumes_shared_resources_once_per_frame() {
    let links = vec![
        Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        },
        Link {
            source: 1,
            target: 1,
            attenuation: 2,
            contact: Some((0, 1)),
        },
        Link {
            source: 0,
            target: 2,
            attenuation: 0,
            contact: Some((0, 0)),
        },
        Link {
            source: 0,
            target: 3,
            attenuation: 0,
            contact: None,
        },
        Link {
            source: 1,
            target: 4,
            attenuation: 3,
            contact: Some((1, 1)),
        },
    ];
    let actions = vec![
        Action {
            input: 0,
            threshold: 0,
            when_powered: true,
            resource: 0,
            from: 0,
            to: 1,
        },
        Action {
            input: 1,
            threshold: 0,
            when_powered: true,
            resource: 1,
            from: 0,
            to: 1,
        },
        Action {
            input: 2,
            threshold: 0,
            when_powered: true,
            resource: 1,
            from: 0,
            to: 2,
        },
        Action {
            input: 3,
            threshold: 0,
            when_powered: true,
            resource: 1,
            from: 1,
            to: 2,
        },
    ];
    let mut net = EventNetlist::new(vec![15, 12], vec![0, 0], 3, 5, links.clone(), actions.clone(), &[(0, 1), (0, 2)]).unwrap();
    net.begin_computation_cycle();
    assert_eq!(net.frame(&[15, 12], &[0, 2, 3], &[4]).unwrap(), [9]);
    assert_eq!(net.contacts, [1, 1]);
    assert!(net.frame(&[15, 16], &[3], &[4]).is_err());
    assert!(net.frame(&[0, 12], &[999], &[4]).is_err());
    assert!(net.frame(&[0, 12], &[3], &[999]).is_err());
    assert_eq!(net.contacts, [1, 1]);
    assert_eq!(net.sources, [15, 12]);
    assert_eq!(net.frame(&[15, 12], &[3], &[4]).unwrap(), [9], "same-cycle loser stays rejected");
    assert_eq!(net.contacts, [1, 1]);
    net.begin_computation_cycle();
    assert_eq!(net.frame(&[15, 12], &[3], &[4]).unwrap(), [0]);
    assert_eq!(net.contacts, [1, 2]);
    assert!(EventNetlist::new(vec![16], vec![], 0, 0, vec![], vec![], &[]).is_err());
    let mut bad = links;
    bad[0].source = 9;
    assert!(EventNetlist::new(vec![15, 12], vec![0, 0], 3, 5, bad, actions, &[]).is_err());
}

#[test]
fn shared_resource_fifo_contention_and_explicit_cycle_rearm() {
    for (requests, expected) in [(&[0, 1][..], 1usize), (&[1, 0][..], 2usize)] {
        let actions = vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 2,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 1,
                to: 2,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 2,
                to: 1,
            },
        ];
        let mut net = EventNetlist::new(
            vec![15],
            vec![0],
            3,
            1,
            vec![Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            }],
            actions,
            &[],
        )
        .unwrap();
        net.begin_computation_cycle();
        net.frame(&[15], requests, &[]).unwrap();
        assert_eq!(net.contacts[0], expected, "first valid FIFO request owns the shared resource");
        net.begin_computation_cycle();
        net.frame(&[15], &[if expected == 1 { 2 } else { 3 }], &[]).unwrap();
        assert_eq!(net.contacts[0], if expected == 1 { 2 } else { 1 }, "new cycle rearms resource");
    }
}

#[test]
fn independent_resources_move_and_stale_request_does_not_consume() {
    let actions = vec![
        Action {
            input: 0,
            threshold: 0,
            when_powered: true,
            resource: 0,
            from: 0,
            to: 1,
        },
        Action {
            input: 0,
            threshold: 0,
            when_powered: true,
            resource: 0,
            from: 1,
            to: 2,
        },
        Action {
            input: 0,
            threshold: 0,
            when_powered: true,
            resource: 1,
            from: 0,
            to: 1,
        },
    ];
    let mut net = EventNetlist::new(
        vec![15],
        vec![0, 0],
        3,
        1,
        vec![Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        }],
        actions,
        &[],
    )
    .unwrap();
    net.frame(&[15], &[0, 1, 2], &[]).unwrap();
    assert_eq!(net.contacts, [1, 1], "stale followup is rejected; independent resource still moves");
    assert_eq!(net.cycle_consumed, None);

    let mut failed_then_valid = EventNetlist::new(
        vec![15],
        vec![0],
        2,
        1,
        vec![Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        }],
        vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 1,
                to: 0,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
        ],
        &[],
    )
    .unwrap();
    failed_then_valid.frame(&[15], &[0, 1], &[]).unwrap();
    assert_eq!(failed_then_valid.contacts, [1], "failed request leaves resource for the valid request");
}
