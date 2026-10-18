//! Serialize the validated general graph as immutable C tables, preserving graph IDs.
use super::*;
use std::fmt::Write;

pub(super) fn program(program: &CircuitProgram) -> Result<String, NativeQueryError> {
    let mut output = String::from("#include \"native_engine.h\"\n");
    let mut profile_blocks: Vec<_> = (0..program.profiles().len())
        .map(|i| program.profile_block(ProfileId::new(i as u32)).unwrap())
        .collect();
    let mut node_profiles = Vec::new();
    for node in program.nodes() {
        let Some(block) = node.block else {
            node_profiles.push(NONE as usize);
            continue;
        };
        let index = profile_blocks.iter().position(|b| *b == block).unwrap_or_else(|| {
            profile_blocks.push(block);
            profile_blocks.len() - 1
        });
        node_profiles.push(index);
    }
    let mut rows = Vec::new();
    for (i, block) in profile_blocks.iter().copied().enumerate() {
        let facts = program.profiles().get(i);
        let electrical = facts.map_or_else(
            || if block == Block::RedstoneBlock { 1 } else { 0 },
            |p| match p.electrical {
                ProfileElectrical::None => 0,
                ProfileElectrical::RedstoneSource => 1,
                ProfileElectrical::Component => 2,
            },
        );
        let flags = u32::from(matches!(block, Block::RedstoneWire { .. }))
            | (u32::from(ContainerType::from_block(block).is_some()) << 1)
            | (u32::from(matches!(block, Block::MovingPiston { .. })) << 2)
            | (u32::from(matches!(block, Block::PistonHead { .. })) << 3)
            | (u32::from(matches!(block, Block::Air)) << 4);
        let wire_bits = match block {
            Block::RedstoneWire { wire } => {
                u32::from(wire.power)
                    | ((wire.north as u32) << 4)
                    | ((wire.south as u32) << 6)
                    | ((wire.east as u32) << 8)
                    | ((wire.west as u32) << 10)
            }
            _ => 0,
        };
        rows.push(format!("{{.electrical={electrical},.registry_type={},.weak_faces={},.strong_faces={},.solid={},.transparent={},.cube={},.flags={flags},.wire_connect_faces={},.wire_bits={wire_bits}}}", block.registry_id(), emission(block, false), emission(block, true), facts.map_or(block.is_solid(), |p| p.solid) as u8, facts.map_or(block.is_transparent(), |p| p.transparent) as u8, facts.map_or(block.is_cube(), |p| p.cube) as u8, connects(block)));
    }
    array(&mut output, "NEProfile", "ne_profiles", &rows);
    let profile_count = rows.len();

    let comparators = program.comparator_inputs();
    rows.clear();
    for (index, node) in program.nodes().iter().enumerate() {
        let mut data = [0; 4];
        let mut flags = 0;
        let (kind, electrical) = match &node.kind {
            CircuitNodeKind::Electrical(ty) => (
                0,
                match ty {
                    NodeType::Repeater { delay, facing_diode } => {
                        data[0] = u32::from(*delay);
                        flags |= u32::from(*facing_diode);
                        0
                    }
                    NodeType::Torch => 1,
                    NodeType::Comparator { mode, facing_diode, .. } => {
                        data[0] = comparators
                            .iter()
                            .position(|p| p.node.index() == index)
                            .ok_or_else(|| error("missing native comparator plan"))? as u32;
                        data[1] = u32::from(*mode == ComparatorMode::Subtract);
                        flags |= u32::from(*facing_diode) << 1;
                        2
                    }
                    NodeType::Lamp => 3,
                    NodeType::CopperBulb => 4,
                    NodeType::Hopper => 5,
                    NodeType::Button => 6,
                    NodeType::Lever => 7,
                    NodeType::PressurePlate => 8,
                    NodeType::Constant => 10,
                    NodeType::CommandBlock { .. } | NodeType::NoteBlock { .. } | NodeType::Trapdoor | NodeType::Observer { .. } | NodeType::Wire => {
                        return Err(error(format!("whole-graph C demo does not yet support {ty:?} at {:?}", node.position)));
                    }
                },
            ),
            CircuitNodeKind::Observer { watched, facing } => {
                data[0] = facing.get_id();
                data[1] = watched.index() as u32;
                (1, 0)
            }
            CircuitNodeKind::BuiltinReset { watched, facing } => {
                data[0] = facing.get_id();
                data[1] = watched.index() as u32;
                (2, 0)
            }
            CircuitNodeKind::Wire(i) => {
                data[0] = *i as u32;
                (3, 0)
            }
            CircuitNodeKind::Piston(i) => {
                data[0] = i.index() as u32;
                (4, 0)
            }
            CircuitNodeKind::Contact(i) => {
                data[0] = i.index() as u32;
                (5, 0)
            }
        };
        let block = node.block.unwrap_or(Block::Air);
        rows.push(format!("{{.kind={kind},.electrical_kind={electrical},.flags={flags},.kind_data={{{},{},{},{}}},.output_route={},.profile={},.registry_type={},.raw_state={},.weak_faces={},.strong_faces={},.wire_connect_faces={},.position={}}}", data[0], data[1], data[2], data[3], id(node.output_route.map(|r| r.index())), node_profiles[index], block.registry_id(), node.raw_state.unwrap_or(0), emission(block, false), emission(block, true), connects(block), position(node.position.unwrap_or(BlockPos::new(0,0,0)))));
    }
    array(&mut output, "NENode", "ne_nodes", &rows);

    let mut children = Vec::new();
    rows.clear();
    for guard in program.guards() {
        let (kind, a, b, start, count, view, test) = match guard {
            Guard::Always => (0, 0, 0, 0, 0, 0, 0),
            Guard::State(s) => {
                let (test, owner, value) = match *s {
                    StateTest::PistonExtended(i) => (0, i.index(), 0),
                    StateTest::PistonHeadPresent(i) => (1, i.index(), 0),
                    StateTest::PistonAvailability { piston, is } => (2, piston.index(), usize::from(availability(is))),
                    StateTest::ResourceAt { resource, contact } => (3, resource.index(), contact.index()),
                    StateTest::ResourceEnabled(i) => (4, i.index(), 0),
                };
                (1, test, owner, value, 0, 0, 0)
            }
            Guard::Site { site, view, test } => {
                let (test, profile) = match test {
                    SiteTest::Solid => (0, 0),
                    SiteTest::Transparent => (1, 0),
                    SiteTest::IsWire => (2, 0),
                    SiteTest::Profile(i) => (3, i.index()),
                };
                (2, site.index(), profile, 0, 0, usize::from(*view == OperandView::Captured), test)
            }
            Guard::Not(child) => (3, child.index(), 0, 0, 0, 0, 0),
            Guard::All(items) | Guard::Any(items) => {
                let start = children.len();
                children.extend(items.iter().map(|i| i.index() as u32));
                (if matches!(guard, Guard::All(_)) { 4 } else { 5 }, 0, 0, start, items.len(), 0, 0)
            }
        };
        let value = if kind == 1 && a == 2 { start } else { 0 };
        rows.push(format!(
            "{{.kind={kind},.a={a},.b={b},.child_start={start},.child_count={count},.view={view},.test={test},.value={value}}}"
        ));
    }
    array(&mut output, "NEGuard", "ne_guards", &rows);
    scalar_array(&mut output, "ne_guard_children", &children);

    rows.clear();
    for site in program.sites() {
        let (kind, index) = match site.source {
            SiteSource::Component(i) => (0, i.index()),
            SiteSource::Contact(i) => (1, i.index()),
            SiteSource::Constant(i) => (2, i.index()),
        };
        rows.push(format!(
            "{{.source_kind={kind},.source_id={index},.source_strength={},.position={}}}",
            site.source_strength,
            position(site.position)
        ));
    }
    array(&mut output, "NESite", "ne_sites", &rows);

    let mut variants = Vec::new();
    rows.clear();
    for contact in program.contacts() {
        let start = variants.len();
        for variant in &contact.variants {
            let (kind, index) = receiver(variant.receiver);
            variants.push(format!(
                "{{.guard={},.profile={},.receiver_kind={kind},.receiver_id={index}}}",
                variant.when.index(),
                variant.profile.index()
            ));
        }
        rows.push(format!(
            "{{.node={},.first_variant={start},.variant_count={},.position={}}}",
            contact.node.index(),
            contact.variants.len(),
            position(contact.position)
        ));
    }
    array(&mut output, "NEContact", "ne_contacts", &rows);
    array(&mut output, "NEContactVariant", "ne_variants", &variants);

    let mut deliveries = Vec::new();
    let mut instant_steps = Vec::new();
    let mut instant_route_count = 0usize;
    rows.clear();
    for (route_index, route) in program.routes().iter().enumerate() {
        let start = deliveries.len();
        let binding = program.instant_route_binding(RouteId::new(route_index as u32));
        instant_route_count += usize::from(binding.is_some());
        let instant_start = binding.map_or(NONE as usize, |_| instant_steps.len());
        if let Some((proxy_index, proxy_route_index)) = binding {
            let Some(instant_route) = program
                .instant_proxies()
                .get(proxy_index as usize)
                .and_then(|proxy| proxy.routes.get(proxy_route_index as usize))
            else {
                return Err(error(format!("invalid instant route binding for route {route_index}")));
            };
            if instant_route.route.index() != route_index {
                return Err(error(format!("instant route binding points at the wrong route for {route_index}")));
            }
            if !route.deliveries.is_empty()
                && (route.deliveries.len() != instant_route.steps.len()
                    || route
                        .deliveries
                        .iter()
                        .zip(&instant_route.steps)
                        .any(|(original, step)| *original != step.delivery))
            {
                return Err(error(format!("instant route {route_index} diverges from its original delivery range")));
            }
            for (step_index, step) in instant_route.steps.iter().enumerate() {
                let delivery = deliveries.len() as u32;
                /* Lowering clears bound `Route.deliveries`; the ordered step
                witnesses retain those exact original rows. Turbo routes
                are excluded from proxy binding, so their offsets stay raw. */
                deliveries.push(delivery_row(route.deliveries.get(step_index).copied().unwrap_or(step.delivery)));
                let (kind, operand) = match step.kind {
                    InstantStepKind::Dispatch => (0, NONE),
                    InstantStepKind::Sample(piston) => (1, piston.index() as u32),
                    InstantStepKind::ArmReset(node) => (2, node.index() as u32),
                };
                instant_steps.push(format!("{{.delivery={delivery},.operand={operand},.kind={kind}}}"));
            }
        } else {
            deliveries.extend(route.deliveries.iter().copied().map(delivery_row));
        }
        rows.push(format!(
            "{{.node={},.first_delivery={start},.delivery_count={},.first_instant_step={},.instant_step_count={}}}",
            route.source.index(),
            if binding.is_some() {
                instant_steps.len() - instant_start
            } else {
                route.deliveries.len()
            },
            if binding.is_some() { instant_start } else { NONE as usize },
            if binding.is_some() { instant_steps.len() - instant_start } else { 0 }
        ));
    }
    array(&mut output, "NERoute", "ne_routes", &rows);
    array(&mut output, "NEDelivery", "ne_deliveries", &deliveries);
    array(&mut output, "NEInstantStep", "ne_instant_steps", &instant_steps);

    let classic = program.classic();
    let mut classic_nets = Vec::new();
    for net in &classic.nets {
        if net.source.index() >= program.nodes().len()
            || net.wires.start > net.wires.end
            || net.wires.end > classic.wires.len()
            || net.inputs.start > net.inputs.end
            || net.inputs.end > classic.inputs.len()
            || net.entries.start > net.entries.end
            || net.entries.end > classic.entries.len()
        {
            return Err(error("invalid classic net table range"));
        }
        classic_nets.push(format!(
            "{{.source={},.first_wire={},.wire_count={},.first_input={},.input_count={},.first_entry={},.entry_count={}}}",
            net.source.index(),
            net.wires.start,
            net.wires.end - net.wires.start,
            net.inputs.start,
            net.inputs.end - net.inputs.start,
            net.entries.start,
            net.entries.end - net.entries.start
        ));
    }
    array(&mut output, "NEClassicNet", "ne_classic_nets", &classic_nets);

    rows.clear();
    for (net_index, net) in classic.nets.iter().enumerate() {
        for wire in &classic.wires[net.wires.clone()] {
            rows.push(format!(
                "{{.node={},.net={net_index},.attenuation={}}}",
                wire.node.index(),
                wire.attenuation
            ));
        }
    }
    array(&mut output, "NEClassicWire", "ne_classic_wires", &rows);

    rows.clear();
    for (net_index, net) in classic.nets.iter().enumerate() {
        for input in &classic.inputs[net.inputs.clone()] {
            rows.push(format!(
                "{{.receiver={},.net={net_index},.attenuation={},.source_attenuation={}}}",
                input.receiver.index(),
                input.attenuation,
                input.source_attenuation
            ));
        }
    }
    array(&mut output, "NEClassicInput", "ne_classic_inputs", &rows);

    rows.clear();
    for entry in &classic.entries {
        if entry.net as usize >= classic.nets.len()
            || entry.ops.start > entry.ops.end
            || entry.ops.end > classic.ops.len()
        {
            return Err(error("invalid classic entry table range"));
        }
        rows.push(format!(
            "{{.net={},.first_op={},.op_count={},.commit={}}}",
            entry.net,
            entry.ops.start,
            entry.ops.end - entry.ops.start,
            u8::from(entry.commit)
        ));
    }
    array(&mut output, "NEClassicEntry", "ne_classic_entries", &rows);

    rows.clear();
    for op in &classic.ops {
        let masks = op.when.iter().map(|mask| format!("UINT64_C({mask})")).collect::<Vec<_>>().join(",");
        rows.push(format!(
            "{{.receiver={},.nonzero={},.when={{{masks}}}}}",
            op.receiver.index(),
            u8::from(op.nonzero)
        ));
    }
    array(&mut output, "NEClassicOp", "ne_classic_ops", &rows);

    if classic.route_ranges.len() != program.routes().len() && !(classic.nets.is_empty() && classic.route_ranges.is_empty()) {
        return Err(error("classic route ranges do not match the route table"));
    }
    let mut classic_route_ranges = Vec::with_capacity(program.routes().len());
    rows.clear();
    for route_index in 0..program.routes().len() {
        let range = classic.route_ranges.get(route_index).cloned().unwrap_or(0..0);
        let route = &program.routes()[route_index];
        let instant = program.instant_route_binding(RouteId::new(route_index as u32));
        if range.start != range.end && instant.is_some() {
            return Err(error(format!("classic and instant proxy both own route {route_index}")));
        }
        if range.start > range.end || range.end > classic.route_entries.len() {
            return Err(error(format!("invalid classic route range for route {route_index}")));
        }
        let start = rows.len();
        let mut previous_ordinal = None;
        for entry in &classic.route_entries[range.clone()] {
            if entry.ordinal as usize >= route.deliveries.len() || entry.entry as usize >= classic.entries.len() {
                return Err(error(format!("invalid classic route entry for route {route_index}")));
            }
            if previous_ordinal.is_some_and(|previous| previous >= entry.ordinal) {
                return Err(error(format!("classic route entries are not strictly ordered for route {route_index}")));
            }
            previous_ordinal = Some(entry.ordinal);
            rows.push(format!("{{.ordinal={},.entry={}}}", entry.ordinal, entry.entry));
        }
        let count = range.end - range.start;
        classic_route_ranges.push(format!("{{.start={start},.count={count}}}"));
    }
    array(&mut output, "NERange", "ne_classic_route_ranges", &classic_route_ranges);
    array(&mut output, "NEClassicRouteEntry", "ne_classic_route_entries", &rows);

    let mut classic_wire_by_node = vec![None; program.nodes().len()];
    if !classic.wire_by_node.is_empty() {
        if classic.wire_by_node.len() != program.nodes().len() {
            return Err(error("classic wire bindings do not match the node table"));
        }
        classic_wire_by_node.clone_from_slice(&classic.wire_by_node);
    }
    rows.clear();
    for binding in classic_wire_by_node {
        rows.push(match binding {
            Some((net, attenuation)) => format!("{{.net={net},.attenuation={attenuation}}}"),
            None => "{.net=NE_NONE}".into(),
        });
    }
    array(&mut output, "NEClassicWireBinding", "ne_classic_wire_by_node", &rows);

    let mut classic_input_by_node = vec![None; program.nodes().len()];
    if !classic.input_by_node.is_empty() {
        if classic.input_by_node.len() != program.nodes().len() {
            return Err(error("classic input bindings do not match the node table"));
        }
        classic_input_by_node.clone_from_slice(&classic.input_by_node);
    }
    rows.clear();
    for binding in classic_input_by_node {
        rows.push(match binding {
            Some((net, attenuation, source_attenuation)) => {
                format!("{{.net={net},.attenuation={attenuation},.source_attenuation={source_attenuation}}}")
            }
            None => "{.net=NE_NONE}".into(),
        });
    }
    array(&mut output, "NEClassicInputBinding", "ne_classic_input_by_node", &rows);

    rows.clear();
    let mut inputs = vec![(0, 0); program.nodes().len() * 3];
    let mut conductors = vec![(0, 0); program.sites().len() * 2];
    for (index, link) in program.power_links().iter().enumerate() {
        let (target_kind, target, target_port, dust) = match link.to {
            PowerTarget::Input { node, port } => {
                let p = port_index(port);
                let range = &mut inputs[node.index() * 3 + p];
                if range.1 == 0 {
                    range.0 = index;
                }
                range.1 += 1;
                (0, node.index(), p, false)
            }
            PowerTarget::Conductor { site, include_dust } => {
                let range = &mut conductors[site.index() * 2 + usize::from(include_dust)];
                if range.1 == 0 {
                    range.0 = index;
                }
                range.1 += 1;
                (1, site.index(), 0, include_dust)
            }
        };
        let (port, face, source_dust) = match link.from_port {
            PowerPort::Output => (0, 0, false),
            PowerPort::Stored => (1, 0, false),
            PowerPort::Weak(f) => (2, f.get_id(), false),
            PowerPort::Strong(f) => (3, f.get_id(), false),
            PowerPort::Conducted { include_dust } => (4, 0, include_dust),
        };
        rows.push(format!("{{.from_site={},.guard={},.target_id={target},.from_port={port},.face={face},.attenuation={},.target_kind={target_kind},.target_port={target_port},.include_dust={}}}", link.from.index(), link.guard.index(), link.attenuation, u8::from(source_dust)));
        let _ = dust; // Target dust selection is represented by the dense range binding.
    }
    array(&mut output, "NEPowerLink", "ne_power", &rows);
    array(
        &mut output,
        "NERange",
        "ne_inputs",
        &inputs.iter().map(|(start, count)| format!("{{{start},{count}}}")).collect::<Vec<_>>(),
    );
    array(
        &mut output,
        "NERange",
        "ne_conductors",
        &conductors.iter().map(|(start, count)| format!("{{{start},{count}}}")).collect::<Vec<_>>(),
    );

    let mut dust_path_bindings = vec![(NONE, NONE); program.wires().len()];
    let mut dust_path_wires = Vec::new();
    rows.clear();
    for (path_index, path) in program.dust_paths().iter().enumerate() {
        let first_wire = dust_path_wires.len();
        for (position, node) in path.wires.iter().enumerate() {
            let CircuitNodeKind::Wire(wire_index) = program.nodes()[node.index()].kind else {
                return Err(error(format!("dust path {path_index} contains a non-wire node")));
            };
            if wire_index >= dust_path_bindings.len() || dust_path_bindings[wire_index].0 != NONE {
                return Err(error(format!("dust path {path_index} has an invalid or duplicate wire binding")));
            }
            dust_path_bindings[wire_index] = (path_index as u32, position as u32);
            dust_path_wires.push(wire_index as u32);
        }
        rows.push(format!(
            "{{.input_node={},.input_site={},.first_wire={first_wire},.wire_count={},.input_face={}}}",
            path.input_node.index(),
            path.input_site.index(),
            path.wires.len(),
            path.input_face.get_id()
        ));
    }
    array(&mut output, "NEDustPath", "ne_dust_paths", &rows);
    scalar_array(&mut output, "ne_dust_path_wires", &dust_path_wires);

    let wire_query_fact_count = program.wires().iter().filter(|wire| wire.execution.query_facts.is_some()).count();
    let wire_transfer_count = program
        .wires()
        .iter()
        .filter(|wire| wire.execution.query_facts.is_some_and(|facts| facts.transfer.is_some()))
        .count();
    let wire_isolated_count = program
        .wires()
        .iter()
        .filter(|wire| wire.execution.query_facts.is_some_and(|facts| facts.isolated))
        .count();
    let wire_instant_reset_count = program.wires().iter().filter(|wire| wire.execution.instant_reset.is_some()).count();
    rows.clear();
    for (index, wire) in program.wires().iter().enumerate() {
        let offsets = program.turbo_callback_index(index).map_or([NONE; 25], |i| i.offsets);
        let (mut flags, mut known, mut solid, mut transparent, mut is_wire, mut zero_no_dust) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
        let (mut seed_direct, mut seed_conditional, mut turbo_direct, mut turbo_conditional) = (0u32, 0u32, 0u32, 0u32);
        if let Some(facts) = wire.execution.query_facts {
            flags |= 1;
            known = facts.known;
            solid = facts.solid;
            transparent = facts.transparent;
            is_wire = facts.wire;
            zero_no_dust = facts.zero_no_dust;
            if let Some(transfer) = facts.transfer {
                flags |= 1 << 1;
                seed_direct = transfer.seed.direct;
                seed_conditional = transfer.seed.conditional;
                turbo_direct = transfer.turbo.direct;
                turbo_conditional = transfer.turbo.conditional;
            }
            if facts.isolated {
                flags |= 1 << 2;
            }
        }
        let mut instant_partner = NONE;
        let mut instant_piston = NONE;
        let mut instant_resource = NONE;
        let mut instant_observer = NONE;
        if let Some(plan) = wire.execution.instant_reset {
            let CircuitNodeKind::Wire(partner) = program.nodes()[plan.partner.index()].kind else {
                return Err(error(format!("instant reset for wire {index} has a non-wire partner")));
            };
            flags |= 1 << 3;
            instant_partner = partner as u32;
            instant_piston = plan.piston.index() as u32;
            instant_resource = plan.resource.index() as u32;
            instant_observer = plan.observer.index() as u32;
        }
        let (dust_path, dust_path_position) = dust_path_bindings[index];
        rows.push(format!(
            "{{.node={},.neighbors={{{}}},.turbo_offsets={{{}}},.shape_route={},.turbo_route={},.query_known={known},.query_solid={solid},.query_transparent={transparent},.query_wire={is_wire},.query_zero_no_dust={zero_no_dust},.seed_direct={seed_direct},.seed_conditional={seed_conditional},.turbo_direct={turbo_direct},.turbo_conditional={turbo_conditional},.dust_path={dust_path},.dust_path_position={dust_path_position},.instant_partner={instant_partner},.instant_piston={instant_piston},.instant_resource={instant_resource},.instant_observer={instant_observer},.flags={flags}}}",
            wire.node.index(),
            wire.canonical_neighbors.iter().map(|n| n.index().to_string()).collect::<Vec<_>>().join(","),
            offsets.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
            id(wire.shape_route.map(|r| r.index())),
            id(wire.turbo_route.map(|r| r.index()))
        ));
    }
    array(&mut output, "NEWire", "ne_wires", &rows);
    rows.clear();
    for comparator in comparators {
        let (direct_node, direct_strength, direct_live) = override_value(comparator.direct_override);
        let (far_site, far_value, far_guard) = comparator.far_override.map_or((NONE, None, NONE), |(site, value, guard)| {
            (site.index() as u32, Some(value), guard.index() as u32)
        });
        let (far_node, far_strength, far_live) = override_value(far_value);
        let mode = match program.nodes()[comparator.node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => u8::from(mode == ComparatorMode::Subtract),
            _ => return Err(error("native comparator node mismatch")),
        };
        rows.push(format!("{{.node={},.rear_site={},.side_port=1,.far_site={far_site},.direct_override_node={direct_node},.far_override_node={far_node},.far_override_guard={far_guard},.mode={mode},.far_threshold={},.direct_override_strength={direct_strength},.far_override_strength={far_strength},.direct_override_live={direct_live},.has_direct_override={},.far_override_live={far_live},.has_far_override={}}}", comparator.node.index(), comparator.rear.index(), comparator.far_threshold, u8::from(comparator.direct_override.is_some()), u8::from(comparator.far_override.is_some())));
    }
    array(&mut output, "NEComparator", "ne_comparators", &rows);

    rows.clear();
    for p in program.pistons() {
        let r = p.routes;
        let routes = [
            Some(r.base_changed),
            Some(r.near_changed),
            r.far_changed,
            r.far_shape_changed,
            Some(r.base_restored),
            Some(r.head_restored),
            r.resource_restored_near,
            r.resource_restored_far,
        ]
        .map(|r| id(r.map(|r| r.index())))
        .join(",");
        let (lane, far) = match p.lane {
            PistonLane::Empty => (0, NONE),
            PistonLane::SingleResource { far } => (1, far.index() as u32),
        };
        rows.push(format!("{{.node={},.base_contact={},.near_contact={},.far_contact={},.lane_kind={lane},.lane_far_contact={far},.facing={},.flags={},.routes={{{routes}}},.position={}}}", p.node.index(), p.base.index(), p.near.index(), p.far.index(), p.facing.get_id(), u8::from(p.sticky), position(p.position)));
    }
    array(&mut output, "NEPiston", "ne_pistons", &rows);
    let mut resource_contacts = Vec::new();
    rows.clear();
    for resource in program.resources() {
        let start = resource_contacts.len();
        resource_contacts.extend(resource.contacts.iter().map(|c| c.index() as u32));
        rows.push(format!(
            "{{.material_profile={},.first_contact={start},.contact_count={}}}",
            resource.material.index(),
            resource.contacts.len()
        ));
    }
    array(&mut output, "NEResource", "ne_resources", &rows);
    scalar_array(&mut output, "ne_resource_contacts", &resource_contacts);
    scalar_array(
        &mut output,
        "ne_resource_for_piston",
        &(0..program.pistons().len())
            .map(|i| program.resource_for_piston(PistonId::new(i as u32)).map_or(NONE, |r| r.index() as u32))
            .collect::<Vec<_>>(),
    );
    let mut component_sites = vec![NONE; program.nodes().len()];
    for (i, site) in program.sites().iter().enumerate() {
        if let SiteSource::Component(n) = site.source {
            component_sites[n.index()] = i as u32;
        }
    }
    scalar_array(&mut output, "ne_component_sites", &component_sites);
    let mut contact_counts = [0; 3];
    for (name, kind) in [("resources", 0), ("bases", 1), ("heads", 2)] {
        let mut offsets = vec![0];
        let mut members = Vec::new();
        for index in 0..program.contacts().len() {
            let c = ContactId::new(index as u32);
            match kind {
                0 => members.extend(program.resources_at_contact(c).iter().map(|i| i.index() as u32)),
                1 => members.extend(program.base_pistons_at_contact(c).iter().map(|i| i.index() as u32)),
                _ => members.extend(program.head_pistons_at_contact(c).iter().map(|i| i.index() as u32)),
            }
            offsets.push(members.len() as u32);
        }
        scalar_array(&mut output, &format!("ne_{name}_offsets"), &offsets);
        scalar_array(&mut output, &format!("ne_{name}_at_contact"), &members);
        contact_counts[kind] = members.len();
    }
    writeln!(output, "static const NEProgram ne_program = {{.node_count={},.site_count={},.route_count={},.delivery_count={},.instant_step_count={},.guard_count={},.guard_child_count={},.profile_count={profile_count},.contact_count={},.contact_variant_count={},.power_link_count={},.wire_count={},.wire_query_fact_count={wire_query_fact_count},.wire_transfer_count={wire_transfer_count},.wire_isolated_count={wire_isolated_count},.wire_instant_reset_count={wire_instant_reset_count},.instant_route_count={instant_route_count},.dust_path_count={},.dust_path_wire_count={},.comparator_count={},.piston_count={},.resource_count={},.resource_contact_count={},.classic_net_count={},.classic_wire_count={},.classic_input_count={},.classic_entry_count={},.classic_op_count={},.classic_route_entry_count={},", program.nodes().len(), program.sites().len(), program.routes().len(), deliveries.len(), instant_steps.len(), program.guards().len(), children.len(), program.contacts().len(), variants.len(), program.power_links().len(), program.wires().len(), program.dust_paths().len(), dust_path_wires.len(), comparators.len(), program.pistons().len(), program.resources().len(), resource_contacts.len(), classic.nets.len(), classic.wires.len(), classic.inputs.len(), classic.entries.len(), classic.ops.len(), classic.route_entries.len()).unwrap();
    writeln!(output, ".resources_at_contact_count={},.base_pistons_at_contact_count={},.head_pistons_at_contact_count={},.sound_button_off={},.sound_bulb_on={},.sound_bulb_off={},.component_sites=ne_component_sites,", contact_counts[0], contact_counts[1], contact_counts[2], crate::sound::event_id("block.stone_button.click_off"), crate::sound::event_id("block.copper_bulb.turn_on"), crate::sound::event_id("block.copper_bulb.turn_off")).unwrap();
    for (field, value) in [
        ("nodes", "ne_nodes"),
        ("routes", "ne_routes"),
        ("deliveries", "ne_deliveries"),
        ("instant_steps", "ne_instant_steps"),
        ("dust_paths", "ne_dust_paths"),
        ("dust_path_wires", "ne_dust_path_wires"),
        ("guards", "ne_guards"),
        ("guard_children", "ne_guard_children"),
        ("profiles", "ne_profiles"),
        ("sites", "ne_sites"),
        ("contacts", "ne_contacts"),
        ("contact_variants", "ne_variants"),
        ("power_links", "ne_power"),
        ("input_power_ranges", "ne_inputs"),
        ("conductor_power_ranges", "ne_conductors"),
        ("wires", "ne_wires"),
        ("comparators", "ne_comparators"),
        ("pistons", "ne_pistons"),
        ("resources", "ne_resources"),
        ("resource_contacts", "ne_resource_contacts"),
        ("resource_for_piston", "ne_resource_for_piston"),
        ("resources_at_contact_offsets", "ne_resources_offsets"),
        ("resources_at_contact", "ne_resources_at_contact"),
        ("base_pistons_at_contact_offsets", "ne_bases_offsets"),
        ("base_pistons_at_contact", "ne_bases_at_contact"),
        ("head_pistons_at_contact_offsets", "ne_heads_offsets"),
        ("head_pistons_at_contact", "ne_heads_at_contact"),
        ("classic_nets", "ne_classic_nets"),
        ("classic_wires", "ne_classic_wires"),
        ("classic_inputs", "ne_classic_inputs"),
        ("classic_entries", "ne_classic_entries"),
        ("classic_ops", "ne_classic_ops"),
        ("classic_route_ranges", "ne_classic_route_ranges"),
        ("classic_route_entries", "ne_classic_route_entries"),
        ("classic_wire_by_node", "ne_classic_wire_by_node"),
        ("classic_input_by_node", "ne_classic_input_by_node"),
    ] {
        writeln!(output, ".{field}={value},").unwrap();
    }
    output.push_str("};\n");
    Ok(output)
}

fn id(value: Option<usize>) -> String {
    value.map_or_else(|| "NE_NONE".into(), |i| i.to_string())
}
fn position(p: BlockPos) -> String {
    format!("{{{},{},{}}}", p.x, p.y, p.z)
}
fn port_index(p: InputPort) -> usize {
    match p {
        InputPort::Main => 0,
        InputPort::Side => 1,
        InputPort::Quasi => 2,
    }
}
fn delivery_row(delivery: RouteDelivery) -> String {
    let (kind, face) = match delivery.kind {
        DeliveryKind::Update { direction } => (0, direction.map_or(255, |d| d.get_id())),
        DeliveryKind::Shape { direction } => (1, direction.get_id()),
    };
    let (receiver_kind, receiver_id) = receiver(delivery.receiver);
    format!(
        "{{.target_site={},.guard={},.receiver_id={receiver_id},.kind={kind},.direction={face},.view={},.receiver_kind={receiver_kind}}}",
        delivery.target.index(),
        delivery.guard.index(),
        u8::from(delivery.view == OperandView::Captured)
    )
}
fn receiver(r: Option<CallbackReceiver>) -> (u8, u32) {
    match r {
        None => (0, NONE),
        Some(CallbackReceiver::Component(i)) => (1, i.index() as u32),
        Some(CallbackReceiver::PistonBase(i)) => (2, i.index() as u32),
        Some(CallbackReceiver::PistonHead(i)) => (3, i.index() as u32),
        Some(CallbackReceiver::Contact(i)) => (4, i.index() as u32),
    }
}
fn emission(block: Block, strong: bool) -> u32 {
    BlockFace::values()
        .into_iter()
        .filter(|f| {
            if strong {
                crate::redpiler::power_rules::emits_strong(block, *f)
            } else {
                crate::redpiler::power_rules::emits_weak(block, *f)
            }
        })
        .fold(0, |mask, f| mask | (1 << f.get_id()))
}
fn connects(block: Block) -> u32 {
    [
        mchprs_blocks::BlockDirection::North,
        mchprs_blocks::BlockDirection::South,
        mchprs_blocks::BlockDirection::East,
        mchprs_blocks::BlockDirection::West,
    ]
    .into_iter()
    .filter(|d| crate::redstone::wire::can_connect_to_uncached(block, *d))
    .fold(0, |mask, d| mask | (1 << d.block_face().get_id()))
}
fn override_value(value: Option<ComparatorOverride>) -> (u32, u8, u8) {
    match value {
        None => (NONE, 0, 0),
        Some(ComparatorOverride::Literal(s)) => (NONE, s, 0),
        Some(ComparatorOverride::LiveComponentOutput(n)) => (n.index() as u32, 0, 1),
    }
}
fn array(output: &mut String, ty: &str, name: &str, rows: &[String]) {
    writeln!(output, "static const {ty} {name}[{}] = {{", rows.len().max(1)).unwrap();
    if rows.is_empty() {
        output.push_str("{0}\n");
    } else {
        for row in rows {
            writeln!(output, "{row},").unwrap();
        }
    }
    output.push_str("};\n");
}
fn scalar_array(output: &mut String, name: &str, values: &[u32]) {
    writeln!(
        output,
        "static const uint32_t {name}[{}] = {{{}}};",
        values.len().max(1),
        if values.is_empty() {
            "0".into()
        } else {
            values.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        }
    )
    .unwrap();
}
