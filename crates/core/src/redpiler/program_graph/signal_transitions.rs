//! Ordered channel effects after specializing interior transport to fixed entry inputs.
use super::{RouteId, TransportNet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalOp {
    /// Bind an external receiver at its original first-discovery cut.
    Capture(u32),
    /// Save admission at the enqueue cut, separately from eventual delivery.
    Select { wire: u32, slot: u8, selection: u32 },
    /// Publish the exact strength needed by subsequent boundary samples.
    SetPower { wire: u32, power: u8 },
    /// Deliver an admitted sample; MAX selection means admission was proved unconditional.
    /// Source route and causal layer retain the original delivery provenance.
    Call { site: u32, selection: u32, layer: u32, route: RouteId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignalTranscript {
    /// Contiguous effects; interior wire visits and topology queries are absent.
    pub(crate) ops: Box<[SignalOp]>,
    /// Scratch Boolean count for enqueue-time admission results.
    pub(crate) selections: u32,
    /// Exact terminal strengths certify state recovery and subsequent entries.
    pub(crate) final_powers: Box<[u8]>,
}

/// Fold only callback decisions proved by the caller, then remove captures that
/// no remaining selection or call reads. Dynamic selection stays at its source cut.
pub(crate) fn minimize_signal_operations(
    net: &TransportNet,
    ops: &mut Vec<SignalOp>,
    selections: &mut u32,
    mut admission: impl FnMut(u32, u8) -> Option<bool>,
) {
    #[derive(Clone, Copy, Default)]
    enum Decision {
        #[default]
        Unseen,
        Known(bool),
        Dynamic(u32),
    }

    let mut decisions = vec![Decision::Unseen; *selections as usize];
    let mut next_selection = 0u32;
    let mut minimized = Vec::with_capacity(ops.len());
    for op in ops.drain(..) {
        match op {
            SignalOp::Select { wire, slot, selection } => {
                if let Some(known) = admission(wire, slot) {
                    decisions[selection as usize] = Decision::Known(known);
                } else {
                    decisions[selection as usize] = Decision::Dynamic(next_selection);
                    minimized.push(SignalOp::Select {
                        wire,
                        slot,
                        selection: next_selection,
                    });
                    next_selection += 1;
                }
            }
            SignalOp::Call {
                site,
                selection,
                layer,
                route,
            } if selection == u32::MAX => minimized.push(SignalOp::Call {
                site,
                selection,
                layer,
                route,
            }),
            SignalOp::Call {
                site,
                selection,
                layer,
                route,
            } => match decisions[selection as usize] {
                Decision::Known(false) => {}
                Decision::Known(true) => minimized.push(SignalOp::Call {
                    site,
                    selection: u32::MAX,
                    layer,
                    route,
                }),
                Decision::Dynamic(compact) => minimized.push(SignalOp::Call {
                    site,
                    selection: compact,
                    layer,
                    route,
                }),
                Decision::Unseen => unreachable!("signal call must follow its selection"),
            },
            other => minimized.push(other),
        }
    }

    let mut needed = rustc_hash::FxHashSet::default();
    for op in &minimized {
        match *op {
            SignalOp::Select { wire, slot, .. } => {
                needed.insert(net.wires[wire as usize].neighbors[slot as usize]);
            }
            SignalOp::Call { site, .. } => {
                needed.insert(site);
            }
            _ => {}
        }
    }
    minimized.retain(|op| !matches!(*op, SignalOp::Capture(site) if !needed.contains(&site)));
    *ops = minimized;
    *selections = next_selection;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::program_graph::{NodeId, SiteId, TransportWire};

    fn test_net() -> TransportNet {
        let mut neighbors = [u32::MAX; 24];
        neighbors[0] = 2;
        neighbors[1] = 3;
        neighbors[2] = 4;
        TransportNet {
            wires: vec![TransportWire {
                node: NodeId::new(0),
                site: 0,
                neighbors,
                inputs: 0,
                conditional_inputs: 0,
                route: RouteId::new(0),
            }]
            .into_boxed_slice(),
            sites: (0..5).map(SiteId::new).collect::<Vec<_>>().into_boxed_slice(),
            site_wire: vec![0, u32::MAX, u32::MAX, u32::MAX, u32::MAX].into_boxed_slice(),
            ordered_receivers: false,
            canonical_repeaters: Box::new([]),
        }
    }

    #[test]
    fn minimizer_folds_known_admission_and_preserves_dynamic_order_and_effects() {
        let net = test_net();
        let route = RouteId::new(7);
        let original = vec![
            SignalOp::Capture(1),
            SignalOp::Capture(2),
            SignalOp::Capture(3),
            SignalOp::Capture(4),
            SignalOp::Select {
                wire: 0,
                slot: 0,
                selection: 0,
            },
            SignalOp::Call {
                site: 2,
                selection: 0,
                layer: 1,
                route,
            },
            SignalOp::Select {
                wire: 0,
                slot: 1,
                selection: 1,
            },
            SignalOp::Call {
                site: 3,
                selection: 1,
                layer: 2,
                route,
            },
            SignalOp::Select {
                wire: 0,
                slot: 2,
                selection: 2,
            },
            SignalOp::Call {
                site: 4,
                selection: 2,
                layer: 3,
                route,
            },
            SignalOp::SetPower { wire: 0, power: 11 },
            SignalOp::SetPower { wire: 0, power: 6 },
        ];
        let mut ops = original.clone();
        let mut selections = 3;
        minimize_signal_operations(&net, &mut ops, &mut selections, |_, slot| match slot {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        });
        assert_eq!(selections, 1);
        assert_eq!(
            ops,
            vec![
                SignalOp::Capture(3),
                SignalOp::Capture(4),
                SignalOp::Call {
                    site: 3,
                    selection: u32::MAX,
                    layer: 2,
                    route
                },
                SignalOp::Select {
                    wire: 0,
                    slot: 2,
                    selection: 0
                },
                SignalOp::Call {
                    site: 4,
                    selection: 0,
                    layer: 3,
                    route
                },
                SignalOp::SetPower { wire: 0, power: 11 },
                SignalOp::SetPower { wire: 0, power: 6 },
            ]
        );

        let effects = |ops: &[SignalOp], dynamic: bool| {
            let mut decisions = rustc_hash::FxHashMap::default();
            let mut effects = Vec::new();
            for &op in ops {
                match op {
                    SignalOp::Select { slot, selection, .. } => {
                        decisions.insert(
                            selection,
                            match slot {
                                0 => false,
                                1 => true,
                                _ => dynamic,
                            },
                        );
                    }
                    SignalOp::Call {
                        site,
                        selection,
                        layer,
                        route,
                    } if selection == u32::MAX || decisions[&selection] => {
                        effects.push(SignalOp::Call {
                            site,
                            selection: u32::MAX,
                            layer,
                            route,
                        });
                    }
                    SignalOp::SetPower { .. } => effects.push(op),
                    _ => {}
                }
            }
            effects
        };
        for dynamic in [false, true] {
            assert_eq!(effects(&original, dynamic), effects(&ops, dynamic));
        }
        let once = ops.clone();
        minimize_signal_operations(&net, &mut ops, &mut selections, |_, slot| (slot < 2).then_some(slot == 1));
        assert_eq!(selections, 1);
        assert_eq!(ops, once);
    }
}
