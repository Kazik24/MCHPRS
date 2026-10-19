//! Compile a whole contactless fixed circuit to dense electrical/callback instructions.
//! This removes general site/guard/route selection while preserving the original
//! captured values, intermediate wire writes, native walk order and delayed future.
//! Main/Side analog inputs and shared receivers remain live; no source-vector proof
//! or local private ownership is required. Unsupported whole circuits decline.
//! Mutable state remains in CircuitState/GraphFuture. Re-derivation checks stale
//! plans; it is not an independent semantic proof of this partial evaluator.
//! Shape deliveries disappear only when the actual entry sides are fixed points
//! for every direction under immutable geometry. Power writes preserve those
//! sides, and backend attachment checks the entry assumption again.
use super::*;
use crate::redpiler::TaskMonitor;
use mchprs_blocks::blocks::{ComparatorMode, RedstoneWire, RedstoneWireSide};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixedClassicKind {
    Wire(u32),
    Torch,
    Repeater { delay: u8, facing_diode: bool },
    Comparator { mode: ComparatorMode, facing_diode: bool },
    Lamp,
    Hopper,
    Lever,
    Button,
    PressurePlate,
    Constant,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixedClassicSite {
    Inert,
    Wire { node: NodeId },
    Gate(NodeId),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixedClassicSource {
    LiveOutput(NodeId),
    CapturedPowered { site: SiteId, node: NodeId },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedClassicOperand {
    pub(crate) source: FixedClassicSource,
    pub(crate) attenuation: u8,
    pub(crate) require_cross: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FixedClassicQuery {
    pub(crate) constant: u8,
    pub(crate) start: u32,
    pub(crate) end: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedClassicComparator {
    pub(crate) direct_override: Option<ComparatorOverride>,
    pub(crate) far_override: Option<ComparatorOverride>,
    pub(crate) far_threshold: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedClassicNode {
    pub(crate) kind: FixedClassicKind,
    pub(crate) inputs: [FixedClassicQuery; 3],
    pub(crate) output_route: Option<RouteId>,
    pub(crate) comparator: Option<FixedClassicComparator>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedClassicWire {
    pub(crate) node: NodeId,
    pub(crate) root: SiteId,
    pub(crate) neighbors: [SiteId; 24],
    /// Every mutable canonical neighbor, including sites with no admitted callback.
    pub(crate) mutable_neighbors: u32,
    pub(crate) seed_mask: u32,
    pub(crate) turbo_mask: u32,
    pub(crate) raw_sides: u8,
    /// Entry sides proven invariant under all shape deliveries; checked at attachment.
    pub(crate) inert_sides: Option<u8>,
    pub(crate) admitted: u32,
    /// BlockFace::values() order: Top, Bottom, North, South, East, West.
    pub(crate) no_dust: [FixedClassicQuery; 6],
    /// The same six pure queries joined once, sharing their contiguous operands.
    pub(crate) block_input: FixedClassicQuery,
    pub(crate) shape_route: Option<RouteId>,
}
impl FixedClassicWire {
    /// Four stored sides, independent of the live analog power.
    pub(crate) fn packed_sides(wire: RedstoneWire) -> u8 {
        (wire.north.get_id() | wire.south.get_id() << 2 | wire.east.get_id() << 4 | wire.west.get_id() << 6) as u8
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixedClassicOpKind {
    Seed,
    Shape(BlockFace),
    Update { direction: Option<BlockFace> },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedClassicRouteOp {
    pub(crate) ordinal: u32,
    pub(crate) target: NodeId,
    pub(crate) kind: FixedClassicOpKind,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FixedClassicProgram {
    pub(crate) nodes: Box<[FixedClassicNode]>,
    pub(crate) sites: Box<[FixedClassicSite]>,
    pub(crate) wires: Box<[FixedClassicWire]>,
    pub(crate) routes: Box<[Range<usize>]>,
    pub(crate) ops: Box<[FixedClassicRouteOp]>,
    pub(crate) operands: Box<[FixedClassicOperand]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FixedClassicPlans {
    pub(crate) program: Option<Arc<FixedClassicProgram>>,
    pub(crate) decline: Option<String>,
}

impl FixedClassicPlans {
    /// Deterministic stale-plan checking, not an independent semantic oracle.
    /// Existing query/wire certificates are validated by their own graph checks.
    pub(crate) fn validate(&self, graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(), String> {
        // A decline owns no executable work. Later builders may add local net
        // ownership; keep the diagnostic from the whole-program attempt.
        if self.program.is_none() {
            return Ok(());
        }
        if self.program.is_some() && self.decline.is_some() {
            return Err("fixed classic plan is both accepted and declined".into());
        }
        match (self.program.as_deref(), FixedClassicProgram::compile_graph(graph, monitor)) {
            (Some(saved), Ok(current)) if *saved == current => Ok(()),
            (_, Err(reason)) if monitor.cancelled() => Err(reason),
            _ => Err("stale or forged fixed classic plan".into()),
        }
    }
}

impl FixedClassicProgram {
    /// Keep the lowered-program entry point for differential tests of both views.
    pub(crate) fn compile(program: &CircuitProgram, state: &CircuitState) -> Result<Self, String> {
        Self::compile_view(FixedClassicView::Lowered(program, state), &TaskMonitor::default())
    }
    pub(crate) fn compile_graph(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        Self::compile_view(FixedClassicView::draft(graph, monitor)?, monitor)
    }
    fn compile_view(program: FixedClassicView<'_>, monitor: &TaskMonitor) -> Result<Self, String> {
        cancelled(monitor)?;
        if !program.contacts().is_empty() || !program.pistons().is_empty() || !program.resources().is_empty() || program.has_private_owners() {
            return Err("fixed classic requires unowned contactless fixed circuitry".into());
        }
        for (index, guard) in program.guards().iter().enumerate() {
            if index % 4096 == 0 {
                cancelled(monitor)?;
            }
            if matches!(guard, Guard::State(_)) {
                return Err("fixed classic does not support state guards".into());
            }
        }
        let base_bytes = program
            .node_count()
            .saturating_mul(std::mem::size_of::<FixedClassicNode>())
            .saturating_add(program.sites().len().saturating_mul(std::mem::size_of::<FixedClassicSite>()))
            .saturating_add(program.wires().len().saturating_mul(std::mem::size_of::<FixedClassicWire>()))
            .saturating_add(program.routes().len().saturating_mul(std::mem::size_of::<Range<usize>>()));
        if base_bytes > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES {
            return Err("fixed classic executable memory bound".into());
        }
        let mut compiler = FixedCompiler {
            program: &program,
            monitor,
            operands: Vec::new(),
        };
        let mut nodes = Vec::with_capacity(program.nodes().len());
        for (index, node) in program.nodes().enumerate() {
            cancelled(monitor)?;
            let id = NodeId::new(index as u32);
            let kind = match (*node.kind).clone() {
                CircuitNodeKind::Wire(index) => FixedClassicKind::Wire(index as u32),
                CircuitNodeKind::Electrical(kind) => match kind {
                    NodeType::Torch => FixedClassicKind::Torch,
                    NodeType::Repeater { delay, facing_diode } => FixedClassicKind::Repeater { delay, facing_diode },
                    NodeType::Comparator { mode, facing_diode, .. } => FixedClassicKind::Comparator { mode, facing_diode },
                    NodeType::Lamp => FixedClassicKind::Lamp,
                    NodeType::Hopper => FixedClassicKind::Hopper,
                    NodeType::Lever => FixedClassicKind::Lever,
                    NodeType::Button => FixedClassicKind::Button,
                    NodeType::PressurePlate => FixedClassicKind::PressurePlate,
                    NodeType::Constant => FixedClassicKind::Constant,
                    other => return Err(format!("fixed classic unsupported gate {other:?} at {id:?}")),
                },
                other => return Err(format!("fixed classic unsupported actor {other:?} at {id:?}")),
            };
            let mut inputs = [FixedClassicQuery::default(); 3];
            for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                inputs[port as usize] = compiler.live_query(PowerTarget::Input { node: id, port })?;
            }
            let comparator = if matches!(kind, FixedClassicKind::Comparator { .. }) {
                let plan = program.comparator_input(id).ok_or_else(|| format!("missing comparator plan {id:?}"))?;
                let far_override = match plan.far_override {
                    Some((_, source, guard)) if compiler.guard(guard, None, 0)? => Some(source),
                    _ => None,
                };
                Some(FixedClassicComparator {
                    direct_override: plan.direct_override,
                    far_override,
                    far_threshold: plan.far_threshold,
                })
            } else {
                None
            };
            nodes.push(FixedClassicNode {
                kind,
                inputs,
                output_route: node.output_route,
                comparator,
            });
        }
        let mut roots = vec![None; nodes.len()];
        let mut sites = Vec::with_capacity(program.sites().len());
        for (index, site) in program.sites().iter().enumerate() {
            cancelled(monitor)?;
            sites.push(match site.source {
                SiteSource::Constant(profile) => {
                    if matches!(program.profile_block(profile), Some(Block::RedstoneWire { .. })) {
                        return Err("fixed classic does not capture boundary constant wires".into());
                    }
                    FixedClassicSite::Inert
                }
                SiteSource::Component(node) => {
                    if roots[node.index()].is_none() || program.node(node.index()).position == Some(site.position) {
                        roots[node.index()] = Some(SiteId::new(index as u32));
                    }
                    match nodes[node.index()].kind {
                        FixedClassicKind::Wire(_) => FixedClassicSite::Wire { node },
                        _ => FixedClassicSite::Gate(node),
                    }
                }
                SiteSource::Contact(_) => return Err("fixed classic contact site".into()),
            });
        }
        let mut wires = Vec::with_capacity(program.wires().len());
        for wire in program.wires() {
            cancelled(monitor)?;
            let facts = wire.execution.query_facts.ok_or("missing fixed wire facts")?;
            if facts.known != WireCallbackSelection::ALL {
                return Err("dynamic wire geometry".into());
            }
            let transfer = facts.transfer.ok_or("missing fixed wire transfer")?;
            if transfer.seed.conditional != 0 || transfer.turbo.conditional != 0 {
                return Err("conditional wire geometry".into());
            }
            let raw_sides = facts.raw_sides.ok_or("missing fixed raw wire sides")?;
            let mut entry_wire = program.initial_wire(wire.node)?;
            entry_wire.power = 0;
            let side = |shift: u32| RedstoneWireSide::from_id(u32::from((raw_sides >> shift) & 3));
            let raw_wire = RedstoneWire::new(side(0), side(2), side(4), side(6), 0);
            let inert_sides = BlockFace::values()
                .into_iter()
                .all(|face| crate::redstone::wire::on_neighbor_changed_from(entry_wire, raw_wire, face) == entry_wire)
                .then(|| FixedClassicWire::packed_sides(entry_wire));
            let selection = wire.execution.callback_selection.ok_or("missing fixed callback selection")?;
            if selection.known != WireCallbackSelection::ALL {
                return Err("dynamic wire callback selection".into());
            }
            let mut no_dust = [FixedClassicQuery::default(); 6];
            let block_start = compiler.operands.len() as u32;
            for (index, (face, slot)) in BlockFace::values().into_iter().zip([3, 2, 4, 5, 1, 0]).enumerate() {
                no_dust[index] = compiler.no_dust(wire.canonical_neighbors[slot], face)?;
            }
            let block_end = compiler.operands.len() as u32;
            let block_input = FixedClassicQuery {
                constant: no_dust.iter().map(|query| query.constant).max().unwrap_or(0),
                start: if block_start == block_end { 0 } else { block_start },
                end: if block_start == block_end { 0 } else { block_end },
            };
            let mutable_neighbors = wire.canonical_neighbors.iter().enumerate().fold(0, |mask, (slot, site)| {
                mask | (u32::from(!matches!(sites[site.index()], FixedClassicSite::Inert)) << slot)
            });
            wires.push(FixedClassicWire {
                node: wire.node,
                root: roots[wire.node.index()].ok_or("wire root missing")?,
                neighbors: wire.canonical_neighbors,
                mutable_neighbors,
                seed_mask: transfer.seed.direct,
                turbo_mask: transfer.turbo.direct,
                raw_sides,
                inert_sides,
                admitted: selection.admitted,
                no_dust,
                block_input,
                shape_route: wire.shape_route,
            });
        }
        let mut ops = Vec::new();
        let mut routes = Vec::with_capacity(program.routes().len());
        let mut executable_routes = vec![false; program.routes().len()];
        for node in program.nodes() {
            // Electrical actors enter output routes. Wire propagation calls
            // typed site handlers, and wires separately enter shape routes.
            if !matches!(node.kind, CircuitNodeKind::Wire(_))
                && let Some(route) = node.output_route
            {
                executable_routes[route.index()] = true;
            }
        }
        for wire in &wires {
            if wire.inert_sides.is_none()
                && let Some(route) = wire.shape_route
            {
                executable_routes[route.index()] = true;
            }
        }
        for (route_index, route) in program.routes().iter().enumerate() {
            cancelled(monitor)?;
            let start = ops.len();
            // Fixed execution enters only actor output and wire-shape routes.
            // Turbo dispatch uses admission/site instructions, never its route rows.
            if !executable_routes[route_index] {
                routes.push(start..start);
                continue;
            }
            for (ordinal, row) in route.deliveries.iter().enumerate() {
                // Output and shape routes have no capture context. Turbo admission is
                // separately compiled above and dispatches the observed site receiver.
                if row.view == OperandView::Captured || !compiler.guard(row.guard, None, 0)? {
                    continue;
                }
                let target = match row.receiver {
                    Some(CallbackReceiver::Component(node)) => Some(node),
                    Some(_) => return Err("fixed classic dynamic callback receiver".into()),
                    None => match (program.sites()[row.target.index()].source, row.kind) {
                        (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(node),
                        _ => None,
                    },
                };
                let Some(target) = target else { continue };
                let kind = match (nodes[target.index()].kind, row.kind) {
                    (FixedClassicKind::Wire(_), DeliveryKind::Update { .. }) => FixedClassicOpKind::Seed,
                    (FixedClassicKind::Wire(index), DeliveryKind::Shape { direction }) => {
                        if wires[index as usize].inert_sides.is_some() {
                            continue;
                        }
                        FixedClassicOpKind::Shape(direction)
                    }
                    (_, DeliveryKind::Shape { .. }) => continue,
                    (_, DeliveryKind::Update { direction }) => FixedClassicOpKind::Update { direction },
                };
                ops.push(FixedClassicRouteOp {
                    ordinal: ordinal as u32,
                    target,
                    kind,
                });
            }
            routes.push(start..ops.len());
        }
        let result = Self {
            nodes: nodes.into_boxed_slice(),
            sites: sites.into_boxed_slice(),
            wires: wires.into_boxed_slice(),
            routes: routes.into_boxed_slice(),
            ops: ops.into_boxed_slice(),
            operands: compiler.operands.into_boxed_slice(),
        };
        if result.inventory().estimated_bytes > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES {
            return Err("fixed classic executable memory bound".into());
        }
        Ok(result)
    }
}

struct FixedCompiler<'a, 'v> {
    program: &'a FixedClassicView<'v>,
    monitor: &'a TaskMonitor,
    operands: Vec<FixedClassicOperand>,
}

/// Partial evaluation of pure reads only; uncertainty keeps the original face query.
pub(super) fn compile_mixed_wire_power(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<MixedClassicProgram, String> {
    let view = FixedClassicView::draft(graph, monitor)?;
    let mut compiler = FixedCompiler {
        program: &view,
        monitor,
        operands: Vec::new(),
    };
    let wire_bytes = graph.wires.len().saturating_mul(std::mem::size_of::<MixedClassicWire>());
    if wire_bytes > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES {
        return Ok(MixedClassicProgram::default());
    }
    let mut wires = Vec::with_capacity(graph.wires.len());
    for wire in &graph.wires {
        cancelled(monitor)?;
        let start = compiler.operands.len();
        let mut constant = 0;
        let mut general_faces = 0;
        for (index, (face, slot)) in BlockFace::values().into_iter().zip([3, 2, 4, 5, 1, 0]).enumerate() {
            let before = compiler.operands.len();
            let site = wire.canonical_neighbors[slot];
            // General captured block reconstruction keeps control flags live.
            // Their face stays general rather than assuming held-powered semantics.
            let live_control = matches!(graph.sites[site.index()].source, SiteSource::Component(node)
                if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)));
            let query = if live_control {
                Err("live control flag".into())
            } else {
                compiler.no_dust(site, face)
            };
            match query {
                Ok(query) => constant = constant.max(query.constant),
                Err(_) => {
                    cancelled(monitor)?;
                    compiler.operands.truncate(before);
                    general_faces |= 1 << index;
                }
            }
        }
        if wire_bytes.saturating_add(compiler.operands.len().saturating_mul(std::mem::size_of::<FixedClassicOperand>()))
            > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES
        {
            return Ok(MixedClassicProgram::default());
        }
        wires.push(MixedClassicWire {
            fixed: FixedClassicQuery {
                constant,
                start: start as u32,
                end: compiler.operands.len() as u32,
            },
            general_faces,
        });
    }
    Ok(MixedClassicProgram {
        wires: wires.into_boxed_slice(),
        operands: compiler.operands.into_boxed_slice(),
    })
}

impl FixedCompiler<'_, '_> {
    fn append(&mut self, constant: u8, inputs: impl IntoIterator<Item = FixedClassicOperand>) -> Result<FixedClassicQuery, String> {
        cancelled(self.monitor)?;
        let mut inputs = inputs.into_iter().peekable();
        if inputs.peek().is_none() {
            return Ok(FixedClassicQuery { constant, start: 0, end: 0 });
        }
        let start = u32::try_from(self.operands.len()).map_err(|_| "fixed operand table overflow")?;
        for input in inputs {
            if self.operands.len() >= crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / std::mem::size_of::<FixedClassicOperand>() {
                return Err("fixed classic operand memory bound".into());
            }
            self.operands.push(input);
        }
        let end = u32::try_from(self.operands.len()).map_err(|_| "fixed operand table overflow")?;
        Ok(FixedClassicQuery { constant, start, end })
    }
    fn live_query(&mut self, target: PowerTarget) -> Result<FixedClassicQuery, String> {
        let program = self.program;
        match program.fixed_signal_query(target) {
            Some((constant, inputs)) => self.append(
                constant,
                inputs.iter().map(|input| FixedClassicOperand {
                    source: FixedClassicSource::LiveOutput(input.node),
                    attenuation: input.attenuation,
                    require_cross: input.require_cross,
                }),
            ),
            None if !self.program.has_power_links(target) => Ok(FixedClassicQuery::default()),
            None => Err(format!("fixed classic missing live query {target:?}")),
        }
    }
    fn block(&self, site: SiteId) -> Result<Block, String> {
        match self.program.sites()[site.index()].source {
            SiteSource::Constant(profile) => self.program.profile_block(profile).ok_or_else(|| "missing constant block".into()),
            SiteSource::Component(node) => self.program.node(node.index()).block.ok_or_else(|| "missing fixed actor block".into()),
            SiteSource::Contact(_) => Err("contact-dependent block".into()),
        }
    }
    fn no_dust(&mut self, site: SiteId, face: BlockFace) -> Result<FixedClassicQuery, String> {
        let block = self.block(site)?;
        let solid = match self.program.sites()[site.index()].source {
            SiteSource::Constant(profile) => self.program.profiles()[profile.index()].solid,
            _ => block.is_solid(),
        };
        if solid {
            return self.live_query(PowerTarget::Conductor { site, include_dust: false });
        }
        if matches!(block, Block::RedstoneWire { .. }) || !crate::redpiler::power_rules::emits_weak(block, face) {
            return Ok(FixedClassicQuery::default());
        }
        // RedstoneBlock's NodeState has strength fifteen but powered=false.
        // Native no-dust emission is the block's literal fifteen, not that flag.
        if block == Block::RedstoneBlock {
            return self.append(15, []);
        }
        match self.program.sites()[site.index()].source {
            SiteSource::Constant(_) => self.append(self.program.sites()[site.index()].source_strength, []),
            SiteSource::Component(node) => {
                if !matches!(
                    *self.program.node(node.index()).kind,
                    CircuitNodeKind::Electrical(
                        NodeType::Torch
                            | NodeType::Repeater { .. }
                            | NodeType::Comparator { .. }
                            | NodeType::Lever
                            | NodeType::Button
                            | NodeType::PressurePlate
                            | NodeType::Constant
                    ) | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                ) {
                    return Err("unsupported fixed no-dust emitter".into());
                }
                let source = if matches!(
                    *self.program.node(node.index()).kind,
                    CircuitNodeKind::Electrical(NodeType::Comparator { .. })
                ) {
                    FixedClassicSource::LiveOutput(node)
                } else {
                    FixedClassicSource::CapturedPowered { site, node }
                };
                self.append(
                    0,
                    [FixedClassicOperand {
                        source,
                        attenuation: 0,
                        require_cross: false,
                    }],
                )
            }
            SiteSource::Contact(_) => Err("contact-dependent no-dust query".into()),
        }
    }
    fn guard(&self, id: GuardId, captured: Option<SiteId>, depth: usize) -> Result<bool, String> {
        cancelled(self.monitor)?;
        if depth > 64 {
            return Err("fixed classic guard depth".into());
        }
        Ok(match &self.program.guards()[id.index()] {
            Guard::Always => true,
            Guard::State(_) => return Err("fixed classic dynamic guard".into()),
            Guard::Site {
                site,
                view: OperandView::Captured,
                ..
            } if captured != Some(*site) => false,
            Guard::Site { site, test, .. } => match self.program.sites()[site.index()].source {
                SiteSource::Constant(profile) => {
                    let record = self.program.profiles()[profile.index()];
                    match test {
                        SiteTest::Solid => record.solid,
                        SiteTest::Transparent => record.transparent,
                        SiteTest::IsWire => matches!(self.block(*site)?, Block::RedstoneWire { .. }),
                        SiteTest::Profile(expected) => *expected == profile,
                    }
                }
                SiteSource::Component(_) => match test {
                    SiteTest::Solid => self.block(*site)?.is_solid(),
                    SiteTest::Transparent => self.block(*site)?.is_transparent(),
                    SiteTest::IsWire => matches!(self.block(*site)?, Block::RedstoneWire { .. }),
                    SiteTest::Profile(_) => false,
                },
                SiteSource::Contact(_) => return Err("fixed classic contact guard".into()),
            },
            Guard::Not(child) => !self.guard(*child, captured, depth + 1)?,
            Guard::All(children) => {
                let mut result = true;
                for child in children {
                    result &= self.guard(*child, captured, depth + 1)?;
                }
                result
            }
            Guard::Any(children) => {
                let mut result = false;
                for child in children {
                    result |= self.guard(*child, captured, depth + 1)?;
                }
                result
            }
        })
    }
}

fn cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while compiling fixed classic circuit".into())
    } else {
        Ok(())
    }
}

/// Borrow the two existing representations; only draft lookup indexes are owned.
/// No draft is cloned, lowered, or recursively validated by this adapter.
enum FixedClassicView<'a> {
    Draft {
        graph: &'a CompileGraph,
        queries: FxHashMap<PowerTarget, usize>,
        readers: rustc_hash::FxHashSet<PowerTarget>,
        comparators: Vec<Option<usize>>,
    },
    Lowered(&'a CircuitProgram, &'a CircuitState),
}
struct FixedClassicNodeView<'a> {
    kind: &'a CircuitNodeKind,
    block: Option<Block>,
    position: Option<BlockPos>,
    output_route: Option<RouteId>,
}
impl<'a> FixedClassicView<'a> {
    fn initial_wire(&self, node: NodeId) -> Result<RedstoneWire, String> {
        let state = match self {
            Self::Draft { graph, .. } => graph.nodes.get(node.index()).map(|node| &node.state),
            Self::Lowered(_, state) => state.components.get(node.index()),
        };
        match state {
            Some(ComponentState::Wire(wire)) => Ok(*wire),
            _ => Err("fixed wire entry state missing or incompatible".into()),
        }
    }

    fn draft(graph: &'a CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        let mut queries = FxHashMap::default();
        for (index, query) in graph.plans.fixed_signals.live.iter().enumerate() {
            if index % 4096 == 0 {
                cancelled(monitor)?;
            }
            queries.insert(query.target, index);
        }
        let mut readers = rustc_hash::FxHashSet::default();
        for (index, link) in graph.power_links.iter().enumerate() {
            if index % 4096 == 0 {
                cancelled(monitor)?;
            }
            readers.insert(link.to);
        }
        let mut comparators = vec![None; graph.nodes.len()];
        for (index, plan) in graph.comparator_inputs.iter().enumerate() {
            if index % 4096 == 0 {
                cancelled(monitor)?;
            }
            comparators[plan.node.index()] = Some(index);
        }
        Ok(Self::Draft {
            graph,
            queries,
            readers,
            comparators,
        })
    }
    fn node_count(&self) -> usize {
        match self {
            Self::Draft { graph, .. } => graph.nodes.len(),
            Self::Lowered(program, _) => program.nodes().len(),
        }
    }
    fn node(&self, index: usize) -> FixedClassicNodeView<'_> {
        match self {
            Self::Draft { graph, .. } => {
                let node = &graph.nodes[index];
                FixedClassicNodeView {
                    kind: &node.kind,
                    block: node.raw_state.map(Block::from_id),
                    position: node.position,
                    output_route: node.output_route,
                }
            }
            Self::Lowered(program, _) => {
                let node = &program.nodes()[index];
                FixedClassicNodeView {
                    kind: &node.kind,
                    block: node.block,
                    position: node.position,
                    output_route: node.output_route,
                }
            }
        }
    }
    fn nodes(&self) -> impl ExactSizeIterator<Item = FixedClassicNodeView<'_>> {
        (0..self.node_count()).map(move |index| self.node(index))
    }
    fn sites(&self) -> &[SiteSpec] {
        match self {
            Self::Draft { graph, .. } => &graph.sites,
            Self::Lowered(program, _) => program.sites(),
        }
    }
    fn profiles(&self) -> &[Profile] {
        match self {
            Self::Draft { graph, .. } => &graph.profiles,
            Self::Lowered(program, _) => program.profiles(),
        }
    }
    fn guards(&self) -> &[Guard] {
        match self {
            Self::Draft { graph, .. } => &graph.guards,
            Self::Lowered(program, _) => program.guards(),
        }
    }
    fn wires(&self) -> &[WirePlan] {
        match self {
            Self::Draft { graph, .. } => &graph.wires,
            Self::Lowered(program, _) => program.wires(),
        }
    }
    fn routes(&self) -> &[Route] {
        match self {
            Self::Draft { graph, .. } => &graph.routes,
            Self::Lowered(program, _) => program.routes(),
        }
    }
    fn contacts(&self) -> &[ContactSpec] {
        match self {
            Self::Draft { graph, .. } => &graph.contacts,
            Self::Lowered(program, _) => program.contacts(),
        }
    }
    fn pistons(&self) -> &[PistonSpec] {
        match self {
            Self::Draft { graph, .. } => &graph.pistons,
            Self::Lowered(program, _) => program.pistons(),
        }
    }
    fn resources(&self) -> &[ResourceSpec] {
        match self {
            Self::Draft { graph, .. } => &graph.resources,
            Self::Lowered(program, _) => program.resources(),
        }
    }
    fn profile_block(&self, profile: ProfileId) -> Option<Block> {
        match self {
            Self::Draft { graph, .. } => graph.profiles.get(profile.index()).map(|record| Block::from_id(record.raw_state)),
            Self::Lowered(program, _) => program.profile_block(profile),
        }
    }
    fn has_private_owners(&self) -> bool {
        match self {
            Self::Draft { graph, .. } => !graph.plans.classic_nets.is_empty() || !graph.plans.instant.proxies.is_empty(),
            Self::Lowered(program, _) => !program.classic().nets.is_empty() || !program.instant_proxies().is_empty(),
        }
    }
    fn comparator_input(&self, node: NodeId) -> Option<&ComparatorInputPlan> {
        match self {
            Self::Draft { graph, comparators, .. } => comparators
                .get(node.index())
                .copied()
                .flatten()
                .map(|index| &graph.comparator_inputs[index]),
            Self::Lowered(program, _) => program.comparator_input(node),
        }
    }
    fn fixed_signal_query(&self, target: PowerTarget) -> Option<(u8, &[WeightedSignal])> {
        match self {
            Self::Draft { graph, queries, .. } => queries.get(&target).map(|&index| {
                let query = &graph.plans.fixed_signals.live[index];
                (query.constant, query.inputs.as_ref())
            }),
            Self::Lowered(program, _) => program.fixed_signal_query(target),
        }
    }
    fn has_power_links(&self, target: PowerTarget) -> bool {
        match self {
            Self::Draft { readers, .. } => readers.contains(&target),
            Self::Lowered(program, _) => !program.power_links_to(target).is_empty(),
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(crate) struct FixedClassicInventory {
    pub(crate) nodes: usize,
    pub(crate) wires: usize,
    pub(crate) sites: usize,
    pub(crate) routes: usize,
    pub(crate) ops: usize,
    pub(crate) operands: usize,
    pub(crate) estimated_bytes: usize,
}
impl FixedClassicProgram {
    pub(crate) fn inventory(&self) -> FixedClassicInventory {
        FixedClassicInventory {
            nodes: self.nodes.len(),
            wires: self.wires.len(),
            sites: self.sites.len(),
            routes: self.routes.len(),
            ops: self.ops.len(),
            operands: self.operands.len(),
            estimated_bytes: std::mem::size_of::<Self>()
                + std::mem::size_of_val(self.nodes.as_ref())
                + std::mem::size_of_val(self.sites.as_ref())
                + std::mem::size_of_val(self.wires.as_ref())
                + std::mem::size_of_val(self.routes.as_ref())
                + std::mem::size_of_val(self.ops.as_ref())
                + std::mem::size_of_val(self.operands.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        plot::PlotWorld,
        world::{World, storage::Chunk},
    };
    fn graph() -> CompileGraph {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(BlockPos::new(5, 20, 5), Block::RedstoneBlock);
        world.set_block(BlockPos::new(6, 19, 5), Block::Stone {});
        world.set_block(BlockPos::new(6, 20, 5), Block::RedstoneWire { wire: Default::default() });
        let monitor = TaskMonitor::default();
        let mut prepared = crate::redpiler::passes::rich::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &monitor,
            crate::redpiler::analysis::AnalysisLimits::default(),
        )
        .unwrap();
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            ..Default::default()
        };
        crate::redpiler::passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &crate::redpiler::passes::default_graph_pipeline(&options),
            &monitor,
        )
        .unwrap();
        prepared.graph
    }
    #[test]
    fn fixed_classic_draft_and_lowered_views_match_and_reject_stale_plan() {
        let graph = graph();
        let compiled = FixedClassicProgram::compile_graph(&graph, &TaskMonitor::default()).unwrap();
        let (program, state) = graph.clone().lower().unwrap();
        assert_eq!(compiled, FixedClassicProgram::compile(&program, &state).unwrap());
        let saved = FixedClassicPlans {
            program: Some(Arc::new(compiled.clone())),
            decline: None,
        };
        saved.validate(&graph, &TaskMonitor::default()).unwrap();
        let mut forged = compiled.clone();
        forged.wires[0].seed_mask ^= 1;
        assert!(
            FixedClassicPlans {
                program: Some(Arc::new(forged)),
                decline: None
            }
            .validate(&graph, &TaskMonitor::default())
            .is_err()
        );
        let mut forged = compiled;
        forged.wires[0].block_input.constant ^= 1;
        assert!(
            FixedClassicPlans {
                program: Some(Arc::new(forged)),
                decline: None,
            }
            .validate(&graph, &TaskMonitor::default())
            .is_err()
        );
        for wire in program.wires() {
            if let Some(route) = wire.turbo_route {
                if !program
                    .nodes()
                    .iter()
                    .any(|node| !matches!(node.kind, CircuitNodeKind::Wire(_)) && node.output_route == Some(route))
                    && !program.wires().iter().any(|wire| wire.shape_route == Some(route))
                {
                    assert!(saved.program.as_ref().unwrap().routes[route.index()].is_empty());
                }
            }
        }
    }
    #[test]
    fn fixed_classic_draft_compilation_honors_cancellation() {
        let graph = graph();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(FixedClassicProgram::compile_graph(&graph, &monitor).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn fixed_classic_shape_certificate_uses_component_entry_sides_instead_of_raw_block() {
        let mut graph = graph();
        let original = FixedClassicProgram::compile_graph(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(original.wires[0].inert_sides, None, "fixture has stale dot sides");
        let node = original.wires[0].node;
        let saved_raw = graph.nodes[node.index()].raw_state;
        let entry = RedstoneWire::new(
            RedstoneWireSide::None,
            RedstoneWireSide::None,
            RedstoneWireSide::Side,
            RedstoneWireSide::Side,
            7,
        );
        graph.nodes[node.index()].state = ComponentState::Wire(entry);
        let compiled = FixedClassicProgram::compile_graph(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph.nodes[node.index()].raw_state, saved_raw);
        assert_eq!(compiled.wires[0].inert_sides, Some(FixedClassicWire::packed_sides(entry)));
        assert!(
            FixedClassicPlans {
                program: Some(Arc::new(original)),
                decline: None
            }
            .validate(&graph, &TaskMonitor::default())
            .is_err(),
            "changing entry sides must invalidate the old shape assumption"
        );
        graph.plans.fixed_classic.program = Some(Arc::new(compiled.clone()));
        let (program, state) = graph.lower().unwrap();
        assert_eq!(compiled, FixedClassicProgram::compile(&program, &state).unwrap());
        // The certificate ignores power because shape transforms preserve it.
        for power in 0..16 {
            let current = RedstoneWire { power, ..entry };
            let bits = compiled.wires[0].raw_sides;
            let side = |shift: u32| RedstoneWireSide::from_id(u32::from((bits >> shift) & 3));
            let raw = RedstoneWire::new(side(0), side(2), side(4), side(6), power);
            for face in BlockFace::values() {
                assert_eq!(crate::redstone::wire::on_neighbor_changed_from(current, raw, face), current);
            }
        }
    }
}
