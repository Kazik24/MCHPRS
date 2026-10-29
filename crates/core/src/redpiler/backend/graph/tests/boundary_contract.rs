//! Independent boundary discovery checks on separate, fixture-derived devices.
use super::*;

#[test]
fn regional_bud_memory_changes_only_after_the_independent_update() {
    let circuit = load_mechanism("bud_instantmemorycellobserverupdate");
    let positions = circuit.positions();
    let data = circuit.place(local(&circuit.ports["inputs"]["data"]));
    let update = circuit.place(local(&circuit.ports["inputs"]["update"]));
    let memory = circuit.place(local(&circuit.ports["observations"]["memory"]));
    let mut saw_action = false;
    let mut sampled_memory = Vec::new();
    for powered in [false, true] {
        let mut native = circuit.build();
        let mut projected = circuit.build();
        let mut backend = GraphBackend::compile_with_options(&native, circuit.bounds(), vec![], &Default::default(), Arc::default()).unwrap();
        let initial_memory = native.get_block(memory);
        let steps = std::iter::once(Step::SetLever { position: data, powered })
            .chain(std::iter::repeat_n(Step::Tick, 32))
            .chain(std::iter::once(Step::SetLever {
                position: update,
                powered: false,
            }))
            .chain(std::iter::repeat_n(Step::Tick, 36));
        for (index, step) in steps.enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut backend, step, toggle).unwrap();
            check_boundary(
                &backend,
                &native,
                &mut projected,
                &positions,
                &format!("BUD data={powered} event={index}"),
            )
            .unwrap();
            if index <= 32 {
                assert_eq!(native.get_block(memory), initial_memory, "QC data alone must not sample memory");
                assert_eq!(projected.get_block(memory), initial_memory);
            }
        }
        saw_action |= backend.execution_counts().piston_actions > 0;
        sampled_memory.push(projected.get_block(memory));
        backend.reset(&mut projected);
        for tick in 0..8 {
            native.tick_interpreted();
            projected.tick_interpreted();
            assert_eq!(
                future(&projected, &positions),
                future(&native, &positions),
                "BUD handoff data={powered} tick={tick}"
            );
        }
    }
    assert!(saw_action, "at least one sampled data value must change the memory mechanism");
    assert_ne!(
        sampled_memory[0], sampled_memory[1],
        "the update must actually sample the two distinct QC data values"
    );
}

pub(super) fn load_mechanism(id: &'static str) -> Adder {
    let directory = root().join("test_data/instant-pistons-io");
    let manifest: Value = serde_json::from_slice(&std::fs::read(directory.join("fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
    let bytes = std::fs::read(root().join(manifest["fixture"].as_str().unwrap())).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(manifest["sha256"], hash);

    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let size = BlockPos::new(clipboard.size_x as i32, clipboard.size_y as i32, clipboard.size_z as i32);
    let mut imported = empty_world();
    paste_clipboard(
        &mut imported,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let mut cells = Vec::new();
    for y in 0..size.y {
        for z in 0..size.z {
            for x in 0..size.x {
                let position = BlockPos::new(x, y, z);
                let block = imported.get_block(ORIGIN + position);
                let entity = imported.get_block_entity(ORIGIN + position).cloned();
                if block != Block::Air || entity.is_some() {
                    cells.push((position, block, entity));
                }
            }
        }
    }
    Adder {
        id,
        hash,
        size,
        anchor: ORIGIN,
        rotation: None,
        cells,
        ports: manifest["ports"].clone(),
    }
}
