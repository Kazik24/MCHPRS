//! Lossless inspection records for compiled instant electrical subprograms.
use super::*;
use crate::redpiler::program_graph::{DriveCondition, DriveSource, SignalOp, StateTest, ValueDistanceRow};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) fn export(graph: &CompileGraph) -> Result<Option<Value>, ExportError> {
    let plans = &graph.plans;
    let drives = &plans.instant_drives;
    let transport_count = plans.instant_transport.nets.len();
    let invalid = || ExportError::new("instant_programs", "unaligned instant program tables");
    let has_compiled_product = plans.signal_channels.channels.iter().any(Option::is_some)
        || plans.stable_instant_logic.nets.iter().any(Option::is_some)
        || plans.instant_value_regions.nets.iter().any(Option::is_some)
        || !plans.instant_transition_programs.nets.is_empty();
    if (!drives.by_wire.is_empty() && drives.by_wire.len() != graph.wires.len())
        || (drives.by_wire.is_empty()
            && (!drives.queries.is_empty() || !drives.terms.is_empty() || !drives.conditions.is_empty() || has_compiled_product))
        || (!plans.signal_channels.channels.is_empty() && plans.signal_channels.channels.len() != transport_count)
        || (!plans.stable_instant_logic.nets.is_empty() && plans.stable_instant_logic.nets.len() != transport_count)
        || (!plans.instant_value_regions.nets.is_empty() && plans.instant_value_regions.nets.len() != transport_count)
        || (!plans.instant_transition_programs.nets.is_empty() && plans.instant_transition_programs.nets.len() != transport_count)
    {
        return Err(invalid());
    }
    for (net, program) in plans.instant_value_regions.nets.iter().enumerate() {
        let Some(program) = program else { continue };
        let wire_count = plans.instant_transport.nets[net].wires.len();
        // These maps are aligned with the containing transport net. Reject
        // malformed tables instead of silently projecting missing entries as
        // `false` or `null`.
        if program.compact_wire_captures.len() != wire_count || program.wire_region.len() != wire_count || program.wire_local.len() != wire_count {
            return Err(invalid());
        }
    }
    let channels = plans
        .signal_channels
        .channels
        .iter()
        .enumerate()
        .filter_map(|(net, channel)| channel.as_ref().map(|channel| signal_channel(graph, net, channel)))
        .collect::<Vec<_>>();
    let stable = plans
        .stable_instant_logic
        .nets
        .iter()
        .enumerate()
        .filter_map(|(net, program)| program.as_ref().map(|program| stable_net(graph, net, program)))
        .collect::<Vec<_>>();
    let values = plans
        .instant_value_regions
        .nets
        .iter()
        .enumerate()
        .filter_map(|(net, program)| program.as_ref().map(|program| value_net(graph, net, program)))
        .collect::<Vec<_>>();
    let transition_attempted = !plans.instant_transition_programs.nets.is_empty();
    if drives.queries.is_empty() && channels.is_empty() && stable.is_empty() && values.is_empty() && !transition_attempted {
        return Ok(None);
    }

    Ok(Some(json!({
        "instant_drives": {
            "wires": drives.by_wire.iter().enumerate().filter_map(|(index, query)| {
                graph.wires.get(index).map(|wire| json!({
                    "node": typed_id("node", wire.node.index()),
                    "query": query,
                }))
            }).collect::<Vec<_>>(),
            "queries": drives.queries.iter().map(|query| json!({
                "constant": query.constant,
                "terms": range(query.terms.start, query.terms.end),
            })).collect::<Vec<_>>(),
            "terms": drives.terms.iter().map(|term| json!({
                "source": drive_source(term.source),
                "condition": term.condition,
                "attenuation": term.attenuation,
            })).collect::<Vec<_>>(),
            "conditions": drives.conditions.iter().map(drive_condition).collect::<Vec<_>>(),
        },
        "signal_channels": channels,
        "stable_logic": stable,
        "value_regions": values,
        "transition_eligibility": transition_attempted.then(|| json!({
            "semantics": "eligibility only; runtime-learned transcripts are unavailable",
            "nets": plans.instant_transition_programs.nets.iter().enumerate().map(|(net, program)| {
                match program {
                    Some(program) => json!({
                        "net": net,
                        "eligible": true,
                        "wire_queries": program.wire_queries.iter().enumerate().map(|(wire, query)| json!({
                            "wire": wire,
                            "node": typed_id("node", plans.instant_transport.nets[net].wires[wire].node.index()),
                            "query": query,
                        })).collect::<Vec<_>>(),
                    }),
                    None => json!({"net": net, "eligible": false}),
                }
            }).collect::<Vec<_>>(),
        })),
    })))
}

fn signal_channel(graph: &CompileGraph, net_index: usize, channel: &crate::redpiler::program_graph::SignalChannel) -> Value {
    let net = &graph.plans.instant_transport.nets[net_index];
    let wire_count = net.wires.len();
    json!({
        "net": net_index,
        "wires": net.wires.iter().map(|wire| typed_id("node", wire.node.index())).collect::<Vec<_>>(),
        "input": {
            "source": drive_source(channel.input.source),
            "condition": channel.input.condition,
        },
        "profiles": channel.profiles,
        "discriminator": channel.discriminator,
        "transitions": channel.transitions.iter().enumerate().map(|(index, transition)| {
            let transcript = transition.as_ref().map(|transcript| json!({
                "selections": transcript.selections,
                "final_powers": transcript.final_powers,
                "ops": transcript.ops.iter().map(|op| match *op {
                    SignalOp::Capture(site) => json!({"op": "capture", "site": site}),
                    SignalOp::Select { wire, slot, selection } => json!({
                        "op": "select", "wire": wire, "slot": slot, "selection": selection,
                    }),
                    SignalOp::SetPower { wire, power } => json!({"op": "set_power", "wire": wire, "power": power}),
                    SignalOp::Call { site, selection, layer, route } => json!({
                        "op": "call", "site": site, "selection": selection, "layer": layer,
                        "route": typed_id("route", route.index()),
                    }),
                }).collect::<Vec<_>>(),
            }));
            json!({"old_input": index / wire_count, "root": index % wire_count, "transcript": transcript})
        }).collect::<Vec<_>>(),
    })
}

fn stable_net(graph: &CompileGraph, net_index: usize, program: &crate::redpiler::program_graph::StableLogicNet) -> Value {
    let transport = &graph.plans.instant_transport.nets[net_index];
    json!({
        "net": net_index,
        "wires": transport.wires.iter().map(|wire| typed_id("node", wire.node.index())).collect::<Vec<_>>(),
        "expressions": program.expressions.iter().map(|expression| json!({
            "constant": expression.constant,
            "terms": range(expression.terms.start, expression.terms.end),
        })).collect::<Vec<_>>(),
        "inputs": program.inputs.iter().map(|input| json!({
            "source": drive_source(input.source), "condition": input.condition,
        })).collect::<Vec<_>>(),
        "terms": program.terms.iter().map(|term| json!({"input": term.input, "attenuation": term.attenuation})).collect::<Vec<_>>(),
        "input_users": ranges(&program.input_users),
        "input_user_expressions": program.input_user_expressions,
        "outputs": program.outputs.iter().map(|output| json!({
            "site": typed_id("site", output.site.index()),
            "candidates": output.candidates.iter().map(|candidate| json!({
                "wire": candidate.wire, "slot": candidate.slot, "admission": candidate.admission,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "wire_outputs": ranges(&program.wire_outputs),
        "wire_output_indices": program.wire_output_indices,
    })
}

fn value_net(graph: &CompileGraph, net_index: usize, program: &crate::redpiler::program_graph::InstantValueRegionNet) -> Value {
    let transport = &graph.plans.instant_transport.nets[net_index];
    json!({
        "net": net_index,
        "wires": transport.wires.iter().enumerate().map(|(index, wire)| json!({
            "node": typed_id("node", wire.node.index()),
            "compact_capture": program.compact_wire_captures[index],
            "region": local_option(Some(program.wire_region[index])),
            "region_local": local_option(Some(program.wire_local[index])),
        })).collect::<Vec<_>>(),
        "regions": program.regions.iter().map(|region| json!({
            "component": region.component,
            "wires": range(region.wires.start, region.wires.end),
            "drive_groups": range(region.drive_groups.start, region.drive_groups.end),
            "conditional_arcs": range(region.conditional_arcs.start, region.conditional_arcs.end),
        })).collect::<Vec<_>>(),
        "region_wires": program.region_wires,
        "drive_groups": program.drive_groups.iter().map(|group| json!({"query": group.query, "row": group.row})).collect::<Vec<_>>(),
        "conditional_arcs": program.conditional_arcs.iter().map(|arc| json!({
            "source": arc.source, "row": arc.row, "selector": arc.selector,
        })).collect::<Vec<_>>(),
        "selectors": program.selectors.iter().map(|selector| json!({
            "horizontal_site": typed_id("site", selector.horizontal_site.index()),
            "ascent_up_site": selector.ascent_up_site.map(|site| typed_id("site", site.index())),
        })).collect::<Vec<_>>(),
        "distance_rows": program.distance_rows.iter().map(distance_row).collect::<Vec<_>>(),
        "distances": program.distances,
    })
}

fn drive_source(source: DriveSource) -> Value {
    match source {
        DriveSource::Output(node) => json!({"kind": "output", "node": typed_id("node", node.index())}),
        DriveSource::Powered(node) => json!({"kind": "powered", "node": typed_id("node", node.index())}),
        DriveSource::Constant(value) => json!({"kind": "constant", "value": value}),
    }
}

fn drive_condition(condition: &DriveCondition) -> Value {
    match condition {
        DriveCondition::Always => json!({"kind": "always"}),
        DriveCondition::Never => json!({"kind": "never"}),
        DriveCondition::State(test) => json!({"kind": "state", "test": state_test(*test)}),
        DriveCondition::WireEmits { node, face, raw_sides } => json!({
            "kind": "wire_emits", "node": typed_id("node", node.index()), "face": super::face(*face), "raw_sides": raw_sides,
        }),
        DriveCondition::Not(child) => json!({"kind": "not", "child": child}),
        DriveCondition::All(children) => json!({"kind": "all", "children": children}),
        DriveCondition::Any(children) => json!({"kind": "any", "children": children}),
    }
}

fn state_test(test: StateTest) -> Value {
    match test {
        StateTest::PistonExtended(piston) => json!({"kind": "piston_extended", "piston": typed_id("piston", piston.index())}),
        StateTest::PistonHeadPresent(piston) => json!({"kind": "piston_head_present", "piston": typed_id("piston", piston.index())}),
        StateTest::PistonAvailability { piston, is } => json!({
            "kind": "piston_availability", "piston": typed_id("piston", piston.index()), "availability": super::piston_availability(is),
        }),
        StateTest::ResourceAt { resource, contact } => json!({
            "kind": "resource_at", "resource": typed_id("resource", resource.index()), "contact": typed_id("contact", contact.index()),
        }),
        StateTest::ResourceEnabled(resource) => json!({"kind": "resource_enabled", "resource": typed_id("resource", resource.index())}),
    }
}

fn range(start: u32, end: u32) -> [u32; 2] {
    [start, end]
}

fn ranges(ranges: &[std::ops::Range<u32>]) -> Vec<[u32; 2]> {
    ranges.iter().map(|range| [range.start, range.end]).collect()
}

fn local_option(index: Option<u32>) -> Option<u32> {
    index.filter(|index| *index != u32::MAX)
}

fn distance_row(row: &ValueDistanceRow) -> Value {
    json!({"start": row.start, "len": row.len})
}

pub(super) fn validate(
    programs: Option<&Value>,
    nodes: &HashSet<String>,
    wire_nodes: &HashSet<String>,
    sites: &HashSet<String>,
    routes: &HashSet<String>,
    pistons: &HashSet<String>,
    resources: &HashSet<String>,
    contacts: &HashSet<String>,
    transport: &[Value],
) -> Result<(), ExportError> {
    let Some(programs) = programs else { return Ok(()) };
    let invalid = || ExportError::new("instant_programs", "invalid instant program export");
    let drives = programs.get("instant_drives").ok_or_else(invalid)?;
    let drive_wires = drives["wires"].as_array().ok_or_else(invalid)?;
    let queries = drives["queries"].as_array().ok_or_else(invalid)?;
    let terms = drives["terms"].as_array().ok_or_else(invalid)?;
    let conditions = drives["conditions"].as_array().ok_or_else(invalid)?;
    if drive_wires.len() != wire_nodes.len() {
        return Err(invalid());
    }
    for net in transport {
        let net_wires = transport_wire_ids(net)?;
        let net_sites = net["sites"].as_array().ok_or_else(invalid)?;
        let site_wire = net["site_wire"].as_array().ok_or_else(invalid)?;
        let canonical_repeaters = net["canonical_repeaters"].as_array().ok_or_else(invalid)?;
        if site_wire.len() != net_sites.len()
            || net_wires.iter().any(|node| !wire_nodes.contains(*node))
            || net_sites.iter().any(|site| site.as_str().is_none_or(|site| !sites.contains(site)))
            || site_wire.iter().any(|owner| {
                owner
                    .as_u64()
                    .is_none_or(|owner| owner != u32::MAX as u64 && owner >= net_wires.len() as u64)
            })
            || canonical_repeaters
                .iter()
                .any(|node| node.as_str().is_none_or(|node| !nodes.contains(node)))
        {
            return Err(invalid());
        }
    }
    let mut seen_drive_wires = HashSet::new();
    for wire in drive_wires {
        let node = wire["node"].as_str().ok_or_else(invalid)?;
        check_ref(nodes, node, "instant drive wire")?;
        if !wire_nodes.contains(node) || !seen_drive_wires.insert(node) {
            return Err(invalid());
        }
        if !wire["query"].is_null() && index(&wire["query"], queries.len()).is_none() {
            return Err(invalid());
        }
    }
    for query in queries {
        if value_u8(&query["constant"]).is_none_or(|value| value > 15) || !valid_range(&query["terms"], terms.len()) {
            return Err(invalid());
        }
    }
    for term in terms {
        if index(&term["condition"], conditions.len()).is_none()
            || value_u8(&term["attenuation"]).is_none_or(|value| value >= 15)
            || !valid_source(&term["source"], nodes)
        {
            return Err(invalid());
        }
    }
    for (at, condition) in conditions.iter().enumerate() {
        let kind = condition["kind"].as_str().ok_or_else(invalid)?;
        let valid = match kind {
            "always" | "never" => true,
            "state" => valid_state_test(&condition["test"], pistons, resources, contacts),
            "wire_emits" => {
                condition["node"].as_str().is_some_and(|node| wire_nodes.contains(node))
                    && matches!(condition["face"].as_str(), Some("bottom" | "top" | "north" | "south" | "west" | "east"))
                    && value_u8(&condition["raw_sides"]).is_some()
            }
            "not" => index(&condition["child"], at).is_some(),
            "all" | "any" => condition["children"]
                .as_array()
                .is_some_and(|children| children.iter().all(|child| index(child, at).is_some())),
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
    }

    let mut seen = HashSet::new();
    for channel in programs["signal_channels"].as_array().ok_or_else(invalid)? {
        let (net_index, net_wires) = net_wires(channel, transport)?;
        if !seen.insert(net_index)
            || !matches_net_wires(&channel["wires"], &net_wires)
            || !valid_source(&channel["input"]["source"], nodes)
            || index(&channel["input"]["condition"], conditions.len()).is_none()
        {
            return Err(invalid());
        }
        let wire_count = net_wires.len();
        let profiles = channel["profiles"].as_array().ok_or_else(invalid)?;
        if profiles.len() != 2 || profiles.iter().any(|profile| !valid_power_array(profile, wire_count)) {
            return Err(invalid());
        }
        if index(&channel["discriminator"], wire_count).is_none() {
            return Err(invalid());
        }
        let transitions = channel["transitions"].as_array().ok_or_else(invalid)?;
        if transitions.len() != wire_count.saturating_mul(2) {
            return Err(invalid());
        }
        let net_sites = transport[net_index]["sites"].as_array().ok_or_else(invalid)?;
        let net_site_ids = net_sites
            .iter()
            .map(|site| site.as_str().ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        for (at, transition) in transitions.iter().enumerate() {
            if transition["old_input"].as_u64() != Some((at / wire_count) as u64) || transition["root"].as_u64() != Some((at % wire_count) as u64) {
                return Err(invalid());
            }
            if !transition["transcript"].is_null() {
                validate_transcript(&transition["transcript"], wire_count, &net_site_ids, sites, routes)?;
            }
        }
    }

    seen.clear();
    for stable in programs["stable_logic"].as_array().ok_or_else(invalid)? {
        let (net_index, net_wires) = net_wires(stable, transport)?;
        let wire_count = net_wires.len();
        let expressions = stable["expressions"].as_array().ok_or_else(invalid)?;
        let inputs = stable["inputs"].as_array().ok_or_else(invalid)?;
        let stable_terms = stable["terms"].as_array().ok_or_else(invalid)?;
        let input_user_expressions = stable["input_user_expressions"].as_array().ok_or_else(invalid)?;
        let outputs = stable["outputs"].as_array().ok_or_else(invalid)?;
        let wire_output_indices = stable["wire_output_indices"].as_array().ok_or_else(invalid)?;
        if !seen.insert(net_index)
            || !matches_net_wires(&stable["wires"], &net_wires)
            || expressions.len() != wire_count
            || inputs
                .iter()
                .any(|input| !valid_source(&input["source"], nodes) || index(&input["condition"], conditions.len()).is_none())
            || expressions.iter().any(|expression| {
                value_u8(&expression["constant"]).is_none_or(|value| value > 15) || !valid_range(&expression["terms"], stable_terms.len())
            })
            || stable_terms
                .iter()
                .any(|term| index(&term["input"], inputs.len()).is_none() || value_u8(&term["attenuation"]).is_none_or(|value| value >= 15))
            || !valid_partitioned_index_lists(&stable["input_users"], inputs.len(), input_user_expressions.len())
            || !valid_index_list_values(&stable["input_user_expressions"], expressions.len())
            || !valid_outputs(outputs, wire_count, sites)
            || !valid_partitioned_index_lists(&stable["wire_outputs"], wire_count, wire_output_indices.len())
            || !valid_index_list_values(&stable["wire_output_indices"], outputs.len())
        {
            return Err(invalid());
        }
        let mut expected_input_users = vec![Vec::new(); inputs.len()];
        for (expression_index, expression) in expressions.iter().enumerate() {
            let range = expression["terms"].as_array().ok_or_else(invalid)?;
            let start = range[0].as_u64().ok_or_else(invalid)? as usize;
            let end = range[1].as_u64().ok_or_else(invalid)? as usize;
            for term in &stable_terms[start..end] {
                expected_input_users[index(&term["input"], inputs.len()).ok_or_else(invalid)?].push(expression_index);
            }
        }
        for users in &mut expected_input_users {
            users.sort_unstable();
            users.dedup();
        }
        if !index_lists_match(&stable["input_users"], &stable["input_user_expressions"], &expected_input_users) {
            return Err(invalid());
        }
        let mut expected_wire_outputs = vec![Vec::new(); wire_count];
        for (output_index, output) in outputs.iter().enumerate() {
            for candidate in output["candidates"].as_array().ok_or_else(invalid)? {
                expected_wire_outputs[index(&candidate["wire"], wire_count).ok_or_else(invalid)?].push(output_index);
            }
        }
        for indices in &mut expected_wire_outputs {
            indices.sort_unstable();
            indices.dedup();
        }
        if !index_lists_match(&stable["wire_outputs"], &stable["wire_output_indices"], &expected_wire_outputs) {
            return Err(invalid());
        }
    }

    seen.clear();
    for value in programs["value_regions"].as_array().ok_or_else(invalid)? {
        let (net_index, net_wires) = net_wires(value, transport)?;
        let wire_count = net_wires.len();
        let regions = value["regions"].as_array().ok_or_else(invalid)?;
        let region_wires = value["region_wires"].as_array().ok_or_else(invalid)?;
        let groups = value["drive_groups"].as_array().ok_or_else(invalid)?;
        let arcs = value["conditional_arcs"].as_array().ok_or_else(invalid)?;
        let selectors = value["selectors"].as_array().ok_or_else(invalid)?;
        let rows = value["distance_rows"].as_array().ok_or_else(invalid)?;
        let distances = value["distances"].as_array().ok_or_else(invalid)?;
        let wires = value["wires"].as_array().ok_or_else(invalid)?;
        if !seen.insert(net_index) || wires.len() != wire_count {
            return Err(invalid());
        }
        for (at, wire) in wires.iter().enumerate() {
            if wire["node"].as_str() != Some(net_wires[at]) || !wire["compact_capture"].is_boolean() {
                return Err(invalid());
            }
            match (index(&wire["region"], regions.len()), wire["region_local"].as_u64()) {
                (None, None) if wire["region"].is_null() && wire["region_local"].is_null() => {}
                (Some(region), Some(local)) => {
                    let Some(local) = usize::try_from(local).ok() else {
                        return Err(invalid());
                    };
                    let Some(wire_range) = regions[region]["wires"].as_array().filter(|range| range.len() == 2) else {
                        return Err(invalid());
                    };
                    let Some(start) = wire_range[0].as_u64().and_then(|value| usize::try_from(value).ok()) else {
                        return Err(invalid());
                    };
                    let Some(end) = wire_range[1].as_u64().and_then(|value| usize::try_from(value).ok()) else {
                        return Err(invalid());
                    };
                    let Some(mapped) = region_wires.get(start.saturating_add(local)).and_then(|value| value.as_u64()) else {
                        return Err(invalid());
                    };
                    if local >= end.saturating_sub(start) || mapped != at as u64 {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        if region_wires.iter().any(|wire| index(wire, wire_count).is_none())
            || regions.iter().any(|region| {
                region["component"].as_u64().is_none_or(|component| component > u32::MAX as u64)
                    || !valid_range(&region["wires"], region_wires.len())
                    || !valid_range(&region["drive_groups"], groups.len())
                    || !valid_range(&region["conditional_arcs"], arcs.len())
            })
            || groups
                .iter()
                .any(|group| index(&group["query"], queries.len()).is_none() || index(&group["row"], rows.len()).is_none())
            || selectors.iter().any(|selector| {
                selector["horizontal_site"].as_str().is_none_or(|site| !sites.contains(site))
                    || (!selector["ascent_up_site"].is_null() && selector["ascent_up_site"].as_str().is_none_or(|site| !sites.contains(site)))
            })
            || arcs.iter().any(|arc| {
                index(&arc["source"], region_wires.len()).is_none()
                    || index(&arc["row"], rows.len()).is_none()
                    || index(&arc["selector"], selectors.len()).is_none()
            })
            || rows.iter().any(|row| !valid_distance_row(row, distances.len()))
            || distances.iter().any(|distance| value_u8(distance).is_none_or(|distance| distance > 15))
        {
            return Err(invalid());
        }
        if !ranges_partition(&regions, "wires", region_wires.len())
            || !ranges_partition(&regions, "drive_groups", groups.len())
            || !ranges_partition(&regions, "conditional_arcs", arcs.len())
        {
            return Err(invalid());
        }
        for (region_index, region) in regions.iter().enumerate() {
            let wire_range = region["wires"].as_array().ok_or_else(invalid)?;
            let wire_start = wire_range[0].as_u64().ok_or_else(invalid)? as usize;
            let wire_end = wire_range[1].as_u64().ok_or_else(invalid)? as usize;
            let region_wire_count = wire_end - wire_start;
            for local in 0..region_wire_count {
                let transport_wire = region_wires[wire_start + local].as_u64().ok_or_else(invalid)? as usize;
                let mapped = &wires[transport_wire];
                if mapped["region"].as_u64() != Some(region_index as u64) || mapped["region_local"].as_u64() != Some(local as u64) {
                    return Err(invalid());
                }
            }
            let group_range = region["drive_groups"].as_array().ok_or_else(invalid)?;
            let group_start = group_range[0].as_u64().ok_or_else(invalid)? as usize;
            let group_end = group_range[1].as_u64().ok_or_else(invalid)? as usize;
            let arc_range = region["conditional_arcs"].as_array().ok_or_else(invalid)?;
            let arc_start = arc_range[0].as_u64().ok_or_else(invalid)? as usize;
            let arc_end = arc_range[1].as_u64().ok_or_else(invalid)? as usize;
            if groups[group_start..group_end]
                .iter()
                .any(|group| row_len(&rows[index(&group["row"], rows.len()).unwrap()]) != Some(region_wire_count))
                || arcs[arc_start..arc_end].iter().any(|arc| {
                    index(&arc["source"], region_wire_count).is_none()
                        || row_len(&rows[index(&arc["row"], rows.len()).unwrap()]) != Some(region_wire_count)
                })
            {
                return Err(invalid());
            }
        }
    }

    if let Some(eligibility) = programs.get("transition_eligibility").filter(|value| !value.is_null()) {
        if eligibility["semantics"].as_str() != Some("eligibility only; runtime-learned transcripts are unavailable") {
            return Err(invalid());
        }
        let nets = eligibility["nets"].as_array().ok_or_else(invalid)?;
        if nets.len() != transport.len() {
            return Err(invalid());
        }
        for (net, program) in nets.iter().enumerate() {
            if program["net"].as_u64() != Some(net as u64) || !program["eligible"].is_boolean() {
                return Err(invalid());
            }
            if program["eligible"] == true {
                let net_wires = transport_wire_ids(&transport[net])?;
                let wire_count = net_wires.len();
                let bindings = program["wire_queries"].as_array().ok_or_else(invalid)?;
                if bindings.len() != wire_count {
                    return Err(invalid());
                }
                for (wire, binding) in bindings.iter().enumerate() {
                    if binding["wire"].as_u64() != Some(wire as u64)
                        || binding["node"].as_str() != Some(net_wires[wire])
                        || binding["node"].as_str().is_none_or(|node| !wire_nodes.contains(node))
                        || index(&binding["query"], queries.len()).is_none()
                    {
                        return Err(invalid());
                    }
                }
            } else if !program["wire_queries"].is_null() {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn validate_transcript(
    transcript: &Value,
    wire_count: usize,
    net_sites: &[&str],
    sites: &HashSet<String>,
    routes: &HashSet<String>,
) -> Result<(), ExportError> {
    let invalid = || ExportError::new("instant_programs", "invalid signal-channel transcript");
    let selections = transcript["selections"].as_u64().ok_or_else(invalid)?;
    if selections > u32::MAX as u64 || !valid_power_array(&transcript["final_powers"], wire_count) {
        return Err(invalid());
    }
    for op in transcript["ops"].as_array().ok_or_else(invalid)? {
        let valid = match op["op"].as_str() {
            Some("capture") | Some("call") => {
                index(&op["site"], net_sites.len()).is_some_and(|site| sites.contains(net_sites[site]))
                    && (op["op"] == "capture"
                        || (op["route"].as_str().is_some_and(|route| routes.contains(route))
                            && op["selection"]
                                .as_u64()
                                .is_some_and(|selection| selection == u32::MAX as u64 || selection < selections)
                            && op["layer"].as_u64().is_some_and(|layer| layer <= u32::MAX as u64)))
            }
            Some("select") => {
                index(&op["wire"], wire_count).is_some()
                    && op["slot"].as_u64().is_some_and(|slot| slot < 24)
                    && op["selection"].as_u64().is_some_and(|selection| selection < selections)
            }
            Some("set_power") => index(&op["wire"], wire_count).is_some() && value_u8(&op["power"]).is_some_and(|power| power <= 15),
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
    }
    Ok(())
}

fn valid_outputs(outputs: &[Value], wire_count: usize, sites: &HashSet<String>) -> bool {
    outputs.iter().all(|output| {
        output["site"].as_str().is_some_and(|site| sites.contains(site))
            && output["candidates"].as_array().is_some_and(|candidates| {
                candidates.iter().all(|candidate| {
                    index(&candidate["wire"], wire_count).is_some()
                        && candidate["slot"].as_u64().is_some_and(|slot| slot < 24)
                        && (candidate["admission"].is_null() || candidate["admission"].is_boolean())
                })
            })
    })
}

fn valid_source(source: &Value, nodes: &HashSet<String>) -> bool {
    match source["kind"].as_str() {
        Some("output") | Some("powered") => source["node"].as_str().is_some_and(|node| nodes.contains(node)),
        Some("constant") => value_u8(&source["value"]).is_some_and(|value| value <= 15),
        _ => false,
    }
}

fn valid_state_test(test: &Value, pistons: &HashSet<String>, resources: &HashSet<String>, contacts: &HashSet<String>) -> bool {
    match test["kind"].as_str() {
        Some("piston_extended") | Some("piston_head_present") => test["piston"].as_str().is_some_and(|id| pistons.contains(id)),
        Some("piston_availability") => {
            test["piston"].as_str().is_some_and(|id| pistons.contains(id))
                && matches!(
                    test["availability"].as_str(),
                    Some("ready" | "head_pending" | "publishing_head_removal" | "base_pending_head_present" | "base_pending")
                )
        }
        Some("resource_at") => {
            test["resource"].as_str().is_some_and(|id| resources.contains(id)) && test["contact"].as_str().is_some_and(|id| contacts.contains(id))
        }
        Some("resource_enabled") => test["resource"].as_str().is_some_and(|id| resources.contains(id)),
        _ => false,
    }
}

fn net_wires<'a>(value: &Value, transport: &'a [Value]) -> Result<(usize, Vec<&'a str>), ExportError> {
    let invalid = || ExportError::new("instant_programs", "invalid transport-net binding");
    let net = index(&value["net"], transport.len()).ok_or_else(invalid)?;
    Ok((net, transport_wire_ids(&transport[net])?))
}

fn transport_wire_ids(net: &Value) -> Result<Vec<&str>, ExportError> {
    net["wires"]
        .as_array()
        .ok_or_else(|| ExportError::new("instant_programs", "missing transport wires"))?
        .iter()
        .map(|wire| {
            wire.as_str()
                .ok_or_else(|| ExportError::new("instant_programs", "invalid transport wire ID"))
        })
        .collect()
}

fn matches_net_wires(value: &Value, expected: &[&str]) -> bool {
    value
        .as_array()
        .is_some_and(|wires| wires.len() == expected.len() && wires.iter().zip(expected).all(|(wire, expected)| wire.as_str() == Some(*expected)))
}

fn valid_partitioned_index_lists(value: &Value, row_count: usize, flat_count: usize) -> bool {
    value.as_array().is_some_and(|rows| {
        if rows.len() != row_count {
            return false;
        }
        let mut cursor = 0usize;
        for row in rows {
            let Some(range) = row.as_array().filter(|range| range.len() == 2) else {
                return false;
            };
            let (Some(start), Some(end)) = (range[0].as_u64(), range[1].as_u64()) else {
                return false;
            };
            if usize::try_from(start).ok() != Some(cursor) || end < start || end > flat_count as u64 || !valid_range(row, flat_count) {
                return false;
            }
            cursor = end as usize;
        }
        cursor == flat_count
    })
}

fn index_lists_match(rows: &Value, flat: &Value, expected: &[Vec<usize>]) -> bool {
    let (Some(rows), Some(flat)) = (rows.as_array(), flat.as_array()) else {
        return false;
    };
    rows.len() == expected.len()
        && expected.iter().zip(rows).all(|(expected, range)| {
            let Some(range) = range.as_array().filter(|range| range.len() == 2) else {
                return false;
            };
            let (Some(start), Some(end)) = (range[0].as_u64(), range[1].as_u64()) else {
                return false;
            };
            let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
                return false;
            };
            end.saturating_sub(start) == expected.len()
                && flat.get(start..end).is_some_and(|actual| {
                    actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.as_u64() == Some(*expected as u64))
                })
        })
}

fn ranges_partition(regions: &[Value], field: &str, total: usize) -> bool {
    let mut cursor = 0usize;
    for region in regions {
        let Some(range) = region[field].as_array().filter(|range| range.len() == 2) else {
            return false;
        };
        let (Some(start), Some(end)) = (range[0].as_u64(), range[1].as_u64()) else {
            return false;
        };
        if usize::try_from(start).ok() != Some(cursor) || end < start || end > total as u64 {
            return false;
        }
        cursor = end as usize;
    }
    cursor == total
}

fn row_len(row: &Value) -> Option<usize> {
    row["len"].as_u64().and_then(|value| usize::try_from(value).ok())
}

fn valid_index_list_values(values: &Value, limit: usize) -> bool {
    values
        .as_array()
        .is_some_and(|values| values.iter().all(|value| index(value, limit).is_some()))
}

fn valid_distance_row(row: &Value, distance_count: usize) -> bool {
    let Some(start) = row["start"].as_u64().and_then(|value| usize::try_from(value).ok()) else {
        return false;
    };
    let Some(len) = row["len"].as_u64().and_then(|value| usize::try_from(value).ok()) else {
        return false;
    };
    start <= u32::MAX as usize && len <= u32::MAX as usize && start.checked_add(len).is_some_and(|end| end <= distance_count)
}

fn valid_power_array(value: &Value, expected: usize) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.len() == expected && values.iter().all(|value| value_u8(value).is_some_and(|power| power <= 15)))
}

fn valid_range(value: &Value, limit: usize) -> bool {
    let Some(range) = value.as_array().filter(|range| range.len() == 2) else {
        return false;
    };
    let Some(start) = range[0].as_u64().and_then(|value| usize::try_from(value).ok()) else {
        return false;
    };
    let Some(end) = range[1].as_u64().and_then(|value| usize::try_from(value).ok()) else {
        return false;
    };
    start <= u32::MAX as usize && end <= u32::MAX as usize && start <= end && end <= limit
}

fn index(value: &Value, length: usize) -> Option<usize> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value < length)
}

fn value_u8(value: &Value) -> Option<u8> {
    value.as_u64().and_then(|value| u8::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_graph() -> (Value, Vec<Value>, HashSet<String>) {
        let programs = json!({
            "instant_drives": {
                "wires": [{"node":"node:0","query":0}],
                "queries": [{"constant":0,"terms":[0,1]}],
                "terms": [{"source":{"kind":"output","node":"node:0"},"condition":0,"attenuation":1}],
                "conditions": [{"kind":"always"}],
            },
            "signal_channels": [{
                "net":0,"wires":["node:0"],
                "input":{"source":{"kind":"constant","value":15},"condition":0},
                "profiles":[[0],[15]],"discriminator":0,
                "transitions":[
                    {"old_input":0,"root":0,"transcript":{"selections":0,"final_powers":[15],"ops":[{"op":"call","site":0,"selection":u32::MAX,"layer":0,"route":"route:0"}]}},
                    {"old_input":1,"root":0,"transcript":null}
                ]
            }],
            "stable_logic":[],
            "transition_eligibility":null,
            "value_regions":[{
                "net":0,
                "wires":[{"node":"node:0","compact_capture":false,"region":0,"region_local":0}],
                "regions":[{"component":0,"wires":[0,1],"drive_groups":[0,1],"conditional_arcs":[0,0]}],
                "region_wires":[0],
                "drive_groups":[{"query":0,"row":0}],
                "conditional_arcs":[],
                "selectors":[],
                "distance_rows":[{"start":0,"len":1}],
                "distances":[0]
            }],
        });
        let transport = vec![json!({
            "wires":["node:0"],"sites":["site:0"],"site_wire":[0],"canonical_repeaters":[]
        })];
        let nodes = HashSet::from(["node:0".to_owned()]);
        (programs, transport, nodes)
    }

    #[test]
    fn absent_program_tables_are_valid_for_legacy_exports() {
        let empty = HashSet::new();
        assert!(validate(None, &empty, &empty, &empty, &empty, &empty, &empty, &empty, &[]).is_ok());
    }

    #[test]
    fn dependency_ranges_index_the_flat_table_not_the_target_table() {
        let ranges = json!([[0, 2], [2, 4]]);
        let values = json!([0, 1, 0, 1]);
        assert!(valid_partitioned_index_lists(&ranges, 2, 4));
        assert!(valid_index_list_values(&values, 2));
    }

    #[test]
    fn validates_ranges_typed_references_and_signal_operation_tags() {
        let (mut programs, transport, nodes) = valid_graph();
        let wire_nodes = nodes.clone();
        let sites = HashSet::from(["site:0".to_owned()]);
        let routes = HashSet::from(["route:0".to_owned()]);
        let validate = |programs: &Value| {
            super::validate(
                Some(programs),
                &nodes,
                &wire_nodes,
                &sites,
                &routes,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::new(),
                &transport,
            )
        };
        assert!(validate(&programs).is_ok());

        programs["instant_drives"]["queries"][0]["terms"] = json!([0, 2]);
        assert!(validate(&programs).is_err(), "out-of-range query terms must reject the export");
        programs["instant_drives"]["queries"][0]["terms"] = json!([0, 1]);
        programs["instant_drives"]["wires"][0]["node"] = json!("node:99");
        assert!(validate(&programs).is_err(), "unknown typed node references must reject the export");
        programs["instant_drives"]["wires"][0]["node"] = json!("node:0");
        programs["signal_channels"][0]["transitions"][0]["transcript"]["ops"][0]["op"] = json!("unknown");
        assert!(validate(&programs).is_err(), "unknown ordered operation tags must reject the export");
    }

    #[test]
    fn rejects_value_arc_sources_outside_the_region_and_mismatched_distance_rows() {
        let (mut programs, transport, nodes) = valid_graph();
        let wire_nodes = nodes.clone();
        let sites = HashSet::from(["site:0".to_owned()]);
        let routes = HashSet::from(["route:0".to_owned()]);
        let validate = |programs: &Value| {
            super::validate(
                Some(programs),
                &nodes,
                &wire_nodes,
                &sites,
                &routes,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::new(),
                &transport,
            )
        };
        assert!(validate(&programs).is_ok());

        programs["value_regions"][0]["selectors"] = json!([{"horizontal_site":"site:0","ascent_up_site":null}]);
        programs["value_regions"][0]["conditional_arcs"] = json!([{"source":1,"row":0,"selector":0}]);
        programs["value_regions"][0]["regions"][0]["conditional_arcs"] = json!([0, 1]);
        assert!(validate(&programs).is_err(), "arc source must be local to its region");

        programs["value_regions"][0]["conditional_arcs"] = json!([]);
        programs["value_regions"][0]["regions"][0]["conditional_arcs"] = json!([0, 0]);
        programs["value_regions"][0]["distance_rows"][0]["len"] = json!(0);
        assert!(validate(&programs).is_err(), "distance row length must match the containing region");
    }
}
