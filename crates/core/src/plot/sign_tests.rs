use super::*;
use mchprs_blocks::block_entities::SignBlockEntity;

fn world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}

#[test]
fn placing_signs_creates_editable_entities_and_preserves_text_on_rotation() {
    let mut world = world();
    let pos = BlockPos::new(31, 20, 47);
    for face in BlockFace::values() {
        world.set_block(pos.offset(face), Block::Stone {});
    }
    for name in [
        "oak_sign",
        "spruce_wall_sign",
        "cherry_sign",
        "bamboo_wall_sign",
    ] {
        let mut block = Block::from_name(name).unwrap();
        crate::interaction::place_in_world(block, &mut world, pos, &None);
        assert!(
            matches!(world.get_block_entity(pos), Some(BlockEntity::Sign(_))),
            "{name}"
        );

        let sign = SignBlockEntity {
            rows: std::array::from_fn(|_| r#"{"text":"front"}"#.into()),
            back_rows: std::array::from_fn(|_| r#"{"text":"back"}"#.into()),
            ..Default::default()
        };
        let entity = BlockEntity::Sign(Box::new(sign));
        world.set_block_entity(pos, entity.clone());
        block.rotate(mchprs_blocks::blocks::RotateAmt::Rotate90);
        world.set_block(pos, block);
        assert_eq!(
            world
                .get_block_entity(pos)
                .unwrap()
                .to_nbt(false)
                .unwrap()
                .content,
            entity.to_nbt(false).unwrap().content
        );

        world.set_block(pos, Block::Stone {});
        assert!(world.get_block_entity(pos).is_none());
    }
}
