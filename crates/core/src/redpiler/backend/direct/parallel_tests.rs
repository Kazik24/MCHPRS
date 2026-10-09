use super::*;
use crate::redpiler::analysis::ports::ConsumerInput;
use crate::redpiler::instant::boolean::{BooleanArena, Variable, TRUE};
use crate::redpiler::instant::logic::WaveLogic;
use crate::redpiler::instant::outputs::{OutputPort, PowerTerm};
use crate::redpiler::instant::program::PreparedInstant;
use rayon::ThreadPoolBuilder;

const INPUTS: usize = 32;

fn backend(regions: usize, ports: usize, parallel: bool) -> DirectBackend {
    let mut backend = DirectBackend::default();
    let node = |ty| Node {
        ty,
        default_inputs: Default::default(),
        side_inputs: Default::default(),
        updates: Default::default(),
        is_io: false,
        powered: false,
        locked: false,
        output_power: 0,
        changed: false,
        pending_tick: false,
    };
    backend.nodes = Nodes::new(
        (0..INPUTS + regions * ports)
            .map(|index| {
                node(if index < INPUTS {
                    NodeType::Lever
                } else {
                    NodeType::InstantSource
                })
            })
            .collect(),
    );
    backend.blocks = vec![None; backend.nodes.inner().len()];
    let pos = |index| BlockPos::new(index as i32, 0, 0);
    let bindings: FxHashMap<_, _> = (0..backend.nodes.inner().len())
        .map(|index| (pos(index), backend.nodes.get(index)))
        .collect();
    let output_bindings = (0..regions * ports)
        .map(|index| (index, backend.nodes.get(INPUTS + index)))
        .collect();
    for region in 0..regions {
        let mut arena = BooleanArena::with_budget(1);
        let inputs: Vec<_> = (0..INPUTS)
            .map(|index| {
                arena.variable(Variable::Signal {
                    pos: pos(index),
                    threshold: (index % 15) as u8,
                    order: index,
                })
            })
            .collect();
        let mut guard = inputs[0];
        let mut outputs = Vec::new();
        for port in 0..ports {
            let input = inputs[(port + 1) % INPUTS];
            let inverse = arena.not(input);
            guard = arena.select(guard, inverse, input);
            outputs.push(OutputPort {
                consumer: pos(INPUTS + region * ports + port),
                input: ConsumerInput::Main,
                initial_strength: 0,
                terms: vec![
                    PowerTerm {
                        guard,
                        source: Some(pos(port % INPUTS)),
                        attenuation: (port % 8) as u8,
                    },
                    PowerTerm {
                        guard: TRUE,
                        source: Some(pos((port + 7) % INPUTS)),
                        attenuation: 12,
                    },
                ],
            });
        }
        // A later region reads the preceding region's published output next pass.
        let mut sources: Vec<_> = (0..INPUTS).map(pos).collect();
        if region > 0 {
            let previous = pos(INPUTS + (region - 1) * ports);
            sources.push(previous);
            let enabled = arena.variable(Variable::Signal {
                pos: previous,
                threshold: 0,
                order: INPUTS,
            });
            outputs[0].terms[0].guard = enabled;
        }
        let program = PreparedInstant {
            pistons: Vec::new(),
            output_offset: region * ports,
            clocked: None,
            independent_memory: Vec::new(),
            sampling: Vec::new(),
            reset_groups: Vec::new(),
            payloads: Vec::new(),
            controls: Vec::new(),
            groups: Vec::new(),
            aliases: Vec::new(),
            owned: Default::default(),
            propagation_wires: Default::default(),
            observable_wires: Default::default(),
            sampling_wires: Default::default(),
            candidate_wire_count: 0,
            wire_retention_reasons: [0; 7],
            template: Vec::new(),
            logical_tick: 0,
            logic: WaveLogic {
                arena,
                responses: Vec::new(),
                response_order: Vec::new(),
                sources,
                response_sources: Vec::new(),
                wires: Default::default(),
                wire_links: Default::default(),
                unprojected_consumer_wires: Default::default(),
                context: Default::default(),
                follows_payload: Vec::new(),
                handoff_wires: Vec::new(),
                outputs,
            },
        };
        let runtime =
            instant::Runtime::bind(program, &bindings, &output_bindings, &backend.nodes).unwrap();
        for source in runtime.source_nodes() {
            let regions = backend.instant_dependencies.entry(source).or_default();
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
        backend.instant.push(runtime);
    }
    backend.instant_dirty = vec![true; regions];
    backend.instant_parallel = Some(parallel);
    backend
}

fn assert_same(actual: &DirectBackend, expected: &DirectBackend) {
    assert_eq!(
        format!("{:?}", actual.nodes.inner()),
        format!("{:?}", expected.nodes.inner())
    );
    assert_eq!(
        format!("{:?}", actual.scheduler),
        format!("{:?}", expected.scheduler)
    );
    assert_eq!(actual.events, expected.events);
    assert_eq!(actual.instant_dirty, expected.instant_dirty);
    assert_eq!(actual.instant_phases, expected.instant_phases);
    assert_eq!(actual.logical_stats(), expected.logical_stats());
    assert!(actual.boundary_deliveries.is_empty());
    assert!(expected.boundary_deliveries.is_empty());
}

#[test]
fn parallel_regions_preserve_delivery_order_strengths_and_convergence() {
    for workers in [1, 2, 4] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        pool.install(|| {
            let mut actual = backend(4, 16, true);
            let mut expected = backend(4, 16, false);
            for backend in [&mut actual, &mut expected] {
                let mut nodes = backend.nodes.inner().to_vec();
                let first_consumer = nodes.len();
                for _ in 0..4 {
                    for ty in [
                        NodeType::CommandBlock {
                            repeating: false,
                            chain: false,
                            automatic: false,
                        },
                        NodeType::CopperBulb,
                    ] {
                        let mut consumer = nodes[0].clone();
                        consumer.ty = ty;
                        consumer.default_inputs.strength_counts[0] = 1;
                        nodes.push(consumer);
                    }
                }
                backend.nodes = Nodes::new(nodes.into_boxed_slice());
                backend.blocks.resize(backend.nodes.inner().len(), None);
                for region in 0..4 {
                    let source = backend.nodes.get(INPUTS + region * 16);
                    for index in 0..2 {
                        let consumer = backend.nodes.get(first_consumer + region * 2 + index);
                        backend.nodes[source]
                            .updates
                            .push(node::ForwardLink::new(consumer, false, 0));
                    }
                }
            }
            for step in 0..48 {
                for input in 0..INPUTS {
                    let power = ((step * 13 + input * 7) % 16) as u8;
                    for backend in [&mut actual, &mut expected] {
                        backend.set_node(backend.nodes.get(input), power > 0, power);
                    }
                }
                // Sparse reverse delivery order must survive sorting for mutable borrows.
                for backend in [&mut actual, &mut expected] {
                    backend.publish_instant_regions([3, 1], false);
                }
                assert_same(&actual, &expected);
                for backend in [&mut actual, &mut expected] {
                    backend.evaluate_instant(false);
                }
                assert_same(&actual, &expected);
                assert!(actual.nodes.inner()[INPUTS..]
                    .iter()
                    .any(|node| node.output_power > 0));
                actual.tick();
                expected.tick();
                assert_same(&actual, &expected);
            }
            assert!(actual
                .events
                .iter()
                .any(|event| matches!(event, Event::CommandBlockPower { .. })));
            assert!(actual
                .events
                .iter()
                .any(|event| matches!(event, Event::CopperBulbToggle { .. })));
            assert_eq!(actual.instant_parallel_batches > 0, workers > 1);
            assert_eq!(expected.instant_parallel_batches, 0);
        });
    }
}

#[test]
fn empty_single_and_clean_regions_do_not_dispatch_parallel_work() {
    let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
    pool.install(|| {
        for regions in [0, 1, 4] {
            let mut actual = backend(regions, 1, true);
            let mut expected = backend(regions, 1, false);
            actual.evaluate_instant(false);
            expected.evaluate_instant(false);
            assert_same(&actual, &expected);
            let dispatched = actual.instant_parallel_batches;
            actual.evaluate_instant(false);
            actual.tick();
            expected.evaluate_instant(false);
            expected.tick();
            assert_same(&actual, &expected);
            assert_eq!(actual.instant_parallel_batches, dispatched);
            assert_eq!(dispatched > 0, regions > 1);
        }
    });
}

#[test]
fn default_dispatch_keeps_small_batches_sequential() {
    for workers in [1, 4] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        pool.install(|| {
            for ports in [1, 16, 256] {
                let mut actual = backend(8, ports, true);
                actual.instant_parallel = None;
                let mut expected = backend(8, ports, false);
                actual.evaluate_instant(false);
                expected.evaluate_instant(false);
                assert_same(&actual, &expected);
                assert_eq!(
                    actual.instant_parallel_batches > 0,
                    workers > 1 && ports == 256
                );
            }
        });
    }
}

#[test]
#[ignore = "run explicitly without concurrent builds to measure region evaluation"]
fn parallel_region_throughput() {
    use std::time::Instant;

    let workers = rayon::current_num_threads();
    for ports in [1, 16, 256, 1024] {
        for parallel in [false, true] {
            let mut samples = Vec::new();
            for _ in 0..3 {
                let mut backend = backend(8, ports, false);
                if parallel {
                    backend.instant_parallel = None;
                }
                for step in 0..32 {
                    for input in 0..INPUTS {
                        let power = ((step * 13 + input * 7) % 16) as u8;
                        backend.set_node(backend.nodes.get(input), power > 0, power);
                    }
                    backend.evaluate_instant(false);
                }
                let start = Instant::now();
                for step in 0..1000 {
                    for input in 0..INPUTS {
                        let power = ((step * 13 + input * 7) % 16) as u8;
                        backend.set_node(backend.nodes.get(input), power > 0, power);
                    }
                    backend.evaluate_instant(false);
                    std::hint::black_box(backend.nodes.inner());
                }
                samples.push(start.elapsed());
            }
            samples.sort();
            eprintln!("8 regions, {ports} ports each, {workers} workers, default_parallel={parallel}: median {:?}, samples {samples:?}", samples[1]);
        }
    }
}
