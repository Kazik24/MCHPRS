//! Opt-in native evidence for the ordinary pistons rejected as shared clocks.
use super::*;
use crate::redpiler::instant::{clocked, regions, sampling};
use crate::redstone::piston::trace::Operation;
use rustc_hash::{FxHashMap, FxHashSet};

#[test]
#[ignore = "PM1 clock-role inventory and bounded native trace; explicit new output required"]
fn pm1_observer_clock_candidates_have_distinct_controls_and_native_edges() {
    inspect_pm1_clocks(
        cpus::Cpu {
            schematic: "PM1_SORT_FIXED.schem",
            sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
            ..cpus::CPUS[0]
        },
        true,
    );
}

#[test]
#[ignore = "fresh author PM1 update-generator inspection; explicit new output required"]
fn pm1_compilation_1_update_generators() {
    inspect_pm1_clocks(
        cpus::Cpu {
            schematic:
                "piston-research/pm1-compilation-1-20261008-fresh/PM1_FIXED_COMPILATION_1.schem",
            sha256: "cf5ef6b5e62defbc02dc3b201b9bb29766feb310f6abfcad2941486312e0bd7c",
            start: BlockPos::new(156, 67, 69),
            stop: Some(BlockPos::new(156, 64, 69)),
            ..cpus::CPUS[0]
        },
        false,
    );
}

fn inspect_pm1_clocks(cpu: cpus::Cpu, require_frozen_prefix: bool) {
    let output = std::path::PathBuf::from(std::env::var("MCHPRS_PM1_CLOCK_OUTPUT").unwrap());
    assert!(
        !output.exists(),
        "diagnostics must not overwrite existing evidence"
    );
    let mut world = cpus::load_cpu(cpu);
    let before = cpus::checkpoint(&world, 0, &[]);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let report = analyze(
        &world,
        world.get_corners(),
        &[],
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let candidates: Vec<_> = report.pistons.iter().enumerate().filter_map(|(actor, p)| {
        (!p.piston.sticky && p.piston.facing == BlockFacing::Down
            && matches!(world.get_block(p.pos.offset(BlockFace::Top)), Block::Observer { observer } if observer.facing == BlockFacing::Down))
            .then_some(actor)
    }).collect();
    assert!(
        candidates.len() > 1,
        "reproduce the multiple-candidate case"
    );
    let positions: FxHashSet<_> = candidates
        .iter()
        .map(|&actor| report.pistons[actor].pos)
        .collect();
    let mut regions_json = Vec::new();
    for region in regions::split(&world, &report, &monitor).unwrap() {
        let count = region
            .pistons
            .iter()
            .filter(|p| positions.contains(&p.pos))
            .count();
        if count == 0 {
            continue;
        }
        let clock_result = clocked::recognize(&world, &region, &monitor, false)
            .map(|clock| clock.is_some())
            .map_err(|e| e.to_string());
        let resets = sampling::reset_candidates(&world, &region);
        // Diagnose the existing independent classifier without publishing a program.
        let independent = sampling::recognize(&world, &region, &monitor, None, &resets)
            .map(|c| json!({"generators": c.generators.len(), "memory": c.memory.len(), "sampling_events": c.events.len()}));
        regions_json.push(
            json!({"pistons": region.pistons.len(), "clock_candidates": count,
            "clock_result": clock_result, "independent_classifier_probe": independent}),
        );
    }
    let mut static_json = Vec::new();
    for &actor in &candidates {
        let piston = &report.pistons[actor];
        let above = piston.pos.offset(BlockFace::Top);
        let watching: Vec<_> = report.observers.iter().copied().filter(|&pos| {
            matches!(world.get_block(pos), Block::Observer { observer } if pos.offset(observer.facing.into()) == piston.pos)
        }).map(|pos| block_state(&world, pos, cpu.origin)).collect();
        let sources: Vec<_> = report.recognition[actor].inputs.sources.iter().map(|source|
            json!({"dependency": source, "state": block_state(&world, source.source, cpu.origin),
                "is_above_reset": source.source == above,
                "notifies_base": sampling::data_notifies(&world, source.source, piston.pos)})
        ).collect();
        let adjacent = |pos: BlockPos| {
            [piston.pos, piston.head].into_iter().any(|source| {
                let delta = source - pos;
                delta.x.abs() + delta.y.abs() + delta.z.abs() == 1
            })
        };
        let update_targets: Vec<_> = report
            .pistons
            .iter()
            .enumerate()
            .filter_map(|(target_actor, target)| {
                if !target.piston.sticky || !(adjacent(target.pos) || adjacent(target.head)) {
                    return None;
                }
                Some(json!({"local": target.pos - cpu.origin,
                "base": block_state(&world, target.pos, cpu.origin),
                "base_notified": adjacent(target.pos), "head_callback": adjacent(target.head),
                "data_sources": report.recognition[target_actor].inputs.sources,
                "updates": report.ports.pistons[target_actor].updates}))
            })
            .collect();
        static_json.push(
            json!({"world": piston.pos, "local": piston.pos - cpu.origin,
            "base": block_state(&world, piston.pos, cpu.origin),
            "head": block_state(&world, piston.head, cpu.origin),
            "far": block_state(&world, piston.head.offset(BlockFace::Bottom), cpu.origin),
            "cap": block_state(&world, above.offset(BlockFace::Top), cpu.origin),
            "watching_observers": watching, "sources": sources,
            "updates": report.ports.pistons[actor].updates, "update_targets": update_targets}),
        );
    }
    assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
    let mut translations = Vec::new();
    for origin in [
        cpu.origin,
        BlockPos::new(16, 8, 8),
        BlockPos::new(8, 8, 16),
        BlockPos::new(16, 16, 16),
    ] {
        let translated = cpus::load_cpu(cpus::Cpu { origin, ..cpu });
        let before = cpus::checkpoint(&translated, 0, &[]);
        for (budget_multiplier, optimize, assume_instant) in [
            (8, false, false),
            (8, true, false),
            (8, false, true),
            (8, true, true),
            (1, false, false),
            (1, true, false),
            (1, false, true),
            (1, true, true),
        ] {
            if origin != cpu.origin && (optimize || assume_instant || budget_multiplier != 8) {
                continue;
            }
            let mut compiler = Compiler::default();
            let now = Instant::now();
            let error = compiler
                .compile(
                    &translated,
                    translated.get_corners(),
                    CompilerOptions {
                        budget_multiplier,
                        optimize,
                        assume_instant,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap_err()
                .to_string();
            let seconds = now.elapsed().as_secs_f64();
            let expected_error = if budget_multiplier == 8 {
                "timed movement notifications require a proven shared-clock contract"
            } else {
                "analysis piston budget exceeded"
            };
            assert!(error.contains(expected_error), "{origin:?}: {error}");
            assert!(!compiler.is_active());
            assert_eq!(cpus::checkpoint(&translated, 0, &[]), before);
            translations.push(
            json!({"origin": origin, "optimize": optimize, "assume_instant": assume_instant, "budget_multiplier": budget_multiplier,
            "error": error, "compile_seconds": seconds, "input_unchanged": true}),
        );
        }
    }
    println!(
        "PM1: {} candidates; region results {}",
        candidates.len(),
        json!(regions_json)
    );
    let mut counts: FxHashMap<BlockPos, BTreeMap<String, usize>> = FxHashMap::default();
    let mut first_events: FxHashMap<BlockPos, Vec<Value>> = FxHashMap::default();
    let mut applied_edges: FxHashMap<BlockPos, Vec<Value>> = FxHashMap::default();
    let mut messages = Vec::new();
    cpus::click_cpu(&mut world, cpu, cpu.start);
    for tick in 1..=500 {
        for (sequence, entry) in trace::capture(|| world.tick_interpreted())
            .into_iter()
            .enumerate()
        {
            let (pos, key, applied) = match entry.operation {
                Operation::Sample {
                    pos,
                    extended,
                    powered,
                } => (
                    pos,
                    format!(
                        "{:?}: sample extended={extended} powered={powered}",
                        entry.phase
                    ),
                    false,
                ),
                Operation::Applied(event) => (
                    event.pos,
                    format!("{:?}: applied {:?}", entry.phase, event.action),
                    true,
                ),
            };
            if !positions.contains(&pos) {
                continue;
            }
            *counts
                .entry(pos)
                .or_default()
                .entry(key.clone())
                .or_default() += 1;
            let event = json!({"game_tick": tick, "logical_tick": entry.tick, "sequence": sequence, "operation": key});
            let first = first_events.entry(pos).or_default();
            if first.len() < 16 {
                first.push(event.clone());
            }
            if applied {
                applied_edges.entry(pos).or_default().push(event);
            }
        }
        cpus::collect_chat(&world, tick, &mut messages);
    }
    let native: Vec<_> = candidates
        .iter()
        .map(|&actor| {
            let pos = report.pistons[actor].pos;
            json!({"world": pos, "local": pos - cpu.origin, "counts": counts.get(&pos),
            "first_operations": first_events.get(&pos), "applied_edges": applied_edges.get(&pos)})
        })
        .collect();
    assert!(
        !applied_edges.is_empty(),
        "the start protocol must exercise candidates"
    );
    let mut expected = cpus::reference(cpus::CPUS[0]);
    expected.chat_trace.retain(|(tick, _)| *tick <= 500);
    let matches_frozen_prefix = messages == expected.chat_trace;
    if require_frozen_prefix {
        assert_eq!(
            messages, expected.chat_trace,
            "diagnostic instrumentation preserves ordered output"
        );
    }
    let result = json!({"fixture": cpu.schematic, "sha256": cpu.sha256, "origin": cpu.origin,
        "native_ticks": 500, "candidate_count": candidates.len(), "regions": regions_json, "translations": translations,
        "static": static_json, "native": native, "messages": messages,
        "static_inspection_unchanged": true, "native_ordered_output_matches_frozen_prefix": matches_frozen_prefix,
        "frozen_prefix_messages": expected.chat_trace});
    std::fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!("PM1 native clock-role evidence: {}", output.display());
}
