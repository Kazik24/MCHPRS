use super::fixer::legacy_1_18 as legacy;
use super::*;
use mchprs_blocks::block_entities::MovingPistonEntity;
use mchprs_blocks::BlockFace;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
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
        assert_eq!(&bytes[8..16], &[3, 0, 0, 0, 229, 16, 0, 0]);
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
