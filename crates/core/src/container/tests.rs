use super::*;

#[test]
fn hopper_and_furnace_creation_locking_and_cake_bites() {
    use crate::world::{storage::Chunk, World};
    use mchprs_blocks::{block_entities::BlockEntity, blocks::Block, BlockFace};
    let mut world =
        crate::plot::PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let pos = BlockPos::new(4, 30, 4);
    for name in ["hopper", "furnace"] {
        let block = Block::from_name(name).unwrap();
        let ty = ContainerType::from_block(block).unwrap();
        world.set_block(pos, block);
        assert!(
            matches!(world.get_block_entity(pos), Some(BlockEntity::Container { inventory, ty: found, .. }) if inventory.is_empty() && *found == ty)
        );
        world.set_block(pos, Block::Stone {});
        assert!(world.get_block_entity(pos).is_none());
    }
    for face in BlockFace::values() {
        assert_ne!(
            mchprs_blocks::blocks::HopperFacing::for_placement(face).to_string(),
            "up"
        );
    }
    world.set_block(pos, Block::from_name("hopper").unwrap());
    world.set_block(pos.offset(BlockFace::East), Block::RedstoneBlock {});
    crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    assert!(matches!(
        world.get_block(pos),
        Block::Hopper { enabled: false, .. }
    ));
    world.set_block(pos.offset(BlockFace::East), Block::Air {});
    crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    assert!(matches!(
        world.get_block(pos),
        Block::Hopper { enabled: true, .. }
    ));
    world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(pos, Block::Cake { bites: 0 });
    for bites in 0..7 {
        assert_eq!(
            crate::redstone::comparator::get_override(world.get_block(pos), &world, pos),
            14 - bites * 2
        );
        eat_cake(&mut world, pos);
    }
    assert_eq!(world.get_block(pos), Block::Air {});
    world.set_block(pos, Block::Cake { bites: 0 });
    crate::interaction::destroy(Block::Stone {}, &mut world, pos.offset(BlockFace::Bottom));
    assert_eq!(world.get_block(pos), Block::Air {});
}

#[test]
fn hopper_and_furnace_menus_use_their_own_slot_boundaries() {
    for ty in [ContainerType::Hopper, ContainerType::Furnace] {
        let size = ty.num_slots() as usize;
        let mut menu = menu();
        menu.ty = ty;
        let mut slots = vec![None; size + 36];
        let mut offhand = None;
        slots[size + 27] = stack(64);
        click(
            &mut menu,
            &mut slots,
            &mut offhand,
            (size + 27) as i16,
            0,
            0,
            true,
        );
        click(&mut menu, &mut slots, &mut offhand, 0, 0, 0, true);
        assert_eq!(count(&slots[0]), 64);
        assert_eq!(
            comparator_strength(&slots[..size]),
            if ty == ContainerType::Hopper { 3 } else { 5 }
        );
        click(&mut menu, &mut slots, &mut offhand, 0, 0, 1, true);
        assert_eq!(count(slots.last().unwrap()), 64);
        assert_eq!(total(&slots, &menu), 64);
        click(
            &mut menu,
            &mut slots,
            &mut offhand,
            (size + 35) as i16,
            0,
            3,
            true,
        );
        assert_eq!(count(&slots[size + 27]), 64);
    }
}

#[test]
fn furnace_output_fuel_and_shift_transfer_rules() {
    let mut menu = menu();
    menu.ty = ContainerType::Furnace;
    let mut slots = vec![None; 39];
    let mut offhand = None;
    menu.cursor = stack(64);
    click(&mut menu, &mut slots, &mut offhand, 2, 0, 0, true);
    click(&mut menu, &mut slots, &mut offhand, 1, 0, 0, true);
    assert!(slots[1].is_none() && slots[2].is_none());
    assert_eq!(count(&menu.cursor), 64);
    slots[2] = stack(10);
    menu.cursor = stack(50);
    click(&mut menu, &mut slots, &mut offhand, 2, 0, 0, true);
    assert_eq!(count(&menu.cursor), 60);
    assert!(slots[2].is_none());
    menu.cursor = None;
    slots[30] = Some(ItemStack {
        item_type: Item::from_name("coal").unwrap(),
        count: 64,
        nbt: None,
    });
    slots[31] = Some(ItemStack {
        item_type: Item::from_name("raw_iron").unwrap(),
        count: 64,
        nbt: None,
    });
    click(&mut menu, &mut slots, &mut offhand, 30, 0, 1, true);
    click(&mut menu, &mut slots, &mut offhand, 31, 0, 1, true);
    assert_eq!(count(&slots[1]), 64);
    assert_eq!(count(&slots[0]), 64);
    slots[1] = None;
    menu.cursor = Some(ItemStack {
        item_type: Item::from_name("bucket").unwrap(),
        count: 16,
        nbt: None,
    });
    click(&mut menu, &mut slots, &mut offhand, 1, 0, 0, true);
    assert_eq!(count(&slots[1]), 1);
    assert_eq!(count(&menu.cursor), 15);
}

#[test]
fn placement_uses_all_axes_and_nearest_diagonal_view() {
    use mchprs_blocks::BlockFacing::*;
    for (yaw, pitch, facing) in [
        (0.0, 0.0, North),
        (90.0, 0.0, East),
        (180.0, 0.0, South),
        (270.0, 0.0, West),
        (0.0, 90.0, Up),
        (0.0, -90.0, Down),
        (45.0, 40.0, Up),
        (45.0, -40.0, Down),
    ] {
        assert_eq!(barrel_facing(yaw, pitch), facing);
    }
}

#[test]
fn barrel_creation_state_changes_removal_and_chunk_round_trip() {
    use crate::plot::PlotWorld;
    use crate::world::{storage::Chunk, World};
    use mchprs_blocks::block_entities::BlockEntity;
    use mchprs_blocks::blocks::Block;
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let pos = BlockPos::new(4, 30, 4);
    world.set_block(pos, Block::from_name("barrel").unwrap());
    assert!(
        matches!(world.get_block_entity(pos),Some(BlockEntity::Container {
        ty:ContainerType::Barrel, inventory, comparator_override:0
    }) if inventory.is_empty())
    );
    let slots = vec![stack(64); 27];
    let entries = inventory_entries(&slots);
    world.set_block_entity(
        pos,
        BlockEntity::Container {
            ty: ContainerType::Barrel,
            inventory: entries.into(),
            comparator_override: 15,
        },
    );
    for id in 19431..=19442 {
        world.set_block_raw(pos, id);
        assert_eq!(
            crate::redstone::comparator::get_override(world.get_block(pos), &world, pos),
            15
        );
        let chunk = world.get_chunk_mut(0, 0).unwrap().save();
        let restored = Chunk::load(0, 0, chunk);
        assert_eq!(restored.get_block(4, 30, 4), id);
        assert!(
            matches!(restored.get_block_entity(pos),Some(BlockEntity::Container { inventory, .. }) if inventory.len()==27)
        );
    }
    world.set_block(pos, Block::Stone {});
    assert!(world.get_block_entity(pos).is_none());
    world.set_block(pos, Block::from_name("barrel").unwrap());
    assert_eq!(
        crate::redstone::comparator::get_override(world.get_block(pos), &world, pos),
        0
    );
}

fn stack(count: u8) -> Option<ItemStack> {
    Some(ItemStack {
        item_type: Item::from_name("redstone").unwrap(),
        count,
        nbt: None,
    })
}
fn menu() -> OpenContainer {
    OpenContainer {
        pos: BlockPos::new(0, 0, 0),
        ty: ContainerType::Barrel,
        window_id: 1,
        state_id: 0,
        cursor: None,
        drag: None,
        last_contents: vec![],
    }
}
fn count(slot: &Option<ItemStack>) -> u8 {
    slot.as_ref().map_or(0, |s| s.count)
}
fn total(slots: &[Option<ItemStack>], menu: &OpenContainer) -> usize {
    slots.iter().map(|s| count(s) as usize).sum::<usize>() + count(&menu.cursor) as usize
}

#[test]
fn pickup_split_merge_shift_and_swap_conserve_items() {
    let mut slots = vec![None; 63];
    slots[54] = stack(63);
    let mut menu = menu();
    let mut offhand = None;
    click(&mut menu, &mut slots, &mut offhand, 54, 1, 0, true);
    assert_eq!(count(&menu.cursor), 32);
    assert_eq!(count(&slots[54]), 31);
    click(&mut menu, &mut slots, &mut offhand, 0, 1, 0, true);
    assert_eq!(count(&slots[0]), 1);
    click(&mut menu, &mut slots, &mut offhand, 0, 0, 0, true);
    assert_eq!(count(&slots[0]), 32);
    assert!(menu.cursor.is_none());
    click(&mut menu, &mut slots, &mut offhand, 54, 0, 1, true);
    assert_eq!(count(&slots[0]), 63);
    assert!(slots[54].is_none());
    assert_eq!(total(&slots, &menu), 63);
    click(&mut menu, &mut slots, &mut offhand, 0, 0, 3, true);
    assert_eq!(count(&slots[54]), 63);
    assert!(slots[0].is_none());
    click(&mut menu, &mut slots, &mut offhand, 54, 40, 3, true);
    assert_eq!(count(&offhand), 63);
    assert_eq!(total(&slots, &menu), 0);
}

#[test]
fn drag_even_and_one_each_and_invalid_sequences() {
    let mut slots = vec![None; 63];
    let mut menu = menu();
    let mut offhand = None;
    menu.cursor = stack(10);
    click(&mut menu, &mut slots, &mut offhand, -999, 0, 5, true);
    for i in [0, 1, 2, 1] {
        click(&mut menu, &mut slots, &mut offhand, i, 1, 5, true);
    }
    click(&mut menu, &mut slots, &mut offhand, -999, 2, 5, true);
    assert_eq!(
        [
            count(&slots[0]),
            count(&slots[1]),
            count(&slots[2]),
            count(&menu.cursor)
        ],
        [3, 3, 3, 1]
    );
    click(&mut menu, &mut slots, &mut offhand, -999, 4, 5, true);
    click(&mut menu, &mut slots, &mut offhand, 3, 5, 5, true);
    click(&mut menu, &mut slots, &mut offhand, -999, 6, 5, true);
    assert_eq!(count(&slots[3]), 1);
    assert!(menu.cursor.is_none());
    let before = signature(&slots, &menu.cursor);
    for (slot, button, mode) in [(32767, 0, 0), (-1, 0, 1), (-999, 15, 5), (0, 9, 3)] {
        click(
            &mut menu,
            &mut slots,
            &mut offhand,
            slot,
            button,
            mode,
            true,
        );
    }
    assert_eq!(signature(&slots, &menu.cursor), before);
}

#[test]
fn collect_clone_throw_and_component_limits() {
    let mut slots = vec![None; 63];
    let mut menu = menu();
    let mut offhand = None;
    slots[0] = stack(30);
    slots[1] = stack(64);
    menu.cursor = stack(10);
    click(&mut menu, &mut slots, &mut offhand, 0, 0, 6, true);
    assert_eq!(count(&menu.cursor), 64);
    assert_eq!(count(&slots[1]), 40);
    menu.cursor = None;
    click(&mut menu, &mut slots, &mut offhand, 1, 2, 2, false);
    assert!(menu.cursor.is_none());
    click(&mut menu, &mut slots, &mut offhand, 1, 2, 2, true);
    assert_eq!(count(&menu.cursor), 64);
    menu.cursor = None;
    click(&mut menu, &mut slots, &mut offhand, 1, 0, 4, true);
    assert_eq!(count(&slots[1]), 39);
    let mut bytes = vec![];
    bytes.write_varint(16);
    bytes.write_varint(Item::Stone {}.get_id() as i32);
    bytes.extend_from_slice(&[1, 0, 1, 16]); // max_stack_size=16 component patch
    let data = mchprs_network::packets::components::read_slot(&mut std::io::Cursor::new(bytes))
        .unwrap()
        .unwrap();
    let custom = ItemStack {
        item_type: Item::Stone {},
        count: 16,
        nbt: data.nbt,
    };
    assert_eq!(max_count(&custom), 16);
    slots[2] = Some(custom.clone());
    menu.cursor = Some(custom);
    click(&mut menu, &mut slots, &mut offhand, 2, 0, 0, true);
    assert_eq!(count(&slots[2]), 16);
    assert_eq!(count(&menu.cursor), 16);
    assert_eq!(comparator_strength(&vec![slots[2].clone(); 27]), 15);
    let restored = inventory_slots(&inventory_entries(&slots[..27]), 27);
    assert_eq!(signature(&restored, &None), signature(&slots[..27], &None));
}
