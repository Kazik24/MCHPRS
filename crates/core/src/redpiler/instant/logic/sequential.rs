//! Local power functions and notification sensors for sampled state networks.
//! Storage and observer boundaries remain explicit instead of expanding a CPU
//! into one combinational expression or executing interpreter callbacks.
use super::*;

pub(crate) struct Extraction {
    pub logic: WaveLogic,
    pub sensors: Vec<(
        BlockPos,
        Vec<PowerTerm>,
        RedstoneWire,
        Vec<(Expr, RedstoneWire)>,
    )>,
}

fn extractor<'a, W: World>(
    world: &'a W,
    report: &'a AnalysisReport,
    monitor: &'a TaskMonitor,
) -> Result<Extractor<'a, W>, String> {
    let mut extractor = Extractor {
        world,
        report,
        monitor,
        // Local expressions leave temporary decisions before the final compaction.
        arena: BooleanArena::with_budget((4 * monitor.budget_multiplier()).min(8)),
        far: Default::default(),
        near: Default::default(),
        bases: Default::default(),
        group_of: vec![0; report.pistons.len()],
        payloads: Vec::new(),
        shapes: Default::default(),
        wires: Default::default(),
        sources: Default::default(),
        steps: 0,
        signal_order: Default::default(),
        output_mode: true,
        terms: Vec::new(),
        context: Default::default(),
        memory: Default::default(),
        sequential: true,
        wire_signals: true,
        ideal: false,
        observers: report
            .observers
            .iter()
            .enumerate()
            .map(|(i, &pos)| (pos, i))
            .collect(),
        owned_reset: Default::default(),
    };
    extractor.bases.extend(
        report
            .pistons
            .iter()
            .enumerate()
            .map(|(actor, p)| (p.pos, actor)),
    );
    for (group, descriptor) in report.payload_groups.iter().enumerate() {
        let empty = descriptor
            .members
            .iter()
            .all(|&actor| !report.pistons[actor].piston.sticky);
        let empty_near = empty
            && descriptor.members.iter().all(|&actor| {
                let p = &report.pistons[actor];
                match world.get_block(p.head) {
                    Block::Air => true,
                    Block::PistonHead { head } => {
                        p.piston.extended
                            && head.facing == p.piston.facing
                            && !head.sticky
                            && !head.short
                    }
                    _ => false,
                }
            });
        for &pos in &descriptor.positions {
            let block = world.get_block(pos);
            let saved_head = descriptor.members.iter().any(|&actor| {
                let p = &report.pistons[actor];
                p.head == pos && matches!(block, Block::PistonHead { .. })
            });
            // An ordinary empty head does not pull or reach the next base.
            // Keep that separately owned base as context, rather than material.
            let stationary_base = empty_near
                && matches!(block, Block::Piston { .. })
                && extractor.bases.contains_key(&pos)
                && !descriptor
                    .members
                    .iter()
                    .any(|&actor| report.pistons[actor].head == pos);
            if block != Block::Air && !supported_payload(block) && !saved_head && !stationary_base {
                return Err(format!(
                    "unsupported sampled payload minecraft:{} at {pos:?}, group {group}",
                    block.get_name()
                ));
            }
        }
        let payloads: Vec<_> = descriptor
            .positions
            .iter()
            .filter_map(|&pos| {
                let block = world.get_block(pos);
                supported_payload(block).then_some(block)
            })
            .collect();
        if payloads.len() != 1 && !(empty && payloads.is_empty()) {
            let actor = &report.pistons[descriptor.members[0]];
            return Err(format!("sampled payload group {group} at {:?} needs one supported payload or an empty ordinary generator; found {}", actor.pos, payloads.len()));
        }
        let payload = payloads.first().copied().unwrap_or(Block::Air);
        extractor.payloads.push(payload);
        let home = report.pistons[descriptor.members[0]]
            .head
            .offset(report.pistons[descriptor.members[0]].piston.facing.into());
        for &actor in &descriptor.members {
            let p = &report.pistons[actor];
            let far = p.head.offset(p.piston.facing.into());
            if far != home {
                return Err(format!(
                    "sampled payload group at {:?} has multiple far destinations",
                    p.pos
                ));
            }
            if payload != Block::Air {
                extractor.far.insert(far, group);
            }
            extractor.near.insert(p.head, actor);
            extractor.group_of[actor] = group;
        }
    }
    Ok(extractor)
}

/// QC needs a sampling boundary only where it can add power beyond the direct
/// inputs. Keep receiving dust signals distinct while proving this across poses.
pub(crate) fn feedback_sources(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
    targets: &FxHashSet<BlockPos>,
    internally_driven: &FxHashSet<BlockPos>,
) -> Result<FxHashMap<BlockPos, Vec<BlockPos>>, String> {
    let mut extractor = extractor(world, report, monitor)?;
    let mut feedback = FxHashMap::default();
    for (actor, piston) in report.pistons.iter().enumerate() {
        if !targets.contains(&piston.pos) {
            continue;
        }
        extractor.terms.clear();
        let mut ignored = FALSE;
        let mut roots = VecDeque::new();
        for side in BlockFace::values() {
            if side != BlockFace::from(piston.piston.facing) {
                extractor.signal(
                    actor,
                    piston.pos.offset(side),
                    side,
                    &mut ignored,
                    &mut roots,
                )?;
            }
        }
        let direct = std::mem::take(&mut extractor.terms);
        let direct = extractor.terms_power(&direct);
        let no_direct = extractor.arena.not(direct);
        let mut sources = FxHashSet::default();
        for side in BlockFace::values() {
            extractor.terms.clear();
            extractor.signal(
                actor,
                piston.pos.offset(BlockFace::Top).offset(side),
                side,
                &mut ignored,
                &mut roots,
            )?;
            let qc = std::mem::take(&mut extractor.terms);
            for term in qc {
                let power = extractor.terms_power(std::slice::from_ref(&term));
                let independent = extractor.arena.and(power, no_direct);
                extractor.check()?;
                if independent == FALSE {
                    continue;
                }
                let Some(source) = term.source else {
                    continue;
                };
                if matches!(world.get_block(source), Block::RedstoneWire { .. }) {
                    // After proving local coupling, recover upstream data through
                    // every conductor pose, rather than the saved occupancy alone.
                    extractor.terms.clear();
                    extractor.wire_signals = false;
                    extractor.walk_wires(
                        actor,
                        FALSE,
                        VecDeque::from([(source, 0, term.guard)]),
                    )?;
                    extractor.wire_signals = true;
                    for input in std::mem::take(&mut extractor.terms) {
                        let possible = extractor.arena.and(input.guard, independent);
                        extractor.check()?;
                        if possible != FALSE {
                            sources.extend(input.source);
                        }
                    }
                } else {
                    sources.insert(source);
                }
            }
        }
        let mut sources: Vec<_> = sources
            .into_iter()
            .filter(|pos| {
                internally_driven.contains(pos)
                    && !matches!(world.get_block(*pos), Block::Observer { .. })
            })
            .collect();
        sources.sort_by_key(|pos| (pos.y, pos.z, pos.x));
        feedback.insert(piston.pos, sources);
    }
    extractor.check()?;
    Ok(feedback)
}

pub(crate) fn extract(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
) -> Result<Extraction, String> {
    let mut extractor = extractor(world, report, monitor)?;
    let mut responses = Vec::with_capacity(report.pistons.len());
    for (actor, p) in report.pistons.iter().enumerate() {
        extractor.terms.clear();
        extractor.piston_power(actor, p)?;
        let terms = std::mem::take(&mut extractor.terms);
        let power = extractor.terms_power(&terms);
        responses.push(extractor.arena.not(power));
    }
    let mut wires = extractor.wires.clone();
    let mut consumers = Vec::new();
    crate::world::for_each_block_optimized(world, report.bounds.0, report.bounds.1, |pos| {
        let block = world.get_block(pos);
        if crate::redpiler::analysis::ports::is_consumer(block) {
            consumers.push((pos, block));
        }
    });
    let mut candidates = Vec::new();
    for (pos, block) in consumers {
        for input in [
            crate::redpiler::analysis::ports::ConsumerInput::Main,
            crate::redpiler::analysis::ports::ConsumerInput::ComparatorSide,
        ] {
            extractor.terms.clear();
            let mut power = FALSE;
            let mut queue = VecDeque::new();
            for (root, face, channel) in
                crate::redpiler::analysis::ports::consumer_roots(block, pos)
            {
                if channel == input {
                    extractor.consumer_signal(block, input, root, face, &mut power, &mut queue)?;
                }
            }
            extractor.walk_wires(usize::MAX, power, queue)?;
            if !extractor.terms.iter().any(|term| {
                term.guard > TRUE
                    || term.source.is_some_and(|pos| {
                        matches!(world.get_block(pos), Block::RedstoneWire { .. })
                    })
            }) {
                continue;
            }
            let terms = std::mem::take(&mut extractor.terms);
            candidates.push(OutputPort {
                consumer: pos,
                input,
                initial_strength: entry_strength(&extractor, &terms),
                terms,
            });
        }
    }
    wires.extend(
        report
            .ports
            .outputs
            .iter()
            .flat_map(|port| port.dependencies.wires.iter().copied()),
    );
    wires.extend(report.ports.pistons.iter().flat_map(|p| {
        p.updates.iter().filter_map(|u| {
            matches!(world.get_block(u.source), Block::RedstoneWire { .. }).then_some(u.source)
        })
    }));
    // Geometry notifications can clear saved wire power even when no power
    // dependency passes through the moving payload (for example, quartz).
    for p in &report.pistons {
        for pos in [p.pos, p.head, p.head.offset(p.piston.facing.into())] {
            wires.extend(
                BlockFace::values()
                    .into_iter()
                    .map(|face| pos.offset(face))
                    .filter(|&pos| matches!(world.get_block(pos), Block::RedstoneWire { .. })),
            );
        }
    }
    for &pos in &report.observers {
        let Block::Observer { observer } = world.get_block(pos) else {
            unreachable!()
        };
        let watched = pos.offset(observer.facing.into());
        if matches!(world.get_block(watched), Block::RedstoneWire { .. }) {
            wires.insert(watched);
        }
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
            if output.terms.iter().any(|term| {
                super::super::sequential::dependencies(&extractor.arena, [term.guard])
                    .iter()
                    .any(|v| matches!(v, Variable::Geometry { .. } | Variable::Observer(_)))
                    || term.source.is_some_and(|pos| indexed.contains(&pos))
            }) {
                for pos in output.terms.iter().filter_map(|term| term.source) {
                    if matches!(world.get_block(pos), Block::RedstoneWire { .. })
                        && indexed.insert(pos)
                    {
                        wires.push(pos);
                    }
                }
                outputs.push(output);
            } else {
                pending.push(output);
            }
        }
        candidates = pending;
        if cursor == wires.len() {
            break;
        }
        while cursor < wires.len() {
            let pos = wires[cursor];
            cursor += 1;
            extractor.terms.clear();
            extractor.walk_wires(usize::MAX, FALSE, VecDeque::from([(pos, 0, TRUE)]))?;
            let terms = std::mem::take(&mut extractor.terms);
            let Block::RedstoneWire { wire } = world.get_block(pos) else {
                unreachable!()
            };
            for pos in terms.iter().filter_map(|t| t.source) {
                if matches!(world.get_block(pos), Block::RedstoneWire { .. }) && indexed.insert(pos)
                {
                    wires.push(pos);
                }
            }
            let mut positions = vec![pos.offset(BlockFace::Top)];
            for face in BlockFace::values()
                .into_iter()
                .filter(|face| face.is_horizontal())
            {
                let neighbor = pos.offset(face);
                positions.extend([
                    neighbor,
                    neighbor.offset(BlockFace::Top),
                    neighbor.offset(BlockFace::Bottom),
                ]);
            }
            let shapes = extractor.output_wire_shapes(pos, wire, &positions, true)?;
            sensors.push((pos, terms, wire, shapes));
        }
    }
    // Keep graph sources from power, outputs and independent notification nets.
    extractor.sources =
        super::super::sequential::dependencies(&extractor.arena, responses.iter().copied())
            .into_iter()
            .filter_map(|variable| match variable {
                Variable::Signal { pos, .. } => Some(pos),
                _ => None,
            })
            .collect();
    extractor.sources.extend(
        outputs
            .iter()
            .flat_map(|o| o.terms.iter().filter_map(|t| t.source)),
    );
    extractor.sources.extend(
        sensors
            .iter()
            .flat_map(|(_, terms, _, _)| terms.iter().filter_map(|t| t.source)),
    );
    let mut roots = responses;
    let response_count = roots.len();
    roots.extend(outputs.iter().flat_map(|o| o.terms.iter().map(|t| t.guard)));
    roots.extend(sensors.iter().flat_map(|(_, terms, _, shapes)| {
        terms
            .iter()
            .map(|t| t.guard)
            .chain(shapes.iter().map(|&(guard, _)| guard))
    }));
    extractor.check()?;
    let arena = extractor.arena.compact(&mut roots);
    let mut guards = roots[response_count..].iter().copied();
    for output in &mut outputs {
        for term in &mut output.terms {
            term.guard = guards.next().unwrap();
        }
    }
    for (_, terms, _, shapes) in &mut sensors {
        for term in terms {
            term.guard = guards.next().unwrap();
        }
        for (guard, _) in shapes {
            *guard = guards.next().unwrap();
        }
    }
    roots.truncate(response_count);
    let mut sources: Vec<_> = extractor
        .sources
        .into_iter()
        .filter(|pos| !indexed.contains(pos))
        .collect();
    sources.sort_by_key(|p| (p.y, p.z, p.x));
    Ok(Extraction {
        sensors,
        logic: WaveLogic {
            arena,
            responses: roots,
            response_order: Vec::new(),
            #[cfg(test)]
            response_sources: sources.clone(),
            sources,
            wires: indexed,
            consumer_wires: Default::default(),
            outputs,
            handoff_wires: Vec::new(),
            follows_payload: vec![false; report.pistons.len()],
            context: extractor.context,
        },
    })
}

fn entry_strength<W: World>(extractor: &Extractor<'_, W>, terms: &[PowerTerm]) -> u8 {
    terms.iter().filter(|term| extractor.arena.evaluate(term.guard, |variable| match variable {
        Variable::Geometry { actor, part } => {
            let p = &extractor.report.pistons[actor];
            match part {
                GeometryPart::FarPayload => extractor.payloads[extractor.group_of[actor]] != Block::Air && extractor.world.get_block(p.head.offset(p.piston.facing.into())) == extractor.payloads[extractor.group_of[actor]],
                GeometryPart::NearPayload => extractor.payloads[extractor.group_of[actor]] != Block::Air && !p.piston.extended && extractor.world.get_block(p.head) == extractor.payloads[extractor.group_of[actor]],
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
