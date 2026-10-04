use super::fixer::legacy_1_18 as legacy;
use super::*;
use mchprs_blocks::block_entities::MovingPistonEntity;
use mchprs_blocks::BlockFace;
use std::fs;
use std::iter::FromIterator;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn format_four_migration_preserves_original_and_command_blocks_save_in_five() {
    use mchprs_blocks::block_entities::CommandBlockEntity;
    let temp = Temp::new();
    let path = temp.0.join("plot");
    let mut data = PlotData::<1> {
        tps: Tps::Limited(20),
        world_send_rate: WorldSendRate(60),
        chunk_data: vec![ChunkData {
            sections: [None],
            block_entities: Default::default(),
        }],
        pending_ticks: vec![],
        piston_state: Default::default(),
        piston_animation: PistonAnimation::Off,
    };
    let mut old = PLOT_MAGIC.to_vec();
    old.extend_from_slice(&4u32.to_le_bytes());
    old.extend_from_slice(&MC_DATA_VERSION.to_le_bytes());
    old.extend_from_slice(
        &bincode::serialize(&(
            data.tps,
            data.world_send_rate,
            &data.chunk_data,
            &data.pending_ticks,
            &data.piston_state,
        ))
        .unwrap(),
    );
    fs::write(&path, &old).unwrap();
    assert_eq!(
        PlotData::<1>::load_from_file(&path, true)
            .unwrap()
            .piston_animation,
        PistonAnimation::Auto
    );
    assert_eq!(fs::read(temp.0.join("plot.bak")).unwrap(), old);
    data.chunk_data[0].block_entities.insert(
        BlockPos::new(1, 2, 3),
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say saved".into(),
            ..Default::default()
        })),
    );
    data.save_to_file(&path).unwrap();
    assert_eq!(
        u32::from_le_bytes(fs::read(&path).unwrap()[8..12].try_into().unwrap()),
        5
    );
    let loaded = PlotData::<1>::load_from_file(&path, false).unwrap();
    assert_eq!(loaded.piston_animation, PistonAnimation::Off);
    assert!(
        matches!(loaded.chunk_data[0].block_entities.get(&BlockPos::new(1,2,3)), Some(BlockEntity::CommandBlock(entity)) if entity.command == "say saved")
    );
}
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mchprs-migration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn old_bytes(state: u32, direct: bool) -> Vec<u8> {
    let bits = if direct { 14 } else { 4 };
    let per = 64 / bits;
    let value = if direct { state as i64 } else { 0 };
    let mut data = vec![0; 4096usize.div_ceil(per)];
    for i in 0..4096 {
        data[i / per] |= value << ((i % per) * bits);
    }
    let mut entities = FxHashMap::default();
    entities.insert(
        BlockPos::new(1, 8, 1),
        legacy::Entity::MovingPiston(MovingPistonEntity {
            block_state: state,
            facing: BlockFace::East,
            extending: true,
            progress: 127,
            source: false,
        }),
    );
    entities.insert(
        BlockPos::new(2, 8, 1),
        legacy::Entity::Sign(Box::new(legacy::Sign {
            rows: ["A".into(), "B".into(), "C".into(), "D".into()],
        })),
    );
    let plot = legacy::Plot::<1> {
        tps: Tps::Limited(20),
        world_send_rate: WorldSendRate(30),
        pending_ticks: vec![],
        chunk_data: vec![legacy::Chunk {
            sections: [Some(ChunkSectionData {
                data,
                palette: vec![state as i32],
                bits_per_block: bits as i8,
                entries: 4096,
                block_count: 4096,
            })],
            block_entities: entities,
        }],
    };
    let mut bytes = PLOT_MAGIC.to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&bincode::serialize(&plot).unwrap());
    bytes
}
#[test]
fn migrates_palette_and_direct_states_with_pistons_signs_and_backup() {
    for direct in [false, true] {
        let temp = Temp::new();
        let path = temp.0.join("plot");
        let original = old_bytes(1483, direct);
        fs::write(&path, &original).unwrap();
        fs::write(temp.0.join("plot.bak"), b"existing backup").unwrap();
        let converted = PlotData::<1>::load_from_file(&path, true).unwrap();
        assert_eq!(converted.world_send_rate.0, 30);
        let section = converted.chunk_data[0].sections[0].as_ref().unwrap();
        assert_eq!(section.palette[0], legacy::state(1483).unwrap() as i32);
        if direct {
            assert_eq!(section.bits_per_block, 15);
            for i in 0..4096 {
                assert_eq!(
                    (section.data[i / 4] as u64 >> ((i % 4) * 15)) & 32767,
                    legacy::state(1483).unwrap() as u64
                );
            }
        }
        assert!(
            matches!(converted.chunk_data[0].block_entities.get(&BlockPos::new(1,8,1)), Some(BlockEntity::MovingPiston(p)) if p.block_state == legacy::state(1483).unwrap() && p.extending && p.progress == 127)
        );
        assert!(
            matches!(converted.chunk_data[0].block_entities.get(&BlockPos::new(2,8,1)), Some(BlockEntity::Sign(s)) if s.rows[0] == "A" && s.back_rows[0] == "{\"text\":\"\"}")
        );
        assert_eq!(
            fs::read(temp.0.join("plot.bak")).unwrap(),
            b"existing backup"
        );
        assert_eq!(fs::read(temp.0.join("plot.bak.1")).unwrap(), original);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            VERSION
        );
        assert_eq!(
            u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            MC_DATA_VERSION
        );
        PlotData::<1>::load_from_file(&path, true).unwrap();
        converted.save_to_file(&path).unwrap(); // Existing destination replacement on Windows.
    }
}
#[test]
fn failed_migration_preserves_original_and_does_not_create_backup() {
    let temp = Temp::new();
    let path = temp.0.join("plot");
    let bytes = old_bytes(u32::MAX, false);
    fs::write(&path, &bytes).unwrap();
    assert!(PlotData::<1>::load_from_file(&path, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}
#[test]
fn rejects_other_minecraft_versions_and_unsupported_save_formats() {
    let temp = Temp::new();
    let path = temp.0.join("plot");
    let mut bytes = PLOT_MAGIC.to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&3955u32.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert!(PlotData::<1>::load_from_file(&path, true).is_err());
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        PlotData::<1>::load_from_file(&path, true),
        Err(PlotLoadError::ConversionUnavailable(2))
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

fn format_three_motion(occupied: bool) -> Vec<u8> {
    use super::fixer::legacy_1_21_5::{LegacyTick, Plot};
    use mchprs_blocks::blocks::{Block, RedstonePiston};
    use mchprs_blocks::BlockFacing;
    let base = BlockPos::new(3, 8, 4);
    let head = base.offset(BlockFace::East);
    let piston = RedstonePiston {
        facing: BlockFacing::East,
        sticky: true,
        extended: false,
    };
    let mut data = vec![0i64; 1024];
    let mut put = |pos: BlockPos, block: Block| {
        let i = pos.x as usize + pos.z as usize * 16 + pos.y as usize * 256;
        data[i / 4] |= (block.get_id() as i64) << (i % 4 * 15);
    };
    put(base, Block::Piston { piston });
    put(
        head,
        Block::MovingPiston {
            moving: piston.into(),
        },
    );
    if occupied {
        put(head.offset(BlockFace::East), Block::GoldBlock {});
    }
    let plot = Plot::<1> {
        tps: Tps::Limited(20),
        world_send_rate: WorldSendRate(30),
        chunk_data: vec![ChunkData {
            sections: [Some(ChunkSectionData {
                data,
                palette: vec![],
                bits_per_block: 15,
                block_count: if occupied { 3 } else { 2 },
                entries: 4096,
            })],
            block_entities: FxHashMap::from_iter([(
                head,
                BlockEntity::MovingPiston(MovingPistonEntity {
                    extending: true,
                    source: true,
                    facing: BlockFace::East,
                    progress: 127,
                    block_state: Block::Stone {}.get_id(),
                }),
            )]),
        }],
        pending_ticks: vec![LegacyTick {
            pos: base,
            ticks_left: 3,
            tick_priority: mchprs_world::TickPriority::Normal,
        }],
    };
    let mut bytes = PLOT_MAGIC.to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&MC_DATA_VERSION.to_le_bytes());
    bytes.extend_from_slice(&bincode::serialize(&plot).unwrap());
    bytes
}

#[test]
fn format_three_motion_migrates_to_destination_entities_and_keeps_backup() {
    use mchprs_blocks::blocks::Block;
    let temp = Temp::new();
    let path = temp.0.join("plot");
    let old = format_three_motion(false);
    fs::write(&path, &old).unwrap();
    let data = PlotData::<1>::load_from_file(&path, true).unwrap();
    assert_eq!(fs::read(temp.0.join("plot.bak")).unwrap(), old);
    let head = BlockPos::new(4, 8, 4);
    assert!(
        matches!(data.chunk_data[0].block_entities.get(&head), Some(BlockEntity::MovingPiston(e))
        if e.source && matches!(Block::from_id(e.block_state), Block::PistonHead { .. }))
    );
    assert!(
        matches!(data.chunk_data[0].block_entities.get(&head.offset(BlockFace::East)), Some(BlockEntity::MovingPiston(e))
        if !e.source && Block::from_id(e.block_state) == Block::Stone {} && e.progress == 127)
    );
    assert_eq!(data.pending_ticks[0].block_type, None);
    PlotData::<1>::load_from_file(&path, true).unwrap();
}

#[test]
fn format_three_occupied_payload_destination_refuses_migration_without_changes() {
    let temp = Temp::new();
    let path = temp.0.join("plot");
    let old = format_three_motion(true);
    fs::write(&path, &old).unwrap();
    assert!(PlotData::<1>::load_from_file(&path, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), old);
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}
