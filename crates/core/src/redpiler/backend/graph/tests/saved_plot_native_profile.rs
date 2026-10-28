use super::*;

#[test]
#[ignore = "requires corrected saved plot; native interpreter profiling"]
fn saved_plot_native_profile() {
    let frames = std::env::var("MCHPRS_PROFILE_FRAMES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(96);
    let samples = std::env::var("MCHPRS_PROFILE_SAMPLES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(3);
    assert!(frames >= 4 && samples > 0);
    let manifest = saved_fpu_manifest();
    let ports = &manifest["ports"];
    let inputs = sorted_positions(
        [
            manifest_positions(&ports["light_blue_operand_inputs"]),
            manifest_positions(&ports["red_operand_inputs"]),
            manifest_positions(&ports["opcode_levers_by_sign_weight_4_2_1"]),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
    assert_eq!(inputs.len(), 35);
    let witness = known_command_witness_word(&manifest, &inputs);
    let discovery = saved_world();
    let readouts = manifest_positions(&manifest["observations"]["output"])
        .into_iter()
        .map(|terminal| {
            let adjacent = BlockFace::values()
                .into_iter()
                .map(|face| terminal.offset(face))
                .filter(|&pos| matches!(discovery.get_block(pos), Block::RedstoneRepeater { .. }))
                .collect::<Vec<_>>();
            assert_eq!(adjacent.len(), 1, "each output terminal must have one adjacent readout");
            adjacent[0]
        })
        .collect::<Vec<_>>();
    let (min, max) = discovery.get_corners();
    let mut neighbors = Vec::new();
    let mut neighbor_observed = Vec::new();
    crate::world::for_each_block_optimized(&discovery, min, max, |pos| {
        if !inside_fpu(pos) {
            let block = discovery.get_block(pos);
            if matches!(block, Block::Lever { .. }) && neighbors.len() < 24 {
                neighbors.push(pos);
            }
            if matches!(
                block,
                Block::RedstoneWire { .. } | Block::RedstoneLamp { .. } | Block::RedstoneRepeater { .. }
            ) {
                neighbor_observed.push(pos);
            }
        }
    });
    assert!(!neighbors.is_empty() && !neighbor_observed.is_empty());
    drop(discovery);

    for sample in 0..samples {
        let mut world = saved_world();
        assert!(matches!(world.get_block(TRIGGER), Block::Lever { lever } if lever.powered));
        assert!(readouts.iter().all(|&pos| matches!(world.get_block(pos), Block::RedstoneRepeater { .. })));
        let entry = inputs.iter().enumerate().fold(0u64, |word, (bit, &pos)| {
            word | (u64::from(matches!(world.get_block(pos), Block::Lever { lever } if lever.powered)) << bit)
        });
        for (bit, &pos) in inputs.iter().enumerate() {
            if (entry ^ witness) & (1 << bit) != 0 {
                native_use_lever(&mut world, pos);
            }
        }
        native_use_lever(&mut world, TRIGGER);
        let before_commands = world.command_output().count();
        for _ in 0..24 {
            world.tick_interpreted();
        }
        assert!(
            world.command_output().count() > before_commands,
            "warmup must execute the FPU command witness"
        );
        let warmup_commands = world.command_output().count();
        let neighbor_before = neighbor_observed.iter().map(|&pos| world.get_block(pos)).collect::<Vec<_>>();
        let mut random = 0x4d43_4850_5253_2026u64;
        let mask = (1u64 << inputs.len()) - 1;
        let _ = next_profile_word(&mut random, mask, entry);
        let mut previous = witness;
        let mut control = std::time::Duration::ZERO;
        let mut ticks = std::time::Duration::ZERO;
        let mut output = std::time::Duration::ZERO;
        let mut words = FxHashSet::default();
        let mut neighbor_changed = false;
        let mut input_updates = 0;
        let mut neighbor_updates = 0;
        for frame in 0..frames {
            let start = Instant::now();
            let (word, input_count, neighbor_count) = drive_profile_frame(&mut random, mask, previous, &inputs, &neighbors, frame, |pos| {
                native_use_lever(&mut world, pos)
            });
            input_updates += input_count;
            neighbor_updates += neighbor_count;
            previous = word;
            control += start.elapsed();
            let start = Instant::now();
            for _ in 0..6 {
                world.tick_interpreted();
            }
            ticks += start.elapsed();
            let start = Instant::now();
            let readout = readouts.iter().enumerate().fold(0u16, |word, (bit, &pos)| {
                word | (u16::from(matches!(world.get_block(pos), Block::RedstoneRepeater { repeater } if repeater.powered)) << bit)
            });
            words.insert(readout);
            neighbor_changed |= neighbor_observed
                .iter()
                .zip(&neighbor_before)
                .any(|(&pos, &block)| world.get_block(pos) != block);
            output += start.elapsed();
        }
        assert!(neighbor_changed, "neighboring circuit must change a wire, lamp or repeater");
        if frames >= 96 {
            assert!(
                words.len() > 1 && words.iter().any(|&word| word != 0),
                "FPU readout must change and become nonzero"
            );
        }
        eprintln!(
            "native sample={sample} frames={frames} ticks={} control_s={:.6} tick_s={:.6} output_s={:.6} input_updates={input_updates} neighbor_updates={neighbor_updates} measured_commands={} readout_words={} readout_nonzero={} neighbor_changed={neighbor_changed}; native state is materialized during tick, no flush",
            frames * 6,
            control.as_secs_f64(),
            ticks.as_secs_f64(),
            output.as_secs_f64(),
            world.command_output().count() - warmup_commands,
            words.len(),
            words.iter().any(|&word| word != 0)
        );
    }
}
