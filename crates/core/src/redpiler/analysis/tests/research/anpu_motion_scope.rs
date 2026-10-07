//! Bounded native evidence for movement roles, without changing admission.
use super::*;
use crate::redstone::piston::trace::Operation;
use mchprs_blocks::block_entities::BlockEntity;
use rustc_hash::{FxHashMap, FxHashSet};

#[test]
fn ordinary_pending_movement_rejection_and_reset_preserve_interpreter_continuation() {
    for paused_after in 0..=2 {
        for sticky in [false, true] {
            let mut worlds = [empty(), empty()];
            for world in &mut worlds {
                world.set_block(
                    BASE,
                    Block::Piston {
                        piston: RedstonePiston {
                            facing: BlockFacing::East,
                            sticky,
                            extended: false,
                        },
                    },
                );
                world.set_block(BASE.offset(BlockFace::East), Block::Stone {});
                world.set_block(BASE.offset(BlockFace::Bottom), Block::RedstoneBlock);
                crate::redstone::update(world.get_block(BASE), world, BASE, None);
                for _ in 0..paused_after {
                    world.tick_interpreted();
                }
            }
            for assume_instant in [false, true] {
                let world = &mut worlds[0];
                let bounds = world.get_corners();
                let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
                let before = cpus::checkpoint(world, paused_after, &[]);
                let mut compiler = Compiler::default();
                let error = compiler
                    .compile(
                        world,
                        bounds,
                        CompilerOptions {
                            assume_instant,
                            ..Default::default()
                        },
                        ticks,
                        Default::default(),
                    )
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains("pending piston events")
                        || error.contains("requires the interpreter"),
                    "{error}"
                );
                assert!(!compiler.is_active());
                compiler.reset(world, bounds);
                assert_eq!(cpus::checkpoint(world, paused_after, &[]), before);
            }
            for tick in paused_after + 1..=8 {
                for world in &mut worlds {
                    world.tick_interpreted();
                }
                assert_eq!(
                    cpus::checkpoint(&worlds[0], tick, &[]),
                    cpus::checkpoint(&worlds[1], tick, &[])
                );
            }
        }
    }
}

#[test]
#[ignore = "ANPU native 5,000-tick movement-role inventory"]
fn anpu_native_motion_roles_distinguish_payloads_from_moving_bases() {
    let cpu = cpus::CPUS[1];
    let mut world = cpus::load_cpu(cpu);
    let (first, last) = world.get_corners();
    let mut pistons = Vec::new();
    crate::world::for_each_block_optimized(&world, first, last, |pos| {
        if let Block::Piston { piston } = world.get_block(pos) {
            pistons.push((pos, piston));
        }
    });
    let bases: FxHashSet<_> = pistons.iter().map(|&(pos, _)| pos).collect();
    let memory: FxHashSet<_> = pistons
        .iter()
        .filter_map(|&(pos, piston)| {
            (piston.facing == BlockFacing::Down
                && matches!(
                    world.get_block(pos.offset(BlockFace::Top)),
                    Block::NoteBlock { .. }
                ))
            .then_some(pos)
        })
        .collect();
    let mut geometry = BTreeMap::new();
    for &(pos, piston) in &pistons {
        let near = pos.offset(piston.facing.into());
        let far = near.offset(piston.facing.into());
        *geometry
            .entry(format!(
                "sticky={}, facing={:?}, extended={}, near={}, far={}, note_above={}",
                piston.sticky,
                piston.facing,
                piston.extended,
                world.get_block(near).get_name(),
                world.get_block(far).get_name(),
                memory.contains(&pos)
            ))
            .or_insert(0) += 1;
    }
    let monitor = std::sync::Arc::new(crate::redpiler::TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let report = analyze(
        &world,
        world.get_corners(),
        &[],
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let regions = crate::redpiler::instant::regions::split(&world, &report, &monitor).unwrap();
    println!("ANPU geometry roles: {geometry:?}");
    println!("ANPU analysis: {} pistons, {} observers, {} payload groups, {} structural reset matches, {} note-above downward cells; region sizes {:?}",
        pistons.len(), report.observers.len(), report.payload_groups.len(),
        report.recognition.iter().filter(|r| r.is_matched()).count(), memory.len(),
        regions.iter().map(|r| r.pistons.len()).collect::<Vec<_>>());
    for pos in [BlockPos::new(117, 30, 74), BlockPos::new(99, 44, 76)] {
        let actor = report.pistons.iter().position(|p| p.pos == pos).unwrap();
        println!(
            "ANPU admission actor: {}",
            json!({"piston": report.pistons[actor],
            "recognition": report.recognition[actor], "ports": report.ports.pistons[actor]})
        );
    }
    let mut unsupported = BTreeMap::new();
    let mut inventory = BTreeMap::<&str, usize>::new();
    let mut geometry_examples = Vec::new();
    for &(base, piston) in &pistons {
        if piston.sticky {
            continue;
        }
        *inventory.entry("ordinary actors").or_default() += 1;
        let near = base.offset(piston.facing.into());
        let far = near.offset(piston.facing.into());
        let near_block = world.get_block(near);
        let far_block = world.get_block(far);
        let owned_head = matches!(near_block, Block::PistonHead { head }
            if piston.extended && head.facing == piston.facing && !head.sticky && !head.short);
        let empty_near = near_block == Block::Air || owned_head;
        let supported_near = crate::redpiler::instant::outputs::supported_payload(near_block);
        for (role, block, saved_head) in
            [("Near", near_block, owned_head), ("Far", far_block, false)]
        {
            if block != Block::Air
                && !saved_head
                && !crate::redpiler::instant::outputs::supported_payload(block)
            {
                *unsupported
                    .entry((role, block.get_name().to_owned()))
                    .or_insert(0) += 1;
            }
        }
        for (label, present) in [
            ("empty Near", empty_near),
            ("supported material at Near", supported_near),
            ("Near is another base", bases.contains(&near)),
            ("Far is another base", bases.contains(&far)),
            (
                "empty Near with foreign base at Far",
                empty_near && bases.contains(&far),
            ),
            (
                "supported Near with foreign base at Far",
                supported_near && bases.contains(&far),
            ),
        ] {
            if present {
                *inventory.entry(label).or_default() += 1;
            }
        }
        if bases.contains(&far) && geometry_examples.len() < 4 {
            geometry_examples.push(
                json!({"base": base, "near": near, "near_block": near_block.get_name(),
                "far": far, "far_block": far_block.get_name(), "empty_near": empty_near}),
            );
        }
    }
    println!("ANPU initial ordinary roles: {inventory:?}");
    println!("ANPU initial unsupported cells by geometric role: {unsupported:?}");
    println!("ANPU foreign Far base examples: {geometry_examples:?}");

    cpus::click_cpu(&mut world, cpu, cpu.start);
    let mut seen = FxHashSet::default();
    let mut counts = BTreeMap::new();
    let mut examples = BTreeMap::<_, Vec<Value>>::new();
    let mut transported_pistons = 0;
    let mut moving_source_bases = 0;
    let mut operations = BTreeMap::new();
    let mut actors = FxHashMap::<BlockPos, FxHashSet<String>>::default();
    let mut rejection_events = BTreeMap::<String, Vec<Value>>::new();
    let mut live = FxHashMap::<u64, (u32, (bool, bool, String))>::default();
    let mut lifetimes = BTreeMap::new();
    for tick in 1..=5_000 {
        for entry in trace::capture(|| world.tick_interpreted()) {
            if let Operation::Applied(event) = entry.operation {
                if [BlockPos::new(117, 30, 74), BlockPos::new(99, 44, 76)].contains(&event.pos) {
                    let events = rejection_events
                        .entry(format!("{:?}", event.pos))
                        .or_default();
                    if events.len() < 12 {
                        events.push(json!({"tick": tick, "phase": entry.phase, "event": event}));
                    }
                }
            }
            let (pos, role) = match entry.operation {
                Operation::Sample {
                    pos,
                    extended,
                    powered,
                } => (pos, format!("sample extended={extended} powered={powered}")),
                Operation::Applied(event) => (event.pos, format!("applied {:?}", event.action)),
            };
            *operations
                .entry(format!(
                    "memory={}, phase={:?}, {role}",
                    memory.contains(&pos),
                    entry.phase
                ))
                .or_insert(0) += 1;
            actors.entry(pos).or_default().insert(role);
        }
        let current: FxHashSet<_> = world
            .piston_state()
            .motions
            .iter()
            .map(|m| m.identity)
            .collect();
        live.retain(|identity, (start, key)| {
            if current.contains(identity) {
                return true;
            }
            *lifetimes.entry((key.clone(), tick - *start)).or_insert(0) += 1;
            false
        });
        for motion in &world.piston_state().motions {
            if !seen.insert(motion.identity) {
                continue;
            }
            let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity(motion.pos) else {
                panic!(
                    "live motion {} has no moving entity at {:?}",
                    motion.identity, motion.pos
                );
            };
            let carried = Block::from_id(entity.block_state);
            let key = (
                entity.source,
                entity.extending,
                carried.get_name().to_owned(),
            );
            live.insert(motion.identity, (tick, key.clone()));
            *counts.entry(key.clone()).or_insert(0) += 1;
            if matches!(carried, Block::Piston { .. }) {
                if entity.source {
                    moving_source_bases += 1;
                } else {
                    transported_pistons += 1;
                }
            }
            let first = examples.entry(key).or_default();
            if first.len() < 3 {
                first.push(
                    json!({"tick": tick, "identity": motion.identity, "pos": motion.pos,
                    "source": entity.source, "extending": entity.extending,
                    "carried": carried.get_name(), "properties": carried.properties()}),
                );
            }
        }
    }
    assert!(
        !seen.is_empty(),
        "Start protocol must produce observed movement"
    );
    println!("ANPU unique motions by (source, extending, carried type): {counts:?}");
    println!("ANPU first motion examples: {examples:?}");
    println!("ANPU observed transported Piston bodies (source=false): {transported_pistons}; own moving bases (source=true): {moving_source_bases}");
    println!("ANPU boundary-observed motion lifetimes (type, removal tick minus first tick): {lifetimes:?}");
    println!("ANPU delivered piston operations: {operations:?}");
    let mut behavior = BTreeMap::new();
    for &(pos, piston) in &pistons {
        let mut roles: Vec<_> = actors.get(&pos).into_iter().flatten().cloned().collect();
        roles.sort();
        *behavior
            .entry(format!(
                "sticky={}, memory={}, {:?}",
                piston.sticky,
                memory.contains(&pos),
                roles
            ))
            .or_insert(0) += 1;
    }
    println!("ANPU per-base observed behaviors: {behavior:?}");
    println!("ANPU first accepted rejection-actor events: {rejection_events:?}");
    for pos in [BlockPos::new(117, 30, 74), BlockPos::new(99, 44, 76)] {
        println!(
            "ANPU rejection actor native operations at {pos:?}: {:?}",
            actors.get(&pos)
        );
    }
    println!("Scope: first 5,000 game ticks after native Start, no paddle inputs; observations are at completed tick boundaries and can miss motions created and finalized within one tick. Far aliases alone do not establish transported payloads, and zero observed transport is not a proof for all inputs.");
}
