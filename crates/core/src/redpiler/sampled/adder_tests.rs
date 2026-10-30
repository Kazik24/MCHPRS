//! Manifest acceptance boundary for the independent MAIR executor.
use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::world::storage::Chunk;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn position(value: &Value) -> BlockPos {
    BlockPos::new(value[0].as_i64().unwrap() as i32, value[1].as_i64().unwrap() as i32, value[2].as_i64().unwrap() as i32)
}

#[test]
fn adder1bit_mair_manifest_arithmetic() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let inspection: Value = serde_json::from_slice(&std::fs::read(root.join(manifest["inspection"].as_str().unwrap())).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(hash, "e80092c97fc4637c313a735d38d0241b69b876f41dd2db7de571a6458369e526");
    assert_eq!(manifest["sha256"], hash);
    assert_eq!(inspection["sha256"], hash);
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!((clipboard.size_x, clipboard.size_y, clipboard.size_z), (21, 7, 7));
    let origin = position(&manifest["coordinates"]["origin"]);
    let anchor = position(&manifest["coordinates"]["paste_anchor"]);
    assert_eq!(anchor, origin + position(&manifest["coordinates"]["loader_offset"]));
    assert_eq!(anchor, origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z));
    let bounds = (origin - BlockPos::new(4, 4, 4), origin + BlockPos::new(24, 10, 10));
    let positions: Vec<_> = (bounds.0.y..=bounds.1.y).flat_map(|y| (bounds.0.z..=bounds.1.z)
        .flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z)))).collect();
    let snapshot = |world: &PlotWorld| {
        (positions.iter().map(|&pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap())).collect::<Vec<_>>(),
         world.scheduler().iter_entries().collect::<Vec<_>>(), serde_json::to_value(world.piston_state()).unwrap())
    };
    let mut cases = 0;
    let mut regression = false;
    for case in manifest["cases"].as_array().unwrap().iter().filter(|case| case["inputs"].is_array()) {
        let id = case["id"].as_str().unwrap();
        let mut world = PlotWorld::from_chunks(0, 0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(), Default::default());
        paste_clipboard(&mut world, &clipboard, anchor, false);
        let before = snapshot(&world);
        let compiled = compile(&world, bounds, ScratchEntry {
            scheduled: world.scheduler().iter_entries().collect(), piston: world.piston_state().clone(),
        });
        assert_eq!(before, snapshot(&world), "{id}: compilation changed host");
        let program = compiled.unwrap_or_else(|error| panic!("{id}: MAIR compilation declined: {error:?}; block={:?}", error.position.map(|pos| world.get_block(pos))));
        let mut runtime = program.start();
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let pos = origin + position(&action["pos"]);
                    let source = program.sources.iter().position(|&index| program.positions[index] == pos).expect("manifest control represented");
                    runtime.set_input(source, action["powered"].as_bool().unwrap()).unwrap();
                }
                "wait" | "wait_ready" => for _ in 0..action["ticks"].as_u64().unwrap() { runtime.tick().unwrap(); },
                op => panic!("unsupported manifest action {op}"),
            }
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            runtime.tick().unwrap();
            for (port, low, high, expected) in [("sum_payload", 1, 3, "sum"), ("sum_repeater", 3, 7, "sum"), ("carry_repeater", 5, 9, "carry")] {
                if !(low..=high).contains(&tick) { continue; }
                let pos = origin + position(&manifest["ports"]["observations"][port]);
                let index = program.positions.iter().position(|&p| p == pos).expect("declared output represented");
                let block = runtime.blocks[index];
                let value = match (port, block) {
                    ("sum_payload", Block::Air) => 1,
                    ("sum_payload", Block::RedstoneBlock) => 0,
                    (_, Block::RedstoneRepeater { repeater }) if port != "sum_payload" => u64::from(!repeater.powered),
                    _ => panic!("{id} {port} response tick {tick}: undecodable output {block:?}"),
                };
                assert_eq!(value, case["expectation"][expected].as_u64().unwrap(), "{id} {port} response tick {tick}");
            }
        }
        assert!(runtime.operations > 0, "{id}: no compiled instructions executed");
        regression |= id == "prepared-0-1-0";
        cases += 1;
    }
    assert_eq!(cases, 8);
    assert!(regression, "prepared-0-1-0 regression must remain in the manifest");
}
