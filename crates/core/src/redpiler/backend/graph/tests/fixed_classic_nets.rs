//! Active opportunity census before attempting ordered multi-input net contraction.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[test]
#[ignore = "research: active CHUNGUS per-net work and callback-boundary census"]
fn chungus_active_binary_net_census() {
    let ticks = std::env::var("MCHPRS_CHUNGUS_TICKS").map_or(16_384, |value| value.parse().unwrap());
    let world = super::super::chungus_census::load_chungus_world();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &world,
        world.get_corners(),
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let fixed = graph.plans.fixed_classic.program.as_ref().unwrap();
    let analysis = ClassicAnalysis::new(graph);
    let regions = passes::compile_classic_redstone::census_regions(graph);
    let mut descriptions = Vec::new();
    for members in &regions {
        let owned = members.iter().copied().collect::<BTreeSet<_>>();
        let mut sources = BTreeSet::new();
        let mut reasons = BTreeSet::new();
        let mut outside_wires = BTreeSet::new();
        let mut receivers = BTreeSet::new();
        for &node in members {
            match analysis.census_expression(node, InputPort::Main) {
                Ok((constant, leaves)) => {
                    if constant != 0 {
                        reasons.insert("constant electrical contribution");
                    }
                    sources.extend(leaves.into_iter().filter(|source| !owned.contains(source)));
                }
                Err(_) => {
                    reasons.insert("unresolved electrical query");
                }
            }
            let FixedClassicKind::Wire(index) = fixed.nodes[node.index()].kind else {
                unreachable!()
            };
            let wire = &fixed.wires[index as usize];
            if wire.inert_sides.is_none() {
                reasons.insert("non-invariant wire shape");
            }
            for (slot, &site) in wire.neighbors.iter().enumerate() {
                if wire.admitted & (1 << slot) == 0 {
                    continue;
                }
                match fixed.sites[site.index()] {
                    FixedClassicSite::Wire { node } if !owned.contains(&node) => {
                        outside_wires.insert(node);
                    }
                    FixedClassicSite::Gate(node) => {
                        receivers.insert(node);
                    }
                    _ => {}
                }
            }
        }
        if members.len() > CLASSIC_MAX_WIRES {
            reasons.insert("more than 32 wires");
        }
        if !(1..=4).contains(&sources.len()) {
            reasons.insert("source count outside 1..4");
        }
        for &source in &sources {
            if !matches!(
                graph.nodes[source.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::Torch | NodeType::Repeater { .. })
            ) {
                reasons.insert("source is not certified binary");
            }
            let ComponentState::Electrical(state) = &graph.nodes[source.index()].state else {
                reasons.insert("source lacks electrical state");
                continue;
            };
            if state.output_strength != u8::from(state.powered) * 15 {
                reasons.insert("inconsistent binary entry state");
            }
            if matches!(graph.nodes[source.index()].kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
                && (state.repeater_locked || !matches!(analysis.census_expression(source, InputPort::Side), Ok((0, leaves)) if leaves.is_empty()))
            {
                reasons.insert("source can lock");
            }
        }
        let boundary_kinds = receivers
            .iter()
            .map(|node| format!("{:?}", graph.nodes[node.index()].kind))
            .collect::<BTreeSet<_>>();
        descriptions.push((reasons, sources.len(), outside_wires.len(), boundary_kinds, analysis.infer(members).err()));
    }
    drop(analysis);
    let mut backend = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
    backend.test_recorder.fixed_classic_activity = vec![[0; 4]; backend.fixed_classic.as_ref().unwrap().wires.len()];
    backend.use_block(super::super::chungus_census::START_BUTTON).unwrap();
    for _ in 0..ticks {
        backend.tick().unwrap();
    }
    let mut totals = [0u64; 4];
    let mut binary = [0u64; 4];
    let mut closed = [0u64; 4];
    let mut histogram = BTreeMap::<String, [u64; 6]>::new();
    let mut rows = Vec::new();
    for (members, (reasons, sources, outside_wires, boundary_kinds, previous_decline)) in regions.iter().zip(descriptions) {
        let mut activity = [0u64; 4];
        for &node in members {
            let FixedClassicKind::Wire(index) = backend.fixed_classic.as_ref().unwrap().nodes[node.index()].kind else {
                unreachable!()
            };
            for (total, value) in activity.iter_mut().zip(backend.test_recorder.fixed_classic_activity[index as usize]) {
                *total += value;
            }
        }
        for (total, value) in totals.iter_mut().zip(activity) {
            *total += value;
        }
        let candidate = reasons.is_empty();
        if candidate {
            for (total, value) in binary.iter_mut().zip(activity) {
                *total += value;
            }
            if outside_wires == 0 {
                for (total, value) in closed.iter_mut().zip(activity) {
                    *total += value;
                }
            }
        }
        let category = if candidate {
            format!("binary_{sources}_sources_outside_wires={}", outside_wires != 0)
        } else {
            "outside_binary_family".into()
        };
        let count = histogram.entry(category).or_default();
        count[0] += 1;
        count[1] += members.len() as u64;
        for (total, value) in count[2..].iter_mut().zip(activity) {
            *total += value;
        }
        rows.push(
            json!({"first_wire":backend.position(members[0]), "wires":members.len(), "sources":sources,
            "candidate":candidate, "electrical_declines":reasons, "outside_callback_wires":outside_wires,
            "boundary_kinds":boundary_kinds, "previous_first_decline":previous_decline, "activity":activity}),
        );
    }
    assert_eq!(totals[0], backend.test_recorder.fixed_classic_counts.seed_calls);
    assert_eq!(totals[1] + totals[2], backend.test_recorder.fixed_classic_counts.wire_steps);
    assert!(totals[1] > 0 && totals[2] > 0);
    rows.sort_by_key(|row| std::cmp::Reverse(row["activity"][2].as_u64().unwrap()));
    let report = json!({"ticks":ticks,"tier":5,"activity_columns":["seed_calls","changed_seeds","frontier_visits","changed_frontier_visits"],
        "totals":totals,"binary_candidates":binary,"binary_without_outside_wire_callbacks":closed,
        "histogram_columns":["regions","wires","seed_calls","changed_seeds","frontier_visits","changed_frontier_visits"],
        "histogram":histogram,"regions":rows,
        "limits":"Work counts, not timings or an eligibility proof. Candidate family checks electrical binary inputs and invariant shapes only. Outside wire callbacks, capture aliases, receiver samples, entry state, imported work and ownership still require certification."});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chungus-stage3-active-net-census.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("ACTIVE NET CENSUS totals={totals:?}, binary={binary:?}, closed={closed:?}, histogram={histogram:?}");
}
