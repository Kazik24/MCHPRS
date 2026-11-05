use super::cpus;
use crate::{plot::PlotWorld, world::World};
use mchprs_blocks::{BlockDirection, BlockPos, blocks::Block};
use mchprs_world::TickPriority;
use serde_json::json;

#[derive(serde::Deserialize)]
struct Fixture {
    min: BlockPos,
    max: BlockPos,
    start: BlockPos,
    blocks: Vec<(BlockPos, u32)>,
    #[serde(default)]
    entities: Vec<(BlockPos, mchprs_blocks::block_entities::BlockEntity)>,
    pending: Vec<mchprs_world::TickEntry>,
}

fn saved_fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scraps/redpilator-regressions/2026-11-04/pm1-piston-slice.json.gz");
    serde_json::from_reader(flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())).unwrap()
}

#[test]
#[ignore = "exports the verified PM1 divergent slice as a portable schematic"]
fn pm1_export_saved_slice_schematic() {
    use crate::plot::worldedit::{WorldEditClipboard, load_schematic, save_schematic};
    use crate::world::storage::PalettedBitBuffer;
    let fixture = saved_fixture();
    let size = fixture.max - fixture.min + BlockPos::new(1, 1, 1);
    let mut data = PalettedBitBuffer::new((size.x * size.y * size.z) as usize, 9);
    for (pos, id) in fixture.blocks {
        let pos = pos - fixture.min;
        data.set_entry(((pos.y * size.z + pos.z) * size.x + pos.x) as usize, id);
    }
    let clipboard = WorldEditClipboard {
        offset_x: 0,
        offset_y: 0,
        offset_z: 0,
        size_x: size.x as u32,
        size_y: size.y as u32,
        size_z: size.z as u32,
        data,
        block_entities: fixture.entities.into_iter().map(|(pos, entity)| (pos - fixture.min, entity)).collect(),
    };
    let name = "regression-pm1-20261104-slice.schem";
    save_schematic(name, &clipboard).unwrap();
    let bytes = std::fs::read(std::path::Path::new("schems").join(name)).unwrap();
    let loaded = load_schematic(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(
        (loaded.size_x, loaded.size_y, loaded.size_z),
        (clipboard.size_x, clipboard.size_y, clipboard.size_z)
    );
    for index in 0..clipboard.data.entries() {
        assert_eq!(
            loaded.data.get_entry(index),
            clipboard.data.get_entry(index),
            "schematic roundtrip cell {index}"
        );
    }
    assert_eq!(loaded.block_entities.len(), clipboard.block_entities.len());
    for (pos, entity) in &clipboard.block_entities {
        assert_eq!(
            serde_json::to_value(loaded.block_entities.get(pos)).unwrap(),
            serde_json::to_value(Some(entity)).unwrap(),
            "schematic entity roundtrip {pos:?}"
        );
    }
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/redpilator-regressions/2026-11-04/pm1-piston-slice.schem");
    std::fs::write(&destination, bytes).unwrap();
    eprintln!("exported {} with {} entities", destination.display(), loaded.block_entities.len());
}

#[test]
#[ignore = "PM1 staged crop investigation; writes diagnostic fixtures"]
fn pm1_staged_piston_slices() {
    use crate::redpilator::{Compiler, CompilerOptions, TaskMonitor};
    use crate::world::{scheduler::TickScheduler, storage::Chunk};
    use std::sync::Arc;
    let cpu = cpus::Cpu {
        schematic: "PM1_SORT_FIXED.schem",
        sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
        ..cpus::CPUS[0]
    };
    let mut full = cpus::load_cpu(cpu);
    let initial = std::env::var_os("MCHPRS_PM1_INITIAL").is_some();
    if !initial {
        cpus::click_cpu(&mut full, cpu, cpu.start);
    }
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pm1-slices");
    std::fs::create_dir_all(&directory).unwrap();
    let actors = [BlockPos::new(155, 10, 126), BlockPos::new(153, 10, 124)];
    let mut attempts = Vec::new();
    for stage in 0..=9 {
        if stage >= 6 || initial && stage == 0 {
            let custom = std::env::var("MCHPRS_PM1_SLICE_BOUNDS").ok().map(|s| {
                let v: Vec<i32> = s.split(',').map(|s| s.parse().unwrap()).collect();
                assert_eq!(v.len(), 6);
                ("custom", BlockPos::new(v[0], v[1], v[2]), BlockPos::new(v[3], v[4], v[5]))
            });
            let bounds = custom.map(|v| vec![v]).unwrap_or_else(|| {
                vec![
                    ("chain", BlockPos::new(8, 8, 90), BlockPos::new(180, 40, 145)),
                    ("pulse", BlockPos::new(140, 9, 122), BlockPos::new(158, 12, 129)),
                    ("control", BlockPos::new(100, 8, 90), BlockPos::new(180, 40, 145)),
                    ("west", BlockPos::new(120, 8, 110), BlockPos::new(180, 20, 140)),
                    ("wide", BlockPos::new(140, 8, 110), BlockPos::new(180, 20, 140)),
                    ("medium", BlockPos::new(150, 8, 120), BlockPos::new(176, 14, 132)),
                    ("small", BlockPos::new(151, 9, 122), BlockPos::new(158, 12, 129)),
                ]
            });
            for (name, min, max) in bounds {
                if initial && (name != "custom" || stage != 0) {
                    continue;
                }
                if !initial && matches!(name, "chain" | "pulse" | "custom" | "control" | "west") && stage != 9 {
                    continue;
                }
                let inside = |p: BlockPos| (min.x..=max.x).contains(&p.x) && (min.y..=max.y).contains(&p.y) && (min.z..=max.z).contains(&p.z);
                let mut crop = PlotWorld::from_chunks(
                    0,
                    0,
                    (0..crate::plot::PLOT_WIDTH)
                        .flat_map(|x| (0..crate::plot::PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
                        .collect(),
                    TickScheduler::default(),
                );
                let mut blocks = Vec::new();
                let mut entities = Vec::new();
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        for x in min.x..=max.x {
                            let pos = BlockPos::new(x, y, z);
                            let block = full.get_block(pos);
                            if block != (Block::Air {}) {
                                crop.set_block(pos, block);
                                blocks.push((pos, block.get_id()));
                            }
                            if let Some(entity) = full.get_block_entity(pos) {
                                crop.set_block_entity(pos, entity.clone());
                                entities.push((pos, entity.clone()));
                            }
                        }
                    }
                }
                let mut pending: Vec<_> = full.scheduler().iter_entries().filter(|e| inside(e.pos)).collect();
                if name == "pulse" {
                    let source = BlockPos::new(140, 10, 126);
                    let block = Block::StoneButton {
                        button: mchprs_blocks::blocks::StoneButton::new(mchprs_blocks::blocks::ButtonFace::Floor, BlockDirection::North, true),
                    };
                    crop.set_block(source, block);
                    blocks.retain(|(p, _)| *p != source);
                    blocks.push((source, block.get_id()));
                    pending.push(mchprs_world::TickEntry {
                        pos: source,
                        ticks_left: 1,
                        tick_priority: TickPriority::Normal,
                        block_type: Some(block.registry_id()),
                    });
                }
                for entry in &pending {
                    crop.restore_scheduled_tick(entry.clone());
                }
                let moving = blocks
                    .iter()
                    .filter(|(_, id)| matches!(Block::from_id(*id), Block::MovingPiston { .. }))
                    .count();
                if moving != 0 {
                    attempts.push(json!({"stage":stage,"name":name,"moving":moving,"result":"skipped active motion"}));
                    continue;
                }
                let mut projected = crop.activation_snapshot();
                let mut compiler = Compiler::default();
                let result = compiler.compile(
                    &crop,
                    (min, max),
                    CompilerOptions {
                        publication: Some(crate::redpilator::PublicationMode::All),
                        ..Default::default()
                    },
                    pending.clone(),
                    Arc::new(TaskMonitor::default()),
                );
                if let Err(error) = result {
                    attempts.push(json!({"stage":stage,"name":name,"result":error.to_string()}));
                    continue;
                }
                let initial = blocks.clone();
                let start = (stage == 0).then_some(cpu.origin + cpu.start);
                if start.is_some() {
                    cpus::click_cpu(&mut crop, cpu, cpu.start);
                    compiler.try_on_use_block(start.unwrap()).unwrap();
                }
                let mut first = None;
                let mut trace = Vec::new();
                for relative in 1..=10 - stage {
                    let events = crate::redstone::piston::trace::capture_detailed(&actors, || crop.tick_interpreted());
                    trace.extend(events);
                    compiler.tick().unwrap();
                    compiler.try_flush(&mut projected).unwrap();
                    let mut differences = Vec::new();
                    for y in min.y..=max.y {
                        for z in min.z..=max.z {
                            for x in min.x..=max.x {
                                let pos = BlockPos::new(x, y, z);
                                let expected = crop.get_block(pos);
                                let actual = projected.get_block(pos);
                                if actual != expected {
                                    differences.push(json!({"pos":pos,"interpreter":format!("{expected:?}"),"rust":format!("{actual:?}")}));
                                }
                            }
                        }
                    }
                    if first.is_none() && !differences.is_empty() {
                        first = Some(json!({"relative_tick":relative,"global_tick":stage+relative,"differences":differences}));
                    }
                }
                let target = BlockPos::new(155, 10, 124);
                let final_target = json!({"interpreter":format!("{:?}",crop.get_block(target)),"rust":format!("{:?}",projected.get_block(target))});
                let record = json!({"stage":stage,"name":name,"min":min,"max":max,"non_air":initial.len(),"pending":pending.len(),"first":first,"final_target":final_target});
                eprintln!("SLICE {record}");
                if first.is_some() || stage == 9 && name == "small" {
                    std::fs::write(directory.join(format!("pm1-{stage}-{name}.json")),serde_json::to_vec_pretty(&json!({"stage":stage,"start":start,"min":min,"max":max,"blocks":initial,"entities":entities,"pending":pending,"observed":first,"final_target":final_target,"interpreter_trace":trace})).unwrap()).unwrap();
                }
                attempts.push(record);
            }
        }
        if stage == 0 && initial {
            cpus::click_cpu(&mut full, cpu, cpu.start);
        }
        if stage < 9 {
            full.tick_interpreted();
        }
    }
    let detail = crate::redstone::piston::trace::capture_detailed(&actors, || full.tick_interpreted());
    std::fs::write(
        directory.join("full-tick10-piston-trace.json"),
        serde_json::to_vec_pretty(&detail).unwrap(),
    )
    .unwrap();
    assert_eq!(
        full.get_block(BlockPos::new(155, 10, 124)),
        Block::Air {},
        "full interpreter target at tick 10"
    );
    std::fs::write(directory.join("slice-attempts.json"), serde_json::to_vec_pretty(&attempts).unwrap()).unwrap();
}

#[test]
#[ignore = "known PM1 tick-10 interpreter/Rust divergence; standalone saved slice"]
fn pm1_saved_piston_slice_matches_interpreter() {
    use crate::redpilator::{Compiler, CompilerOptions, PublicationMode, TaskMonitor};
    use crate::world::{scheduler::TickScheduler, storage::Chunk};
    use std::sync::Arc;
    let mut fixture = saved_fixture();
    if let Some(path) = std::env::var_os("MCHPRS_PM1_SCHEM_PATH") {
        let clip = crate::plot::worldedit::load_schematic(std::fs::File::open(path).unwrap()).unwrap();
        fixture.blocks.clear();
        for y in 0..clip.size_y {
            for z in 0..clip.size_z {
                for x in 0..clip.size_x {
                    let id = clip.data.get_entry(((y * clip.size_z + z) * clip.size_x + x) as usize);
                    if id != (Block::Air {}).get_id() {
                        fixture.blocks.push((fixture.min + BlockPos::new(x as i32, y as i32, z as i32), id));
                    }
                }
            }
        }
        fixture.entities = clip.block_entities.into_iter().map(|(pos, entity)| (fixture.min + pos, entity)).collect();
    }
    let mut interpreter = PlotWorld::from_chunks(
        0,
        0,
        (0..crate::plot::PLOT_WIDTH)
            .flat_map(|x| (0..crate::plot::PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        TickScheduler::default(),
    );
    interpreter.disable_command_output_limits_for_replay();
    for (pos, id) in fixture.blocks {
        interpreter.set_block(pos, Block::from_id(id));
    }
    for (pos, entity) in fixture.entities {
        interpreter.set_block_entity(pos, entity);
    }
    for entry in &fixture.pending {
        interpreter.restore_scheduled_tick(entry.clone());
    }
    let mut published = interpreter.activation_snapshot();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &published,
            (fixture.min, fixture.max),
            CompilerOptions {
                publication: Some(PublicationMode::All),
                ..Default::default()
            },
            fixture.pending,
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    cpus::click(&mut interpreter, fixture.start - BlockPos::new(8, 8, 8));
    compiler.try_on_use_block(fixture.start).unwrap();
    let payload = BlockPos::new(155, 10, 124);
    let wire = BlockPos::new(156, 10, 124);
    for tick in 1..=10 {
        interpreter.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush(&mut published).unwrap();
        if tick < 10 {
            assert_eq!(interpreter.get_block(payload), Block::RedstoneBlock {});
        } else {
            assert_eq!(interpreter.get_block(payload), Block::Air {}, "saved slice lost original PM1 transition");
            assert!(matches!(interpreter.get_block(wire),Block::RedstoneWire { wire } if wire.power==0));
        }
        for y in fixture.min.y..=fixture.max.y {
            for z in fixture.min.z..=fixture.max.z {
                for x in fixture.min.x..=fixture.max.x {
                    let pos = BlockPos::new(x, y, z);
                    assert_eq!(
                        published.get_block(pos),
                        interpreter.get_block(pos),
                        "saved PM1 slice first differs at tick {tick}, {pos:?}"
                    );
                }
            }
        }
    }
}
