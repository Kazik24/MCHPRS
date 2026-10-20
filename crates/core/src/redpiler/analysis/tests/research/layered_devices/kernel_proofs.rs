//! Probe real proof cuts without relaxing certificates or changing public execution.
use super::*;
use crate::redpiler::analysis::instant::{layers, regions};

mod witnesses;

// shortcut: the single-cell compiler still searches global port tables; bound this diagnostic,
// rather than disguising a sampled proof attempt as whole-CPU coverage.
const EXTERNAL_WIRE_PROBE_CELLS: usize = 64;

#[test]
#[ignore = "explicit CPU instant proof audit over full and conservatively cut ownership catalogs"]
fn bubblesort_kernel_proofs_report_reset_and_ownership_cuts() {
    let (world, bounds, _) = super::large_builds::load_cpu(cpus::CPUS[2]);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(
        &world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::experimental_boundary_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let mut graph = prepared.graph;
    let catalog = graph.plans.instant.boundaries.take().unwrap();
    let mut reports = Vec::new();
    for family in [
        "all-admitted",
        "observer-resets",
        "private-own-observer",
        "sample-private-cells-external-wires",
    ] {
        let cut = cut_catalog(&catalog, family);
        let partition = regions::partition(&graph, &cut, &monitor).unwrap();
        let plan = layers::compile_layers(&graph, &cut, &monitor).unwrap();
        let mut declines = BTreeMap::<String, usize>::new();
        for decline in &plan.declines {
            *declines.entry(format!("{:?}", decline.reason)).or_default() += decline.cells.len();
        }
        let actors = cut.cells.iter().map(|cell| cell.uses.len()).sum::<usize>();
        let compiled_actors = plan
            .regions
            .iter()
            .flat_map(|region| region.cells.iter())
            .map(|&cell| cut.cells[cell as usize].uses.len())
            .sum::<usize>();
        assert_eq!(
            plan.regions.iter().map(|region| region.cells.len()).sum::<usize>()
                + plan.declines.iter().map(|region| region.cells.len()).sum::<usize>(),
            cut.cells.len(),
            "every retained cell has an explicit proof outcome"
        );
        let mut summary = json!({
            "family": family, "cells": cut.cells.len(), "actors": actors,
            "corridors": cut.corridors.len(), "dependency_regions": partition.regions.len(),
            "largest_dependency_region_cells": partition.regions.iter().map(|region| region.cells.len()).max().unwrap_or(0),
            "proof_regions": plan.regions.len() + plan.declines.len(),
            "largest_proof_region_cells": plan.regions.iter().map(|region| region.cells.len()).chain(plan.declines.iter().map(|region| region.cells.len())).max().unwrap_or(0),
            "compiled_regions": plan.regions.len(), "compiled_actors": compiled_actors,
            "declined_cells_by_reason": declines,
            "witnesses": plan.declines.iter().take(8).map(|decline| format!("{:?}: {:?}", decline.reason, decline.witness)).collect::<Vec<_>>(),
            "runtime_replacement": false, "sampled": family == "sample-private-cells-external-wires",
        });
        if family == "all-admitted" {
            summary["decline_examples"] = witnesses::examples(&graph, &cut, &plan);
        }
        graph.plans.instant.boundaries = Some(cut);
        graph.plans.instant.layers = Some(plan);
        let kernels = layers::lower_kernels(&graph, &monitor).unwrap();
        summary["executable_kernels"] = json!(kernels.len());
        summary["executable_kernel_actors"] = json!(kernels.iter().map(|kernel| kernel.actors.len()).sum::<usize>());
        let mut console = summary.clone();
        console.as_object_mut().unwrap().remove("decline_examples");
        println!("{console}");
        reports.push(summary);
    }
    save_report(
        "cpu-kernel-proof-cuts",
        &json!({
            "fixture": "cpu_bubblesort", "schematic_sha256": cpus::CPUS[2].sha256,
            "scope": "Static proof attempts only; removed owners remain external Graph devices. No altered reset or ordering assumptions.",
            "reports": reports,
        }),
    );
}

/// Keep complete resource owners; every cut dependency remains an outside input or output.
fn cut_catalog(catalog: &BoundaryCatalog, family: &str) -> BoundaryCatalog {
    let mut remap = vec![None; catalog.cells.len()];
    let mut cells = Vec::new();
    for (index, cell) in catalog.cells.iter().enumerate() {
        let keep = match family {
            "all-admitted" => true,
            "observer-resets" => cell.uses.iter().all(|use_| use_.reset.observer().is_some()),
            "private-own-observer" | "sample-private-cells-external-wires" => {
                cell.uses.len() == 1
                    && cell.resources.len() == 1
                    && cell.resources[0].owners.as_ref() == [cell.uses[0].actor]
                    && cell.uses[0]
                        .reset
                        .observer_wake()
                        .is_some_and(|wake| wake.watched_actor == cell.uses[0].actor)
            }
            _ => panic!("unknown proof family"),
        };
        let sample_full = family == "sample-private-cells-external-wires" && cells.len() >= EXTERNAL_WIRE_PROBE_CELLS;
        if keep && !sample_full {
            remap[index] = Some(cells.len() as u32);
            cells.push(cell.clone());
        }
    }
    let external_wires = family == "sample-private-cells-external-wires";
    let ports = catalog
        .ports
        .iter()
        .filter_map(|port| {
            let owner = match port.owner {
                BoundaryOwner::Cell(cell) => BoundaryOwner::Cell(remap[cell as usize]?),
                BoundaryOwner::Corridor(_) if external_wires => return None,
                owner => owner,
            };
            Some(BoundaryPort { owner, ..*port })
        })
        .collect();
    BoundaryCatalog {
        cells: cells.into(),
        declines: catalog.declines.clone(),
        corridors: if external_wires { Box::new([]) } else { catalog.corridors.clone() },
        ports,
    }
}
