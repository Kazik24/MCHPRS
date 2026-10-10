use super::*;

#[test]
fn divider_cross_and_dot_sample_the_same_power_on_comparator_edges() {
    use crate::redpiler::analysis::topology::PowerRoute;

    for name in ["fpu_divider", "divider_explanation"] {
        let fixture = manifest(name);
        let base = origin(&fixture) + BlockPos::new(26, 9, 2);
        let data = base + BlockPos::new(1, 1, 2);
        let wire_pos = base + BlockPos::new(0, 1, 1);
        let conductor = base + BlockPos::new(0, 1, 2);
        let mut traces = Vec::new();
        for dot in [false, true] {
            let (mut world, _) = load(&fixture);
            let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                panic!("missing cross");
            };
            assert!(crate::redstone::wire::is_cross(wire));
            if dot {
                crate::redstone::wire::on_use(wire, &mut world, wire_pos);
                let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                    unreachable!();
                };
                assert!(
                    crate::redstone::wire::is_dot(wire),
                    "click must produce a valid dot"
                );
            }
            let report = analyze_world(&world);
            let actor = report.pistons.iter().position(|p| p.pos == base).unwrap();
            let inputs = &report.recognition[actor].inputs.sources;
            assert!(inputs
                .iter()
                .any(|s| s.source == data && s.route == PowerRoute::Direct));
            assert_eq!(
                inputs
                    .iter()
                    .any(|s| s.source == data && s.route == PowerRoute::QuasiConnectivity),
                !dot
            );
            let mut edges = Vec::new();
            for present in [true, false] {
                if !present {
                    world.set_block(conductor, Block::Air);
                    crate::redstone::update_wire_neighbors(&mut world, conductor);
                }
                for strength in [0, 15] {
                    let Block::RedstoneComparator { comparator } = world.get_block(data) else {
                        unreachable!();
                    };
                    world.set_block(
                        data.offset(comparator.facing.block_face()),
                        if strength == 0 {
                            Block::Air
                        } else {
                            Block::RedstoneBlock
                        },
                    );
                    let samples: Vec<_> = trace::capture(|| crate::redstone::comparator::tick(comparator, &mut world, data))
                        .into_iter().filter(|e| matches!(e.operation, trace::Operation::Sample { pos, .. } if pos == base)).collect();
                    let Block::RedstoneWire { wire } = world.get_block(wire_pos) else {
                        unreachable!();
                    };
                    let expected = if present { strength } else { 0 };
                    assert_eq!(wire.power, expected);
                    assert_eq!(
                        crate::redstone::get_redstone_power(
                            world.get_block(wire_pos.offset(BlockFace::Bottom)),
                            &world,
                            wire_pos.offset(BlockFace::Bottom),
                            BlockFace::South
                        ),
                        expected
                    );
                    assert_eq!(
                        crate::redstone::get_redstone_power(
                            world.get_block(wire_pos),
                            &world,
                            wire_pos,
                            BlockFace::South
                        ),
                        if dot { 0 } else { expected }
                    );
                    assert_eq!(samples.is_empty(), !present);
                    edges.push(samples);
                }
            }
            traces.push(edges);
        }
        assert_eq!(
            traces[0], traces[1],
            "{name}: ordered piston samples must match"
        );
    }
}
