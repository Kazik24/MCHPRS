//! Compile the ordered dust algorithm over guarded valuations, not concrete input masks.
//!
//! Captures, visited flags, headings and queue membership exist only while deriving expressions.
//! Each guarded occurrence retains FIFO order for every valuation that selects it.
use super::*;
use std::collections::VecDeque;

struct WalkSite {
    /// This records which valuations have already captured the site's initial properties.
    known: Formula,
    /// These alternatives retain the first captured values separately for each valuation.
    view: SiteView,
    /// This records whether this site has been visited for each valuation.
    visited: Formula,
    /// These nine predicates select the current (-1..=1, -1..=1) traversal bias.
    bias: [Formula; 9],
    /// These four predicates retain the first identified heading for each valuation.
    heading: [Formula; 4],
    /// This distinguishes a cached heading from the default all-false heading predicates.
    identified: Formula,
    /// These disjoint predicates select the last enqueued layer for each valuation.
    layers: Vec<(u32, Formula)>,
}

impl WalkSite {
    fn new() -> Self {
        Self {
            known: Formula::FALSE,
            view: SiteView { variants: Vec::new() },
            visited: Formula::FALSE,
            bias: std::array::from_fn(|index| if index == 4 { Formula::TRUE } else { Formula::FALSE }),
            heading: [Formula::FALSE; 4],
            identified: Formula::FALSE,
            layers: vec![(0, Formula::TRUE)],
        }
    }
}

struct Occurrence {
    /// This is the site whose captured receiver will be dispatched.
    site: SiteId,
    /// This selects precisely the valuations that enqueued this occurrence.
    when: Formula,
}

#[derive(Default)]
struct Walk {
    /// This owns independent captures and traversal metadata for this walk only.
    sites: FxHashMap<SiteId, WalkSite>,
    /// These are current, next-layer and following-layer FIFO occurrences.
    queues: [VecDeque<Occurrence>; 3],
    /// This is the compile-time layer currently being expanded.
    layer: u32,
}

impl Compiler<'_> {
    fn capture_walk(&mut self, walk: &mut Walk, site: SiteId, when: Formula) -> Result<()> {
        let known = walk.sites.entry(site).or_insert_with(WalkSite::new).known;
        let unknown = self.logic.not(known)?;
        let discover = self.logic.and(when, unknown)?;
        if discover == Formula::FALSE {
            return Ok(());
        }
        self.step()?;
        let view = self.observe(site)?;
        let entry = walk.sites.get_mut(&site).unwrap();
        for mut variant in view.variants {
            variant.when = self.logic.and(discover, variant.when)?;
            if variant.when != Formula::FALSE {
                entry.view.variants.push(variant);
            }
        }
        entry.known = self.logic.or(known, discover)?;
        Ok(())
    }

    fn any_visited(&mut self, walk: &Walk, neighbors: &[SiteId; 24], slots: [usize; 3]) -> Result<Formula> {
        let mut result = Formula::FALSE;
        for slot in slots {
            result = self.logic.or(result, walk.sites[&neighbors[slot]].visited)?;
        }
        Ok(result)
    }

    fn axis_bias(&mut self, from_negative: Formula, from_positive: Formula) -> Result<[Formula; 3]> {
        let not_negative = self.logic.not(from_negative)?;
        let not_positive = self.logic.not(from_positive)?;
        let negative = self.logic.and(from_positive, not_negative)?;
        let positive = self.logic.and(from_negative, not_positive)?;
        let nonzero = self.logic.xor(from_negative, from_positive)?;
        let zero = self.logic.not(nonzero)?;
        Ok([negative, zero, positive])
    }

    fn identify(&mut self, walk: &mut Walk, root: SiteId, neighbors: &[SiteId; 24], when: Formula) -> Result<[Formula; 4]> {
        self.capture_walk(walk, root, when)?;
        let identified = walk.sites[&root].identified;
        let unknown = self.logic.not(identified)?;
        let fresh = self.logic.and(when, unknown)?;
        if fresh == Formula::FALSE {
            return Ok(walk.sites[&root].heading);
        }
        for &site in neighbors {
            self.capture_walk(walk, site, fresh)?;
        }
        let west = self.any_visited(walk, neighbors, [0, 7, 8])?;
        let east = self.any_visited(walk, neighbors, [1, 12, 13])?;
        let north = self.any_visited(walk, neighbors, [4, 17, 20])?;
        let south = self.any_visited(walk, neighbors, [5, 18, 21])?;
        let x_values = self.axis_bias(west, east)?;
        let z_values = self.axis_bias(north, south)?;
        let old = walk.sites[&root].bias;
        let mut biases = [Formula::FALSE; 9];
        // These are nine finite traversal directions, not valuations of the region's data inputs.
        for (xi, x_condition) in x_values.into_iter().enumerate() {
            for (zi, z_condition) in z_values.into_iter().enumerate() {
                let direction = self.logic.and(x_condition, z_condition)?;
                let direction = self.logic.and(direction, fresh)?;
                if direction == Formula::FALSE {
                    continue;
                }
                for (previous, old_condition) in old.into_iter().enumerate() {
                    let active = self.logic.and(direction, old_condition)?;
                    if active == Formula::FALSE {
                        continue;
                    }
                    let (mut x, mut z) = (xi as i32 - 1, zi as i32 - 1);
                    let (old_x, old_z) = (previous as i32 % 3 - 1, previous as i32 / 3 - 1);
                    if x == 0 && z == 0 {
                        (x, z) = (old_x, old_z);
                    } else if x != 0 && z != 0 {
                        if old_x != 0 {
                            z = 0;
                        }
                        if old_z != 0 {
                            x = 0;
                        }
                    }
                    let index = ((x + 1) + 3 * (z + 1)) as usize;
                    biases[index] = self.logic.or(biases[index], active)?;
                }
            }
        }
        for &site in neighbors {
            let target = walk.sites.get_mut(&site).unwrap();
            for (index, value) in target.bias.iter_mut().enumerate() {
                *value = self.logic.select(fresh, biases[index], *value)?;
            }
        }
        let root = walk.sites.get_mut(&root).unwrap();
        for (bias, active) in biases.into_iter().enumerate() {
            let heading = crate::redstone::wire::compute_heading(bias as i32 % 3 - 1, bias as i32 / 3 - 1);
            root.heading[heading] = self.logic.or(root.heading[heading], active)?;
        }
        root.identified = self.logic.or(identified, fresh)?;
        Ok(root.heading)
    }

    fn advance_layer(&mut self, entry: &mut WalkSite, layer: u32, when: Formula) -> Result<Formula> {
        let mut blocked = Formula::FALSE;
        for &(old_layer, condition) in &entry.layers {
            if old_layer >= layer {
                blocked = self.logic.or(blocked, condition)?;
            }
        }
        let not_blocked = self.logic.not(blocked)?;
        let active = self.logic.and(when, not_blocked)?;
        if active != Formula::FALSE {
            let retain = self.logic.not(active)?;
            for (_, condition) in &mut entry.layers {
                *condition = self.logic.and(*condition, retain)?;
            }
            entry.layers.retain(|(_, condition)| *condition != Formula::FALSE);
            entry.layers.push((layer, active));
        }
        Ok(active)
    }

    fn eligible_turbo(&mut self, route: RouteId, site: SiteId, view: &SiteView) -> Result<Formula> {
        let graph = self.graph;
        let capture = Capture::from_iter([(site, view.clone())]);
        let mut eligible = Formula::FALSE;
        // Enqueueing tests existence; dispatch later uses the captured receiver, not the matched row.
        for delivery in graph.routes[route.index()].deliveries.iter() {
            if delivery.target == site && delivery.receiver.is_some() && delivery.view == OperandView::Captured {
                let guard = self.guard(delivery.guard, Some(&capture))?;
                eligible = self.logic.or(eligible, guard)?;
            }
        }
        Ok(eligible)
    }

    fn propagate(&mut self, walk: &mut Walk, node: NodeId, when: Formula) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        self.step()?;
        let CircuitNodeKind::Wire(index) = self.graph.nodes[node.index()].kind else {
            unreachable!()
        };
        let Some(route) = self.graph.wires[index].turbo_route else {
            return Ok(());
        };
        let neighbors = self.wire_neighbors(node)?;
        let root = self
            .graph
            .sites
            .iter()
            .position(|site| site.source == SiteSource::Component(node))
            .unwrap();
        let heading = self.identify(walk, SiteId::new(root as u32), &neighbors, when)?;
        for (heading, selected) in heading.into_iter().enumerate() {
            let selected = self.logic.and(when, selected)?;
            if selected == Formula::FALSE {
                continue;
            }
            let order = &crate::redstone::wire::TURBO_ORDER[heading];
            for (queue, slots) in [(1, order.as_slice()), (2, &order[..4])] {
                for &slot in slots {
                    let site = neighbors[slot];
                    let entry = walk.sites.get_mut(&site).unwrap();
                    let active = self.advance_layer(entry, walk.layer + queue as u32, selected)?;
                    if active == Formula::FALSE {
                        continue;
                    }
                    let eligible = self.eligible_turbo(route, site, &entry.view)?;
                    let when = self.logic.and(active, eligible)?;
                    if when != Formula::FALSE {
                        walk.queues[queue].push_back(Occurrence { site, when });
                    }
                }
            }
        }
        Ok(())
    }

    fn visit_wire(&mut self, walk: &mut Walk, node: NodeId, site: SiteId, when: Formula, captured: &SiteView) -> Result<()> {
        let entry = walk.sites.get_mut(&site).unwrap();
        entry.visited = self.logic.or(entry.visited, when)?;
        let neighbors = self.wire_neighbors(node)?;
        let headings = self.identify(walk, site, &neighbors, when)?;
        let capture = walk.sites.iter().map(|(&site, value)| (site, value.view.clone())).collect();
        let mut value = self.captured_component(node, Some(captured))?;
        let mut power = Signal::literal(0)?;
        for (heading, selected) in headings.into_iter().enumerate() {
            if self.logic.and(when, selected)? == Formula::FALSE {
                continue;
            }
            let result = self.captured_wire_power(node, &capture, heading)?;
            power = self.logic.signal_select(selected, result, power)?;
        }
        let equal = self.logic.signal_equal(power, value.signal)?;
        let differs = self.logic.not(equal)?;
        let changed = self.logic.and(when, differs)?;
        value.signal = power;
        self.write_wire(node, changed, &value)?;
        if changed == Formula::FALSE {
            return Ok(());
        }
        for variant in &mut walk.sites.get_mut(&site).unwrap().view.variants {
            if let Some(component) = &mut variant.component {
                component.signal = self.logic.signal_select(changed, power, component.signal)?;
            }
        }
        self.propagate(walk, node, changed)
    }

    fn visit_site(&mut self, walk: &mut Walk, occurrence: Occurrence, receivers: &mut impl Receivers) -> Result<()> {
        self.step()?;
        let view = walk.sites[&occurrence.site].view.clone();
        for variant in view.variants {
            let when = self.logic.and(occurrence.when, variant.when)?;
            if when == Formula::FALSE {
                continue;
            }
            let Some(receiver) = variant.receiver else {
                continue;
            };
            let captured = SiteView { variants: vec![variant] };
            if let CallbackReceiver::Component(node) = receiver
                && matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_))
            {
                self.visit_wire(walk, node, occurrence.site, when, &captured)?;
            } else {
                receivers.deliver(
                    self,
                    Delivery {
                        when,
                        target: occurrence.site,
                        receiver,
                        kind: DeliveryKind::Update { direction: None },
                        captured: Some(captured),
                    },
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn walk(&mut self, node: NodeId, when: Formula, receivers: &mut impl Receivers) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        let root = self
            .graph
            .sites
            .iter()
            .position(|site| site.source == SiteSource::Component(node))
            .unwrap();
        let root = SiteId::new(root as u32);
        let mut walk = Walk::default();
        self.capture_walk(&mut walk, root, when)?;
        walk.sites.get_mut(&root).unwrap().visited = when;
        self.propagate(&mut walk, node, when)?;
        walk.queues.rotate_left(1);
        walk.layer = 1;
        while !walk.queues[0].is_empty() || !walk.queues[1].is_empty() {
            self.step()?;
            while let Some(occurrence) = walk.queues[0].pop_front() {
                self.visit_site(&mut walk, occurrence, receivers)?;
            }
            walk.queues.rotate_left(1);
            walk.layer = walk.layer.checked_add(1).ok_or(SymbolicError::BudgetExceeded)?;
        }
        Ok(())
    }
}
