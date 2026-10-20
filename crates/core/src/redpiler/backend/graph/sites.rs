use super::*;

#[derive(Debug, Clone)]
pub(super) struct ResolvedSite {
    pub(super) profile: Option<ProfileId>,
    pub(super) receiver: Option<CallbackReceiver>,
    pub(super) block: Block,
    pub(super) properties: ObservedProperties,
    pub(super) solid: bool,
    pub(super) transparent: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedSiteView {
    pub(super) profile: Option<ProfileId>,
    pub(super) receiver: Option<CallbackReceiver>,
    pub(super) block: Block,
    pub(super) solid: bool,
    pub(super) transparent: bool,
}

impl GraphBackend {
    /// Execute the recipe at discovery; contacts and mutable properties remain live.
    pub(super) fn observe_wire_site(&self, site: SiteId) -> Option<ObservedSite> {
        let capture = match self.program.wire_capture(site) {
            Some(capture) => capture,
            None => return self.observe_site(site),
        };
        let (node, properties) = match capture {
            WireCapture::Fixed(observed) => return Some(observed.clone()),
            WireCapture::Contact(contact) => {
                return self.program.contacts()[contact.index()]
                    .variants
                    .iter()
                    .find(|variant| self.guard_matches(variant.when, None))
                    .map(|variant| self.observe_profile(variant.profile, variant.receiver));
            }
            WireCapture::Electrical(node) => {
                let state = self.node_state(*node);
                (
                    *node,
                    ObservedProperties::Electrical {
                        powered: state.powered,
                        locked: state.repeater_locked,
                    },
                )
            }
            WireCapture::Bulb(node) => {
                let state = self.node_state(*node);
                (
                    *node,
                    ObservedProperties::CopperBulb {
                        powered: state.powered,
                        lit: state.output_strength > 0,
                    },
                )
            }
            WireCapture::Wire(node) => (*node, ObservedProperties::Wire(self.wire_state(*node))),
            WireCapture::Piston { node, piston } => (
                *node,
                ObservedProperties::Piston {
                    extended: self.state.pistons[piston.index()].extended,
                },
            ),
        };
        Some(ObservedSite {
            profile: None,
            receiver: Some(CallbackReceiver::Component(node)),
            properties,
        })
    }

    /// Capture only the values a queued hop owns; block geometry stays in the program.
    pub(super) fn observe_site(&self, site: SiteId) -> Option<ObservedSite> {
        match self.program.sites()[site.index()].source {
            SiteSource::Component(node) => Some(ObservedSite {
                profile: None,
                receiver: Some(CallbackReceiver::Component(node)),
                properties: self.node_properties(node),
            }),
            SiteSource::Constant(profile) => Some(self.observe_profile(profile, None)),
            SiteSource::Contact(contact) => self.program.contacts()[contact.index()]
                .variants
                .iter()
                .find(|variant| self.guard_matches(variant.when, None))
                .map(|variant| self.observe_profile(variant.profile, variant.receiver)),
        }
    }

    fn observe_profile(&self, profile: ProfileId, receiver: Option<CallbackReceiver>) -> ObservedSite {
        let block = self.program.profile_block(profile).expect("validated profile");
        ObservedSite {
            profile: Some(profile),
            receiver,
            properties: self.contact_properties(receiver, block),
        }
    }

    pub(super) fn site_block(&self, site: SiteId) -> Block {
        self.live_site(site).block
    }

    pub(super) fn live_site(&self, site: SiteId) -> ResolvedSiteView {
        self.resolve_site_view(site, None)
            .expect("wire query site was validated during preparation")
    }

    /// Test a guard using the live or captured view specified for each site.
    /// State tests always read current state. A site test with a missing capture is false.
    pub(super) fn guard_matches(&self, id: GuardId, captured: Option<&[Option<ObservedSite>]>) -> bool {
        self.guard_matches_with_context(id, captured, None)
    }

    pub(super) fn guard_matches_for_site(&self, id: GuardId, site: SiteId, observed: &ObservedSite) -> bool {
        self.guard_matches_with_context(id, None, Some((site, observed)))
    }

    fn guard_matches_with_context(
        &self,
        id: GuardId,
        captured: Option<&[Option<ObservedSite>]>,
        captured_site: Option<(SiteId, &ObservedSite)>,
    ) -> bool {
        match self.program.guards().get(id.index()) {
            Some(Guard::Always) => return true,
            Some(Guard::State(test)) => return self.state.matches(*test),
            _ => {}
        }
        if let Some(predicate) = self.program.guard_predicate(id) {
            return self.program.evaluate_guard_predicate(
                predicate,
                |read| match read {
                    // Raw recursion is essential: unsupported site atoms may name their own guard.
                    CpuRead::Guard(guard) => self.guard_matches_using(guard, captured, captured_site),
                    other => self.cpu_read(other),
                },
                self.instant_kernel.clause_matcher,
            );
        }
        self.guard_matches_using(id, captured, captured_site)
    }

    pub(super) fn guard_matches_using(
        &self,
        id: GuardId,
        captured: Option<&[Option<ObservedSite>]>,
        captured_site: Option<(SiteId, &ObservedSite)>,
    ) -> bool {
        #[cfg(test)]
        if self.test_recorder.guard_kind_census_enabled {
            let kind = match &self.program.guards()[id.index()] {
                Guard::Always => 0,
                Guard::State(_) => 1,
                Guard::Not(_) => 2,
                Guard::All(_) => 3,
                Guard::Any(_) => 4,
                Guard::Site { view, test, .. } => {
                    let base = if *view == OperandView::Live { 5 } else { 9 };
                    base + match test {
                        SiteTest::Solid => 0,
                        SiteTest::Transparent => 1,
                        SiteTest::IsWire => 2,
                        SiteTest::Profile(_) => 3,
                    }
                }
            };
            let count = &self.test_recorder.guard_kind_census[kind];
            count.set(count.get() + 1);
        }
        match &self.program.guards()[id.index()] {
            Guard::Always => true,
            Guard::State(test) => self.state.matches(*test),
            Guard::Site { site, view, test } => {
                let resolved = match view {
                    OperandView::Live => self.resolve_site(*site, None),
                    OperandView::Captured => captured_site
                        .filter(|(captured_id, _)| captured_id == site)
                        .map(|(_, observed)| observed)
                        .or_else(|| captured.and_then(|sites| sites.get(site.index())).and_then(Option::as_ref))
                        .and_then(|observed| self.resolve_observed_site(observed)),
                };
                let Some(resolved) = resolved else {
                    return false;
                };
                match test {
                    SiteTest::Solid => resolved.solid,
                    SiteTest::Transparent => resolved.transparent,
                    SiteTest::IsWire => matches!(resolved.block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(profile) => resolved.profile == Some(*profile),
                }
            }
            Guard::Not(child) => !self.guard_matches_using(*child, captured, captured_site),
            Guard::All(children) => children.iter().all(|child| self.guard_matches_using(*child, captured, captured_site)),
            Guard::Any(children) => children.iter().any(|child| self.guard_matches_using(*child, captured, captured_site)),
        }
    }

    /// Look up the block and receiver at a site.
    /// Uses the supplied capture, or reads the current component, fixed block
    /// profile, or contact occupant from the graph.
    pub(super) fn resolve_site(&self, site: SiteId, captured: Option<&ObservedSite>) -> Option<ResolvedSite> {
        if let Some(observed) = captured {
            return self.resolve_observed_site(observed);
        }
        let spec = *self.program.sites().get(site.index())?;
        match spec.source {
            SiteSource::Constant(profile) => Some(self.resolve_profile(profile, None)),
            SiteSource::Component(node) => {
                let block = self.node_block(node);
                Some(ResolvedSite {
                    profile: None,
                    receiver: Some(CallbackReceiver::Component(node)),
                    properties: self.node_properties(node),
                    block,
                    solid: block.is_solid(),
                    transparent: block.is_transparent(),
                })
            }
            SiteSource::Contact(contact) => {
                let spec = &self.program.contacts()[contact.index()];
                for variant in spec.variants.iter() {
                    if self.guard_matches(variant.when, None) {
                        return Some(self.resolve_profile(variant.profile, variant.receiver));
                    }
                }
                None
            }
        }
    }

    pub(super) fn resolve_site_view(&self, site: SiteId, captured: Option<&ObservedSite>) -> Option<ResolvedSiteView> {
        if let Some(observed) = captured {
            return self.resolve_observed_site_view(observed);
        }
        let spec = *self.program.sites().get(site.index())?;
        match spec.source {
            SiteSource::Constant(profile) => Some(self.resolve_profile_view(profile, None)),
            SiteSource::Component(node) => {
                let block = self.node_block(node);
                Some(ResolvedSiteView {
                    profile: None,
                    receiver: Some(CallbackReceiver::Component(node)),
                    solid: block.is_solid(),
                    transparent: block.is_transparent(),
                    block,
                })
            }
            SiteSource::Contact(contact) => {
                let spec = &self.program.contacts()[contact.index()];
                for variant in spec.variants.iter() {
                    if self.guard_matches(variant.when, None) {
                        return Some(self.resolve_profile_view(variant.profile, variant.receiver));
                    }
                }
                None
            }
        }
    }

    fn resolve_profile_view(&self, profile: ProfileId, receiver: Option<CallbackReceiver>) -> ResolvedSiteView {
        let facts = self.program.profiles()[profile.index()];
        ResolvedSiteView {
            profile: Some(profile),
            receiver,
            block: match receiver {
                Some(CallbackReceiver::Component(node)) => self.node_block(node),
                _ => self.program.profile_block(profile).expect("validated profile"),
            },
            solid: facts.solid,
            transparent: facts.transparent,
        }
    }

    fn resolve_observed_site_view(&self, observed: &ObservedSite) -> Option<ResolvedSiteView> {
        let block = if let Some(receiver) = observed.receiver {
            match receiver {
                CallbackReceiver::Component(node) => self.node_block_with_properties(node, &observed.properties),
                _ => self.block_for_profile(observed.profile?)?,
            }
        } else {
            self.block_for_profile(observed.profile?)?
        };
        let (solid, transparent) = if let Some(profile) = observed.profile {
            let facts = self.program.profiles()[profile.index()];
            (facts.solid, facts.transparent)
        } else {
            (block.is_solid(), block.is_transparent())
        };
        Some(ResolvedSiteView {
            profile: observed.profile,
            receiver: observed.receiver,
            block,
            solid,
            transparent,
        })
    }

    fn resolve_profile(&self, profile: ProfileId, receiver: Option<CallbackReceiver>) -> ResolvedSite {
        let facts = self.program.profiles()[profile.index()];
        let block = match receiver {
            Some(CallbackReceiver::Component(node)) => self.node_block(node),
            _ => self.program.profile_block(profile).expect("validated profile"),
        };
        ResolvedSite {
            profile: Some(profile),
            receiver,
            block,
            properties: self.contact_properties(receiver, block),
            solid: facts.solid,
            transparent: facts.transparent,
        }
    }

    fn resolve_observed_site(&self, observed: &ObservedSite) -> Option<ResolvedSite> {
        let view = self.resolve_observed_site_view(observed)?;
        Some(ResolvedSite {
            profile: view.profile,
            receiver: view.receiver,
            block: view.block,
            properties: observed.properties.clone(),
            solid: view.solid,
            transparent: view.transparent,
        })
    }

    fn block_for_profile(&self, profile: ProfileId) -> Option<Block> {
        self.program.profile_block(profile)
    }

    fn contact_properties(&self, receiver: Option<CallbackReceiver>, block: Block) -> ObservedProperties {
        match receiver {
            Some(CallbackReceiver::Component(node)) => self.node_properties(node),
            Some(CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => ObservedProperties::Piston {
                extended: self.state.pistons[piston.index()].extended,
            },
            _ => match block {
                Block::RedstoneWire { wire } => ObservedProperties::Wire(wire),
                _ => ObservedProperties::Passive,
            },
        }
    }

    fn node_properties(&self, node: NodeId) -> ObservedProperties {
        match &self.state.components[node.index()] {
            ComponentState::Electrical(state) if self.is_bulb(node) => ObservedProperties::CopperBulb {
                powered: state.powered,
                lit: state.output_strength > 0,
            },
            ComponentState::Electrical(state) => ObservedProperties::Electrical {
                powered: state.powered,
                locked: state.repeater_locked,
            },
            ComponentState::Wire(_) => ObservedProperties::Wire(self.wire_state(node)),
            ComponentState::Specialized => match self.program.nodes()[node.index()].kind {
                CircuitNodeKind::Piston(piston) => ObservedProperties::Piston {
                    extended: self.state.pistons[piston.index()].extended,
                },
                _ => ObservedProperties::Passive,
            },
        }
    }

    pub(super) fn node_block(&self, node: NodeId) -> Block {
        self.node_block_with_properties(node, &self.node_properties(node))
    }

    fn node_block_with_properties(&self, node: NodeId, properties: &ObservedProperties) -> Block {
        let mut block = self.program.nodes()[node.index()].block.unwrap();
        if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
            let wire = match properties {
                ObservedProperties::Wire(wire) => *wire,
                _ => self.wire_state(node),
            };
            return Block::RedstoneWire { wire };
        }
        let state = self.node_state(node);
        if block.is_copper_bulb() {
            let (powered, lit) = match properties {
                ObservedProperties::CopperBulb { powered, lit } => (*powered, *lit),
                _ => (state.powered, state.output_strength > 0),
            };
            return block.with_copper_bulb_state(lit, powered).unwrap();
        }
        match &mut block {
            Block::Lever { lever } => lever.powered = state.powered,
            Block::StoneButton { button } => button.powered = state.powered,
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => *lit = state.powered,
            Block::RedstoneRepeater { repeater } => {
                repeater.powered = state.powered;
                repeater.locked = state.repeater_locked;
            }
            Block::RedstoneComparator { comparator } => comparator.powered = state.powered,
            Block::RedstoneLamp { lit } => *lit = state.powered,
            Block::Hopper { enabled, .. } => *enabled = !state.powered,
            Block::Observer { observer } => observer.powered = state.powered,
            Block::NoteBlock { instrument, powered, .. } => {
                *powered = state.powered;
                if let CircuitNodeKind::Electrical(NodeType::NoteBlock {
                    instrument: initial_instrument,
                    ..
                }) = self.program.nodes()[node.index()].kind
                {
                    *instrument = state.note_instrument.unwrap_or(initial_instrument);
                }
            }
            Block::StonePressurePlate { powered } => *powered = state.powered,
            _ => {
                if let Some(updated) = block.with_pressure_plate_power(state.powered) {
                    block = updated;
                }
            }
        }
        if let ObservedProperties::Electrical { powered, locked } = properties {
            match &mut block {
                Block::RedstoneRepeater { repeater } => {
                    repeater.powered = *powered;
                    repeater.locked = *locked;
                }
                Block::RedstoneComparator { comparator } => comparator.powered = *powered,
                Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => *lit = *powered,
                Block::RedstoneLamp { lit } => *lit = *powered,
                Block::Hopper { enabled, .. } => *enabled = !*powered,
                Block::Observer { observer } => observer.powered = *powered,
                _ => {}
            }
        }
        block
    }
}

impl ResolvedSite {
    pub(super) fn observed(self) -> ObservedSite {
        ObservedSite {
            profile: self.profile,
            receiver: self.receiver,
            properties: self.properties,
        }
    }
}
