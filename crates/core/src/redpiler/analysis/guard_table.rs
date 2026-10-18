//! Fold immutable live geometry while constructing electrical guards, before memory admission.
//! Captured operands retain their presence checks; movable contacts and signal state stay dynamic.
use crate::redpiler::program_graph::{CircuitNodeDraft, Guard, GuardId, OperandView, Profile, ProfileId, SiteSource, SiteSpec, SiteTest};
use mchprs_blocks::blocks::Block;
use std::ops::Deref;

#[derive(Clone, Copy)]
struct FixedSite {
    solid: bool,
    transparent: bool,
    wire: bool,
    profile: Option<ProfileId>,
}

pub(super) struct PreparationGuards {
    guards: Vec<Guard>,
    // Only immutable geometry, never component power or contact occupants.
    sites: Vec<Option<FixedSite>>,
    never: Option<GuardId>,
    child_bytes: usize,
}

impl PreparationGuards {
    pub(super) fn new(sites: &[SiteSpec], profiles: &[Profile], nodes: &[CircuitNodeDraft]) -> Self {
        let sites = sites
            .iter()
            .map(|site| {
                let (block, profile, solid, transparent) = match site.source {
                    SiteSource::Contact(_) => return None,
                    SiteSource::Constant(profile) => {
                        let properties = &profiles[profile.index()];
                        (
                            Block::from_id(properties.raw_state),
                            Some(profile),
                            properties.solid,
                            properties.transparent,
                        )
                    }
                    SiteSource::Component(node) => {
                        let block = Block::from_id(nodes[node.index()].raw_state?);
                        (block, None, block.is_solid(), block.is_transparent())
                    }
                };
                Some(FixedSite {
                    solid,
                    transparent,
                    wire: matches!(block, Block::RedstoneWire { .. }),
                    profile,
                })
            })
            .collect();
        Self {
            guards: vec![Guard::Always],
            sites,
            never: None,
            child_bytes: 0,
        }
    }

    fn constant(&mut self, value: bool) -> GuardId {
        if value {
            return GuardId::new(0);
        }
        if let Some(id) = self.never {
            return id;
        }
        let id = self.append(Guard::Not(GuardId::new(0)));
        self.never = Some(id);
        id
    }

    pub(super) fn push(&mut self, guard: Guard) -> GuardId {
        match guard {
            Guard::Always => GuardId::new(0),
            Guard::Site {
                site,
                view: OperandView::Live,
                test,
            } => {
                if let Some(Some(fixed)) = self.sites.get(site.index()) {
                    let value = match test {
                        SiteTest::Solid => fixed.solid,
                        SiteTest::Transparent => fixed.transparent,
                        SiteTest::IsWire => fixed.wire,
                        SiteTest::Profile(profile) => fixed.profile == Some(profile),
                    };
                    self.constant(value)
                } else {
                    self.append(Guard::Site {
                        site,
                        view: OperandView::Live,
                        test,
                    })
                }
            }
            Guard::Not(child) if child == GuardId::new(0) => self.constant(false),
            Guard::Not(child) if Some(child) == self.never => GuardId::new(0),
            Guard::All(children) => self.join(children, true),
            Guard::Any(children) => self.join(children, false),
            guard => self.append(guard),
        }
    }

    fn join(&mut self, children: Box<[GuardId]>, all: bool) -> GuardId {
        let always = GuardId::new(0);
        if children
            .iter()
            .any(|child| if all { Some(*child) == self.never } else { *child == always })
        {
            return self.constant(!all);
        }
        let children = children
            .into_vec()
            .into_iter()
            .filter(|child| if all { *child != always } else { Some(*child) != self.never })
            .collect::<Vec<_>>();
        match children.as_slice() {
            [] => self.constant(all),
            [only] => *only,
            _ => self.append(if all {
                Guard::All(children.into_boxed_slice())
            } else {
                Guard::Any(children.into_boxed_slice())
            }),
        }
    }

    fn append(&mut self, guard: Guard) -> GuardId {
        if let Guard::All(children) | Guard::Any(children) = &guard {
            self.child_bytes = self.child_bytes.saturating_add(std::mem::size_of_val(children.as_ref()));
        }
        let id = GuardId::new(self.guards.len() as u32);
        self.guards.push(guard);
        id
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.guards
            .capacity()
            .saturating_mul(std::mem::size_of::<Guard>())
            .saturating_add(self.child_bytes)
    }

    pub(super) fn into_vec(self) -> Vec<Guard> {
        self.guards
    }
}

impl Deref for PreparationGuards {
    type Target = Vec<Guard>;
    fn deref(&self) -> &Self::Target {
        &self.guards
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::program_graph::{ContactId, SiteId};
    use mchprs_blocks::BlockPos;

    #[test]
    fn folds_live_geometry_but_preserves_capture_and_contact_presence() {
        let profile = Profile {
            raw_state: Block::Air.get_id(),
            registry_type: 0,
            solid: false,
            transparent: true,
            cube: false,
            electrical: crate::redpiler::program_graph::ProfileElectrical::None,
        };
        let sites = [
            SiteSpec {
                position: BlockPos::new(0, 0, 0),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 0,
            },
            SiteSpec {
                position: BlockPos::new(1, 0, 0),
                source: SiteSource::Contact(ContactId::new(0)),
                source_strength: 0,
            },
        ];
        let mut guards = PreparationGuards::new(&sites, &[profile], &[]);
        let test = |site, view, test| Guard::Site {
            site: SiteId::new(site),
            view,
            test,
        };
        let never = guards.push(test(0, OperandView::Live, SiteTest::Solid));
        assert_ne!(never, GuardId::new(0));
        assert_eq!(guards.push(test(0, OperandView::Live, SiteTest::Transparent)), GuardId::new(0));
        assert_eq!(
            guards.push(test(0, OperandView::Live, SiteTest::Profile(ProfileId::new(0)))),
            GuardId::new(0)
        );
        assert_eq!(guards.push(Guard::Not(never)), GuardId::new(0));
        let captured = guards.push(test(0, OperandView::Captured, SiteTest::Transparent));
        let contact = guards.push(test(1, OperandView::Live, SiteTest::Solid));
        assert!(matches!(
            guards[captured.index()],
            Guard::Site {
                view: OperandView::Captured,
                ..
            }
        ));
        assert!(matches!(guards[contact.index()], Guard::Site { .. }));
        assert_eq!(guards.push(Guard::All(vec![GuardId::new(0), contact].into_boxed_slice())), contact);
        assert_eq!(guards.push(Guard::All(vec![captured, never].into_boxed_slice())), never);
        assert_eq!(guards.push(Guard::Any(vec![never, captured].into_boxed_slice())), captured);
        let before = guards.retained_bytes();
        guards.push(Guard::All(vec![contact; 1024].into_boxed_slice()));
        assert!(guards.retained_bytes() >= before + 1024 * std::mem::size_of::<GuardId>());
    }
}
