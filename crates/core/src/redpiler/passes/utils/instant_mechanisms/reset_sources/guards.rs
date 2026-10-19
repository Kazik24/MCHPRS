//! Partial guard evaluation for electrical reachability and stationary callback receivers.
use super::*;

pub(super) struct Facts<'a> {
    /// This is the immutable source of every predicate.
    graph: &'a CompileGraph,
    /// This optional actor is ready and retracted at the reset-sampling cut.
    ready_actor: Option<PistonId>,
    /// These facts are cached only within this one evaluation context.
    values: FxHashMap<GuardId, Option<bool>>,
}

impl<'a> Facts<'a> {
    pub(super) fn electrical(graph: &'a CompileGraph) -> Self {
        Self {
            graph,
            ready_actor: None,
            values: FxHashMap::default(),
        }
    }

    pub(super) fn ready(graph: &'a CompileGraph, actor: PistonId) -> Self {
        Self {
            graph,
            ready_actor: Some(actor),
            values: FxHashMap::default(),
        }
    }

    pub(super) fn value(&mut self, guard: GuardId) -> Option<bool> {
        self.evaluate(guard, 0)
    }

    fn evaluate(&mut self, guard: GuardId, depth: usize) -> Option<bool> {
        if let Some(&value) = self.values.get(&guard) {
            return value;
        }
        if depth >= 128 {
            return None;
        }
        let value = match &self.graph.guards[guard.index()] {
            Guard::Always => Some(true),
            Guard::State(test) => match *test {
                StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) if Some(actor) == self.ready_actor => Some(false),
                StateTest::PistonAvailability { piston, is } if Some(piston) == self.ready_actor => Some(is == PistonAvailability::Ready),
                _ => None,
            },
            Guard::Not(child) => self.evaluate(*child, depth + 1).map(|value| !value),
            Guard::All(children) | Guard::Any(children) => {
                let all = matches!(self.graph.guards[guard.index()], Guard::All(_));
                let mut unknown = false;
                for &child in children {
                    match self.evaluate(child, depth + 1) {
                        Some(value) if value != all => return Some(!all),
                        None => unknown = true,
                        _ => {}
                    }
                }
                (!unknown).then_some(all)
            }
            Guard::Site {
                view: OperandView::Captured, ..
            } if self.ready_actor.is_none() => Some(false),
            Guard::Site { site, test, .. } => self.site(*site, *test, depth + 1),
        };
        self.values.insert(guard, value);
        value
    }

    fn site(&mut self, site: SiteId, test: SiteTest, depth: usize) -> Option<bool> {
        let source = self.graph.sites[site.index()].source;
        if let Some(actor) = self.ready_actor {
            let spec = self.graph.pistons[actor.index()];
            if source == SiteSource::Contact(spec.base) || source == SiteSource::Component(spec.node) {
                // Piston nodes need no raw block: their immutable specification supplies the pose.
                let block = Block::Piston {
                    piston: mchprs_blocks::blocks::RedstonePiston {
                        facing: spec.facing.into(),
                        sticky: spec.sticky,
                        extended: false,
                    },
                };
                return Some(match test {
                    SiteTest::Profile(profile) => self.graph.profiles[profile.index()].raw_state == block.get_id(),
                    _ => block_matches(block, test),
                });
            }
        }
        match source {
            SiteSource::Constant(profile) => Some(self.profile(profile, test)),
            SiteSource::Component(node) => {
                let node = &self.graph.nodes[node.index()];
                let block = match node.state {
                    ComponentState::Wire(wire) => Block::RedstoneWire { wire },
                    _ => Block::from_id(node.raw_state?),
                };
                match test {
                    SiteTest::Profile(_) => None,
                    _ => Some(block_matches(block, test)),
                }
            }
            SiteSource::Contact(contact) => {
                for variant in &self.graph.contacts[contact.index()].variants {
                    match self.evaluate(variant.when, depth + 1) {
                        Some(false) => continue,
                        Some(true) => return Some(self.profile(variant.profile, test)),
                        None => return None,
                    }
                }
                None
            }
        }
    }

    fn profile(&self, profile: ProfileId, test: SiteTest) -> bool {
        match test {
            SiteTest::Profile(expected) => expected == profile,
            _ => block_matches(Block::from_id(self.graph.profiles[profile.index()].raw_state), test),
        }
    }
}

fn block_matches(block: Block, test: SiteTest) -> bool {
    match test {
        SiteTest::Solid => block.is_solid(),
        SiteTest::Transparent => block.is_transparent(),
        SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
        SiteTest::Profile(_) => false,
    }
}
