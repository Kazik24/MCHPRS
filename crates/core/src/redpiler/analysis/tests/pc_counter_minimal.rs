//! Saved minimal PC counter admission reproduction; coordinates are selection-local.
use super::*;

const FIXTURE: &str =
    "test_data/piston-research/pc-counter-minimal-20261009/PC_COUNTER_MINIMAL_FAIL.schem";

fn load() -> PlotWorld {
    let bytes = std::fs::read(root().join(FIXTURE)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "2b2df49a3bd2f9bf7049126f751a67e9d251a3ecd4e2234e867634d51887ea8b"
    );
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    world
}

fn result(world: &PlotWorld, assume_instant: bool, optimize: bool) -> Result<(), String> {
    let mut compiler = Compiler::default();
    compiler
        .compile(
            world,
            world.get_corners(),
            CompilerOptions {
                assume_instant,
                optimize,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .map_err(|error| error.to_string())
}

#[test]
fn either_reduced_pc_counter_minimal_variant_compiles() {
    for removed in [
        [[4, 7, 5], [3, 7, 5], [4, 8, 5]],
        [[0, 4, 5], [0, 4, 4], [0, 5, 5]],
    ] {
        let mut world = load();
        for [x, y, z] in removed {
            world.set_block(BASE + BlockPos::new(x, y, z), Block::Air);
        }
        for assume_instant in [false, true] {
            for optimize in [false, true] {
                result(&world, assume_instant, optimize).unwrap();
            }
        }
    }
}

#[test]
fn readb_delivers_three_distinct_memory_callbacks_without_a_qc_data_callback() {
    let mut world = load();
    let cell = BASE + BlockPos::new(1, 3, 2);
    let source = BASE + BlockPos::new(0, 4, 2);
    let callbacks = crate::redstone::instant_piston_tests::capture_at(&[cell], || {
        lever_action(&mut world, BASE + BlockPos::new(0, 4, 7), false);
        world.tick_interpreted();
    });
    let directions: Vec<_> = callbacks
        .iter()
        .filter(|entry| entry["kind"] == "callback" && entry["data"]["source"] == json!(source))
        .map(|entry| {
            assert_eq!(entry["phase"], "PistonEvents");
            assert_eq!(entry["data"]["piston_power"], true);
            entry["data"]["dir"].clone()
        })
        .collect();
    assert_eq!(directions, vec![Value::Null, json!("West"), json!("Top")]);
    assert!(callbacks.iter().all(|entry| entry["kind"] != "callback"
        || entry["data"]["source"] != json!(BASE + BlockPos::new(1, 6, 4))));
}

#[test]
#[ignore = "current admission failure reproduction; acceptance requires a sampled QC boundary"]
fn saved_pc_counter_minimal_exposes_internally_driven_qc_admission() {
    let world = load();
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let error = result(&world, assume_instant, optimize).unwrap_err();
            assert!(
                error.contains("unsupported internally driven QC sampling interface"),
                "{error}"
            );
            eprintln!("minimal A={assume_instant} O={optimize}: {error}");
        }
    }
}

#[test]
#[ignore = "read-only minimal admission and callback diagnostic"]
fn trace_pc_counter_minimal_variants() {
    let mut cases = Vec::new();
    for (name, removed) in [
        ("original", vec![]),
        (
            "without_input_instant",
            vec![[4, 7, 5], [3, 7, 5], [4, 8, 5]],
        ),
        ("without_readb", vec![[0, 4, 5], [0, 4, 4], [0, 5, 5]]),
        (
            "without_readb_and_payload",
            vec![[0, 4, 5], [0, 4, 4], [0, 5, 5], [0, 4, 3]],
        ),
    ] {
        let mut world = load();
        for [x, y, z] in removed {
            world.set_block(BASE + BlockPos::new(x, y, z), Block::Air);
        }
        let report = analyze_world(&world);
        let monitor = crate::redpiler::TaskMonitor::default();
        let targets = report.pistons.iter().map(|piston| piston.pos).collect();
        let mut sources = rustc_hash::FxHashSet::default();
        crate::world::for_each_block_optimized(&world, report.bounds.0, report.bounds.1, |pos| {
            if matches!(
                world.get_block(pos),
                Block::RedstoneRepeater { .. }
                    | Block::RedstoneWallTorch { .. }
                    | Block::RedstoneTorch { .. }
            ) {
                sources.insert(pos);
            }
        });
        let feedback = crate::redpiler::instant::logic::sequential::feedback_sources(
            &world, &report, &monitor, &targets, &sources,
        )
        .unwrap();
        let feedback: Vec<_> = feedback.into_iter().map(|(target,sources)| json!({"target":target-BASE,"sources":sources.into_iter().map(|pos|pos-BASE).collect::<Vec<_>>()})).collect();
        let compile: Vec<_> = [false,true].into_iter().flat_map(|assume_instant| [false,true].into_iter().map(move |optimize|(assume_instant,optimize))).map(|(assume_instant,optimize)|json!({"assume_instant":assume_instant,"optimize":optimize,"result":result(&world,assume_instant,optimize)})).collect();
        let regions = crate::redpiler::instant::regions::split(&world, &report, &monitor).unwrap();
        let actors: Vec<_> = report.pistons.iter().enumerate().map(|(actor,piston)|json!({"pos":piston.pos-BASE,"recognition":report.recognition[actor],"ports":report.ports.pistons[actor]})).collect();
        eprintln!("{name}: {}", json!({"compile":compile,"feedback":feedback}));
        cases.push(json!({"name":name,"compile":compile,"feedback":feedback,"actors":actors,"regions":regions.iter().map(|region|region.pistons.iter().map(|p|p.pos-BASE).collect::<Vec<_>>()).collect::<Vec<_>>()}));
    }
    if let Ok(path) = std::env::var("MCHPRS_PC_COUNTER_MINIMAL_TRACE") {
        std::fs::write(path, serde_json::to_vec_pretty(&cases).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "physical callback evidence for the saved minimal QC boundary"]
fn trace_pc_counter_minimal_physical_deliveries() {
    let mut world = load();
    let watched: Vec<_> = [
        [1, 3, 2],
        [1, 3, 4],
        [0, 4, 2],
        [0, 4, 3],
        [0, 4, 4],
        [0, 4, 5],
        [1, 5, 3],
        [1, 6, 4],
        [1, 7, 5],
        [4, 7, 5],
    ]
    .into_iter()
    .map(|[x, y, z]| BASE + BlockPos::new(x, y, z))
    .collect();
    let mut steps = Vec::new();
    for (name, control, ticks) in [
        ("unlock_update", Some([3, 6, 4]), 20),
        ("release_reada", Some([2, 3, 8]), 20),
        ("release_readb", Some([0, 4, 7]), 20),
        ("change_qc_data", Some([6, 7, 6]), 20),
        ("sample_readb", Some([0, 4, 7]), 20),
        ("change_qc_data_again", Some([6, 7, 6]), 20),
        ("sample_readb_again", Some([0, 4, 7]), 20),
    ] {
        let callbacks = crate::redstone::instant_piston_tests::capture_at(&watched, || {
            if let Some([x, y, z]) = control {
                let pos = BASE + BlockPos::new(x, y, z);
                let Block::Lever { lever } = world.get_block(pos) else {
                    panic!("missing lever at {pos:?}");
                };
                lever_action(&mut world, pos, !lever.powered);
            }
            for _ in 0..ticks {
                world.tick_interpreted();
            }
        });
        let state: Vec<_> = watched.iter().map(|&pos|json!({"pos":pos-BASE,"block":world.get_block(pos).get_name(),"properties":world.get_block(pos).properties()})).collect();
        steps.push(json!({"name":name,"state":state,"callbacks":callbacks}));
    }
    let path = std::env::var("MCHPRS_PC_COUNTER_MINIMAL_CALLBACKS").unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&steps).unwrap()).unwrap();
}
