//! AOT symbolic semantics of the prepared graph, not concrete trajectory sampling.
//!
//! Mutable bindings are compiler-only SSA values. Later reads observe conditional writes;
//! captures retain their earlier expressions. Executable kernels will keep only live roots.
pub(crate) mod aot;
pub(crate) mod cycle;
pub(crate) mod entry;
pub(crate) mod execution;
mod power;
mod routes;
mod shape;
mod state;
mod walk;
mod wire;
pub(crate) mod work;
use super::*;
use mchprs_blocks::blocks::{Block, RedstoneWire, RedstoneWireSide};
use mchprs_blocks::{BlockDirection, BlockFace};
use mchprs_redpiler_opt::{
    Limits,
    symbolic::{Diagram, Formula, Signal, SymbolicError},
};
pub(crate) use routes::{Delivery, Receivers};
use rustc_hash::{FxHashMap, FxHashSet};
pub(crate) use state::*;

type Result<T> = std::result::Result<T, CompileError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompileError {
    /// This symbolic operation exceeded its budget or rejected invalid values.
    Logic(SymbolicError),
    /// The ready-state equations contradict each other, so this candidate has no admissible entry.
    InconsistentEntry,
    /// This node contains host-owned data that this symbolic semantic family does not model.
    UnsupportedNode(NodeId),
    /// This power dependency recurses without a certified transfer relation.
    RecursivePower(PowerTarget),
    /// This resource cannot occupy the requested contact in the prepared graph.
    InvalidResourceContact {
        /// This is the shared payload whose allowed footprint was exceeded.
        resource: ResourceId,
        /// This is the requested position absent from that payload's certified contacts.
        contact: ContactId,
    },
}

impl From<SymbolicError> for CompileError {
    fn from(value: SymbolicError) -> Self {
        Self::Logic(value)
    }
}

pub(crate) struct Compiler<'a> {
    /// These immutable tables supply geometry, guards and ordered routes during compilation only.
    graph: &'a CompileGraph,
    /// This canonical Boolean arena reduces signal/state expressions over all input valuations.
    pub(crate) logic: Diagram,
    /// These bindings hold the current symbolic component, actuator and resource values.
    pub(crate) state: State,
    /// This generation prevents reuse of live query results across symbolic state writes.
    epoch: u64,
    /// These live guard results are cached only for the state generation that produced them.
    guards: Vec<Option<(u64, Formula)>>,
    /// These live power results are cached only for the state generation that produced them.
    power: FxHashMap<PowerTarget, (u64, Signal)>,
    /// This catches unsupported recursive conduction instead of recursing indefinitely.
    active_power: FxHashSet<PowerTarget>,
    /// These immutable incoming-link indices avoid discovering electrical topology while tracing.
    power_inputs: FxHashMap<PowerTarget, Vec<usize>>,
    /// This bounds semantic visits even when all Boolean expressions fold to constants.
    remaining_work: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SiteView {
    /// These mutually exclusive alternatives freeze profile and receiver selection at capture time.
    pub(crate) variants: Vec<SiteVariant>,
}

#[derive(Debug, Clone)]
pub(crate) struct SiteVariant {
    /// This formula selects this particular profile/receiver for the captured input valuation.
    pub(crate) when: Formula,
    /// This is the profile identity; fixed electrical components have no selectable profile.
    pub(crate) profile: Option<ProfileId>,
    /// This supplies static material and direction facts without reading a World.
    pub(crate) block: Block,
    /// This retains the compiled solidity fact, including a selected contact profile.
    pub(crate) solid: bool,
    /// This retains the compiled transparency fact, including a selected contact profile.
    pub(crate) transparent: bool,
    /// This retains the receiver selected by this alternative at capture time.
    pub(crate) receiver: Option<CallbackReceiver>,
    /// This freezes component properties when the selected receiver is a component.
    pub(crate) component: Option<Component>,
}

/// A capture owns expressions rather than concrete observations; nested walks use separate maps.
pub(crate) type Capture = FxHashMap<SiteId, SiteView>;

impl<'a> Compiler<'a> {
    /// The caller must supply the validated draft used by the compiler pipeline.
    pub(crate) fn new(graph: &'a CompileGraph, limits: Limits) -> Result<Self> {
        let mut power_inputs: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for (index, link) in graph.power_links.iter().enumerate() {
            power_inputs.entry(link.to).or_default().push(index);
        }
        Ok(Self {
            graph,
            logic: Diagram::new(limits)?,
            state: State::initial(graph)?,
            epoch: 0,
            guards: vec![None; graph.guards.len()],
            power: FxHashMap::default(),
            active_power: FxHashSet::default(),
            power_inputs,
            remaining_work: limits.max_work,
        })
    }

    fn step(&mut self) -> Result<()> {
        self.remaining_work = self.remaining_work.checked_sub(1).ok_or(SymbolicError::BudgetExceeded)?;
        Ok(())
    }

    pub(crate) fn remaining_work(&self) -> usize {
        self.remaining_work
    }

    /// Every semantic write invalidates live reads; captured expressions remain unchanged.
    pub(crate) fn state_written(&mut self) {
        self.epoch = self.epoch.checked_add(1).expect("symbolic work budget bounds state generations");
    }

    pub(crate) fn guard(&mut self, id: GuardId, captured: Option<&Capture>) -> Result<Formula> {
        if captured.is_none()
            && let Some((epoch, value)) = self.guards[id.index()]
            && epoch == self.epoch
        {
            return Ok(value);
        }
        let graph = self.graph;
        let value = match &graph.guards[id.index()] {
            Guard::Always => Formula::TRUE,
            Guard::State(test) => match *test {
                StateTest::PistonExtended(actor) => self.state.actors[actor.index()].extended,
                StateTest::PistonHeadPresent(actor) => self.state.actors[actor.index()].head,
                StateTest::PistonAvailability { piston, is } => self.state.actors[piston.index()].availability[is as usize],
                StateTest::ResourceAt { resource, contact } => self.state.resources[resource.index()].at(contact),
                StateTest::ResourceEnabled(resource) => self.state.resources[resource.index()].enabled,
            },
            Guard::Not(child) => {
                let child = self.guard(*child, captured)?;
                self.logic.not(child)?
            }
            Guard::All(children) => {
                let mut result = Formula::TRUE;
                for &child in children.iter() {
                    let value = self.guard(child, captured)?;
                    result = self.logic.and(result, value)?;
                }
                result
            }
            Guard::Any(children) => {
                let mut result = Formula::FALSE;
                for &child in children.iter() {
                    let value = self.guard(child, captured)?;
                    result = self.logic.or(result, value)?;
                }
                result
            }
            Guard::Site { site, view, test } => {
                let view = match view {
                    OperandView::Live => Some(self.observe(*site)?),
                    OperandView::Captured => captured.and_then(|capture| capture.get(site)).cloned(),
                };
                match view {
                    Some(view) => self.site_test(&view, |variant| match test {
                        SiteTest::Solid => variant.solid,
                        SiteTest::Transparent => variant.transparent,
                        SiteTest::IsWire => matches!(variant.block, Block::RedstoneWire { .. }),
                        SiteTest::Profile(profile) => variant.profile == Some(*profile),
                    })?,
                    None => Formula::FALSE,
                }
            }
        };
        if captured.is_none() {
            self.guards[id.index()] = Some((self.epoch, value));
        }
        Ok(value)
    }

    pub(crate) fn observe(&mut self, site: SiteId) -> Result<SiteView> {
        let graph = self.graph;
        let mut variants = Vec::new();
        match graph.sites[site.index()].source {
            SiteSource::Component(node) => {
                let block = Block::from_id(graph.nodes[node.index()].raw_state.ok_or(CompileError::UnsupportedNode(node))?);
                variants.push(SiteVariant {
                    when: Formula::TRUE,
                    profile: None,
                    block,
                    solid: block.is_solid(),
                    transparent: block.is_transparent(),
                    receiver: Some(CallbackReceiver::Component(node)),
                    component: Some(self.state.components[node.index()].clone()),
                });
            }
            SiteSource::Constant(profile) => variants.push(self.profile_view(profile, None, Formula::TRUE)),
            SiteSource::Contact(contact) => {
                let mut remaining = Formula::TRUE;
                for variant in graph.contacts[contact.index()].variants.iter() {
                    let matches = self.guard(variant.when, None)?;
                    let selected = self.logic.and(remaining, matches)?;
                    if selected != Formula::FALSE {
                        variants.push(self.profile_view(variant.profile, variant.receiver, selected));
                    }
                    let excluded = self.logic.not(matches)?;
                    remaining = self.logic.and(remaining, excluded)?;
                }
            }
        }
        Ok(SiteView { variants })
    }

    fn profile_view(&self, profile: ProfileId, receiver: Option<CallbackReceiver>, when: Formula) -> SiteVariant {
        SiteVariant {
            when,
            profile: Some(profile),
            block: Block::from_id(self.graph.profiles[profile.index()].raw_state),
            receiver,
            solid: self.graph.profiles[profile.index()].solid,
            transparent: self.graph.profiles[profile.index()].transparent,
            component: match receiver {
                Some(CallbackReceiver::Component(node)) => Some(self.state.components[node.index()].clone()),
                _ => None,
            },
        }
    }

    pub(crate) fn site_test(&mut self, view: &SiteView, predicate: impl Fn(&SiteVariant) -> bool) -> Result<Formula> {
        let mut result = Formula::FALSE;
        for variant in &view.variants {
            if predicate(variant) {
                result = self.logic.or(result, variant.when)?;
            }
        }
        Ok(result)
    }
}
