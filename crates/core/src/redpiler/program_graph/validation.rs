//! Structural integrity and certificate checks before a draft can execute.
use super::*;

impl CircuitDraft {
    pub(crate) fn validate(&self) -> Result<(), GraphBuildError> {
        self.validate_inner()
    }

    fn validate_inner(&self) -> Result<(), GraphBuildError> {
        // TODO: extract to helper function just like all others.
        for (name, len) in [
            ("nodes", self.nodes.len()),
            ("sites", self.sites.len()),
            ("pistons", self.pistons.len()),
            ("resources", self.resources.len()),
            ("contacts", self.contacts.len()),
            ("profiles", self.profiles.len()),
            ("guards", self.guards.len()),
            ("routes", self.routes.len()),
            ("wires", self.wires.len()),
            ("dust paths", self.plans.dust_paths.len()),
            ("wire relations", self.wire_relations.len()),
            ("instant regions", self.plans.instant.regions.len()),
            ("instant proxies", self.plans.instant.proxies.len()),
        ] {
            if u32::try_from(len).is_err() {
                return Err(GraphBuildError::TooManyEntries { table: name });
            }
        }

        self.validate_nodes()?;
        self.validate_guards()?;
        self.validate_sites_and_wires()?;
        self.validate_turbo_callback_indices()?;
        self.validate_wire_relations()?;
        self.validate_dust_paths()?;
        self.validate_contacts_and_pistons()?;
        self.validate_routes()?;
        self.validate_links()?;
        self.validate_power()?;
        // Certificate inference follows contact variants, so their references must be valid first.
        for (index, wire) in self.wires.iter().enumerate() {
            if let Some(facts) = wire.execution.query_facts
                && facts != WireQueryFacts::infer(self, wire)
            {
                return Err(GraphBuildError::InvalidWireQueryFacts { wire: index });
            }
        }
        self.validate_instant_resets()?;
        self.validate_instant_regions()?;
        if let Some(machine) = &self.plans.instant.machine {
            crate::redpiler::analysis::instant_regions::machine::validate(self, machine)?;
        }
        Ok(())
    }

    fn validate_instant_regions(&self) -> Result<(), GraphBuildError> {
        use crate::redpiler::analysis::instant_regions::{infer_proxies, infer_regions, infer_stable_regions};
        if self.plans.instant.regions.is_empty() && self.plans.instant.proxies.is_empty() {
            return Ok(());
        }
        let monitor = crate::redpiler::TaskMonitor::default();
        let inferred = if self.plans.instant.regions.iter().any(|region| region.stable_inputs) {
            infer_stable_regions(self, &monitor)
        } else {
            infer_regions(self, &monitor)
        };
        let regions = inferred.map_err(|_| GraphBuildError::InvalidInstantRegion { table: "regions", index: 0 })?;
        if self.plans.instant.regions != regions {
            let index = self
                .plans
                .instant
                .regions
                .iter()
                .zip(&regions)
                .position(|(actual, expected)| actual != expected)
                .unwrap_or(self.plans.instant.regions.len().min(regions.len()));
            return Err(GraphBuildError::InvalidInstantRegion { table: "regions", index });
        }
        if !self.plans.instant.proxies.is_empty() {
            let proxies =
                infer_proxies(self, &regions, &monitor).map_err(|_| GraphBuildError::InvalidInstantRegion { table: "proxies", index: 0 })?;
            if self.plans.instant.proxies != proxies {
                let index = self
                    .plans
                    .instant
                    .proxies
                    .iter()
                    .zip(&proxies)
                    .position(|(actual, expected)| actual != expected)
                    .unwrap_or(self.plans.instant.proxies.len().min(proxies.len()));
                return Err(GraphBuildError::InvalidInstantRegion { table: "proxies", index });
            }
        }
        Ok(())
    }

    fn validate_instant_resets(&self) -> Result<(), GraphBuildError> {
        if !self.wires.iter().any(|wire| wire.execution.instant_reset.is_some()) {
            return Ok(());
        }
        let mut inferred = vec![None; self.wires.len()];
        for (wire, plan) in crate::redpiler::analysis::instant_resets::infer_instant_resets(self) {
            inferred[wire] = Some(plan);
        }
        for (index, wire) in self.wires.iter().enumerate() {
            if let Some(plan) = wire.execution.instant_reset
                && inferred[index] != Some(plan)
            {
                return Err(GraphBuildError::InvalidInstantReset { wire: index });
            }
        }
        Ok(())
    }

    fn validate_turbo_callback_indices(&self) -> Result<(), GraphBuildError> {
        if self.plans.turbo_callback_indices.is_empty() {
            return Ok(());
        }
        if self.plans.turbo_callback_indices.len() != self.wires.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "Turbo callback indices",
                descriptions: self.wires.len(),
                states: self.plans.turbo_callback_indices.len(),
            });
        }
        for (wire_index, index) in self.plans.turbo_callback_indices.iter().enumerate() {
            let Some(index) = index else {
                continue;
            };
            let wire = &self.wires[wire_index];
            let route_id = wire.turbo_route.ok_or(GraphBuildError::InvalidTurboCallbackIndex {
                wire: wire_index,
                reason: "indexed wire has no Turbo route",
            })?;
            let route = self.routes.get(route_id.index()).ok_or(GraphBuildError::InvalidTurboCallbackIndex {
                wire: wire_index,
                reason: "indexed Turbo route is missing",
            })?;
            let deliveries = &route.deliveries;
            let route_len = u32::try_from(deliveries.len()).map_err(|_| GraphBuildError::InvalidTurboCallbackIndex {
                wire: wire_index,
                reason: "Turbo route length exceeds indexed range",
            })?;
            if index.offsets[0] != 0
                || index.offsets[24] != route_len
                || index
                    .offsets
                    .windows(2)
                    .any(|range| range[0] > range[1] || usize::try_from(range[1]).unwrap() > deliveries.len())
            {
                return Err(GraphBuildError::InvalidTurboCallbackIndex {
                    wire: wire_index,
                    reason: "offsets are not ordered route bounds",
                });
            }
            for slot in 0..24 {
                let target = wire.canonical_neighbors[slot];
                if wire.canonical_neighbors[..slot].contains(&target) {
                    return Err(GraphBuildError::InvalidTurboCallbackIndex {
                        wire: wire_index,
                        reason: "canonical targets are repeated",
                    });
                }
                for delivery in &deliveries[index.offsets[slot] as usize..index.offsets[slot + 1] as usize] {
                    if delivery.target != target {
                        return Err(GraphBuildError::InvalidTurboCallbackIndex {
                            wire: wire_index,
                            reason: "slot range contains a different target",
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_dust_paths(&self) -> Result<(), GraphBuildError> {
        let mut assigned = vec![false; self.wires.len()];
        for (path_index, path) in self.plans.dust_paths.iter().enumerate() {
            if !(2..=3).contains(&path.wires.len()) {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "path length must be two or three wires",
                });
            }
            check_id("nodes", path.input_node.index(), self.nodes.len())?;
            let input_site = self.sites.get(path.input_site.index()).ok_or(GraphBuildError::InvalidReference {
                table: "sites",
                index: path.input_site.index(),
                len: self.sites.len(),
            })?;
            if input_site.source != SiteSource::Component(path.input_node)
                || !matches!(self.nodes[path.input_node.index()].kind, CircuitNodeKind::Electrical(_))
                || self.nodes[path.input_node.index()].raw_state.is_none()
            {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "input site is not owned by an electrical component",
                });
            }
            check_id("nodes", path.wires[0].index(), self.nodes.len())?;
            let first_position = self.nodes[path.wires[0].index()].position.ok_or(GraphBuildError::InvalidDustPath {
                path: path_index,
                reason: "wire positions are required",
            })?;
            if input_site.position != first_position.offset(path.input_face) {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "input site is not directly adjacent on the declared face",
                });
            }
            if !matches!(path.input_face, BlockFace::North | BlockFace::South | BlockFace::East | BlockFace::West)
                || !self.power_links.iter().any(|link| {
                    link.from == path.input_site
                        && link.from_port == PowerPort::Weak(path.input_face)
                        && link.to
                            == (PowerTarget::Input {
                                node: path.wires[0],
                                port: InputPort::Main,
                            })
                        && link.face == path.input_face
                        && link.attenuation == 0
                        && self.guards.get(link.guard.index()) == Some(&Guard::Always)
                })
            {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "direct ingress power binding is missing or non-horizontal",
                });
            }
            let mut previous: Option<BlockPos> = None;
            let mut axis = None;
            for &node in path.wires.iter() {
                check_id("nodes", node.index(), self.nodes.len())?;
                let CircuitNodeKind::Wire(wire_index) = self.nodes[node.index()].kind else {
                    return Err(GraphBuildError::InvalidDustPath {
                        path: path_index,
                        reason: "path contains a non-wire node",
                    });
                };
                let wire = self.wires.get(wire_index).ok_or(GraphBuildError::InvalidReference {
                    table: "wires",
                    index: wire_index,
                    len: self.wires.len(),
                })?;
                if wire.node != node || assigned[wire_index] {
                    return Err(GraphBuildError::InvalidDustPath {
                        path: path_index,
                        reason: "wire is mismatched or assigned to multiple paths",
                    });
                }
                assigned[wire_index] = true;
                let position = self.nodes[node.index()].position.ok_or(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "wire positions are required",
                })?;
                if let Some(previous) = previous {
                    let dx = position.x - previous.x;
                    let dz = position.z - previous.z;
                    let this_axis = if position.y == previous.y && dx.abs() == 1 && dz == 0 {
                        Some(0)
                    } else if position.y == previous.y && dz.abs() == 1 && dx == 0 {
                        Some(1)
                    } else {
                        None
                    };
                    if this_axis.is_none() || axis.is_some_and(|axis| Some(axis) != this_axis) {
                        return Err(GraphBuildError::InvalidDustPath {
                            path: path_index,
                            reason: "wires are not a straight horizontal path",
                        });
                    }
                    axis = this_axis;
                }
                previous = Some(position);
            }
        }
        Ok(())
    }

    fn validate_power(&self) -> Result<(), GraphBuildError> {
        for link in &self.power_links {
            check_id("sites", link.from.index(), self.sites.len())?;
            check_id("guards", link.guard.index(), self.guards.len())?;
            if link.attenuation > 15 {
                return Err(GraphBuildError::InvalidPowerAttenuation {
                    from: link.from,
                    attenuation: link.attenuation,
                });
            }
            match link.to {
                PowerTarget::Input { node, .. } => check_id("nodes", node.index(), self.nodes.len())?,
                PowerTarget::Conductor { site, .. } => check_id("sites", site.index(), self.sites.len())?,
            }
        }
        for plan in &self.comparator_inputs {
            check_id("nodes", plan.node.index(), self.nodes.len())?;
            check_id("sites", plan.rear.index(), self.sites.len())?;
            if !matches!(
                self.nodes[plan.node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { .. })
            ) {
                return Err(GraphBuildError::InvalidComponentBinding { node: plan.node });
            }
            let validate_override = |source| match source {
                ComparatorOverride::Literal(strength) if strength > 15 => Err(GraphBuildError::InvalidStrength { node: plan.node, strength }),
                ComparatorOverride::LiveComponentOutput(node) => check_id("nodes", node.index(), self.nodes.len()),
                _ => Ok(()),
            };
            if let Some(source) = plan.direct_override {
                validate_override(source)?;
            }
            if let Some((site, source, guard)) = plan.far_override {
                check_id("sites", site.index(), self.sites.len())?;
                check_id("guards", guard.index(), self.guards.len())?;
                validate_override(source)?;
            }
            if plan.far_threshold > 15 {
                return Err(GraphBuildError::InvalidStrength {
                    node: plan.node,
                    strength: plan.far_threshold,
                });
            }
        }
        Ok(())
    }

    fn validate_nodes(&self) -> Result<(), GraphBuildError> {
        for (index, node) in self.nodes.iter().enumerate() {
            let id = NodeId::try_from_index(index).expect("node count was checked");
            let valid_state = match (&node.kind, &node.state) {
                (CircuitNodeKind::Electrical(ty), ComponentState::Electrical(state)) => {
                    if node.raw_state.is_none() {
                        return Err(GraphBuildError::MissingComponentState { node: id });
                    }
                    if matches!(ty, NodeType::Wire | NodeType::Observer { .. }) {
                        return Err(GraphBuildError::UseSpecializedNode { node: id });
                    }
                    validate_strength(
                        id,
                        state.output_strength,
                        match ty {
                            NodeType::Comparator { far_input, .. } => *far_input,
                            _ => None,
                        },
                    )?;
                    true
                }
                (CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. }, ComponentState::Electrical(state)) => {
                    if node.raw_state.is_none() {
                        return Err(GraphBuildError::MissingComponentState { node: id });
                    }
                    check_id("sites", watched.index(), self.sites.len())?;
                    validate_strength(id, state.output_strength, None)?;
                    if let CircuitNodeKind::BuiltinReset { facing, .. } = node.kind {
                        if node.output_route.is_some()
                            || !matches!(node.raw_state.map(Block::from_id), Some(Block::Observer { observer }) if BlockFace::from(observer.facing) == facing)
                        {
                            return Err(GraphBuildError::NodeKindMismatch { node: id });
                        }
                    }
                    true
                }
                (CircuitNodeKind::Wire(plan), ComponentState::Wire(state)) => {
                    if node.raw_state.is_none() {
                        return Err(GraphBuildError::MissingComponentState { node: id });
                    }
                    if state.power > 15 {
                        return Err(GraphBuildError::InvalidStrength {
                            node: id,
                            strength: state.power,
                        });
                    }
                    let wire = self.wires.get(*plan).ok_or(GraphBuildError::InvalidReference {
                        table: "wires",
                        index: *plan,
                        len: self.wires.len(),
                    })?;
                    if wire.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                (CircuitNodeKind::Piston(piston), ComponentState::Specialized) => {
                    let spec = self.pistons.get(piston.index()).ok_or(GraphBuildError::InvalidReference {
                        table: "pistons",
                        index: piston.index(),
                        len: self.pistons.len(),
                    })?;
                    if spec.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                (CircuitNodeKind::Contact(contact), ComponentState::Specialized) => {
                    let spec = self.contacts.get(contact.index()).ok_or(GraphBuildError::InvalidReference {
                        table: "contacts",
                        index: contact.index(),
                        len: self.contacts.len(),
                    })?;
                    if spec.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                _ => false,
            };
            if !valid_state {
                return Err(GraphBuildError::InvalidNodeState { node: id });
            }
            if let Some(route) = node.output_route {
                check_id("routes", route.index(), self.routes.len())?;
                if self.routes[route.index()].source != id {
                    return Err(GraphBuildError::RouteSourceMismatch { route, node: id });
                }
            }
        }
        Ok(())
    }

    fn validate_guards(&self) -> Result<(), GraphBuildError> {
        for guard in &self.guards {
            match guard {
                Guard::Always => {}
                Guard::State(test) => self.validate_state_test(*test)?,
                Guard::Site { site, test, .. } => {
                    check_id("sites", site.index(), self.sites.len())?;
                    if let SiteTest::Profile(profile) = test {
                        check_id("profiles", profile.index(), self.profiles.len())?;
                    }
                }
                Guard::Not(other) => check_id("guards", other.index(), self.guards.len())?,
                Guard::All(others) | Guard::Any(others) => {
                    for other in others.iter() {
                        check_id("guards", other.index(), self.guards.len())?;
                    }
                }
            }
        }

        let mut marks = vec![0u8; self.guards.len()];
        for root_index in 0..self.guards.len() {
            if marks[root_index] != 0 {
                continue;
            }
            let root = GuardId::try_from_index(root_index).expect("guard count was checked");
            let mut stack = vec![(root, false)];
            while let Some((id, leaving)) = stack.pop() {
                let guard_count = marks.len();
                let mark = marks.get_mut(id.index()).ok_or(GraphBuildError::InvalidReference {
                    table: "guards",
                    index: id.index(),
                    len: guard_count,
                })?;
                if leaving {
                    *mark = 2;
                    continue;
                }
                if *mark == 1 {
                    return Err(GraphBuildError::CyclicGuard { guard: id });
                }
                if *mark == 2 {
                    continue;
                }
                *mark = 1;
                stack.push((id, true));
                if let Some(guard) = self.guards.get(id.index()) {
                    for child in guard_children(guard).iter().rev() {
                        if marks.get(child.index()) == Some(&1) {
                            return Err(GraphBuildError::CyclicGuard { guard: *child });
                        }
                        if marks.get(child.index()) == Some(&0) {
                            stack.push((*child, false));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_state_test(&self, test: StateTest) -> Result<(), GraphBuildError> {
        match test {
            StateTest::PistonExtended(piston) | StateTest::PistonHeadPresent(piston) | StateTest::PistonAvailability { piston, .. } => {
                check_id("pistons", piston.index(), self.pistons.len())?;
            }
            StateTest::ResourceAt { resource, contact } => {
                check_id("contacts", contact.index(), self.contacts.len())?;
                let spec = self.resources.get(resource.index()).ok_or(GraphBuildError::InvalidReference {
                    table: "resources",
                    index: resource.index(),
                    len: self.resources.len(),
                })?;
                if !spec.contacts.contains(&contact) {
                    return Err(GraphBuildError::InvalidResourceSelection { resource, contact });
                }
            }
            StateTest::ResourceEnabled(resource) => {
                check_id("resources", resource.index(), self.resources.len())?;
            }
        }
        Ok(())
    }

    fn validate_sites_and_wires(&self) -> Result<(), GraphBuildError> {
        for site in &self.sites {
            match site.source {
                SiteSource::Component(node) => self.validate_component_binding(node)?,
                SiteSource::Contact(contact) => {
                    check_id("contacts", contact.index(), self.contacts.len())?;
                }
                SiteSource::Constant(profile) => {
                    check_id("profiles", profile.index(), self.profiles.len())?;
                }
            }
        }

        let mut hop_root_counts = vec![0u32; self.nodes.len()];
        if self.wires.iter().any(|wire| wire.execution.hop_root.is_some()) {
            for site in &self.sites {
                if let SiteSource::Component(node) = site.source {
                    hop_root_counts[node.index()] += 1;
                }
            }
        }
        for (index, wire) in self.wires.iter().enumerate() {
            let node = self.nodes.get(wire.node.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: wire.node.index(),
                len: self.nodes.len(),
            })?;
            if node.kind != CircuitNodeKind::Wire(index) {
                return Err(GraphBuildError::NodeKindMismatch { node: wire.node });
            }
            if let Some(root) = wire.execution.hop_root
                && (hop_root_counts[wire.node.index()] != 1
                    || !self
                        .sites
                        .get(root.index())
                        .is_some_and(|site| site.source == SiteSource::Component(wire.node)))
            {
                return Err(GraphBuildError::InvalidWireHopRoot { wire: index });
            }
            for site in wire.canonical_neighbors {
                check_id("sites", site.index(), self.sites.len())?;
            }
            for route in wire.shape_route.into_iter().chain(wire.turbo_route) {
                check_id("routes", route.index(), self.routes.len())?;
                if self.routes[route.index()].source != wire.node {
                    return Err(GraphBuildError::RouteSourceMismatch { route, node: wire.node });
                }
            }
        }
        Ok(())
    }

    fn validate_wire_relations(&self) -> Result<(), GraphBuildError> {
        for relation in &self.wire_relations {
            let node = self.nodes.get(relation.from.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: relation.from.index(),
                len: self.nodes.len(),
            })?;
            if !matches!(node.kind, CircuitNodeKind::Wire(_)) {
                return Err(GraphBuildError::NodeKindMismatch { node: relation.from });
            }
            check_id("sites", relation.to.index(), self.sites.len())?;
            check_id("guards", relation.guard.index(), self.guards.len())?;
        }
        Ok(())
    }

    fn validate_contacts_and_pistons(&self) -> Result<(), GraphBuildError> {
        for (index, contact) in self.contacts.iter().enumerate() {
            let id = ContactId::try_from_index(index).expect("contact count was checked");
            let node = self.nodes.get(contact.node.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: contact.node.index(),
                len: self.nodes.len(),
            })?;
            if node.kind != CircuitNodeKind::Contact(id) {
                return Err(GraphBuildError::NodeKindMismatch { node: contact.node });
            }
            if contact.variants.is_empty() {
                return Err(GraphBuildError::EmptyContact { contact: id });
            }
            for variant in contact.variants.iter() {
                check_id("guards", variant.when.index(), self.guards.len())?;
                self.validate_contact_guard(id, variant.when)?;
                check_id("profiles", variant.profile.index(), self.profiles.len())?;
                if let Some(receiver) = variant.receiver {
                    self.validate_receiver(receiver)?;
                }
            }
        }

        for (index, piston) in self.pistons.iter().enumerate() {
            let id = PistonId::try_from_index(index).expect("piston count was checked");
            let node = self.nodes.get(piston.node.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: piston.node.index(),
                len: self.nodes.len(),
            })?;
            if node.kind != CircuitNodeKind::Piston(id) {
                return Err(GraphBuildError::NodeKindMismatch { node: piston.node });
            }
            if node.position != Some(piston.position) {
                return Err(GraphBuildError::NodeKindMismatch { node: piston.node });
            }
            check_id("contacts", piston.base.index(), self.contacts.len())?;
            check_id("contacts", piston.near.index(), self.contacts.len())?;
            check_id("contacts", piston.far.index(), self.contacts.len())?;
            if let PistonLane::SingleResource { far } = piston.lane {
                check_id("contacts", far.index(), self.contacts.len())?;
                if far != piston.far {
                    return Err(GraphBuildError::PistonFarContactMismatch { node: piston.node });
                }
            }
            self.validate_piston_routes(piston.node, piston.lane, piston.routes)?;
        }

        if self.initial_pistons.len() != self.pistons.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "pistons",
                descriptions: self.pistons.len(),
                states: self.initial_pistons.len(),
            });
        }
        if self.initial_resources.len() != self.resources.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "resources",
                descriptions: self.resources.len(),
                states: self.initial_resources.len(),
            });
        }

        let mut contact_owners = HashMap::new();
        for (index, resource) in self.resources.iter().enumerate() {
            let id = ResourceId::try_from_index(index).expect("resource count was checked");
            check_id("profiles", resource.material.index(), self.profiles.len())?;
            if resource.contacts.is_empty() {
                return Err(GraphBuildError::EmptyResource { resource: id });
            }
            for contact in resource.contacts.iter() {
                check_id("contacts", contact.index(), self.contacts.len())?;
                if let Some(first) = contact_owners.insert(*contact, id) {
                    return Err(GraphBuildError::ResourcesShareContact {
                        first,
                        second: id,
                        contact: *contact,
                    });
                }
            }
            let state = self.initial_resources.get(index).copied().expect("resource state count was validated");
            if !resource.contacts.contains(&state.at) {
                return Err(GraphBuildError::InvalidResourceSelection {
                    resource: id,
                    contact: state.at,
                });
            }
        }

        let mut selected_contacts = HashMap::with_capacity(self.initial_resources.len());
        for (index, state) in self.initial_resources.iter().enumerate() {
            let resource = ResourceId::try_from_index(index).expect("resource count was checked");
            if let Some(first) = selected_contacts.insert(state.at, resource) {
                return Err(GraphBuildError::ResourcesShareContact {
                    first,
                    second: resource,
                    contact: state.at,
                });
            }
        }
        Ok(())
    }

    fn validate_contact_guard(&self, contact: ContactId, root: GuardId) -> Result<(), GraphBuildError> {
        let mut visited = rustc_hash::FxHashSet::default();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let guard = &self.guards[id.index()];
            if let Guard::Site { site, .. } = guard {
                return Err(GraphBuildError::ContactGuardReadsSite {
                    contact,
                    guard: id,
                    site: *site,
                });
            }
            pending.extend(guard_children(guard).iter().copied());
        }
        Ok(())
    }

    fn validate_receiver(&self, receiver: CallbackReceiver) -> Result<(), GraphBuildError> {
        match receiver {
            CallbackReceiver::Component(node) => self.validate_component_binding(node),
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => check_id("pistons", piston.index(), self.pistons.len()),
            CallbackReceiver::Contact(contact) => check_id("contacts", contact.index(), self.contacts.len()),
        }
    }

    fn validate_component_binding(&self, node: NodeId) -> Result<(), GraphBuildError> {
        let kind = &self
            .nodes
            .get(node.index())
            .ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: node.index(),
                len: self.nodes.len(),
            })?
            .kind;
        if matches!(
            kind,
            CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Wire(_)
        ) {
            Ok(())
        } else {
            Err(GraphBuildError::InvalidComponentBinding { node })
        }
    }

    fn validate_piston_routes(&self, source: NodeId, lane: PistonLane, routes: PistonRoutes) -> Result<(), GraphBuildError> {
        let has_resource_routes = routes.resource_restored_near.is_some() && routes.resource_restored_far.is_some();
        if matches!(lane, PistonLane::SingleResource { .. }) != has_resource_routes
            || routes.resource_restored_near.is_some() != routes.resource_restored_far.is_some()
        {
            return Err(GraphBuildError::PistonResourceRestorationMismatch { node: source });
        }
        for route in [
            Some(routes.base_changed),
            Some(routes.near_changed),
            routes.far_changed,
            routes.far_shape_changed,
            Some(routes.base_restored),
            Some(routes.head_restored),
            routes.resource_restored_near,
            routes.resource_restored_far,
        ]
        .into_iter()
        .flatten()
        {
            check_id("routes", route.index(), self.routes.len())?;
            if self.routes[route.index()].source != source {
                return Err(GraphBuildError::RouteSourceMismatch { route, node: source });
            }
        }
        Ok(())
    }

    fn validate_routes(&self) -> Result<(), GraphBuildError> {
        for (route_index, route) in self.routes.iter().enumerate() {
            let route_id = RouteId::try_from_index(route_index).expect("route count was checked");
            check_id("nodes", route.source.index(), self.nodes.len())?;
            let mut previous_sequence = 0;
            for (delivery_index, delivery) in route.deliveries.iter().enumerate() {
                check_id("sites", delivery.target.index(), self.sites.len())?;
                check_id("guards", delivery.guard.index(), self.guards.len())?;
                if delivery_index > 0 && delivery.sequence < previous_sequence {
                    return Err(GraphBuildError::UnorderedRoute { route: route_id });
                }
                previous_sequence = delivery.sequence;
                if let Some(receiver) = delivery.receiver {
                    self.validate_receiver(receiver)?;
                    self.validate_delivery_receiver(route_id, delivery, receiver)?;
                }
            }
        }
        Ok(())
    }

    fn receiver_node(&self, receiver: CallbackReceiver) -> Result<NodeId, GraphBuildError> {
        match receiver {
            CallbackReceiver::Component(node) => Ok(node),
            CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id) => {
                self.pistons
                    .get(id.index())
                    .map(|piston| piston.node)
                    .ok_or(GraphBuildError::InvalidReference {
                        table: "pistons",
                        index: id.index(),
                        len: self.pistons.len(),
                    })
            }
            CallbackReceiver::Contact(id) => self
                .contacts
                .get(id.index())
                .map(|contact| contact.node)
                .ok_or(GraphBuildError::InvalidReference {
                    table: "contacts",
                    index: id.index(),
                    len: self.contacts.len(),
                }),
        }
    }

    fn validate_delivery_receiver(&self, route: RouteId, delivery: &RouteDelivery, receiver: CallbackReceiver) -> Result<(), GraphBuildError> {
        let site = self.sites[delivery.target.index()];
        let valid = match (delivery.kind, site.source, receiver) {
            (DeliveryKind::Shape { .. }, SiteSource::Contact(contact), CallbackReceiver::Contact(target)) => {
                contact == target && self.contact_guard_matches(delivery, contact, None)
            }
            (DeliveryKind::Shape { .. }, SiteSource::Component(node), CallbackReceiver::Component(target)) => node == target,
            (DeliveryKind::Update { .. }, SiteSource::Component(node), CallbackReceiver::Component(target)) => node == target,
            (DeliveryKind::Update { .. }, SiteSource::Contact(contact), receiver) => self.contacts[contact.index()]
                .variants
                .iter()
                .any(|variant| variant.receiver == Some(receiver) && self.contact_guard_matches(delivery, contact, Some(variant.profile))),
            (_, SiteSource::Constant(_), _) => false,
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(GraphBuildError::CallbackEdgeMismatch {
                route,
                sequence: delivery.sequence,
            })
        }
    }

    fn contact_guard_matches(&self, delivery: &RouteDelivery, contact: ContactId, profile: Option<ProfileId>) -> bool {
        match delivery.view {
            OperandView::Live => self.contacts[contact.index()]
                .variants
                .iter()
                .filter(|variant| profile.map_or(true, |profile| variant.profile == profile))
                .any(|variant| matches!(self.guards[variant.when.index()], Guard::Always) || self.guard_implies(delivery.guard, variant.when)),
            OperandView::Captured => self.contacts[contact.index()]
                .variants
                .iter()
                .filter(|variant| profile.map_or(true, |profile| variant.profile == profile))
                .any(|variant| self.guard_implies_site_profile(delivery.guard, delivery.target, variant.profile)),
        }
    }

    fn guard_implies(&self, root: GuardId, expected: GuardId) -> bool {
        if root == expected {
            return true;
        }
        match &self.guards[root.index()] {
            Guard::All(children) => children.iter().any(|child| self.guard_implies(*child, expected)),
            Guard::Any(children) => !children.is_empty() && children.iter().all(|child| self.guard_implies(*child, expected)),
            _ => false,
        }
    }

    fn guard_implies_site_profile(&self, root: GuardId, site: SiteId, profile: ProfileId) -> bool {
        if matches!(
            self.guards[root.index()],
                Guard::Site {
                    site: candidate_site,
                    view: OperandView::Captured,
                    test: SiteTest::Profile(candidate_profile),
                } if candidate_site == site && candidate_profile == profile
        ) {
            return true;
        }
        match &self.guards[root.index()] {
            Guard::All(children) => children.iter().any(|child| self.guard_implies_site_profile(*child, site, profile)),
            Guard::Any(children) => !children.is_empty() && children.iter().all(|child| self.guard_implies_site_profile(*child, site, profile)),
            _ => false,
        }
    }

    fn validate_links(&self) -> Result<(), GraphBuildError> {
        let mut callback_edges = HashMap::<(CallbackEdgeRef, NodeId, NodeId, LinkKind), usize>::new();
        for link in &self.links {
            check_id("nodes", link.from.index(), self.nodes.len())?;
            check_id("nodes", link.to.index(), self.nodes.len())?;
            let guard = match link.kind {
                LinkKind::Update { guard, .. } | LinkKind::Shape { guard, .. } => guard,
            };
            check_id("guards", guard.index(), self.guards.len())?;
            let reference = link.callback;
            let route = self.routes.get(reference.route.index()).ok_or(GraphBuildError::InvalidReference {
                table: "routes",
                index: reference.route.index(),
                len: self.routes.len(),
            })?;
            if route.source != link.from {
                return Err(GraphBuildError::CallbackEdgeMismatch {
                    route: reference.route,
                    sequence: reference.sequence,
                });
            }
            *callback_edges.entry((reference, link.from, link.to, link.kind)).or_default() += 1;
        }

        for (route_index, route) in self.routes.iter().enumerate() {
            let route_id = RouteId::try_from_index(route_index).expect("route count was checked");
            for delivery in route.deliveries.iter() {
                let Some(receiver) = delivery.receiver else {
                    continue;
                };
                let to = self.receiver_node(receiver)?;
                let key = (
                    CallbackEdgeRef {
                        route: route_id,
                        sequence: delivery.sequence,
                    },
                    route.source,
                    to,
                    delivery.link_kind(),
                );
                let Some(count) = callback_edges.get_mut(&key) else {
                    return Err(GraphBuildError::CallbackEdgeMismatch {
                        route: route_id,
                        sequence: delivery.sequence,
                    });
                };
                let Some(remaining) = count.checked_sub(1) else {
                    return Err(GraphBuildError::CallbackEdgeMismatch {
                        route: route_id,
                        sequence: delivery.sequence,
                    });
                };
                *count = remaining;
            }
        }
        if let Some(((reference, _, _, _), _)) = callback_edges.iter().find(|(_, count)| **count != 0) {
            return Err(GraphBuildError::CallbackEdgeMismatch {
                route: reference.route,
                sequence: reference.sequence,
            });
        }
        Ok(())
    }
}

fn guard_children(guard: &Guard) -> &[GuardId] {
    match guard {
        Guard::Not(id) => std::slice::from_ref(id),
        Guard::All(ids) | Guard::Any(ids) => ids,
        _ => &[],
    }
}

fn validate_strength(node: NodeId, output: u8, far_input: Option<u8>) -> Result<(), GraphBuildError> {
    for strength in std::iter::once(output).chain(far_input) {
        if strength > 15 {
            return Err(GraphBuildError::InvalidStrength { node, strength });
        }
    }
    Ok(())
}

fn check_id(table: &'static str, index: usize, len: usize) -> Result<(), GraphBuildError> {
    if index >= len {
        Err(GraphBuildError::InvalidReference { table, index, len })
    } else {
        Ok(())
    }
}
