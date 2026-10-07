//! Local power functions and notification sensors for sampled state networks.
//! Storage and observer boundaries remain explicit instead of expanding a CPU
//! into one combinational expression or executing interpreter callbacks.
use super::*;

pub(crate) struct Extraction {
    pub logic: WaveLogic,
    pub sensors: Vec<(BlockPos, Vec<PowerTerm>, RedstoneWire, Vec<(Expr, RedstoneWire)>)>,
    pub payloads: Vec<Block>,
}

pub(crate) fn extract(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
) -> Result<Extraction, String> {
    let mut extractor = Extractor {
        world, report, monitor,
        // Local expressions leave temporary decisions before the final compaction.
        arena: BooleanArena::with_budget((4 * monitor.budget_multiplier()).min(8)),
        far: Default::default(), near: Default::default(), bases: Default::default(),
        group_of: vec![0; report.pistons.len()], payloads: Vec::new(),
        shapes: Default::default(), wires: Default::default(), sources: Default::default(),
        steps: 0, signal_order: Default::default(), output_mode: true,
        terms: Vec::new(), context: Default::default(), memory: Default::default(),
        sequential: true,
        observers: report.observers.iter().enumerate().map(|(i, &pos)| (pos, i)).collect(),
    };
    for (group, descriptor) in report.payload_groups.iter().enumerate() {
        for &pos in &descriptor.positions {
            let block = world.get_block(pos);
            let saved_head = descriptor.members.iter().any(|&actor| {
                let p = &report.pistons[actor];
                p.head == pos && matches!(block, Block::PistonHead { .. })
            });
            if block != Block::Air && !supported_payload(block) && !saved_head {
                return Err(format!("unsupported sampled payload minecraft:{} at {pos:?}, group {group}", block.get_name()));
            }
        }
        let payloads: Vec<_> = descriptor.positions.iter().filter_map(|&pos| {
            let block = world.get_block(pos);
            supported_payload(block).then_some(block)
        }).collect();
        let empty = descriptor.members.iter().all(|&actor| !report.pistons[actor].piston.sticky);
        if payloads.len() != 1 && !(empty && payloads.is_empty()) {
            return Err(format!("sampled payload group {group} needs one supported payload or an empty ordinary generator; found {}", payloads.len()));
        }
        let payload = payloads.first().copied().unwrap_or(Block::Air);
        extractor.payloads.push(payload);
        let home = report.pistons[descriptor.members[0]].head.offset(report.pistons[descriptor.members[0]].piston.facing.into());
        for &actor in &descriptor.members {
            let p = &report.pistons[actor];
            let far = p.head.offset(p.piston.facing.into());
            if far != home { return Err(format!("sampled payload group at {:?} has multiple far destinations", p.pos)); }
            if payload != Block::Air { extractor.far.insert(far, group); }
            extractor.near.insert(p.head, actor);
            extractor.bases.insert(p.pos, actor);
            extractor.group_of[actor] = group;
        }
    }
    let mut responses = Vec::with_capacity(report.pistons.len());
    for (actor, p) in report.pistons.iter().enumerate() {
        extractor.terms.clear();
        extractor.piston_power(actor, p)?;
        let terms = std::mem::take(&mut extractor.terms);
        let mut power = FALSE;
        for term in terms {
            let source = if let Some(pos) = term.source {
                extractor.sources.insert(pos);
                let next = extractor.signal_order.len();
                let order = *extractor.signal_order.entry(pos).or_insert(next);
                extractor.arena.variable(Variable::Signal { pos, threshold: term.attenuation, order })
            } else { TRUE };
            let powered = extractor.arena.and(term.guard, source);
            power = extractor.arena.or(power, powered);
        }
        responses.push(extractor.arena.not(power));
    }
    let mut wires = extractor.wires.clone();
    let mut consumers = Vec::new();
    crate::world::for_each_block_optimized(world, report.bounds.0, report.bounds.1, |pos| {
        let block = world.get_block(pos);
        if crate::redpiler::analysis::ports::is_consumer(block) { consumers.push((pos, block)); }
    });
    let mut candidates = Vec::new();
    for (pos, block) in consumers {
        for input in [crate::redpiler::analysis::ports::ConsumerInput::Main, crate::redpiler::analysis::ports::ConsumerInput::ComparatorSide] {
            extractor.terms.clear();
            let mut power = FALSE;
            let mut queue = VecDeque::new();
            for (root, face, channel) in crate::redpiler::analysis::ports::consumer_roots(block, pos) {
                if channel == input { extractor.consumer_signal(block, input, root, face, &mut power, &mut queue)?; }
            }
            extractor.walk_wires(usize::MAX, power, queue)?;
            if !extractor.terms.iter().any(|term| term.guard > TRUE || term.source.is_some_and(|pos| matches!(world.get_block(pos), Block::RedstoneWire { .. }))) { continue; }
            let terms = std::mem::take(&mut extractor.terms);
            candidates.push(OutputPort { consumer: pos, input, initial_strength: entry_strength(&extractor, &terms), terms });
        }
    }
    wires.extend(report.ports.outputs.iter().flat_map(|port| port.dependencies.wires.iter().copied()));
    wires.extend(report.ports.pistons.iter().flat_map(|p| p.updates.iter().filter_map(|u| {
        matches!(world.get_block(u.source), Block::RedstoneWire { .. }).then_some(u.source)
    })));
    // Geometry notifications can clear saved wire power even when no power
    // dependency passes through the moving payload (for example, quartz).
    for p in &report.pistons {
        for pos in [p.pos, p.head, p.head.offset(p.piston.facing.into())] {
            wires.extend(BlockFace::values().into_iter().map(|face| pos.offset(face))
                .filter(|&pos| matches!(world.get_block(pos), Block::RedstoneWire { .. })));
        }
    }
    for &pos in &report.observers {
        let Block::Observer { observer } = world.get_block(pos) else { unreachable!() };
        let watched = pos.offset(observer.facing.into());
        if matches!(world.get_block(watched), Block::RedstoneWire { .. }) { wires.insert(watched); }
    }
    let mut wires: Vec<_> = wires.into_iter().collect();
    wires.sort_by_key(|p| (p.y, p.z, p.x));
    let mut sensors = Vec::new();
    let mut outputs = Vec::new();
    let mut indexed: FxHashSet<_> = wires.iter().copied().collect();
    let mut cursor = 0;
    loop {
        let mut pending = Vec::new();
        for output in candidates {
            if output.terms.iter().any(|term| super::super::sequential::dependencies(&extractor.arena, [term.guard]).iter().any(|v| matches!(v, Variable::Geometry { .. } | Variable::Observer(_))) || term.source.is_some_and(|pos| indexed.contains(&pos))) {
                for pos in output.terms.iter().filter_map(|term| term.source) {
                    if matches!(world.get_block(pos), Block::RedstoneWire { .. }) && indexed.insert(pos) { wires.push(pos); }
                }
                outputs.push(output);
            } else { pending.push(output); }
        }
        candidates = pending;
        if cursor == wires.len() { break; }
        while cursor < wires.len() {
        let pos = wires[cursor];
        cursor += 1;
        extractor.terms.clear();
        extractor.walk_wires(usize::MAX, FALSE, VecDeque::from([(pos, 0, TRUE)]))?;
        let terms = std::mem::take(&mut extractor.terms);
        let Block::RedstoneWire { wire } = world.get_block(pos) else { unreachable!() };
        for pos in terms.iter().filter_map(|t| t.source) {
            if matches!(world.get_block(pos), Block::RedstoneWire { .. }) && indexed.insert(pos) { wires.push(pos); }
        }
        let mut positions = vec![pos.offset(BlockFace::Top)];
        for face in BlockFace::values().into_iter().filter(|face| face.is_horizontal()) {
            let neighbor = pos.offset(face);
            positions.extend([neighbor, neighbor.offset(BlockFace::Top), neighbor.offset(BlockFace::Bottom)]);
        }
        let shapes = extractor.output_wire_shapes(pos, wire, &positions, true)?;
        sensors.push((pos, terms, wire, shapes));
        }
    }
    // Keep graph sources from power, outputs and independent notification nets.
    extractor.sources = super::super::sequential::dependencies(&extractor.arena, responses.iter().copied()).into_iter().filter_map(|variable| match variable {
        Variable::Signal { pos, .. } => Some(pos), _ => None,
    }).collect();
    extractor.sources.extend(outputs.iter().flat_map(|o| o.terms.iter().filter_map(|t| t.source)));
    extractor.sources.extend(sensors.iter().flat_map(|(_, terms, _, _)| terms.iter().filter_map(|t| t.source)));
    let mut roots = responses;
    let response_count = roots.len();
    roots.extend(outputs.iter().flat_map(|o| o.terms.iter().map(|t| t.guard)));
    roots.extend(sensors.iter().flat_map(|(_, terms, _, shapes)| terms.iter().map(|t| t.guard).chain(shapes.iter().map(|&(guard, _)| guard))));
    let arena = extractor.arena.compact(&mut roots);
    let mut guards = roots[response_count..].iter().copied();
    for output in &mut outputs { for term in &mut output.terms { term.guard = guards.next().unwrap(); } }
    for (_, terms, _, shapes) in &mut sensors {
        for term in terms { term.guard = guards.next().unwrap(); }
        for (guard, _) in shapes { *guard = guards.next().unwrap(); }
    }
    roots.truncate(response_count);
    let mut sources: Vec<_> = extractor.sources.into_iter().filter(|pos| !indexed.contains(pos)).collect();
    sources.sort_by_key(|p| (p.y, p.z, p.x));
    Ok(Extraction { payloads: extractor.payloads, sensors,
        logic: WaveLogic { arena, responses: roots, response_sources: sources.clone(), sources,
            wires: indexed, consumer_wires: Default::default(), outputs,
            follows_payload: vec![false; report.pistons.len()], context: extractor.context },
    })
}

fn entry_strength<W: World>(extractor: &Extractor<'_, W>, terms: &[PowerTerm]) -> u8 {
    terms.iter().filter(|term| extractor.arena.evaluate(term.guard, |variable| match variable {
        Variable::Geometry { actor, part } => {
            let p = &extractor.report.pistons[actor];
            match part {
                GeometryPart::FarPayload => extractor.world.get_block(p.head.offset(p.piston.facing.into())) == extractor.payloads[extractor.group_of[actor]],
                GeometryPart::NearPayload => !p.piston.extended && extractor.world.get_block(p.head) == extractor.payloads[extractor.group_of[actor]],
                GeometryPart::Head => matches!(extractor.world.get_block(p.head), Block::PistonHead { .. }),
                GeometryPart::RetractedBase => !p.piston.extended,
                GeometryPart::MovingBase => false,
            }
        }
        Variable::Observer(id) => matches!(extractor.world.get_block(extractor.report.observers[id]), Block::Observer { observer } if observer.powered),
        Variable::WireDot(pos) => matches!(extractor.world.get_block(pos), Block::RedstoneWire { wire } if redstone::wire::is_dot(wire)),
        _ => unreachable!(),
    })).map(|term| term.source.map_or(15, |pos| crate::redstone::source_strength(extractor.world.get_block(pos), extractor.world, pos)).saturating_sub(term.attenuation)).max().unwrap_or(0)
}
