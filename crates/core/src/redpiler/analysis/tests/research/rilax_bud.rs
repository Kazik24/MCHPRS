//! Narrow native trace for the Rilax BUD storage/read pair.
use super::*;
use crate::redpiler::passes::utils::instant_mechanisms::index_power;
use crate::redpiler::program_graph::{InputPort, PowerTarget, SiteSource};
use crate::redstone::piston::trace::{self, Operation, Stage};

#[test]
#[ignore = "research: one Rilax memory cell's native write/read timing and raw power sources"]
fn rilax_single_cell_bud_write_and_read_trace() {
    let fixture = manifest("rilax_memory_bank_bud");
    let (mut world, bounds) = load(&fixture);
    let storage_pos = memory_pos(0, 0);
    let reader_pos = fixture["observations"]["read_gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(local_pos)
        .find(|pos| pos.x == storage_pos.x && pos.z == storage_pos.z && pos.y == storage_pos.y - 4)
        .unwrap();
    assert!(
        fixture["observations"]["memory"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pos| local_pos(pos) == storage_pos)
    );
    assert_eq!(storage_pos.y - reader_pos.y, 4);

    // Compile only the static wiring graph, then inspect the reader's actual Quasi sources.
    let monitor = TaskMonitor::default();
    let graph = crate::redpiler::passes::rich::prepare(
        &world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph;
    let storage_index = graph.pistons.iter().position(|piston| piston.position == storage_pos).unwrap();
    let reader_index = graph.pistons.iter().position(|piston| piston.position == reader_pos).unwrap();
    let storage = graph.pistons[storage_index];
    let reader = graph.pistons[reader_index];
    let power = index_power(&graph);
    let quasi = PowerTarget::Input {
        node: reader.node,
        port: InputPort::Quasi,
    };
    let quasi_links = power.get(&quasi).cloned().unwrap_or_default();
    let quasi_sources: Vec<_> = quasi_links
        .iter()
        .map(|link| {
            let site = &graph.sites[link.from.index()];
            let contact = match site.source {
                SiteSource::Contact(contact) => Some(contact),
                _ => None,
            };
            let source_resources: Vec<_> = contact
                .into_iter()
                .flat_map(|contact| {
                    graph
                        .resources
                        .iter()
                        .enumerate()
                        .filter_map(move |(index, resource)| resource.contacts.contains(&contact).then_some((index, resource)))
                })
                .map(|(index, resource)| {
                    json!({
                        "resource":index,
                        "contacts":resource.contacts.iter().map(|id| id.index()).collect::<Vec<_>>(),
                        "material":format!("{:?}",Block::from_id(graph.profiles[resource.material.index()].raw_state)),
                    })
                })
                .collect();
            json!({
                "source":format!("{:?}",site.source),
                "contact":contact.map(|id|id.index()),
                "attenuation":link.attenuation,
                "from_port":format!("{:?}",link.from_port),
                "guard":format!("{:?}",graph.guards[link.guard.index()]),
                "source_is_storage_far":contact == Some(storage.far),
                "source_resources":source_resources,
            })
        })
        .collect();

    let storage_far_resources: Vec<_> = graph
        .resources
        .iter()
        .enumerate()
        .filter(|(_, resource)| resource.contacts.contains(&storage.far))
        .map(|(index, resource)| {
            json!({
                "resource":index,
                "contacts":resource.contacts.iter().map(|id|id.index()).collect::<Vec<_>>(),
                "material":format!("{:?}",Block::from_id(graph.profiles[resource.material.index()].raw_state)),
            })
        })
        .collect();

    // Set up a single bit, then capture the full write-enable rise/fall and read-enable rise/fall.
    address(&mut world, 0, true);
    advance(&mut world, 24);
    data(&mut world, 1);
    advance(&mut world, 24);
    let observed_positions = [storage_pos, reader_pos];
    let mut detailed = Vec::new();
    let broad = trace::capture(|| {
        detailed = trace::capture_detailed(&observed_positions, || {
            lever_action(&mut world, BASE + WRITE_ENABLE, true);
            advance(&mut world, 24);
            lever_action(&mut world, BASE + WRITE_ENABLE, false);
            advance(&mut world, 24);
            address(&mut world, 0, false);
            advance(&mut world, 24);
            lever_action(&mut world, BASE + READ_ENABLE, true);
            advance(&mut world, 24);
            lever_action(&mut world, BASE + READ_ENABLE, false);
            advance(&mut world, 24);
        });
    });

    let update_retracts: Vec<_> = broad
        .iter()
        .filter_map(|entry| match entry.operation {
            Operation::Applied(event) if !event.sticky && event.action == PistonAction::Retract => {
                Some(json!({"tick":entry.tick,"phase":entry.phase,"position":event.pos}))
            }
            _ => None,
        })
        .collect();
    let memory_samples: Vec<_> = broad
        .iter()
        .filter_map(|entry| match entry.operation {
            Operation::Sample { pos, extended, powered } if pos == storage_pos => {
                Some(json!({"tick":entry.tick,"phase":entry.phase,"extended":extended,"powered":powered}))
            }
            _ => None,
        })
        .collect();
    let final_words = memory_words(&world);
    let final_output = output_word(&world);
    assert_eq!(final_words[0] & 1, 1, "write pulse should commit the selected bit");
    assert_eq!(final_output & 1, 0, "read release should clear the selected output gate");
    assert!(detailed.iter().any(|entry| entry.pos == storage_pos && entry.stage == Stage::Sample));
    assert!(detailed.iter().any(|entry| entry.pos == reader_pos && entry.stage == Stage::Sample));

    let report = json!({
        "fixture":"rilax_memory_bank_bud",
        "storage":{"position":storage_pos,"piston_id":storage_index,"facing":format!("{:?}",storage.facing),"sticky":storage.sticky,"near":storage.near.index(),"far":storage.far.index(),"far_resources":storage_far_resources},
        "reader":{"position":reader_pos,"piston_id":reader_index,"facing":format!("{:?}",reader.facing),"sticky":reader.sticky,"quasi_link_count":quasi_links.len(),"quasi_reads_storage_far":quasi_links.iter().any(|link| matches!(graph.sites[link.from.index()].source, SiteSource::Contact(contact) if contact == storage.far)),"quasi_sources":quasi_sources},
        "protocol":"write enable rise then fall; address 0 and data bit 0 held stable; then read address 0 enable rise/fall",
        "ordered_selected_samples_and_actions":detailed,
        "selected_storage_samples_broad_trace":memory_samples,
        "ordinary_updater_retract_events":update_retracts,
        "final_memory_word0":final_words[0],
        "final_output":final_output,
        "limits":"One cell only; static graph source links plus native event timing. No compiler replacement claim.",
    });
    let path = root().join("target/rilax-bud-single-cell-detail.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!(
        "Rilax BUD storage {:?}, reader {:?}: quasi_links={}, storage_far_resources={}, updater_retracts={}, detailed_events={}, final word0={:#04x}, output={:#04x}; report {}",
        storage_pos,
        reader_pos,
        quasi_links.len(),
        storage_far_resources.len(),
        update_retracts.len(),
        detailed.len(),
        final_words[0],
        final_output,
        path.display(),
    );
}
