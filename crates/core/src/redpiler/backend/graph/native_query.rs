use super::*;
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const MAX_NATIVE_LINKS: usize = 256;
const MAX_GUARD_DEPTH: usize = 128;
const NO_TARGET: u32 = u32::MAX;
static NEXT_ARTIFACT: AtomicU64 = AtomicU64::new(0);

type NativeQueryFn = unsafe extern "C" fn(u32, *const NativeQueryState) -> u8;

#[repr(C)]
struct NativeQueryState {
    components: *const u8,
    pistons: *const u8,
    resources: *const u8,
    classic_signals: *const u8,
}

#[derive(Clone, Debug)]
pub struct NativeQueryStats {
    pub compile_time: Duration,
    pub total_targets: usize,
    pub compiled_targets: usize,
    pub compiled_links: usize,
    pub empty_root_hits: u64,
    pub hits: u64,
    pub fallbacks: u64,
    pub verified_queries: u64,
    pub source: PathBuf,
    pub library: PathBuf,
}

#[derive(Debug)]
pub struct NativeQueryError(String);

impl NativeQueryError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for NativeQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NativeQueryError {}

#[derive(Clone)]
struct NativeLink {
    guard: String,
    source: String,
    attenuation: u8,
}

struct QueryPlan {
    id: u32,
    links: Vec<NativeLink>,
}

pub(super) struct NativePowerQueries {
    _library: libloading::Library,
    query: NativeQueryFn,
    input_targets: Vec<u32>,
    conductor_targets: Vec<u32>,
    stats: NativeQueryStats,
    hits: Cell<u64>,
    empty_root_hits: Cell<u64>,
    fallbacks: Cell<u64>,
    verified_queries: Cell<u64>,
    verify_general: bool,
}

impl NativePowerQueries {
    pub(super) fn compile(
        program: &CircuitProgram,
        state: &CircuitState,
        clang: &Path,
        linker: Option<&Path>,
        artifact_dir: &Path,
        verify_general: bool,
    ) -> Result<Self, NativeQueryError> {
        let started = Instant::now();
        let mut grouped = BTreeMap::<PowerTarget, Vec<PowerLink>>::new();
        for &link in program.power_links() {
            grouped.entry(link.to).or_default().push(link);
        }

        let targets: Vec<_> = grouped.keys().copied().collect();
        let mut compiler = TargetCompiler::new(program, state, &grouped);
        for target in &targets {
            let _ = compiler.compile_target(*target)?;
        }
        if compiler.compiled_targets == 0 {
            return Err(NativeQueryError(
                "lowered graph has no supported power-query roots for the native demo".into(),
            ));
        }

        let mut input_targets = vec![
            0;
            program
                .nodes()
                .len()
                .checked_mul(3)
                .ok_or_else(|| NativeQueryError("too many input roots".into()))?
        ];
        let mut conductor_targets = vec![
            0;
            program
                .sites()
                .len()
                .checked_mul(2)
                .ok_or_else(|| NativeQueryError("too many conductor roots".into()))?
        ];
        for target in &targets {
            let id = compiler
                .target_status(*target)
                .filter(|(_, supported)| *supported)
                .map(|(id, _)| id)
                .unwrap_or(NO_TARGET);
            match *target {
                PowerTarget::Input { node, port } => {
                    let index = node.index() * 3 + input_port_index(port);
                    if let Some(binding) = input_targets.get_mut(index) {
                        *binding = id;
                    }
                }
                PowerTarget::Conductor { site, include_dust } => {
                    let index = site.index() * 2 + usize::from(include_dust);
                    if let Some(binding) = conductor_targets.get_mut(index) {
                        *binding = id;
                    }
                }
            }
        }

        let plans: Vec<_> = compiler.plans.into_values().collect();
        let generated = generate_c(&plans, &compiler.guards.bodies, false);
        fs::create_dir_all(artifact_dir).map_err(|error| NativeQueryError(format!("create native artifact directory: {error}")))?;
        let unique = NEXT_ARTIFACT.fetch_add(1, Ordering::Relaxed);
        let stem = format!("power-query-{}-{unique}", std::process::id());
        let source = artifact_dir.join(format!("{stem}.c"));
        #[cfg(windows)]
        let library = artifact_dir.join(format!("{stem}.dll"));
        #[cfg(not(windows))]
        let library = artifact_dir.join(format!("{stem}.so"));
        fs::write(&source, generated).map_err(|error| NativeQueryError(format!("write generated C source: {error}")))?;

        #[cfg(windows)]
        compile_windows(
            clang,
            linker.ok_or_else(|| NativeQueryError("Windows native demo needs an lld-link path".into()))?,
            &source,
            &library,
        )?;
        #[cfg(not(windows))]
        compile_unix(clang, &source, &library)?;

        let loaded =
            unsafe { libloading::Library::new(&library) }.map_err(|error| NativeQueryError(format!("load generated native library: {error}")))?;
        let query = unsafe {
            *loaded
                .get::<NativeQueryFn>(b"mchprs_query\0")
                .map_err(|error| NativeQueryError(format!("find generated query function: {error}")))?
        };
        let compiled_links = plans.iter().map(|plan| plan.links.len()).sum();
        let stats = NativeQueryStats {
            compile_time: started.elapsed(),
            total_targets: grouped.len(),
            compiled_targets: compiler.compiled_targets,
            compiled_links,
            empty_root_hits: 0,
            hits: 0,
            fallbacks: 0,
            verified_queries: 0,
            source,
            library,
        };
        Ok(Self {
            _library: loaded,
            query,
            input_targets,
            conductor_targets,
            stats,
            hits: Cell::new(0),
            empty_root_hits: Cell::new(0),
            fallbacks: Cell::new(0),
            verified_queries: Cell::new(0),
            verify_general,
        })
    }

    pub(super) fn target_id(&self, target: PowerTarget) -> Option<u32> {
        let binding = match target {
            PowerTarget::Input { node, port } => self.input_targets.get(node.index() * 3 + input_port_index(port)),
            PowerTarget::Conductor { site, include_dust } => self.conductor_targets.get(site.index() * 2 + usize::from(include_dust)),
        }?;
        (*binding != NO_TARGET).then_some(*binding)
    }

    pub(super) fn evaluate(&self, target_id: u32, state: &CircuitState) -> u8 {
        self.hits.set(self.hits.get().saturating_add(1));
        if target_id == 0 {
            self.empty_root_hits.set(self.empty_root_hits.get().saturating_add(1));
        }
        let live = NativeQueryState {
            components: state.components.as_ptr().cast(),
            pistons: state.pistons.as_ptr().cast(),
            resources: state.resources.as_ptr().cast(),
            classic_signals: state.classic_signals.as_ptr(),
        };
        // SAFETY: the function and its offsets were generated together from this backend's
        // immutable program and current state layout. Every pointer is refreshed from the live
        // boxed slices for this synchronous call, and the generated function reads only offsets
        // whose indices were validated against these slices during compilation.
        unsafe { (self.query)(target_id, &live) }
    }

    pub(super) fn verify_general(&self) -> bool {
        self.verify_general
    }

    pub(super) fn verified(&self) {
        self.verified_queries.set(self.verified_queries.get().saturating_add(1));
    }

    pub(super) fn fallback(&self) {
        self.fallbacks.set(self.fallbacks.get().saturating_add(1));
    }

    pub(super) fn stats(&self) -> NativeQueryStats {
        let mut stats = self.stats.clone();
        stats.empty_root_hits = self.empty_root_hits.get();
        stats.hits = self.hits.get();
        stats.fallbacks = self.fallbacks.get();
        stats.verified_queries = self.verified_queries.get();
        stats
    }
}

fn input_port_index(port: InputPort) -> usize {
    match port {
        InputPort::Main => 0,
        InputPort::Side => 1,
        InputPort::Quasi => 2,
    }
}

#[derive(Clone, Copy)]
enum TargetStatus {
    Visiting(u32),
    Supported(u32),
    Unsupported(u32),
}

struct TargetCompiler<'a> {
    program: &'a CircuitProgram,
    state: &'a CircuitState,
    grouped: &'a BTreeMap<PowerTarget, Vec<PowerLink>>,
    statuses: BTreeMap<PowerTarget, TargetStatus>,
    plans: BTreeMap<u32, QueryPlan>,
    next_id: u32,
    compiled_targets: usize,
    guards: GuardCompiler<'a>,
    compact: bool,
}

impl<'a> TargetCompiler<'a> {
    fn new(program: &'a CircuitProgram, state: &'a CircuitState, grouped: &'a BTreeMap<PowerTarget, Vec<PowerLink>>) -> Self {
        let mut plans = BTreeMap::new();
        plans.insert(0, QueryPlan { id: 0, links: Vec::new() });
        Self {
            program,
            state,
            grouped,
            statuses: BTreeMap::new(),
            plans,
            next_id: 1,
            compiled_targets: 0,
            guards: GuardCompiler::new(program, state),
            compact: false,
        }
    }

    fn target_status(&self, target: PowerTarget) -> Option<(u32, bool)> {
        match self.statuses.get(&target).copied()? {
            TargetStatus::Supported(id) => Some((id, true)),
            TargetStatus::Unsupported(id) | TargetStatus::Visiting(id) => Some((id, false)),
        }
    }

    fn compile_target(&mut self, target: PowerTarget) -> Result<Option<u32>, NativeQueryError> {
        if !self.grouped.contains_key(&target) {
            return Ok(Some(0));
        }
        match self.statuses.get(&target).copied() {
            Some(TargetStatus::Supported(id)) => return Ok(Some(id)),
            Some(TargetStatus::Unsupported(_) | TargetStatus::Visiting(_)) => return Ok(None),
            None => {}
        }

        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| NativeQueryError("too many native query functions".into()))?;
        self.statuses.insert(target, TargetStatus::Visiting(id));
        let links = self.grouped.get(&target).cloned().unwrap_or_default();
        let mut compiled = Vec::with_capacity(links.len());
        let supported = if links.len() > MAX_NATIVE_LINKS {
            false
        } else {
            let mut supported = true;
            for link in links {
                let Some(guard) = self.guards.compile(link.guard, 0) else {
                    supported = false;
                    break;
                };
                let Some(source) = self.compile_source(&link) else {
                    supported = false;
                    break;
                };
                compiled.push(NativeLink {
                    guard,
                    source,
                    attenuation: link.attenuation,
                });
            }
            supported
        };

        if supported {
            self.plans.insert(id, QueryPlan { id, links: compiled });
            self.statuses.insert(target, TargetStatus::Supported(id));
            self.compiled_targets += 1;
            Ok(Some(id))
        } else {
            self.statuses.insert(target, TargetStatus::Unsupported(id));
            Ok(None)
        }
    }

    fn compile_source(&mut self, link: &PowerLink) -> Option<String> {
        let site = self.program.sites().get(link.from.index())?;
        if let PowerPort::Conducted { include_dust } = link.from_port {
            let target = PowerTarget::Conductor {
                site: link.from,
                include_dust,
            };
            let id = self.compile_target(target).ok()??;
            if let SiteSource::Contact(contact) = site.source {
                let variants = self.program.contacts().get(contact.index())?.variants.to_vec();
                let guards = variants
                    .iter()
                    .map(|variant| self.guards.compile(variant.when, 0))
                    .collect::<Option<Vec<_>>>()?;
                if guards.is_empty() {
                    return Some("0".into());
                }
                return Some(format!("(({}) ? query_{id}(s) : 0)", guards.join(" || ")));
            }
            return Some(format!("query_{id}(s)"));
        }
        match site.source {
            SiteSource::Component(node) => self.component_source(node, link.from_port),
            SiteSource::Constant(profile) => {
                let block = self.program.profile_block(profile)?;
                let emits = source_emission(SiteSource::Constant(profile), block, link.from_port)?;
                Some(if emits { site.source_strength.to_string() } else { "0".into() })
            }
            SiteSource::Contact(contact) => self.contact_source(contact, link.from_port),
        }
    }

    fn component_source(&self, node: NodeId, port: PowerPort) -> Option<String> {
        let block = self.program.nodes().get(node.index())?.block?;
        if !source_emission(SiteSource::Component(node), block, port)? {
            return Some("0".into());
        }
        self.component_strength(node)
    }

    fn contact_source(&mut self, contact: ContactId, port: PowerPort) -> Option<String> {
        let variants = self.program.contacts().get(contact.index())?.variants.to_vec();
        let mut expression = "0".to_owned();
        for variant in variants.iter().rev() {
            let guard = self.guards.compile(variant.when, 0)?;
            let source = self.profile_source(contact, variant.profile, variant.receiver, port)?;
            expression = format!("(({guard}) ? ({source}) : ({expression}))");
        }
        Some(expression)
    }

    fn profile_source(&self, contact: ContactId, profile: ProfileId, receiver: Option<CallbackReceiver>, port: PowerPort) -> Option<String> {
        let block = match receiver {
            Some(CallbackReceiver::Component(node)) => self.program.nodes().get(node.index())?.block?,
            _ => self.program.profile_block(profile)?,
        };
        let source = receiver.map_or(SiteSource::Contact(contact), |receiver| match receiver {
            CallbackReceiver::Component(node) => SiteSource::Component(node),
            _ => SiteSource::Contact(contact),
        });
        if !source_emission(source, block, port)? {
            return Some("0".into());
        }
        match receiver {
            Some(CallbackReceiver::Component(node)) => self.component_strength(node),
            Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_)) | None => {
                let strength = match self.program.profiles().get(profile.index())?.electrical {
                    ProfileElectrical::RedstoneSource => 15,
                    ProfileElectrical::None | ProfileElectrical::Component => 0,
                };
                Some(strength.to_string())
            }
        }
    }

    fn component_strength(&self, node: NodeId) -> Option<String> {
        if let Some((net, attenuation)) = self.program.classic().wire_by_node.get(node.index()).copied().flatten() {
            return Some(format!(
                "(s->classic_signals[{net}] > {attenuation} ? s->classic_signals[{net}] - {attenuation} : 0)"
            ));
        }
        if self.compact {
            return Some(match self.state.components.get(node.index())? {
                ComponentState::Electrical(_) => format!("((const NEComponentState *)s->components)[{}].output_strength", node.index()),
                ComponentState::Wire(_) => format!("((const NEComponentState *)s->components)[{}].wire_power", node.index()),
                ComponentState::Specialized => "0".into(),
            });
        }
        component_strength_expression(self.state, node)
    }
}

struct GuardCompiler<'a> {
    program: &'a CircuitProgram,
    state: &'a CircuitState,
    bodies: BTreeMap<usize, String>,
    active: Vec<bool>,
    compact: bool,
    captured_is_false: bool,
}

impl<'a> GuardCompiler<'a> {
    fn new(program: &'a CircuitProgram, state: &'a CircuitState) -> Self {
        Self {
            program,
            state,
            bodies: BTreeMap::new(),
            active: vec![false; program.guards().len()],
            compact: false,
            captured_is_false: true,
        }
    }

    fn compile(&mut self, id: GuardId, depth: usize) -> Option<String> {
        if depth > MAX_GUARD_DEPTH {
            return None;
        }
        let index = id.index();
        if let Some(body) = self.bodies.get(&index) {
            return Some(if body == "0" || body == "1" {
                body.clone()
            } else {
                format!("guard_{index}(s)")
            });
        }
        let guard = self.program.guards().get(index)?.clone();
        if self.active[index] {
            return None;
        }
        if guard == Guard::Always {
            return Some("1".into());
        }
        self.active[index] = true;
        let body = match guard {
            Guard::Always => Some("1".to_owned()),
            Guard::State(test) => self.state_test(test),
            Guard::Site { site, view, test } => self.site_test(site, view, test, depth + 1),
            Guard::Not(child) => self.compile(child, depth + 1).map(|expr| match expr.as_str() {
                "0" => "1".into(),
                "1" => "0".into(),
                _ => format!("!({expr})"),
            }),
            Guard::All(children) => self.compile_children(&children, "&&", "1", depth + 1),
            Guard::Any(children) => self.compile_children(&children, "||", "0", depth + 1),
        };
        self.active[index] = false;
        if let Some(body) = body {
            let expression = if body == "0" || body == "1" {
                body.clone()
            } else {
                format!("guard_{index}(s)")
            };
            self.bodies.insert(index, body);
            Some(expression)
        } else {
            None
        }
    }

    fn compile_children(&mut self, children: &[GuardId], op: &str, identity: &str, depth: usize) -> Option<String> {
        if children.is_empty() {
            return Some(identity.to_owned());
        }
        let mut expressions = children.iter().map(|child| self.compile(*child, depth + 1)).collect::<Option<Vec<_>>>()?;
        let decisive = if op == "&&" { "0" } else { "1" };
        if expressions.iter().any(|expr| expr == decisive) {
            return Some(decisive.into());
        }
        expressions.retain(|expr| expr != identity);
        Some(match expressions.len() {
            0 => identity.into(),
            1 => expressions.pop().unwrap(),
            _ => format!("({})", expressions.join(&format!(" {op} "))),
        })
    }

    fn state_test(&self, test: StateTest) -> Option<String> {
        if self.compact {
            return Some(match test {
                StateTest::PistonExtended(piston) => format!("((const NEPistonState *)s->pistons)[{}].extended != 0", piston.index()),
                StateTest::PistonHeadPresent(piston) => format!("((const NEPistonState *)s->pistons)[{}].head_present != 0", piston.index()),
                StateTest::PistonAvailability { piston, is } => format!(
                    "((const NEPistonState *)s->pistons)[{}].availability == {}",
                    piston.index(),
                    super::native_engine::availability(is)
                ),
                StateTest::ResourceEnabled(resource) => format!("((const NEResourceState *)s->resources)[{}].enabled != 0", resource.index()),
                StateTest::ResourceAt { resource, contact } => {
                    format!("((const NEResourceState *)s->resources)[{}].at == {}", resource.index(), contact.index())
                }
            });
        }
        match test {
            StateTest::PistonExtended(piston) => {
                bool_piston_field(self.state, piston, |item| &item.extended).map(|offset| format!("s->pistons[{offset}] != 0"))
            }
            StateTest::PistonHeadPresent(piston) => {
                bool_piston_field(self.state, piston, |item| &item.head_present).map(|offset| format!("s->pistons[{offset}] != 0"))
            }
            StateTest::PistonAvailability { piston, is } => {
                let offset = piston_availability_offset(self.state, piston)?;
                let tag = piston_availability_tag(is)?;
                Some(format!("s->pistons[{offset}] == {tag}"))
            }
            StateTest::ResourceEnabled(resource) => {
                bool_resource_field(self.state, resource, |item| &item.enabled).map(|offset| format!("s->resources[{offset}] != 0"))
            }
            StateTest::ResourceAt { resource, contact } => {
                let offset = resource_contact_offset(self.state, resource)?;
                let bytes = contact_id_bytes(contact)?;
                Some(format!(
                    "({})",
                    bytes
                        .iter()
                        .enumerate()
                        .map(|(i, byte)| format!("s->resources[{}] == {byte}", offset + i))
                        .collect::<Vec<_>>()
                        .join(" && ")
                ))
            }
        }
    }

    fn site_test(&mut self, site: SiteId, view: OperandView, test: SiteTest, depth: usize) -> Option<String> {
        if view == OperandView::Captured {
            // Graph power queries evaluate guards with no captured delivery snapshot.
            return self.captured_is_false.then(|| "0".into());
        }
        let source = self.program.sites().get(site.index())?.source;
        match source {
            SiteSource::Constant(profile) => self.profile_site_test(profile, None, test),
            SiteSource::Component(node) => {
                let node = self.program.nodes().get(node.index())?;
                Some(site_test_value(test, node.block, Some(&node.kind), None, None))
            }
            SiteSource::Contact(contact) => {
                let variants = self.program.contacts().get(contact.index())?.variants.to_vec();
                let mut expression = "0".to_owned();
                for variant in variants.iter().rev() {
                    let guard = self.compile(variant.when, depth + 1)?;
                    let value = self.profile_site_test(variant.profile, variant.receiver, test)?;
                    expression = format!("(({guard}) ? ({value}) : ({expression}))");
                }
                Some(expression)
            }
        }
    }

    fn profile_site_test(&self, profile: ProfileId, receiver: Option<CallbackReceiver>, test: SiteTest) -> Option<String> {
        let facts = self.program.profiles().get(profile.index())?;
        let (block, kind) = match receiver {
            Some(CallbackReceiver::Component(node)) => {
                let node = self.program.nodes().get(node.index())?;
                (node.block, Some(&node.kind))
            }
            _ => (self.program.profile_block(profile), None),
        };
        Some(site_test_value(test, block, kind, Some(profile), Some(facts)))
    }
}

fn site_test_value(
    test: SiteTest,
    block: Option<Block>,
    kind: Option<&CircuitNodeKind>,
    profile: Option<ProfileId>,
    facts: Option<&Profile>,
) -> String {
    let value = match test {
        SiteTest::Solid => facts.map_or_else(|| block.is_some_and(Block::is_solid), |facts| facts.solid),
        SiteTest::Transparent => facts.map_or_else(|| block.is_some_and(Block::is_transparent), |facts| facts.transparent),
        SiteTest::IsWire => {
            kind.is_some_and(|kind| matches!(kind, CircuitNodeKind::Wire(_)))
                || block.is_some_and(|block| matches!(block, Block::RedstoneWire { .. }))
        }
        SiteTest::Profile(expected) => profile == Some(expected),
    };
    if value { "1" } else { "0" }.to_owned()
}

fn source_emission(source: SiteSource, block: Block, port: PowerPort) -> Option<bool> {
    match port {
        PowerPort::Output | PowerPort::Stored => Some(true),
        PowerPort::Weak(face) => {
            if matches!(block, Block::RedstoneWire { .. }) && matches!(source, SiteSource::Component(_)) {
                return match face {
                    BlockFace::Top => Some(true),
                    BlockFace::Bottom => Some(false),
                    _ => None,
                };
            }
            Some(crate::redpiler::power_rules::emits_weak(block, face))
        }
        PowerPort::Strong(face) => {
            if matches!(block, Block::RedstoneWire { .. }) && matches!(source, SiteSource::Component(_)) {
                return match face {
                    BlockFace::Top => Some(true),
                    BlockFace::Bottom => Some(false),
                    _ => None,
                };
            }
            Some(crate::redpiler::power_rules::emits_strong(block, face))
        }
        PowerPort::Conducted { .. } => None,
    }
}

fn component_strength_expression(state: &CircuitState, node: NodeId) -> Option<String> {
    let component = state.components.get(node.index())?;
    let element_address = component as *const ComponentState as usize;
    let offset = match component {
        ComponentState::Electrical(component) => {
            array_field_offset::<ComponentState>(node.index(), element_address, &component.output_strength as *const u8 as usize)?
        }
        ComponentState::Wire(wire) => array_field_offset::<ComponentState>(node.index(), element_address, &wire.power as *const u8 as usize)?,
        ComponentState::Specialized => return Some("0".into()),
    };
    Some(format!("s->components[{offset}]"))
}

fn bool_piston_field(state: &CircuitState, piston: PistonId, field: impl FnOnce(&PistonState) -> &bool) -> Option<usize> {
    let item = state.pistons.get(piston.index())?;
    array_field_offset::<PistonState>(piston.index(), item as *const _ as usize, field(item) as *const bool as usize)
}

fn bool_resource_field(state: &CircuitState, resource: ResourceId, field: impl FnOnce(&ResourceState) -> &bool) -> Option<usize> {
    let item = state.resources.get(resource.index())?;
    array_field_offset::<ResourceState>(resource.index(), item as *const _ as usize, field(item) as *const bool as usize)
}

fn piston_availability_offset(state: &CircuitState, piston: PistonId) -> Option<usize> {
    let item = state.pistons.get(piston.index())?;
    array_field_offset::<PistonState>(
        piston.index(),
        item as *const _ as usize,
        &item.availability as *const PistonAvailability as usize,
    )
}

fn piston_availability_tag(value: PistonAvailability) -> Option<u8> {
    if std::mem::size_of::<PistonAvailability>() != 1 {
        return None;
    }
    let variants = [
        PistonAvailability::Ready,
        PistonAvailability::HeadPending,
        PistonAvailability::PublishingHeadRemoval,
        PistonAvailability::BasePendingHeadPresent,
        PistonAvailability::BasePending,
    ];
    // SAFETY: this fieldless enum is accepted only when its measured size is one byte, and each
    // value is a valid fully initialized enum representation from this exact Rust build.
    let byte = |value: PistonAvailability| unsafe { *(&value as *const PistonAvailability).cast::<u8>() };
    let variant_count = variants.len();
    let values: Vec<_> = variants.iter().copied().map(byte).collect();
    if values.iter().copied().collect::<std::collections::BTreeSet<_>>().len() != variant_count {
        return None;
    }
    Some(byte(value))
}

fn resource_contact_offset(state: &CircuitState, resource: ResourceId) -> Option<usize> {
    let item = state.resources.get(resource.index())?;
    if std::mem::size_of::<ContactId>() != 4 {
        return None;
    }
    array_field_offset::<ResourceState>(resource.index(), item as *const _ as usize, &item.at as *const ContactId as usize)
}

fn contact_id_bytes(contact: ContactId) -> Option<[u8; 4]> {
    if std::mem::size_of::<ContactId>() != 4 {
        return None;
    }
    // SAFETY: ContactId is a four-byte valid graph id for this build. Byte reads preserve its
    // actual in-process representation without assuming endianness or a public repr attribute.
    Some(unsafe { std::array::from_fn(|index| *(&contact as *const ContactId).cast::<u8>().add(index)) })
}

fn array_field_offset<T>(index: usize, element_address: usize, field_address: usize) -> Option<usize> {
    let inside = field_address.checked_sub(element_address)?;
    if inside >= std::mem::size_of::<T>() {
        return None;
    }
    index.checked_mul(std::mem::size_of::<T>())?.checked_add(inside)
}

/// Reuse expression lowering inside the whole executor, with its compact C state ABI.
pub(super) fn compact_power_source(program: &CircuitProgram, state: &CircuitState) -> Result<(String, usize, usize, usize), NativeQueryError> {
    use std::fmt::Write;
    let mut grouped = BTreeMap::<PowerTarget, Vec<PowerLink>>::new();
    for &link in program.power_links() {
        grouped.entry(link.to).or_default().push(link);
    }
    let mut compiler = TargetCompiler::new(program, state, &grouped);
    compiler.compact = true;
    compiler.guards.compact = true;
    for &target in grouped.keys() {
        compiler.compile_target(target)?;
    }
    let mut inputs = vec![0; program.nodes().len() * 3];
    let mut conductors = vec![0; program.sites().len() * 2];
    for &target in grouped.keys() {
        let id = compiler
            .target_status(target)
            .filter(|(_, supported)| *supported)
            .map_or(NO_TARGET, |(id, _)| id);
        match target {
            PowerTarget::Input { node, port } => inputs[node.index() * 3 + input_port_index(port)] = id,
            PowerTarget::Conductor { site, include_dust } => conductors[site.index() * 2 + usize::from(include_dust)] = id,
        }
    }
    let supported = compiler.compiled_targets;
    // Callback guards can own snapshots, unlike power-query guards. Decline every
    // root whose expression would need a captured operand, including dependencies.
    let mut callback_guards = GuardCompiler::new(program, state);
    callback_guards.compact = true;
    callback_guards.captured_is_false = false;
    let mut requested_guards = vec![false; program.guards().len()];
    for route in program.routes() {
        for delivery in &route.deliveries {
            requested_guards[delivery.guard.index()] = true;
        }
    }
    for proxy in program.instant_proxies() {
        for route in &proxy.routes {
            for step in &route.steps {
                requested_guards[step.delivery.guard.index()] = true;
            }
        }
    }
    for contact in program.contacts() {
        for variant in &contact.variants {
            requested_guards[variant.when.index()] = true;
        }
    }
    for link in program.power_links() {
        requested_guards[link.guard.index()] = true;
    }
    let guard_roots: Vec<_> = (0..program.guards().len())
        .map(|index| requested_guards[index] && callback_guards.compile(GuardId::new(index as u32), 0).is_some())
        .collect();
    let compiled_guards = guard_roots.iter().filter(|&&supported| supported).count();
    let guard_kinds: Vec<_> = guard_roots
        .iter()
        .enumerate()
        .map(|(id, &supported)| {
            if !supported {
                0
            } else if program.guards()[id] == Guard::Always {
                2
            } else {
                match callback_guards.bodies.get(&id).map(String::as_str) {
                    Some("0") => 1,
                    Some("1") => 2,
                    _ => 3,
                }
            }
        })
        .collect();
    for (id, body) in callback_guards.bodies {
        if let Some(existing) = compiler.guards.bodies.get(&id) {
            if existing != &body {
                return Err(NativeQueryError::new("captured and live guard lowering disagree"));
            }
        } else {
            compiler.guards.bodies.insert(id, body);
        }
    }
    let plans: Vec<_> = compiler.plans.into_values().collect();
    let mut source = generate_c(&plans, &compiler.guards.bodies, true);
    writeln!(
        source,
        "static int (*const ne_guard_functions[{}])(const QueryState *) = {{",
        guard_roots.len().max(1)
    )
    .unwrap();
    if guard_roots.is_empty() {
        source.push_str("0,\n");
    }
    for (id, &kind) in guard_kinds.iter().enumerate() {
        if kind != 3 {
            source.push_str("0,\n");
        } else {
            writeln!(source, "guard_{id},").unwrap();
        }
    }
    source.push_str("};\n");
    writeln!(source, "static const uint8_t ne_guard_kinds[{}] = {{", guard_kinds.len().max(1)).unwrap();
    if guard_kinds.is_empty() {
        source.push('0');
    }
    for kind in &guard_kinds {
        write!(source, "{kind},").unwrap();
    }
    writeln!(source, "}};\nstatic uint8_t ne_compiled_guard(NEStateView state, uint32_t guard, uint8_t *handled) {{ *handled = 0; if (guard >= {} || ne_guard_kinds[guard] == 0) return 0; *handled = 1; if (ne_guard_kinds[guard] < 3) return ne_guard_kinds[guard] == 2; return ne_guard_functions[guard](&state) != 0; }}", guard_roots.len()).unwrap();
    for (name, values) in [("ne_compiled_inputs", &inputs), ("ne_compiled_conductors", &conductors)] {
        write!(source, "static const u32 {name}[{}] = {{", values.len().max(1)).unwrap();
        if values.is_empty() {
            source.push('0');
        }
        for id in values {
            write!(source, "{id},").unwrap();
        }
        source.push_str("};\n");
    }
    writeln!(
        source,
        "static uint8_t ne_compiled_power(NEStateView state, uint8_t kind, uint32_t target, uint8_t port, uint8_t *handled) {{
        *handled = 0;
        u32 id;
        if (kind == NE_TARGET_INPUT && target < {} && port < 3) id = ne_compiled_inputs[target * 3 + port];
        else if (kind == NE_TARGET_CONDUCTOR && target < {} && port < 2) id = ne_compiled_conductors[target * 2 + port];
        else return 0;
        if (id == 4294967295u) return 0;
        *handled = 1;
        if (id == 0) return 0;
        return mchprs_query(id, &state);
    }}",
        program.nodes().len(),
        program.sites().len()
    )
    .unwrap();
    Ok((source, supported, grouped.len(), compiled_guards))
}

fn generate_c(plans: &[QueryPlan], guards: &BTreeMap<usize, String>, compact: bool) -> String {
    let mut source = String::from("typedef unsigned char u8; typedef unsigned int u32;\n");
    source.push_str(if compact {
        "typedef NEStateView QueryState;\n"
    } else {
        "typedef struct { const u8 *components; const u8 *pistons; const u8 *resources; const u8 *classic_signals; } QueryState;\n"
    });
    source.push_str(
        "#if defined(_WIN32)\n#define EXPORT __declspec(dllexport)\n#else\n#define EXPORT __attribute__((visibility(\"default\")))\n#endif\n",
    );
    for plan in plans {
        source.push_str(&format!("static u8 query_{}(const QueryState *s);\n", plan.id));
    }
    for (index, body) in guards {
        if body != "0" && body != "1" {
            source.push_str(&format!("static int guard_{index}(const QueryState *s);\n"));
        }
    }
    for (index, body) in guards {
        if body != "0" && body != "1" {
            source.push_str(&format!("static int guard_{index}(const QueryState *s) {{ return {body}; }}\n"));
        }
    }
    for plan in plans {
        source.push_str(&format!("static u8 query_{}(const QueryState *s) {{ u8 m = 0;\n", plan.id));
        for (index, link) in plan.links.iter().enumerate() {
            source.push_str(&format!(
                "if ({}) {{ u8 raw_{index} = (u8)({}); if (raw_{index} > {}) {{ u8 value_{index} = (u8)(raw_{index} - {}); if (value_{index} > m) m = value_{index}; }} }} if (m == 15) return m;\n",
                link.guard, link.source, link.attenuation, link.attenuation
            ));
        }
        source.push_str("return m; }\n");
    }
    if compact {
        let count = plans.iter().map(|plan| plan.id as usize + 1).max().unwrap_or(1);
        source.push_str(&format!("static u8 (*const query_functions[{count}])(const QueryState *) = {{\n"));
        for plan in plans {
            source.push_str(&format!("[{}] = query_{},\n", plan.id, plan.id));
        }
        source.push_str(&format!(
            "}};\nEXPORT u8 mchprs_query(u32 target, const QueryState *s) {{ if (target < {count} && query_functions[target]) return query_functions[target](s); return 0; }}\n"
        ));
    } else {
        source.push_str("EXPORT u8 mchprs_query(u32 target, const QueryState *s) { switch (target) {\n");
        for plan in plans {
            source.push_str(&format!("case {}: return query_{}(s);\n", plan.id, plan.id));
        }
        source.push_str("default: return 0; } }\n");
    }
    source
}

#[cfg(windows)]
fn compile_windows(clang: &Path, linker: &Path, source: &Path, library: &Path) -> Result<(), NativeQueryError> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(NativeQueryError("the Windows native demo currently targets x86_64".into()));
    }
    let object = library.with_extension("obj");
    run_tool(
        Command::new(clang)
            .args(["--target=x86_64-pc-windows-msvc", "-O3", "-c"])
            .arg(source)
            .arg("-o")
            .arg(&object),
        "Clang C compilation",
    )?;
    let result = run_tool(
        Command::new(linker)
            .args(["/dll", "/noentry", "/nodefaultlib", "/machine:x64", "/export:mchprs_query"])
            .arg(format!("/out:{}", library.display()))
            .arg(&object),
        "lld-link DLL creation",
    );
    let _ = fs::remove_file(object);
    result
}

#[cfg(not(windows))]
fn compile_unix(clang: &Path, source: &Path, library: &Path) -> Result<(), NativeQueryError> {
    run_tool(
        Command::new(clang).args(["-O3", "-shared", "-fPIC"]).arg(source).arg("-o").arg(library),
        "Clang shared-library creation",
    )
}

fn run_tool(command: &mut Command, name: &str) -> Result<(), NativeQueryError> {
    let output = command.output().map_err(|error| NativeQueryError(format!("start {name}: {error}")))?;
    if !output.status.success() {
        return Err(NativeQueryError(format!(
            "{name} failed ({}): {}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_wire_direction_is_only_compiled_when_shape_is_static() {
        let wire = Block::RedstoneWire {
            wire: RedstoneWire::default(),
        };
        assert_eq!(
            source_emission(SiteSource::Component(NodeId::new(0)), wire, PowerPort::Weak(BlockFace::Top)),
            Some(true)
        );
        assert_eq!(
            source_emission(SiteSource::Component(NodeId::new(0)), wire, PowerPort::Weak(BlockFace::Bottom)),
            Some(false)
        );
        assert_eq!(
            source_emission(SiteSource::Component(NodeId::new(0)), wire, PowerPort::Weak(BlockFace::North)),
            None
        );
        assert_eq!(
            source_emission(SiteSource::Constant(ProfileId::new(0)), wire, PowerPort::Weak(BlockFace::Top)),
            Some(true)
        );
    }

    #[test]
    fn measured_state_offsets_and_guard_bytes_match_live_fields() {
        let state = CircuitState {
            classic_signals: Box::new([]),
            components: Box::new([]),
            pistons: vec![PistonState {
                extended: true,
                head_present: false,
                availability: PistonAvailability::BasePending,
                revision: 0,
            }]
            .into_boxed_slice(),
            resources: vec![ResourceState {
                at: ContactId::new(0x1020_3040),
                enabled: true,
                revision: 0,
            }]
            .into_boxed_slice(),
        };
        let extended = bool_piston_field(&state, PistonId::new(0), |item| &item.extended).unwrap();
        let enabled = bool_resource_field(&state, ResourceId::new(0), |item| &item.enabled).unwrap();
        assert_eq!(unsafe { *state.pistons.as_ptr().cast::<u8>().add(extended) }, 1);
        assert_eq!(unsafe { *state.resources.as_ptr().cast::<u8>().add(enabled) }, 1);
        assert!(piston_availability_tag(PistonAvailability::BasePending).is_some());
        assert_eq!(contact_id_bytes(ContactId::new(0x1020_3040)).unwrap(), unsafe {
            std::array::from_fn(|index| *(&state.resources[0].at as *const ContactId).cast::<u8>().add(index))
        });
        let source = StateTest::ResourceEnabled(ResourceId::new(0));
        let offset = match source {
            StateTest::ResourceEnabled(resource) => bool_resource_field(&state, resource, |item| &item.enabled).unwrap(),
            _ => unreachable!(),
        };
        let c = format!("s->resources[{offset}] != 0");
        assert!(c.contains("resources["));
    }

    #[test]
    fn c_emits_guarded_live_sources_with_max_and_saturating_attenuation() {
        let plan = QueryPlan {
            id: 7,
            links: vec![
                NativeLink {
                    guard: "guard_3(s)".into(),
                    source: "s->components[4]".into(),
                    attenuation: 0,
                },
                NativeLink {
                    guard: "1".into(),
                    source: "query_9(s)".into(),
                    attenuation: 2,
                },
            ],
        };
        let source = generate_c(
            &[plan, QueryPlan { id: 9, links: Vec::new() }],
            &BTreeMap::from([(3, "s->resources[2] != 0".into())]),
            false,
        );
        assert!(source.contains("case 7: return query_7(s)"));
        assert!(source.contains("if (guard_3(s))"));
        assert!(source.contains("if (raw_0 > 0)"));
        assert!(source.contains("if (raw_1 > 2)"));
        assert!(source.contains("if (value_1 > m) m = value_1"));
        assert!(source.contains("s->components[4]"));
        assert!(source.contains("query_9(s)"));
    }

    #[test]
    fn dense_target_bindings_keep_input_and_conductor_roots_separate() {
        assert_eq!(input_port_index(InputPort::Main), 0);
        assert_eq!(input_port_index(InputPort::Side), 1);
        assert_eq!(input_port_index(InputPort::Quasi), 2);
        assert_eq!(SiteId::new(4).index() * 2 + usize::from(false), 8);
        assert_eq!(SiteId::new(4).index() * 2 + usize::from(true), 9);
    }

    #[test]
    fn compact_query_dispatch_accepts_sparse_ids_without_a_large_switch() {
        let source = generate_c(&[QueryPlan { id: 7, links: Vec::new() }], &BTreeMap::new(), true);
        assert!(source.contains("query_functions[8]"));
        assert!(source.contains("[7] = query_7,"));
        assert!(source.contains("target < 8 && query_functions[target]"));
        assert!(!source.contains("switch (target)"));
    }

    #[test]
    fn callback_guard_lowering_declines_transitive_captured_operands() {
        use crate::world::World;
        use mchprs_blocks::blocks::{Lever, LeverFace};
        let mut world = crate::plot::PlotWorld::from_chunks(0, 0, vec![crate::world::storage::Chunk::empty(0, 0)], Default::default());
        world.set_block(
            BlockPos::new(1, 1, 1),
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
            },
        );
        let mut draft = crate::redpiler::passes::rich::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(3, 3, 3)),
            vec![],
            &crate::redpiler::TaskMonitor::default(),
            crate::redpiler::analysis::AnalysisLimits::default(),
        )
        .unwrap()
        .graph;
        assert!(!draft.sites.is_empty());
        let captured = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::Site {
            site: SiteId::new(0),
            view: OperandView::Captured,
            test: SiteTest::IsWire,
        });
        let negated = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::Not(captured));
        let live = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::Site {
            site: SiteId::new(0),
            view: OperandView::Live,
            test: SiteTest::IsWire,
        });
        let always = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::Always);
        let all = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::All(vec![always, live].into_boxed_slice()));
        let any = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::Any(vec![always, live].into_boxed_slice()));
        let captured_any = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::Any(vec![always, captured].into_boxed_slice()));
        let (program, state) = draft.lower().unwrap();
        let mut power = GuardCompiler::new(&program, &state);
        assert_eq!(power.compile(captured, 0).as_deref(), Some("0"));
        assert_eq!(power.compile(negated, 0).as_deref(), Some("1"));
        let mut callback = GuardCompiler::new(&program, &state);
        callback.captured_is_false = false;
        assert!(callback.compile(captured, 0).is_none());
        assert!(callback.compile(negated, 0).is_none());
        assert_eq!(callback.compile(live, 0).as_deref(), Some("0"));
        assert_eq!(callback.compile(all, 0).as_deref(), Some("0"));
        assert_eq!(callback.compile(any, 0).as_deref(), Some("1"));
        assert!(callback.compile(captured_any, 0).is_none());
        let source = generate_c(&[], &callback.bodies, true);
        assert!(!source.contains(&format!("guard_{}(", live.index())));
    }
}
