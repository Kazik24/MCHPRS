//! Focused native trace for the first persistent BubbleSort RAM-bit change.
use super::*;
use mchprs_blocks::BlockFace;
use serde_json::json;

#[test]
#[ignore = "focused native BUD RAM sample/request/accept trace; writes a diagnostic artifact"]
fn report_first_cpu_bubblesort_ram_bit_transition() {
    let fixture = manifest("cpu_bubblesort");
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "cpu_bubblesort").unwrap();
    let mut world = cpus::load_cpu(cpu);
    // `collect_ram` orders 16 words per bank. The first stable changed word
    // in the frozen trace is entry 48: bank y=37, row 0. Probe bit zero.
    let word_index = 48usize;
    let bank_y = [7, 17, 27, 37][word_index / 16];
    let row_index = word_index % 16;
    let bit_index = 0usize;
    let target = cpu.origin + BlockPos::new(175 + 4 * bit_index as i32, bank_y, 62 - 4 * row_index as i32);
    // In the analogous ANPU reader layout, a possible storage piston sits four
    // blocks above the observed lower reader. This is a topology candidate,
    // not an ownership claim; record its actual block and contacts explicitly.
    let upper_candidate = target + BlockPos::new(0, 4, 0);
    let generator_candidate = fixture["observations"]["update_generators"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|position| position[1] == bank_y)
        .min_by_key(|position| (position[2].as_i64().unwrap() - i64::from(target.z - cpu.origin.z)).abs())
        .unwrap();
    let generator = cpu.origin
        + BlockPos::new(
            generator_candidate[0].as_i64().unwrap() as i32,
            generator_candidate[1].as_i64().unwrap() as i32,
            generator_candidate[2].as_i64().unwrap() as i32,
        );
    let initial = ram_bit(&world, target).expect("declared RAM observation must start on a piston");
    let upper_candidate_initial_state = piston_snapshot(&world, upper_candidate);
    let generator_initial_state = piston_snapshot(&world, generator);
    let reference = cpus::reference(cpu);
    let (expected_tick, expected_word) = reference
        .ram_trace
        .iter()
        .find(|(_, words)| {
            words
                .get(word_index)
                .and_then(|word| *word)
                .is_some_and(|word| word & 1 != u16::from(initial))
        })
        .map(|(tick, words)| (*tick, words[word_index].unwrap()))
        .expect("frozen reference contains an available changed word for the selected RAM bit");
    // Capture the observed cell and nearby piston bases, plus the coordinate-grid
    // candidate generator. This neighborhood is a trace window, not an ownership claim.
    let mut selected = Vec::new();
    for x in -3..=3 {
        for y in -3..=3 {
            for z in -3..=3 {
                let pos = target + BlockPos::new(x, y, z);
                if matches!(world.get_block(pos), Block::Piston { .. }) {
                    selected.push(pos);
                }
            }
        }
    }
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let pos = generator + BlockPos::new(x, y, z);
                if matches!(world.get_block(pos), Block::Piston { .. }) {
                    selected.push(pos);
                }
            }
        }
    }
    selected.push(target);
    if matches!(world.get_block(upper_candidate), Block::Piston { .. }) {
        selected.push(upper_candidate);
    }
    selected.sort_by_key(|pos| (pos.x, pos.y, pos.z));
    selected.dedup();
    let start_action = trace::capture_detailed(&selected, || cpus::click_cpu(&mut world, cpu, cpu.start));

    let mut ticks = Vec::new();
    let mut first_observed_bit_change_active_tick = None;
    let mut last_observed_bit = Some(initial);
    for active_tick in 1..=expected_tick {
        let detail = trace::capture_detailed(&selected, || world.tick_interpreted());
        let value = ram_bit(&world, target);
        let tick = world.piston_state().logical_tick;
        let mut event_positions: Vec<_> = detail.iter().map(|entry| entry.pos).collect();
        event_positions.sort_by_key(|pos| (pos.x, pos.y, pos.z));
        event_positions.dedup();
        let frozen_word = reference
            .ram_trace
            .iter()
            .find(|(sample_tick, _)| *sample_tick == active_tick)
            .and_then(|(_, words)| words.get(word_index).copied().flatten());
        if let Some(word) = frozen_word {
            assert_eq!(value, Some(word & 1 != 0), "selected RAM bit at native sample tick {active_tick}");
        }
        if let Some(value) = value {
            if last_observed_bit.is_some_and(|previous| previous != value) && first_observed_bit_change_active_tick.is_none() {
                first_observed_bit_change_active_tick = Some(active_tick);
            }
            last_observed_bit = Some(value);
        }
        ticks.push(json!({
            "active_tick":active_tick,
            "tick":tick,
            "ram_bit_after_tick":value,
            "detailed_piston_operations":detail,
            "target_after_tick":piston_snapshot(&world, target),
            "upper_candidate_after_tick":piston_snapshot(&world, upper_candidate),
            "watched_event_bases_after_tick":event_positions.iter().map(|&pos| json!({"pos":pos,"state":piston_snapshot(&world,pos)})).collect::<Vec<_>>(),
        }));
    }
    assert_eq!(
        ram_bit(&world, target),
        Some(expected_word & 1 != 0),
        "selected RAM bit at its first available changed-word checkpoint"
    );
    let report = json!({
        "fixture_sha256":fixture["sha256"],
        "target_ram_word_index":word_index,
        "target_bank_y":bank_y,
        "target_row":row_index,
        "target_bit":bit_index,
        "target_world_position":target,
        "upper_storage_candidate_world_position":upper_candidate,
        "upper_storage_candidate_initial_state":upper_candidate_initial_state,
        "candidate_update_generator_manifest_position":generator_candidate,
        "candidate_update_generator_world_position":generator,
        "candidate_update_generator_initial_state":generator_initial_state,
        "initial_ram_bit":initial,
        "start_action_detailed_piston_operations":start_action,
        "first_reference_available_changed_word_tick":expected_tick,
        "expected_word_at_checkpoint":expected_word,
        "first_observed_bit_change_active_tick":first_observed_bit_change_active_tick,
        "watch_positions":selected,
        "ticks":ticks,
        "limits":"Detailed entries expose sample, request, acceptance and applied cuts only for watched piston bases. The regular-grid update-generator and piston four blocks above the lower RAM readout are candidates, not proven mappings/owners. The local watch window is not a complete route/resource ownership proof; block IDs at each selected piston front/far are recorded, while unrelated observers and non-piston notifications are not.",
    });
    let path = root().join("target/logical-inference/cpu-bubblesort-first-ram-transition.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!(
        "BubbleSort selected RAM-bit trace through active tick {expected_tick}; report: {}",
        path.display()
    );
}

fn ram_bit(world: &PlotWorld, pos: BlockPos) -> Option<bool> {
    match world.get_block(pos) {
        Block::Piston { piston } => Some(!piston.extended),
        _ => None,
    }
}

fn piston_snapshot(world: &PlotWorld, pos: BlockPos) -> serde_json::Value {
    let block = world.get_block(pos);
    match block {
        Block::Piston { piston } => {
            let facing: BlockFace = piston.facing.into();
            let front = pos.offset(facing);
            let far = front.offset(facing);
            json!({
                "block":"piston",
                "extended":piston.extended,
                "sticky":piston.sticky,
                "facing":format!("{facing:?}"),
                "current_power":crate::redstone::piston::should_piston_extend(world,piston.facing,pos),
                "front_position":front,
                "front_block_id":world.get_block(front).get_id(),
                "far_position":far,
                "far_block_id":world.get_block(far).get_id(),
            })
        }
        _ => json!({"block":format!("{}", block.get_name()),"id":block.get_id()}),
    }
}
