//! Synchronous callback expansion during AOT compilation; no route is a runtime instruction.
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Delivery {
    /// This identifies the exact source route row for an outgoing boundary effect.
    pub(crate) occurrence: Option<RouteOccurrence>,
    /// This guard selects the valuations under which this occurrence is delivered.
    pub(crate) when: Formula,
    /// This identifies the observed boundary position without retaining physical execution.
    pub(crate) target: SiteId,
    /// This selects the exact component or actuator handler for the occurrence.
    pub(crate) receiver: CallbackReceiver,
    /// This preserves update/shape distinction and callback direction.
    pub(crate) kind: DeliveryKind,
    /// This freezes operand properties for captured callbacks only.
    pub(crate) captured: Option<SiteView>,
}

/// Component compilation can start a nested walk; it must finish before the outer walk resumes.
pub(crate) trait Receivers {
    fn deliver(&mut self, compiler: &mut Compiler<'_>, delivery: Delivery) -> Result<()>;
}

impl Compiler<'_> {
    pub(crate) fn route(&mut self, route: RouteId, when: Formula, capture: Option<&Capture>, receivers: &mut impl Receivers) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        let graph = self.graph;
        for (ordinal, delivery) in graph.routes[route.index()].deliveries.iter().enumerate() {
            self.step()?;
            let guard = self.guard(delivery.guard, capture)?;
            let active = self.logic.and(when, guard)?;
            if active == Formula::FALSE {
                continue;
            }
            let captured = match delivery.view {
                OperandView::Live => None,
                OperandView::Captured => match capture.and_then(|sites| sites.get(&delivery.target)) {
                    Some(value) => Some(value.clone()),
                    None => continue,
                },
            };
            let receiver = delivery
                .receiver
                .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                    (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
            if let Some(receiver) = receiver {
                receivers.deliver(
                    self,
                    Delivery {
                        occurrence: Some(RouteOccurrence {
                            route,
                            ordinal: ordinal as u32,
                        }),
                        when: active,
                        target: delivery.target,
                        receiver,
                        kind: delivery.kind,
                        captured,
                    },
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn captured_component(&mut self, node: NodeId, captured: Option<&SiteView>) -> Result<Component> {
        self.symbolize_component(node)?;
        let mut result = self.state.components[node.index()].clone();
        if let Some(view) = captured {
            for variant in &view.variants {
                if let Some(value) = &variant.component {
                    result.signal = self.logic.signal_select(variant.when, value.signal, result.signal)?;
                    result.powered = self.logic.select(variant.when, value.powered, result.powered)?;
                    result.locked = self.logic.select(variant.when, value.locked, result.locked)?;
                    if let (Some(new), Some(old)) = (value.shape, result.shape) {
                        result.shape = Some(self.select_shape(variant.when, new, old)?);
                    }
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn seed_wire(&mut self, node: NodeId, when: Formula, captured: Option<&SiteView>, receivers: &mut impl Receivers) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        self.set_trace_stage("seed_wire captured_component");
        let mut value = self.captured_component(node, captured)?;
        self.set_trace_stage("seed_wire seed_power");
        let power = self.seed_power(node)?;
        self.set_trace_stage("seed_wire compare_power");
        let equal = self.logic.signal_equal(power, value.signal)?;
        let differs = self.logic.not(equal)?;
        let changes = self.logic.and(when, differs)?;
        value.signal = power;
        self.write_wire(node, changes, &value)?;
        self.set_trace_stage("seed_wire walk");
        self.walk(node, changes, receivers)
    }

    pub(crate) fn shape_wire(
        &mut self,
        node: NodeId,
        face: BlockFace,
        when: Formula,
        captured: Option<&SiteView>,
        receivers: &mut impl Receivers,
    ) -> Result<()> {
        if when == Formula::FALSE || face == BlockFace::Top {
            return Ok(());
        }
        let mut value = self.captured_component(node, captured)?;
        let old = value.shape.ok_or(CompileError::UnsupportedNode(node))?;
        let raw = self.raw_shape(node)?;
        let shape = self.neighbor_shape(old, raw, face)?;
        let equal = self.shape_equal(shape, old)?;
        let differs = self.logic.not(equal)?;
        let changes = self.logic.and(when, differs)?;
        value.shape = Some(shape);
        self.write_wire(node, changes, &value)?;
        let CircuitNodeKind::Wire(index) = self.graph.nodes[node.index()].kind else {
            unreachable!()
        };
        if let Some(route) = self.graph.wires[index].shape_route {
            self.route(route, changes, None, receivers)?;
        }
        Ok(())
    }
}
