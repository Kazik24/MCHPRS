//! Native-vs-compiled diagnostic for the reference TEST_PISTION pulse replay.
use super::*;

#[test]
fn test_pistion_pulse_2_matches_native_outputs() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture_dir = root.join("test_data/piston-research/test-prefix-20261008");
    let manifest: Value = serde_json::from_slice(&std::fs::read(fixture_dir.join("download-manifest.json")).unwrap()).unwrap();
    let sample_data: Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/reference/test-reference-focused-replays.json")).unwrap())
            .unwrap();
    let sample = sample_data["samples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["name"] == "TEST_PISTION.schem")
        .unwrap();
    let bytes = std::fs::read(fixture_dir.join("TEST_PISTION.schem")).unwrap();
    let manifest_entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "TEST_PISTION.schem")
        .unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), manifest_entry["sha256"].as_str().unwrap());

    let origin = BlockPos::new(40, 30, 40);
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let mut native = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(
        &mut native,
        &clipboard,
        origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    for z in 0..clipboard.size_z as i32 {
        for x in 0..clipboard.size_x as i32 {
            let pos = origin + BlockPos::new(x, 0, z);
            let below = pos.offset(mchprs_blocks::BlockFace::Bottom);
            if crate::interaction::attachment_support(native.get_block(pos), pos).is_some_and(|(support, _)| support == below)
                && native.get_block(below) == Block::Air
            {
                native.set_block(below, Block::Stone {});
            }
        }
    }
    let bounds = (
        origin - BlockPos::new(4, 4, 4),
        origin + BlockPos::new(clipboard.size_x as i32 + 4, clipboard.size_y as i32 + 4, clipboard.size_z as i32 + 4),
    );
    let program = compile(
        &native,
        bounds,
        ScratchEntry {
            scheduled: native.scheduler().iter_entries().collect(),
            piston: native.piston_state().clone(),
        },
    )
    .unwrap();
    let mut runtime = program.start();
    let record = &sample["scenarios"]["pulse-2"];
    let control = origin + position(&record["actions"][0]["control"]);
    let source = program.sources.iter().position(|&node| program.positions[node] == control).unwrap();
    let outputs: Vec<_> = sample["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|output| {
            origin
                + BlockPos::new(
                    output["x"].as_i64().unwrap() as i32,
                    output["y"].as_i64().unwrap() as i32,
                    output["z"].as_i64().unwrap() as i32,
                )
        })
        .collect();

    for frame in record["frames"].as_array().unwrap() {
        let tick = frame["tick"].as_u64().unwrap();
        for action in record["actions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|action| action["tick"].as_u64() == Some(tick))
        {
            crate::interaction::use_redstone(native.get_block(control), &mut native, control, None);
            runtime.set_input(source, action["event"] == "rise").unwrap();
        }
        native.tick_interpreted();
        runtime.tick().unwrap();
        for &pos in &outputs {
            assert_eq!(
                native.get_block(pos),
                runtime.project(pos).unwrap_or(Block::Air),
                "native/compiled mismatch in TEST_PISTION pulse-2 tick {tick} at {pos:?}"
            );
        }
    }
}
