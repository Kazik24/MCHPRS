//! Bounded native evidence for movement roles, without changing admission.
use super::*;
use mchprs_blocks::block_entities::BlockEntity;
use rustc_hash::FxHashSet;

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
    for tick in 1..=5_000 {
        world.tick_interpreted();
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
    println!("Scope: first 5,000 game ticks after native Start, no paddle inputs; observations are at completed tick boundaries and can miss motions created and finalized within one tick. Far aliases alone do not establish transported payloads, and zero observed transport is not a proof for all inputs.");
}
