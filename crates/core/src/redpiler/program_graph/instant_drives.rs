//! Typed source expressions for the non-dust input sampled by a Turbo wire.
//!
//! This pass specializes the six directional no-dust reads into a max of
//! attenuated live source values. It preserves u8 output values and guarded
//! constants; it does not infer settled wire state or remove a wire owner.
//! A query is executable only when every traversed power link and emission
//! predicate can be represented by the typed condition table. Unsupported
//! contacts, dust-inclusive conduction, or unrecognized block behavior leave
//! that wire on the ordinary query path. The condition/term tables are flat,
//! hash-consed and bounded by the source graph plus its link fanout.
//!
//! `compile_instant_drives` validates this product by independently deriving
//! it again. `backend::graph::transport` may use the same source expression
//! when it generates exact FIFO transcripts; transcript replay remains a
//! separate program and must retain every observation and callback cut.
use super::*;
use crate::redpiler::power_rules;
use rustc_hash::FxHashMap;
use std::{collections::HashMap, ops::Range};

const MAX_EXPR_WORK: usize = 8_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantDriveProgram {
    pub(crate) conditions: Box<[DriveCondition]>,
    pub(crate) terms: Box<[DriveTerm]>,
    pub(crate) queries: Box<[DriveExpr]>,
    /// Entries align with the original wire table.
    pub(crate) by_wire: Box<[Option<u32>]>,
    pub(crate) stats: DriveCompileStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DriveCompileStats {
    pub(crate) wires: usize,
    pub(crate) compiled: usize,
    pub(crate) fallback: usize,
    pub(crate) shared_binary_nets: usize,
    pub(crate) shared_binary_wires: usize,
    pub(crate) all_guarded_15_nets: usize,
    pub(crate) shared_predicate_nets: usize,
    pub(crate) multiple_predicate_nets: usize,
    pub(crate) other_drive_shape_nets: usize,
    pub(crate) channel_declines: std::collections::BTreeMap<String, usize>,
    pub(crate) work: usize,
    pub(crate) declines: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriveExpr {
    pub(crate) constant: u8,
    pub(crate) terms: Range<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct DriveTerm {
    pub(crate) source: DriveSource,
    pub(crate) condition: u32,
    pub(crate) attenuation: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum DriveSource {
    Output(NodeId),
    Powered(NodeId),
    Constant(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DriveCondition {
    Always,
    Never,
    State(StateTest),
    WireEmits { node: NodeId, face: BlockFace, raw_sides: u8 },
    Not(u32),
    All(Box<[u32]>),
    Any(Box<[u32]>),
}

#[derive(Default)]
struct Expr {
    constant: u8,
    // Draining source terms can intern predicates; stable ordering is required
    // so independent plan rederivation assigns identical condition IDs.
    terms: std::collections::BTreeMap<(DriveSource, u32), u8>,
}

fn has_complement_pair(conditions: &[DriveCondition], ids: &[u32]) -> bool {
    let ids = ids.iter().copied().collect::<std::collections::HashSet<_>>();
    // The common case is a direct literal pair. Keep that O(n), then bound
    // structural De Morgan checks so large source guards cannot consume an
    // unbounded compile budget. Skipping a check only misses simplification.
    for &id in &ids {
        if matches!(conditions.get(id as usize), Some(DriveCondition::Not(child)) if ids.contains(child)) {
            return true;
        }
    }
    if ids.len() > 16 {
        return false;
    }
    let operands = ids.iter().copied().collect::<Vec<_>>();
    let mut budget = 256usize;
    for (index, &left) in operands.iter().enumerate() {
        for &right in &operands[index + 1..] {
            if complementary(conditions, left, right, &mut budget) {
                return true;
            }
            if budget == 0 {
                return false;
            }
        }
    }
    false
}

fn complementary(conditions: &[DriveCondition], left: u32, right: u32, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    if left == right {
        return false;
    }
    match (conditions.get(left as usize), conditions.get(right as usize)) {
        (Some(DriveCondition::Always), Some(DriveCondition::Never)) | (Some(DriveCondition::Never), Some(DriveCondition::Always)) => true,
        (Some(DriveCondition::Not(child)), _) if *child == right => true,
        (_, Some(DriveCondition::Not(child))) if *child == left => true,
        (Some(DriveCondition::All(left)), Some(DriveCondition::Any(right))) | (Some(DriveCondition::Any(left)), Some(DriveCondition::All(right))) => {
            if left.len() != right.len() {
                return false;
            }
            for &left_child in left.iter() {
                let mut found = false;
                for &right_child in right.iter() {
                    if *budget == 0 {
                        return false;
                    }
                    if complementary(conditions, left_child, right_child, budget) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

impl Expr {
    fn include(&mut self, other: Self, attenuation: u8) {
        if self.constant == 15 {
            return;
        }
        self.constant = self.constant.max(other.constant.saturating_sub(attenuation));
        if self.constant == 15 {
            self.terms.clear();
            return;
        }
        for ((source, condition), loss) in other.terms {
            let loss = loss.saturating_add(attenuation);
            if loss < 15 {
                self.terms
                    .entry((source, condition))
                    .and_modify(|old| *old = (*old).min(loss))
                    .or_insert(loss);
            }
        }
    }
}

struct Compiler<'g, 'm> {
    graph: &'g CompileGraph,
    monitor: &'m crate::redpiler::TaskMonitor,
    incoming: FxHashMap<PowerTarget, Vec<&'g PowerLink>>,
    conditions: Vec<DriveCondition>,
    condition_ids: HashMap<DriveCondition, u32>,
    work: usize,
}

impl Compiler<'_, '_> {
    fn intern(&mut self, condition: DriveCondition) -> Result<u32, &'static str> {
        if let Some(&id) = self.condition_ids.get(&condition) {
            return Ok(id);
        }
        let id = u32::try_from(self.conditions.len()).map_err(|_| "condition table overflow")?;
        self.conditions.push(condition.clone());
        self.condition_ids.insert(condition, id);
        Ok(id)
    }

    fn condition(&mut self, id: GuardId, depth: usize) -> Result<u32, &'static str> {
        self.visit()?;
        if depth > 64 {
            return Err("guard nesting limit");
        }
        let value = match self.graph.guards[id.index()].clone() {
            Guard::Always => DriveCondition::Always,
            Guard::State(test) => DriveCondition::State(test),
            // Electrical queries do not carry a captured observation.
            Guard::Site {
                view: OperandView::Captured, ..
            } => DriveCondition::Never,
            Guard::Site { site, test, .. } => return self.site_test(site, test, depth + 1),
            Guard::Not(child) => {
                let child = self.condition(child, depth + 1)?;
                return self.not_condition(child);
            }
            Guard::All(children) => {
                let mut ids = Vec::with_capacity(children.len());
                for child in children.iter().copied() {
                    let child = self.condition(child, depth + 1)?;
                    ids.push(child);
                }
                return self.all_condition(ids);
            }
            Guard::Any(children) => {
                let mut ids = Vec::with_capacity(children.len());
                for child in children.iter().copied() {
                    let child = self.condition(child, depth + 1)?;
                    ids.push(child);
                }
                return self.any_condition(ids);
            }
        };
        self.intern(value)
    }

    fn static_site_test(&self, site: SiteId, test: SiteTest) -> Option<bool> {
        let source = self.graph.sites.get(site.index())?.source;
        let (block, profile_id, solid, transparent) = match source {
            SiteSource::Constant(profile_id) => {
                let profile = *self.graph.profiles.get(profile_id.index())?;
                (Block::from_id(profile.raw_state), Some(profile_id), profile.solid, profile.transparent)
            }
            SiteSource::Component(node) => {
                let spec = self.graph.nodes.get(node.index())?;
                if matches!(test, SiteTest::Profile(_)) {
                    return Some(false);
                }
                if matches!(test, SiteTest::IsWire) {
                    return Some(matches!(&spec.kind, CircuitNodeKind::Wire(_)));
                }
                if matches!(&spec.kind, CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Trapdoor)) {
                    return None;
                }
                let block = if let ComponentState::Wire(wire) = &spec.state {
                    Block::RedstoneWire { wire: *wire }
                } else {
                    Block::from_id(spec.raw_state?)
                };
                (block, None, block.is_solid(), block.is_transparent())
            }
            SiteSource::Contact(_) => return None,
        };
        match test {
            SiteTest::Solid => Some(solid),
            SiteTest::Transparent => Some(transparent),
            SiteTest::IsWire => Some(matches!(block, Block::RedstoneWire { .. })),
            SiteTest::Profile(profile) => Some(profile_id == Some(profile)),
        }
    }

    fn site_test(&mut self, site: SiteId, test: SiteTest, depth: usize) -> Result<u32, &'static str> {
        if let Some(value) = self.static_site_test(site, test) {
            return self.intern(if value { DriveCondition::Always } else { DriveCondition::Never });
        }
        let SiteSource::Contact(contact) = self.graph.sites.get(site.index()).ok_or("site out of range")?.source else {
            return Err("variable component geometry is not certified");
        };
        let variants = self.contact_variants_selected(contact, depth + 1)?;
        let mut selected_tests = Vec::new();
        for (variant, selected) in variants.iter() {
            if matches!(test, SiteTest::Solid | SiteTest::Transparent)
                && matches!(variant.receiver, Some(CallbackReceiver::Component(node))
                    if matches!(&self.graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Trapdoor)))
            {
                return Err("contact trapdoor geometry is state-dependent");
            }
            let profile = self.graph.profiles.get(variant.profile.index()).ok_or("profile out of range")?;
            let block = Block::from_id(profile.raw_state);
            let matches = match test {
                SiteTest::Solid => profile.solid,
                SiteTest::Transparent => profile.transparent,
                SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                SiteTest::Profile(profile_id) => profile_id == variant.profile,
            };
            if matches {
                selected_tests.push(*selected);
            }
        }
        self.any_condition(selected_tests)
    }

    fn contact_variants(&self, contact: ContactId) -> Result<Box<[ContactVariant]>, &'static str> {
        self.graph
            .contacts
            .get(contact.index())
            .map(|contact| contact.variants.clone())
            .ok_or("contact out of range")
    }

    fn contact_variants_selected(&mut self, contact: ContactId, depth: usize) -> Result<Vec<(ContactVariant, u32)>, &'static str> {
        if depth > 64 {
            return Err("contact selection nesting limit");
        }
        let variants = self.contact_variants(contact)?;
        let mut prior = Vec::with_capacity(variants.len());
        let mut selected_variants = Vec::with_capacity(variants.len());
        for variant in variants.iter() {
            let active = self.condition(variant.when, depth + 1)?;
            let blocked = self.any_condition(prior.clone())?;
            let not_blocked = self.not_condition(blocked)?;
            let selected = self.all_condition(vec![active, not_blocked])?;
            selected_variants.push((variant.clone(), selected));
            prior.push(active);
        }
        Ok(selected_variants)
    }

    fn contact_source_variant(&mut self, variant: &ContactVariant, port: PowerPort) -> Result<Expr, &'static str> {
        let profile = *self.graph.profiles.get(variant.profile.index()).ok_or("profile out of range")?;
        let block = Block::from_id(profile.raw_state);
        if matches!(block, Block::RedstoneWire { .. }) {
            return Err("movable wire power source requires carried-state lowering");
        }
        let source = match port {
            PowerPort::Output | PowerPort::Stored => match variant.receiver {
                Some(CallbackReceiver::Component(node)) => DriveSource::Output(node),
                _ if profile.electrical == ProfileElectrical::RedstoneSource => DriveSource::Constant(15),
                _ => DriveSource::Constant(0),
            },
            PowerPort::Weak(face) => {
                if !power_rules::emits_weak(block, face) {
                    return Ok(Expr::default());
                }
                self.contact_component_source(variant, profile)?
            }
            PowerPort::Strong(face) => {
                if !power_rules::emits_strong(block, face) {
                    return Ok(Expr::default());
                }
                self.contact_component_source(variant, profile)?
            }
            PowerPort::Conducted { .. } => return Err("nested conducted contact source"),
        };
        self.source_expr(source, 0)
    }

    fn contact_component_source(&self, variant: &ContactVariant, profile: Profile) -> Result<DriveSource, &'static str> {
        match variant.receiver {
            Some(CallbackReceiver::Component(node)) => {
                let block = Block::from_id(profile.raw_state);
                let emits_state = matches!(
                    block,
                    Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::RedstoneBlock
                        | Block::StonePressurePlate { .. }
                        | Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::Observer { .. }
                ) || block.pressure_plate_powered().is_some();
                if emits_state {
                    Ok(DriveSource::Output(node))
                } else {
                    Ok(DriveSource::Constant(0))
                }
            }
            _ if profile.electrical == ProfileElectrical::RedstoneSource => Ok(DriveSource::Constant(15)),
            _ => Ok(DriveSource::Constant(0)),
        }
    }

    /// `power_no_dust` reads the live block's boolean powered property for
    /// these emitters. This differs from a PowerLink Weak/Strong read, which
    /// deliberately uses `stored_component_strength` and must continue through
    /// `contact_component_source`/`site_source` above.
    fn direct_no_dust_component_source(&self, node: NodeId, block: Block) -> Result<DriveSource, &'static str> {
        if matches!(block, Block::RedstoneBlock) {
            return Ok(DriveSource::Constant(15));
        }
        let kind = &self.graph.nodes.get(node.index()).ok_or("node out of range")?.kind;
        match kind {
            CircuitNodeKind::Electrical(
                crate::redpiler::components::NodeType::Torch
                | crate::redpiler::components::NodeType::Repeater { .. }
                | crate::redpiler::components::NodeType::Button
                | crate::redpiler::components::NodeType::Lever
                | crate::redpiler::components::NodeType::PressurePlate
                | crate::redpiler::components::NodeType::Observer { .. },
            )
            | CircuitNodeKind::Observer { .. } => Ok(DriveSource::Powered(node)),
            CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Comparator { .. }) => Ok(DriveSource::Output(node)),
            // `site_source_strength` returns zero for other fixed component
            // receivers. Do not accidentally expose their stored output.
            _ => Ok(DriveSource::Constant(0)),
        }
    }

    fn contact_no_dust_source_variant(&self, variant: &ContactVariant, profile: Profile, face: BlockFace) -> Result<Expr, &'static str> {
        let block = Block::from_id(profile.raw_state);
        if matches!(block, Block::RedstoneWire { .. }) {
            return Err("movable wire power source requires carried-state lowering");
        }
        if !power_rules::emits_weak(block, face) {
            return Ok(Expr::default());
        }
        let source = match variant.receiver {
            Some(CallbackReceiver::Component(node)) => self.direct_no_dust_component_source(node, block)?,
            _ if profile.electrical == ProfileElectrical::RedstoneSource => DriveSource::Constant(15),
            _ => DriveSource::Constant(0),
        };
        self.source_expr(source, 0)
    }

    fn no_dust_sample(&mut self, site: SiteId, face: BlockFace) -> Result<Expr, &'static str> {
        self.visit()?;
        match self.graph.sites.get(site.index()).ok_or("site out of range")?.source {
            SiteSource::Contact(contact) => {
                let variants = self.contact_variants_selected(contact, 0)?;
                let mut result = Expr::default();
                for (variant, selected) in variants {
                    let profile = *self.graph.profiles.get(variant.profile.index()).ok_or("profile out of range")?;
                    let block = Block::from_id(profile.raw_state);
                    let mut branch = if profile.solid {
                        self.query(PowerTarget::Conductor { site, include_dust: false }, 0)?
                    } else if matches!(block, Block::RedstoneWire { .. }) {
                        Expr::default()
                    } else {
                        self.contact_no_dust_source_variant(&variant, profile, face)?
                    };
                    branch = self.gate_expr(branch, selected)?;
                    result.include(branch, 0);
                }
                Ok(result)
            }
            SiteSource::Constant(profile) => {
                let profile = self.graph.profiles.get(profile.index()).ok_or("profile out of range")?;
                let block = Block::from_id(profile.raw_state);
                if profile.solid {
                    self.query(PowerTarget::Conductor { site, include_dust: false }, 0)
                } else if matches!(block, Block::RedstoneWire { .. }) {
                    Ok(Expr::default())
                } else {
                    let source = if matches!(block, Block::RedstoneBlock) {
                        15
                    } else {
                        self.graph.sites[site.index()].source_strength
                    };
                    let source = if power_rules::emits_weak(block, face) { source } else { 0 };
                    self.source_expr(DriveSource::Constant(source), 0)
                }
            }
            SiteSource::Component(node) => {
                let spec = self.graph.nodes.get(node.index()).ok_or("node out of range")?;
                let block = if let ComponentState::Wire(wire) = &spec.state {
                    Block::RedstoneWire { wire: *wire }
                } else {
                    Block::from_id(spec.raw_state.ok_or("dynamic component block")?)
                };
                if block.is_solid() {
                    self.query(PowerTarget::Conductor { site, include_dust: false }, 0)
                } else if matches!(block, Block::RedstoneWire { .. }) {
                    Ok(Expr::default())
                } else {
                    let source = self.direct_no_dust_component_source(node, block)?;
                    if power_rules::emits_weak(block, face) {
                        self.source_expr(source, 0)
                    } else {
                        Ok(Expr::default())
                    }
                }
            }
        }
    }

    fn gate_expr(&mut self, source: Expr, condition: u32) -> Result<Expr, &'static str> {
        if matches!(&self.conditions[condition as usize], DriveCondition::Always) {
            return Ok(source);
        }
        if matches!(&self.conditions[condition as usize], DriveCondition::Never) {
            return Ok(Expr::default());
        }
        let mut gated = Expr::default();
        for ((value, old_condition), loss) in source.terms {
            let combined = self.combine_condition(old_condition, condition)?;
            gated
                .terms
                .entry((value, combined))
                .and_modify(|old| *old = (*old).min(loss))
                .or_insert(loss);
        }
        if source.constant != 0 {
            gated
                .terms
                .entry((DriveSource::Constant(source.constant), condition))
                .and_modify(|old| *old = 0)
                .or_insert(0);
        }
        Ok(gated)
    }

    fn all_condition(&mut self, ids: Vec<u32>) -> Result<u32, &'static str> {
        let mut flat = Vec::with_capacity(ids.len());
        let mut pending = ids;
        while let Some(id) = pending.pop() {
            match self.conditions.get(id as usize).ok_or("condition out of range")? {
                DriveCondition::Always => {}
                DriveCondition::Never => return self.intern(DriveCondition::Never),
                DriveCondition::All(children) => pending.extend(children.iter().copied()),
                _ => flat.push(id),
            }
        }
        flat.sort_unstable();
        flat.dedup();
        if has_complement_pair(&self.conditions, &flat) {
            return self.intern(DriveCondition::Never);
        }
        // A && (A || B) == A. Apply direct absorption after flattening.
        let factors = flat.iter().copied().collect::<std::collections::HashSet<_>>();
        flat.retain(
            |id| !matches!(&self.conditions[*id as usize], DriveCondition::Any(children) if children.iter().any(|child| factors.contains(child))),
        );
        match flat.len() {
            0 => self.intern(DriveCondition::Always),
            1 => Ok(flat[0]),
            _ => self.intern(DriveCondition::All(flat.into_boxed_slice())),
        }
    }

    fn any_condition(&mut self, ids: Vec<u32>) -> Result<u32, &'static str> {
        let mut flat = Vec::with_capacity(ids.len());
        let mut pending = ids;
        while let Some(id) = pending.pop() {
            match self.conditions.get(id as usize).ok_or("condition out of range")? {
                DriveCondition::Never => {}
                DriveCondition::Always => return self.intern(DriveCondition::Always),
                DriveCondition::Any(children) => pending.extend(children.iter().copied()),
                _ => flat.push(id),
            }
        }
        flat.sort_unstable();
        flat.dedup();
        if has_complement_pair(&self.conditions, &flat) {
            return self.intern(DriveCondition::Always);
        }
        // A || (A && B) == A. Apply direct absorption after flattening.
        let factors = flat.iter().copied().collect::<std::collections::HashSet<_>>();
        flat.retain(
            |id| !matches!(&self.conditions[*id as usize], DriveCondition::All(children) if children.iter().any(|child| factors.contains(child))),
        );
        match flat.len() {
            0 => self.intern(DriveCondition::Never),
            1 => Ok(flat[0]),
            _ => self.intern(DriveCondition::Any(flat.into_boxed_slice())),
        }
    }

    fn not_condition(&mut self, id: u32) -> Result<u32, &'static str> {
        match self.conditions.get(id as usize).cloned().ok_or("condition out of range")? {
            DriveCondition::Always => self.intern(DriveCondition::Never),
            DriveCondition::Never => self.intern(DriveCondition::Always),
            DriveCondition::Not(child) => Ok(child),
            DriveCondition::All(children) => {
                let mut negated = Vec::with_capacity(children.len());
                for child in children {
                    negated.push(self.not_condition(child)?);
                }
                self.any_condition(negated)
            }
            DriveCondition::Any(children) => {
                let mut negated = Vec::with_capacity(children.len());
                for child in children {
                    negated.push(self.not_condition(child)?);
                }
                self.all_condition(negated)
            }
            _ => self.intern(DriveCondition::Not(id)),
        }
    }

    fn visit(&mut self) -> Result<(), &'static str> {
        self.work += 1;
        if self.work % 256 == 0 && self.monitor.cancelled() {
            return Err("cancelled");
        }
        if self.work > MAX_EXPR_WORK {
            return Err("drive expression work budget");
        }
        Ok(())
    }

    fn site_source(&mut self, site: SiteId, port: PowerPort, depth: usize) -> Result<Expr, &'static str> {
        self.visit()?;
        if let PowerPort::Conducted { include_dust } = port {
            if include_dust {
                return Err("dust-inclusive conductor query");
            }
            return self.query(PowerTarget::Conductor { site, include_dust: false }, depth + 1);
        }
        let source = self.graph.sites.get(site.index()).ok_or("site out of range")?.source;
        let (block, source_value) = match source {
            SiteSource::Constant(profile) => {
                let profile = self.graph.profiles.get(profile.index()).ok_or("profile out of range")?;
                (
                    Block::from_id(profile.raw_state),
                    Some(DriveSource::Constant(self.graph.sites[site.index()].source_strength)),
                )
            }
            SiteSource::Component(node) => {
                let spec = self.graph.nodes.get(node.index()).ok_or("node out of range")?;
                let block = if let ComponentState::Wire(wire) = &spec.state {
                    Block::RedstoneWire { wire: *wire }
                } else {
                    Block::from_id(spec.raw_state.ok_or("dynamic component block")?)
                };
                (block, Some(DriveSource::Output(node)))
            }
            SiteSource::Contact(contact) => {
                let variants = self.contact_variants_selected(contact, depth + 1)?;
                let mut result = Expr::default();
                for (variant, selected) in variants {
                    let branch = self.contact_source_variant(&variant, port)?;
                    result.include(self.gate_expr(branch, selected)?, 0);
                }
                return Ok(result);
            }
        };
        let (face, strong) = match port {
            PowerPort::Weak(face) => (face, false),
            PowerPort::Strong(face) if matches!(block, Block::RedstoneWire { .. }) || power_rules::emits_strong(block, face) => (face, true),
            PowerPort::Strong(_) => return Ok(Expr::default()),
            PowerPort::Output | PowerPort::Stored => return self.source_expr(source_value.ok_or("source value")?, 0),
            PowerPort::Conducted { .. } => unreachable!(),
        };
        if matches!(block, Block::RedstoneWire { .. }) {
            if let SiteSource::Component(node) = source {
                let gate = match face {
                    BlockFace::Top => DriveCondition::Always,
                    BlockFace::Bottom => DriveCondition::Never,
                    _ => {
                        let CircuitNodeKind::Wire(wire_index) = &self.graph.nodes[node.index()].kind else {
                            return Err("wire emission source kind mismatch");
                        };
                        let wire = self.graph.wires.get(*wire_index).ok_or("wire source index out of range")?;
                        let raw_sides = wire
                            .execution
                            .query_facts
                            .or_else(|| Some(WireQueryFacts::infer(self.graph, wire)))
                            .and_then(|facts| facts.raw_sides)
                            .ok_or("wire emission shape unavailable")?;
                        DriveCondition::WireEmits { node, face, raw_sides }
                    }
                };
                let condition = self.intern(gate)?;
                return self.source_expr_condition(source_value.ok_or("wire source")?, condition, 0);
            }
            return Err("wire emission shape unavailable");
        }
        if if strong {
            !power_rules::emits_strong(block, face)
        } else {
            !power_rules::emits_weak(block, face)
        } {
            return Ok(Expr::default());
        }
        self.source_expr(source_value.ok_or("source value")?, 0)
    }

    fn source_expr(&self, source: DriveSource, attenuation: u8) -> Result<Expr, &'static str> {
        self.source_expr_condition(source, 0, attenuation)
    }

    fn source_expr_condition(&self, source: DriveSource, condition: u32, attenuation: u8) -> Result<Expr, &'static str> {
        let mut expr = Expr::default();
        if let DriveSource::Constant(value) = source {
            if condition == 0 {
                expr.constant = value.saturating_sub(attenuation);
            } else {
                expr.terms.insert((source, condition), attenuation);
            }
        } else if attenuation < 15 {
            expr.terms.insert((source, condition), attenuation);
        }
        Ok(expr)
    }

    fn query(&mut self, target: PowerTarget, depth: usize) -> Result<Expr, &'static str> {
        if depth > 16 {
            return Err("conductor depth/cycle");
        }
        let mut result = Expr::default();
        let links = self.incoming.get(&target).cloned().unwrap_or_default();
        for link in links {
            self.visit()?;
            if link.attenuation >= 15 {
                continue;
            }
            let condition = self.condition(link.guard, 0)?;
            if matches!(&self.conditions[condition as usize], DriveCondition::Never) {
                continue;
            }
            let mut source = self.site_source(link.from, link.from_port, depth)?;
            if matches!(&self.conditions[condition as usize], DriveCondition::Always) {
                result.include(source, link.attenuation);
            } else {
                let mut gated = Expr::default();
                gated.constant = 0;
                let source_terms = std::mem::take(&mut source.terms);
                for ((value, old_condition), loss) in source_terms {
                    let combined = self.combine_condition(old_condition, condition)?;
                    gated
                        .terms
                        .entry((value, combined))
                        .and_modify(|old| *old = (*old).min(loss))
                        .or_insert(loss);
                }
                // A conditional literal remains a term so its state guard is preserved.
                if source.constant != 0 {
                    gated
                        .terms
                        .entry((DriveSource::Constant(source.constant), condition))
                        .and_modify(|old| *old = 0)
                        .or_insert(0);
                }
                result.include(gated, link.attenuation);
            }
        }
        Ok(result)
    }

    fn combine_condition(&mut self, a: u32, b: u32) -> Result<u32, &'static str> {
        if a == b || matches!(&self.conditions[a as usize], DriveCondition::Always) {
            return Ok(b);
        }
        if matches!(&self.conditions[b as usize], DriveCondition::Always) {
            return Ok(a);
        }
        if matches!(&self.conditions[a as usize], DriveCondition::Never) || matches!(&self.conditions[b as usize], DriveCondition::Never) {
            return self.intern(DriveCondition::Never);
        }
        let mut ids = [a, b];
        ids.sort_unstable();
        self.intern(DriveCondition::All(ids.into()))
    }
}

pub(crate) fn infer_instant_drives(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<InstantDriveProgram, String> {
    if monitor.cancelled() {
        return Err("cancelled compiling instant drives".into());
    }
    let incoming = graph
        .power_links
        .iter()
        .fold(FxHashMap::<PowerTarget, Vec<_>>::default(), |mut map, link| {
            map.entry(link.to).or_default().push(link);
            map
        });
    let mut compiler = Compiler {
        graph,
        monitor,
        incoming,
        conditions: vec![DriveCondition::Always],
        condition_ids: HashMap::from([(DriveCondition::Always, 0)]),
        work: 0,
    };
    let mut result = InstantDriveProgram {
        by_wire: vec![None; graph.wires.len()].into_boxed_slice(),
        ..Default::default()
    };
    let mut all_terms = Vec::new();
    let mut all_queries = Vec::new();
    for (index, wire) in graph.wires.iter().enumerate() {
        if index % 128 == 0 && monitor.cancelled() {
            return Err("cancelled compiling instant drives".into());
        }
        result.stats.wires += 1;
        let mut expr = Expr::default();
        let mut ok = true;
        let mut decline = "unsupported source site";
        for (face, slot) in BlockFace::values().into_iter().zip([3usize, 2, 4, 5, 1, 0]) {
            let site = wire.canonical_neighbors[slot];
            match compiler.no_dust_sample(site, face) {
                Ok(next) => expr.include(next, 0),
                Err(reason) => {
                    if monitor.cancelled() {
                        return Err(format!("cancelled compiling instant drives: {reason}"));
                    }
                    ok = false;
                    decline = reason;
                    break;
                }
            }
        }
        if !ok {
            result.stats.fallback += 1;
            *result.stats.declines.entry(decline.to_owned()).or_default() += 1;
            continue;
        }
        let query_index = all_queries.len() as u32;
        let start = all_terms.len() as u32;
        let mut normalized = expr.terms.into_iter().collect::<Vec<_>>();
        normalized.sort_by_key(|((source, condition), loss)| (*condition, *source, *loss));
        for ((source, condition), attenuation) in normalized {
            all_terms.push(DriveTerm {
                source,
                condition,
                attenuation,
            });
        }
        let end = all_terms.len() as u32;
        all_queries.push(DriveExpr {
            constant: expr.constant,
            terms: start..end,
        });
        result.by_wire[index] = Some(query_index);
        result.stats.compiled += 1;
    }
    // This is an admission census only. Runtime transcripts still derive exact
    // FIFO behavior and retain dynamic callback acceptance at its sampling cut.
    for (net_index, net) in graph.plans.instant_transport.nets.iter().enumerate() {
        if net_index % 256 == 0 && monitor.cancelled() {
            return Err("cancelled while certifying shared-binary drive channels".into());
        }
        let mut shared: Option<(DriveSource, u32)> = None;
        let mut decline = None;
        let mut has_input = false;
        for transport_wire in net.wires.iter() {
            let Some(wire_index) = (match graph.nodes.get(transport_wire.node.index()).map(|node| &node.kind) {
                Some(CircuitNodeKind::Wire(index)) => Some(*index),
                _ => None,
            }) else {
                decline = Some("non-wire transport member");
                break;
            };
            let facts = graph.wires[wire_index]
                .execution
                .query_facts
                .unwrap_or_else(|| WireQueryFacts::infer(graph, &graph.wires[wire_index]));
            let Some(transfer) = facts.transfer else {
                decline = Some("missing transfer facts");
                break;
            };
            if transfer.seed.conditional != 0 || transfer.turbo.conditional != 0 {
                decline = Some("conditional seed/turbo transfer");
                break;
            }
            if transfer.seed.direct != transfer.turbo.direct {
                decline = Some("seed/turbo direct-mask mismatch");
                break;
            }
            let Some(query_index) = result.by_wire[wire_index] else {
                decline = Some("missing compiled drive");
                break;
            };
            let Some(query) = all_queries.get(query_index as usize) else {
                decline = Some("invalid compiled drive index");
                break;
            };
            if query.constant != 0 {
                decline = Some("nonzero drive constant");
                break;
            }
            for term in &all_terms[query.terms.start as usize..query.terms.end as usize] {
                if term.attenuation != 0 {
                    decline = Some("attenuated source term");
                    break;
                }
                match term.source {
                    DriveSource::Powered(_) => {
                        decline = Some("powered binary source candidate");
                        break;
                    }
                    DriveSource::Constant(15) => {
                        decline = Some("guarded constant-15 source candidate");
                        break;
                    }
                    DriveSource::Constant(_) => {
                        decline = Some("nonbinary constant source");
                        break;
                    }
                    DriveSource::Output(node) if !binary_output(graph, node) => {
                        decline = Some("nonbinary component output");
                        break;
                    }
                    DriveSource::Output(_) => {}
                }
                let key = (term.source, term.condition);
                if let Some(previous) = shared {
                    if previous.0 != key.0 {
                        decline = Some("multiple source expressions");
                        break;
                    }
                    if previous.1 != key.1 {
                        decline = Some("multiple source conditions");
                        break;
                    }
                }
                shared = Some(key);
                has_input = true;
            }
            if decline.is_some() {
                break;
            }
        }
        if decline.is_none() && !has_input {
            decline = Some("no varying source term");
        }
        if decline.is_none() {
            result.stats.shared_binary_nets += 1;
            result.stats.shared_binary_wires += net.wires.len();
        } else {
            *result.stats.channel_declines.entry(decline.unwrap().to_owned()).or_default() += 1;
        }
    }
    // Report the complete source shape independently of strict admission.
    // This scan never changes eligibility; it distinguishes a common guarded
    // constant drive from per-wire predicates that would require a wider ABI.
    for net in &graph.plans.instant_transport.nets {
        let mut conditions = Vec::with_capacity(net.wires.len());
        let mut all_guarded_15 = true;
        for transport_wire in net.wires.iter() {
            let Some(wire_index) = (match graph.nodes.get(transport_wire.node.index()).map(|node| &node.kind) {
                Some(CircuitNodeKind::Wire(index)) => Some(*index),
                _ => None,
            }) else {
                all_guarded_15 = false;
                break;
            };
            let Some(query_index) = result.by_wire[wire_index] else {
                all_guarded_15 = false;
                break;
            };
            let Some(query) = all_queries.get(query_index as usize) else {
                all_guarded_15 = false;
                break;
            };
            if query.constant != 0 || query.terms.end - query.terms.start != 1 {
                all_guarded_15 = false;
                break;
            }
            let term = &all_terms[query.terms.start as usize];
            if term.source != DriveSource::Constant(15)
                || term.attenuation != 0
                || matches!(
                    compiler.conditions[term.condition as usize],
                    DriveCondition::Always | DriveCondition::Never
                )
            {
                all_guarded_15 = false;
                break;
            }
            conditions.push(term.condition);
        }
        if all_guarded_15 && !conditions.is_empty() {
            result.stats.all_guarded_15_nets += 1;
            if conditions.iter().all(|condition| *condition == conditions[0]) {
                result.stats.shared_predicate_nets += 1;
            } else {
                result.stats.multiple_predicate_nets += 1;
            }
        } else {
            result.stats.other_drive_shape_nets += 1;
        }
    }
    result.conditions = compiler.conditions.into_boxed_slice();
    result.terms = all_terms.into_boxed_slice();
    result.queries = all_queries.into_boxed_slice();
    result.stats.work = compiler.work;
    Ok(result)
}

fn binary_output(graph: &CompileGraph, node: NodeId) -> bool {
    let Some(spec) = graph.nodes.get(node.index()) else {
        return false;
    };
    matches!(
        &spec.kind,
        CircuitNodeKind::Electrical(
            crate::redpiler::components::NodeType::Repeater { .. }
                | crate::redpiler::components::NodeType::Torch
                | crate::redpiler::components::NodeType::Button
                | crate::redpiler::components::NodeType::Lever
                | crate::redpiler::components::NodeType::PressurePlate
                | crate::redpiler::components::NodeType::Observer { .. }
                | crate::redpiler::components::NodeType::Hopper
                | crate::redpiler::components::NodeType::CopperBulb
                | crate::redpiler::components::NodeType::Trapdoor
        ) | CircuitNodeKind::Observer { .. }
    )
}

#[cfg(test)]
mod condition_tests {
    use super::*;
    use crate::redpiler::TaskMonitor;

    fn graph(guards: Vec<Guard>, contacts: Vec<ContactSpec>) -> CompileGraph {
        CompileGraph {
            nodes: vec![],
            links: vec![],
            power_links: vec![],
            comparator_inputs: vec![],
            profiles: vec![],
            contacts,
            routes: vec![],
            wires: vec![],
            wire_relations: vec![],
            sites: vec![],
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
            plans: Default::default(),
            guards,
        }
    }

    fn compiler<'g, 'm>(graph: &'g CompileGraph, monitor: &'m TaskMonitor) -> Compiler<'g, 'm> {
        Compiler {
            graph,
            monitor,
            incoming: FxHashMap::default(),
            conditions: vec![DriveCondition::Always],
            condition_ids: HashMap::from([(DriveCondition::Always, 0)]),
            work: 0,
        }
    }

    fn atom(compiler: &mut Compiler<'_, '_>, index: u32) -> u32 {
        compiler
            .intern(DriveCondition::State(StateTest::PistonExtended(PistonId::new(index))))
            .unwrap()
    }

    fn eval(conditions: &[DriveCondition], id: u32, mask: u64) -> bool {
        match &conditions[id as usize] {
            DriveCondition::Always => true,
            DriveCondition::Never => false,
            DriveCondition::State(StateTest::PistonExtended(piston)) => mask & (1u64 << piston.index()) != 0,
            DriveCondition::Not(child) => !eval(conditions, *child, mask),
            DriveCondition::All(children) => children.iter().all(|&child| eval(conditions, child, mask)),
            DriveCondition::Any(children) => children.iter().any(|&child| eval(conditions, child, mask)),
            DriveCondition::WireEmits { .. } | DriveCondition::State(_) => unreachable!("test uses only piston atoms"),
        }
    }

    #[test]
    fn boolean_condition_pool_flattens_absorbs_and_detects_complements_exactly() {
        let graph = graph(vec![], vec![]);
        let monitor = TaskMonitor::default();
        let mut compiler = compiler(&graph, &monitor);
        let a = atom(&mut compiler, 0);
        let b = atom(&mut compiler, 1);
        let not_a = compiler.not_condition(a).unwrap();
        let not_b = compiler.not_condition(b).unwrap();
        let ab = compiler.all_condition(vec![a, b]).unwrap();
        let nested = compiler.all_condition(vec![ab, a]).unwrap();
        let a_or_b = compiler.any_condition(vec![a, b]).unwrap();
        let a_and_b = compiler.all_condition(vec![a, b]).unwrap();
        let absorbed_and = compiler.all_condition(vec![a, a_or_b]).unwrap();
        let absorbed_or = compiler.any_condition(vec![a, a_and_b]).unwrap();
        let contradiction = compiler.all_condition(vec![a, not_a]).unwrap();
        let tautology = compiler.any_condition(vec![a, not_a]).unwrap();
        let demorgan = compiler.not_condition(ab).unwrap();
        let de_morgan_form = compiler.any_condition(vec![not_a, not_b]).unwrap();

        assert_eq!(nested, ab, "associative nesting and duplicate operands share one ID");
        assert_eq!(absorbed_and, a);
        assert_eq!(absorbed_or, a);
        assert!(matches!(compiler.conditions[contradiction as usize], DriveCondition::Never));
        assert!(matches!(compiler.conditions[tautology as usize], DriveCondition::Always));
        assert_eq!(demorgan, de_morgan_form);
        for mask in 0..4 {
            let expected_a = mask & 1 != 0;
            let expected_b = mask & 2 != 0;
            assert_eq!(eval(&compiler.conditions, nested, mask), expected_a && expected_b);
            assert_eq!(eval(&compiler.conditions, absorbed_and, mask), expected_a);
            assert_eq!(eval(&compiler.conditions, absorbed_or, mask), expected_a);
            assert!(!eval(&compiler.conditions, contradiction, mask));
            assert!(eval(&compiler.conditions, tautology, mask));
            assert_eq!(eval(&compiler.conditions, demorgan, mask), !(expected_a && expected_b));
        }
    }

    #[test]
    fn canonicalization_preserves_ordered_first_match_contact_variants() {
        let guards = vec![
            Guard::State(StateTest::PistonExtended(PistonId::new(0))),
            Guard::State(StateTest::PistonExtended(PistonId::new(1))),
        ];
        let contacts = vec![ContactSpec {
            node: NodeId::new(0),
            position: mchprs_blocks::BlockPos::new(0, 0, 0),
            variants: vec![
                ContactVariant {
                    when: GuardId::new(0),
                    profile: ProfileId::new(0),
                    receiver: None,
                },
                ContactVariant {
                    when: GuardId::new(1),
                    profile: ProfileId::new(0),
                    receiver: None,
                },
            ]
            .into_boxed_slice(),
        }];
        let graph = graph(guards, contacts);
        let monitor = TaskMonitor::default();
        let mut compiler = compiler(&graph, &monitor);
        let selected = compiler.contact_variants_selected(ContactId::new(0), 0).unwrap();
        assert_eq!(selected.len(), 2);
        for mask in 0..4 {
            let first = mask & 1 != 0;
            let second = mask & 2 != 0;
            assert_eq!(eval(&compiler.conditions, selected[0].1, mask), first);
            assert_eq!(eval(&compiler.conditions, selected[1].1, mask), second && !first);
        }
    }

    #[test]
    fn wide_boolean_nodes_remain_exact_with_bounded_compound_complement_work() {
        let graph = graph(vec![], vec![]);
        let monitor = TaskMonitor::default();
        let mut compiler = compiler(&graph, &monitor);
        let atoms = (0..20).map(|index| atom(&mut compiler, index)).collect::<Vec<_>>();
        let other_atoms = (20..40).map(|index| atom(&mut compiler, index)).collect::<Vec<_>>();
        let all_atoms = compiler.all_condition(atoms.clone()).unwrap();
        let not_all_atoms = compiler.not_condition(all_atoms).unwrap();
        let contradiction = compiler.all_condition(vec![all_atoms, not_all_atoms]).unwrap();
        let all_other_atoms = compiler.all_condition(other_atoms.clone()).unwrap();
        let not_all_other_atoms = compiler.not_condition(all_other_atoms).unwrap();
        let bounded_pair = compiler.all_condition(vec![all_atoms, not_all_other_atoms]).unwrap();

        // Compound complement matching is capped at 256 recursive comparisons;
        // missing this simplification is safe, and the expression remains exact.
        assert!(compiler.conditions.len() <= 100);
        assert!(matches!(compiler.conditions[bounded_pair as usize], DriveCondition::All(_)));
        let full_first = (1u64 << 20) - 1;
        let full_both = (1u64 << 40) - 1;
        let samples = (0..1024u64)
            .map(|sample| sample.wrapping_mul(2_654_435_761) & full_both)
            .chain([full_first, full_both, full_first - 1]);
        for mask in samples {
            let all = mask & full_first == full_first;
            let all_other = mask & (full_both ^ full_first) == (full_both ^ full_first);
            assert_eq!(eval(&compiler.conditions, all_atoms, mask), all);
            assert_eq!(eval(&compiler.conditions, not_all_atoms, mask), !all);
            assert!(!eval(&compiler.conditions, contradiction, mask));
            assert_eq!(eval(&compiler.conditions, bounded_pair, mask), all && !all_other);
        }

        let not_atoms = atoms.iter().map(|&id| compiler.not_condition(id).unwrap()).collect::<Vec<_>>();
        let direct_complement = compiler.all_condition([atoms, not_atoms].concat()).unwrap();
        assert!(matches!(compiler.conditions[direct_complement as usize], DriveCondition::Never));
    }
}
