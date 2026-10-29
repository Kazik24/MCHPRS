use super::*;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::program_graph::{Guard, GuardId, OperandView, ProfileId, SiteTest};

fn old_captured_profile_test(
    backend: &GraphBackend,
    site: SiteId,
    profile: ProfileId,
    captures: Option<&[Option<ObservedSite>]>,
    captured_site: Option<(SiteId, &ObservedSite)>,
) -> bool {
    let observed = captured_site
        .filter(|(id, _)| *id == site)
        .map(|(_, observed)| observed)
        .or_else(|| captures.and_then(|sites| sites.get(site.index())).and_then(Option::as_ref));
    observed
        .and_then(|observed| backend.resolve_observed_site(observed))
        .is_some_and(|resolved| resolved.profile == Some(profile))
}

#[test]
fn captured_profile_guards_match_the_resolver_snapshot() {
    let (world, bounds) = crate::redpiler::test_fixtures::piston_reset_fixture();
    let monitor = TaskMonitor::default();
    let mut graph = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default())
        .unwrap()
        .graph;
    let mut cases = Vec::new();
    for site_index in 0..graph.sites.len() {
        let site = SiteId::new(site_index as u32);
        for profile_index in 0..graph.profiles.len() {
            let profile = ProfileId::new(profile_index as u32);
            let guard = GuardId::try_from_index(graph.guards.len()).unwrap();
            graph.guards.push(Guard::Site {
                site,
                view: OperandView::Captured,
                test: SiteTest::Profile(profile),
            });
            cases.push((site, profile, guard));
        }
    }
    let not_guard = GuardId::try_from_index(graph.guards.len()).unwrap();
    graph.guards.push(Guard::Not(cases[0].2));
    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let captures = (0..backend.program.sites().len())
        .map(|index| backend.observe_site(SiteId::new(index as u32)))
        .collect::<Vec<_>>();
    let profiles = captures
        .iter()
        .filter_map(|observed| observed.as_ref().and_then(|observed| observed.profile))
        .collect::<rustc_hash::FxHashSet<_>>();
    assert!(!profiles.is_empty());
    assert!(backend.program.sites().iter().any(|site| matches!(site.source, SiteSource::Component(_))));
    assert!(backend.program.sites().iter().any(|site| matches!(site.source, SiteSource::Constant(_))));
    assert!(backend.program.sites().iter().any(|site| matches!(site.source, SiteSource::Contact(_))));

    // Captures retain their selected IDs even if live piston/resource state changes.
    for piston in &mut backend.state.pistons {
        piston.extended = !piston.extended;
        piston.head_present = !piston.head_present;
    }
    for (index, resource) in backend.state.resources.iter_mut().enumerate() {
        resource.enabled = !resource.enabled;
        if let Some(&contact) = backend.program.resources()[index].contacts.last() {
            resource.at = contact;
        }
        resource.revision = resource.revision.wrapping_add(1);
    }

    for &(site, profile, guard) in &cases {
        let expected = old_captured_profile_test(&backend, site, profile, Some(&captures), None);
        assert_eq!(
            backend.guard_matches_using(guard, Some(&captures), None),
            expected,
            "site {site:?}, profile {profile:?}"
        );
    }

    let captured_site = captures
        .iter()
        .enumerate()
        .find_map(|(index, observed)| {
            observed
                .as_ref()
                .and_then(|observed| observed.profile.map(|_| (SiteId::new(index as u32), observed)))
        })
        .unwrap();
    let (site, captured) = captured_site;
    let site_guard = cases
        .iter()
        .find(|(candidate, profile, _)| *candidate == site && Some(*profile) == captured.profile)
        .unwrap()
        .2;
    let mismatched = captures
        .iter()
        .enumerate()
        .find_map(|(index, observed)| {
            observed
                .as_ref()
                .filter(|observed| observed.profile != captured.profile)
                .map(|observed| (SiteId::new(index as u32), observed))
        })
        .unwrap();
    assert!(old_captured_profile_test(
        &backend,
        site,
        captured.profile.unwrap(),
        Some(&captures),
        None
    ));
    assert!(backend.guard_matches_using(site_guard, Some(&captures), Some(mismatched)));
    assert!(!backend.guard_matches_using(site_guard, Some(&captures), Some((site, mismatched.1))));

    let no_capture = vec![None; captures.len()];
    assert!(!backend.guard_matches_using(site_guard, Some(&no_capture), None));
    assert!(backend.guard_matches_using(not_guard, Some(&no_capture), None));
}
