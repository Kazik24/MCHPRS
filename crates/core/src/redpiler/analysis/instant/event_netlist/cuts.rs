//! Source cuts retain live native reads without transferring their writers.
use super::discovery::EventCandidates;
use super::ports::PortRole;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES;
use crate::redpiler::program_graph::{BoundaryCatalog, BoundaryPortKind, CircuitNodeKind, CompileGraph, NodeId, ResourceId, SiteSource, StateTest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceOwner {
    /// The source remains in Graph, including any complete resource owner group.
    Native,
    /// Another candidate is only a prospective owner; it has not been installed.
    Candidate(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CutSource {
    /// The indexed power link retains its original face, attenuation and guard.
    Wire { node: NodeId, link: u32 },
    /// The original guard-state row specifies enabled versus contact membership.
    Resource { resource: ResourceId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceCut {
    pub(crate) region: u32,
    pub(crate) row: u32,
    pub(crate) source: CutSource,
    pub(crate) owner: SourceOwner,
}

/// Collect a supported subset of source cuts. This does not certify the other
/// rows, grant exclusive ownership, or authorize suppression of Graph work.
pub(crate) fn source_cuts(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    candidates: &EventCandidates,
    monitor: &TaskMonitor,
) -> Result<Vec<SourceCut>, String> {
    const MAX_CUT_WORK: usize = 32 * 1024 * 1024;
    if monitor.cancelled() {
        return Err("cancelled while deriving source cuts".into());
    }
    let count = candidates.ports.len();
    if count > MAX_CUT_WORK.saturating_mul(monitor.budget_multiplier())
        || count.checked_mul(std::mem::size_of::<SourceCut>()).ok_or("source cut size overflow")?
            > (MAX_GRAPH_PREPARATION_BYTES / 4).saturating_mul(monitor.budget_multiplier())
    {
        return Err("source cut budget exceeded".into());
    }
    let mut cuts = Vec::with_capacity(count);
    for port in &candidates.ports {
        if monitor.cancelled() {
            return Err("cancelled while deriving source cuts".into());
        }
        if port.role != PortRole::Read {
            continue;
        }
        let row = catalog.ports.get(port.row as usize).ok_or("source cut catalog row is missing")?;
        let (source, owner) = match row.kind {
            BoundaryPortKind::Power { link } => {
                let edge = graph.power_links.get(link as usize).ok_or("source cut power link is missing")?;
                let site = graph.sites.get(edge.from.index()).ok_or("source cut power site is missing")?;
                let SiteSource::Component(node) = site.source else { continue };
                if !matches!(
                    graph.nodes.get(node.index()).ok_or("source cut node is missing")?.kind,
                    CircuitNodeKind::Wire(_)
                ) {
                    continue;
                }
                let owner = *candidates
                    .partition
                    .owners
                    .nodes
                    .get(node.index())
                    .ok_or("source cut node owner is missing")?;
                (CutSource::Wire { node, link }, owner)
            }
            BoundaryPortKind::GuardState {
                test: StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource),
                ..
            } => {
                let owner = *candidates
                    .partition
                    .owners
                    .resources
                    .get(resource.index())
                    .ok_or("source cut resource owner is missing")?;
                if let Some(owner) = owner {
                    let cell = catalog.cells.get(owner).ok_or("source cut resource owner is not a cell")?;
                    let pin = cell
                        .resources
                        .iter()
                        .find(|pin| pin.resource == resource)
                        .ok_or("source cut resource pin is missing")?;
                    if pin.owners.is_empty()
                        || pin
                            .owners
                            .iter()
                            .any(|actor| candidates.partition.owners.actors.get(actor.index()).copied().flatten() != Some(owner))
                    {
                        return Err("source cut resource owner group is incomplete".into());
                    }
                }
                (CutSource::Resource { resource }, owner)
            }
            _ => continue,
        };
        let owner = source_owner(owner, &candidates.selected, &candidates.partition.owner_region)?;
        if owner != SourceOwner::Candidate(port.region) {
            cuts.push(SourceCut {
                region: port.region,
                row: port.row,
                source,
                owner,
            });
        }
    }
    Ok(cuts)
}

fn source_owner(owner: Option<usize>, selected: &[bool], regions: &[u32]) -> Result<SourceOwner, String> {
    let Some(owner) = owner else { return Ok(SourceOwner::Native) };
    if !*selected.get(owner).ok_or("source cut owner is missing")? {
        return Ok(SourceOwner::Native);
    }
    Ok(SourceOwner::Candidate(*regions.get(owner).ok_or("source cut region is missing")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_selection_does_not_transfer_native_ownership() {
        let selected = [true, false, true];
        let regions = [1, 2, 3];
        assert_eq!(source_owner(None, &selected, &regions).unwrap(), SourceOwner::Native);
        assert_eq!(source_owner(Some(1), &selected, &regions).unwrap(), SourceOwner::Native);
        assert_eq!(source_owner(Some(2), &selected, &regions).unwrap(), SourceOwner::Candidate(3));
        assert!(source_owner(Some(3), &selected, &regions).is_err());
    }
}
