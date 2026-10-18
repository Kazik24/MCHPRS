//! Independent boundary discovery checks on separate, fixture-derived devices.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::instant_regions::infer_stable_regions;

fn independent_devices() -> (CompileGraph, NodeId) {
    let first = load_adder("adder_1bit", ORIGIN, None);
    let second = load_adder("adder_1bit", ORIGIN + BlockPos::new(48, 0, 0), None);
    let mut world = first.build();
    for (position, block, entity) in &second.cells {
        let position = second.place(*position);
        world.set_block(position, *block);
        if let Some(entity) = entity {
            world.set_block_entity(position, entity.clone());
        }
    }
    let bounds = (first.bounds().0.min(second.bounds().0), first.bounds().1.max(second.bounds().1));
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let control = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(first.trigger()))
        .unwrap();
    (prepared.graph, NodeId::new(control as u32))
}

#[test]
fn shared_external_power_does_not_merge_independent_instant_devices() {
    let (mut graph, control) = independent_devices();
    let monitor = TaskMonitor::default();
    let graph = &mut graph;
    let original = infer_stable_regions(graph, &monitor).unwrap();
    assert!(original.len() >= 2, "the separate fixtures must begin with independent owners");
    let ownership: Vec<_> = original.iter().map(|region| region.actors.clone()).collect();
    for region in &original {
        let sides: Vec<_> = region
            .actors
            .iter()
            .map(|actor| graph.pistons[actor.index()].position.x >= ORIGIN.x + 48)
            .collect();
        assert!(
            sides.iter().all(|side| *side == sides[0]),
            "a starting region must not span both fixtures"
        );
    }
    let source = graph.sites.iter().position(|site| site.source == SiteSource::Component(control)).unwrap();
    let always = graph.guards.iter().position(|guard| *guard == Guard::Always).unwrap();
    let mut ports = Vec::new();
    for region in &original {
        let actor = region.actors[0];
        let link = graph.power_links.len() as u32;
        ports.push((actor, link));
        // Graph-level contributions isolate boundary classification from spatial wire parsing.
        graph.power_links.push(PowerLink {
            from: SiteId::new(source as u32),
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node: graph.pistons[actor.index()].node,
                port: InputPort::Main,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(always as u32),
        });
    }
    let regions = infer_stable_regions(graph, &monitor).unwrap();
    assert_eq!(
        regions.iter().map(|region| region.actors.clone()).collect::<Vec<_>>(),
        ownership,
        "common input power is a boundary dependency and must preserve the original ownership partition"
    );
    for (actor, link) in ports {
        let region = regions.iter().find(|region| region.actors.contains(&actor)).unwrap();
        assert!(region.power_inputs.contains(&link));
    }
}

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
        let mut backend = stable_region_tests::stable_backend_at(&circuit, &native, 3);
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
