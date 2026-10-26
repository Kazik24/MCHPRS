//! Actual O6 execution and projection with stable inputs for each instant frame.
use super::*;
use std::collections::BTreeSet;

pub(super) fn run(descriptor: &Value, episodes: usize, iterations: usize, seed: u64, output: Option<&str>) -> Result<()> {
    let offset = origin(descriptor);
    let inputs = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .context("missing operand inputs")?
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().context("missing operand inputs")?)
        .chain(
            descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"]
                .as_array()
                .context("missing selector inputs")?,
        )
        .map(|p| offset + position(p))
        .collect::<Vec<_>>();
    let outputs = descriptor["observations"]["output"]
        .as_array()
        .context("missing outputs")?
        .iter()
        .map(|p| offset + position(p))
        .collect::<Vec<_>>();
    let trigger = offset + position(&descriptor["ports"]["trigger"]);
    ensure!(inputs.len() == 35 && outputs.len() == 16, "unexpected FPU fixture ports");
    let readout_delay = descriptor
        .get("terminal_readout_delay")
        .map(|value| {
            value
                .as_u64()
                .filter(|delay| (1..=4).contains(delay))
                .context("terminal_readout_delay must be 1..=4")
        })
        .transpose()?;
    let mut samples = Vec::new();
    for sample in 0..iterations {
        let mut world = load(descriptor)?;
        if let Some(delay) = readout_delay {
            for &position in &outputs {
                world.set_block(
                    position.offset(mchprs_blocks::BlockFace::East),
                    Block::RedstoneRepeater {
                        repeater: mchprs_blocks::blocks::RedstoneRepeater {
                            facing: mchprs_blocks::BlockDirection::West,
                            delay: delay as u8,
                            powered: false,
                            locked: false,
                        },
                    },
                );
            }
        }
        world.disable_command_output_limits_for_replay();
        let mut mask = inputs.iter().enumerate().try_fold(0u64, |word, (bit, &p)| -> Result<u64> {
            let Block::Lever { lever } = world.get_block(p) else {
                bail!("input is not a lever")
            };
            Ok(word | (u64::from(lever.powered) << bit))
        })?;
        ensure!(
            matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered),
            "frame benchmark requires the saved stopped clock"
        );
        let mut compiler = Compiler::default();
        let compile_start = Instant::now();
        compiler.compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                optimization_level: Some(6),
                budget_multiplier: 8,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )?;
        let compile_seconds = compile_start.elapsed().as_secs_f64();
        let before = compiler.graph_execution_counts().context("missing counters")?;
        let words = input_stimuli(Workload::Random, mask, 16, 3, seed, episodes);
        let mut observed = BTreeSet::new();
        let mut saw_readout_on = false;
        let start = Instant::now();
        compiler.on_use_block(trigger);
        for word in words {
            for (bit, &p) in inputs.iter().enumerate() {
                if (word ^ mask) & (1 << bit) != 0 {
                    compiler.on_use_block(p);
                }
            }
            mask = word;
            // Hold each input frame for six game ticks, including its reset interval.
            for _ in 0..6 {
                compiler.tick_with_world(&mut world)?;
            }
            compiler.flush(&mut world);
            let word = outputs.iter().fold(0u16, |word, &p| {
                (word << 1) | u16::from(matches!(world.get_block(p), Block::RedstoneWire { wire } if wire.power > 0))
            });
            observed.insert(word);
            if readout_delay.is_some() {
                saw_readout_on |= outputs.iter().any(|position| {
                    matches!(world.get_block(position.offset(mchprs_blocks::BlockFace::East)), Block::RedstoneRepeater { repeater } if repeater.powered)
                });
            }
        }
        let seconds = start.elapsed().as_secs_f64();
        let after = compiler.graph_execution_counts().context("missing counters")?;
        let logical_kernel_evaluations = after.logical_kernel_evaluations - before.logical_kernel_evaluations;
        let wire_walks = after.wire_walks - before.wire_walks;
        let piston_samples = after.piston_samples - before.piston_samples;
        let piston_actions = after.piston_actions - before.piston_actions;
        let deferred_publications = after.deferred_publications - before.deferred_publications;
        let callbacks = after.callbacks - before.callbacks;
        let command_output_count = world.command_output().count();
        ensure!(
            logical_kernel_evaluations == episodes as u64,
            "logical device did not execute once per six-game-tick frame"
        );
        ensure!(wire_walks == 0, "frame workload performed physical wire walks: {wire_walks}");
        ensure!(observed.len() > 1, "no changing FPU output activity");
        ensure!(command_output_count > 0, "frame workload produced no real command output");
        ensure!(
            readout_delay.is_none() || saw_readout_on,
            "terminal readout repeaters never published a high output"
        );
        let ticks = episodes as u64 * 6;
        let report = json!({"sample":sample + 1,"compile_seconds":compile_seconds,
            "game_ticks":ticks,"seconds":seconds,"game_tps":ticks as f64 / seconds,
            "calculations":logical_kernel_evaluations,
            "logical_operations":after.logical_kernel_operations - before.logical_kernel_operations,
            "output_words":observed,"callbacks":callbacks,
            "wire_walks":wire_walks,"piston_samples":piston_samples,"piston_actions":piston_actions,
            "deferred_publications":deferred_publications,"command_output_count":command_output_count});
        println!("{report}");
        samples.push(report);
    }
    let mut rates = samples.iter().map(|s| s["game_tps"].as_f64().unwrap()).collect::<Vec<_>>();
    rates.sort_by(f64::total_cmp);
    let report = json!({"measurement_kind":"production_o6_fpu_frames","optimized":!cfg!(debug_assertions),
        "terminal_readout_delay":readout_delay,
        "flags":"-O6","input_contract":"compile stopped circuit, then toggle trigger to start; inputs held for six game ticks; no warm-up",
        "warmup_game_ticks":0,"trigger_edges_per_sample":1,
        "scope":"bulk logical frame plus three retained native output cells; includes control updates, piston samples/actions, deferred publications, callbacks, and command output",
        "correctness_scope":"activity check; benchmark-frame equivalence and native clock timing are tested separately",
        "median_game_tps":rates[rates.len()/2],"samples":samples});
    if let Some(path) = output {
        let path = root().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    println!("median production game TPS: {}", report["median_game_tps"]);
    Ok(())
}
