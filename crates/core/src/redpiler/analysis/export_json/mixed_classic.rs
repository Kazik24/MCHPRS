//! Export the exact mixed fixed-face query tables used by execution.
use super::*;
use crate::redpiler::program_graph::FixedClassicSource;

#[derive(Debug, Serialize)]
pub(super) struct PlanDto {
    wires: Vec<WireDto>,
    operands: Vec<OperandDto>,
}

#[derive(Debug, Serialize)]
struct WireDto {
    node: String,
    constant: u8,
    start: u32,
    end: u32,
    general_faces: u8,
}

#[derive(Debug, Serialize)]
struct OperandDto {
    node: String,
    /// A site means held powered-state emission; absence means live output.
    captured_site: Option<String>,
    attenuation: u8,
    require_cross: bool,
}

pub(super) fn export(graph: &CompileGraph) -> Option<PlanDto> {
    let plan = &graph.plans.mixed_classic;
    if plan.wires.is_empty() {
        return None;
    }
    Some(PlanDto {
        wires: plan
            .wires
            .iter()
            .zip(&graph.wires)
            .map(|(query, wire)| WireDto {
                node: typed_id("node", wire.node.index()),
                constant: query.fixed.constant,
                start: query.fixed.start,
                end: query.fixed.end,
                general_faces: query.general_faces,
            })
            .collect(),
        operands: plan
            .operands
            .iter()
            .map(|operand| {
                let (node, captured_site) = match operand.source {
                    FixedClassicSource::LiveOutput(node) => (node, None),
                    FixedClassicSource::CapturedPowered { node, site } => (node, Some(typed_id("site", site.index()))),
                };
                OperandDto {
                    node: typed_id("node", node.index()),
                    captured_site,
                    attenuation: operand.attenuation,
                    require_cross: operand.require_cross,
                }
            })
            .collect(),
    })
}

pub(super) fn validate(plan: &Option<PlanDto>, graph: &GraphTablesDto, nodes: &HashSet<String>, sites: &HashSet<String>) -> Result<(), ExportError> {
    let Some(plan) = plan else {
        return Ok(());
    };
    if plan.wires.len() != graph.wire_plans.len() {
        return Err(ExportError::new("validation", "mixed classic wire count"));
    }
    for (query, wire) in plan.wires.iter().zip(&graph.wire_plans) {
        if query.node != wire.node
            || query.constant > 15
            || query.general_faces & !63 != 0
            || query.start > query.end
            || query.end as usize > plan.operands.len()
        {
            return Err(ExportError::new("validation", "invalid mixed classic wire query"));
        }
        check_ref(nodes, &query.node, "mixed classic wire")?;
    }
    for operand in &plan.operands {
        check_ref(nodes, &operand.node, "mixed classic operand")?;
        if let Some(site) = &operand.captured_site {
            check_ref(sites, site, "mixed classic capture")?;
            if !graph
                .sites
                .iter()
                .any(|candidate| candidate.id == *site && matches!(&candidate.source, SiteSourceDto::Component { node } if *node == operand.node))
            {
                return Err(ExportError::new("validation", "mixed classic capture owner"));
            }
        }
        if operand.attenuation >= 15 || operand.captured_site.is_some() && operand.require_cross {
            return Err(ExportError::new("validation", "invalid mixed classic operand"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::{TaskMonitor, analysis::AnalysisLimits};
    use crate::{
        plot::PlotWorld,
        world::{World, storage::Chunk},
    };
    use mchprs_blocks::{BlockPos, blocks::Block};

    #[test]
    fn mixed_query_export_validates_ranges_masks_and_references() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(BlockPos::new(8, 19, 8), Block::Stone {});
        world.set_block(BlockPos::new(8, 20, 8), Block::RedstoneWire { wire: Default::default() });
        world.set_block(BlockPos::new(12, 20, 8), Block::from_name("note_block").unwrap());
        let mut export = super::super::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            &[],
            &CompilerOptions {
                optimization_level: Some(2),
                ..Default::default()
            },
            &TaskMonitor::default(),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(export.capabilities.contains(&"mixed_classic_queries"));
        export.to_bytes().unwrap();
        export.graph.mixed_classic.as_mut().unwrap().wires[0].end = u32::MAX;
        assert!(export.to_bytes().is_err());
        export.graph.mixed_classic.as_mut().unwrap().wires[0].end = 0;
        export.graph.mixed_classic.as_mut().unwrap().wires[0].general_faces = 64;
        assert!(export.to_bytes().is_err());
        export.graph.mixed_classic.as_mut().unwrap().wires[0].general_faces = 0;
        export.graph.mixed_classic.as_mut().unwrap().wires[0].node = "node:999999".into();
        assert!(export.to_bytes().is_err());
    }
}
