use super::*;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{BlockPos, blocks::Block};
use serde_json::json;

fn recorder(ticks: u64) -> Recorder {
    let mut recorder = Recorder::default();
    recorder.replace(vec![Probe {
        name: "clock".into(),
        pos: BlockPos::new(1, 64, 1),
    }]);
    recorder.on(ticks, 0, vec![Some(false)], 64).unwrap();
    recorder
}
fn wire(power: u8) -> Block {
    let mut block = Block::from_name("redstone_wire").unwrap();
    if let Block::RedstoneWire { wire } = &mut block {
        wire.power = power;
    }
    block
}
fn page(recorder: &Recorder, cursor: Option<api::Cursor>, limit: usize) -> serde_json::Value {
    recorder
        .page(
            (3, -2),
            1,
            &api::Query {
                cursor,
                limit,
                metadata: false,
            },
            false,
        )
        .unwrap()
}
#[test]
fn binary_glitches_text_and_quiet_rollover_are_ordered() {
    let mut recorder = recorder(3);
    let pos = recorder.probes[0].pos;
    recorder.tick(1, true);
    recorder.changed(pos, Some(true), 1);
    recorder.changed(pos, Some(true), 1);
    recorder.text(pos, r#"{"text":"Hello ","color":"red","extra":[{"text":"world"}]}"#, 1);
    recorder.changed(pos, Some(false), 1);
    recorder.tick(1, false);
    let initial = page(&recorder, None, 4096);
    assert_eq!(
        initial["ticks"][0][1],
        json!([[1, 0, 0, 1], [2, 1, 1, 64, 1, "Hello world", 0], [3, 0, 0, 0]])
    );
    let cursor = api::Cursor::parse(initial["cursor"].as_str().unwrap()).unwrap();
    recorder.tick(10, false);
    let tail = page(&recorder, Some(cursor), 4096);
    assert_eq!(tail["ticks"], json!([]));
    assert_eq!(tail["window"]["first_tick"], "8");
    assert_eq!(page(&recorder, None, 4096)["snapshot"]["baseline"], json!([0]));
}
#[test]
fn pagination_and_actual_history_loss_require_resnapshot() {
    let mut recorder = recorder(3);
    let pos = recorder.probes[0].pos;
    for tick in 1..=3 {
        recorder.tick(tick, false);
        recorder.changed(pos, Some(tick % 2 == 1), tick);
    }
    let first = page(&recorder, None, 1);
    assert_eq!(first["more"], true);
    let cursor = api::Cursor::parse(first["cursor"].as_str().unwrap()).unwrap();
    let second = page(&recorder, Some(cursor.clone()), 1);
    assert_eq!(second["ticks"][0][1][0], json!([1, 0, 0, 0]));
    recorder.tick(10, false);
    assert_eq!(
        recorder
            .page(
                (3, -2),
                1,
                &api::Query {
                    cursor: Some(cursor),
                    limit: 1,
                    metadata: false
                },
                false
            )
            .unwrap_err()
            .code,
        "history_expired"
    );
}
#[test]
fn raw_world_writes_and_command_output_use_authoritative_hooks() {
    let mut world = crate::plot::PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let pos = BlockPos::new(1, 64, 1);
    world.set_block(pos, wire(0));
    world.enable_waveform_for_test(vec![Probe { name: "clock".into(), pos }], 10).unwrap();
    world.set_block(pos, wire(15));
    world.set_block(pos, wire(7));
    world
        .execute_command_block_at(pos, "tellraw @a {\"text\":\"ready\",\"color\":\"red\"}", "@")
        .unwrap();
    assert!(world.execute_command_block_at(pos, "stop", "@").is_err());
    world.set_block(pos, Block::Air {});
    let data = world.waveform_page_for_test();
    assert_eq!(data["ticks"][0][1], json!([[1, 0, 0, 1], [2, 1, 1, 64, 1, "ready", 0], [3, 0, 0, null]]));
}

#[test]
fn powered_components_record_their_own_state() {
    let mut world = crate::plot::PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let pos = BlockPos::new(8, 64, 8);
    world.set_block(pos, Block::from_name("lever").unwrap());
    world
        .enable_waveform_for_test(
            vec![Probe {
                name: "component".into(),
                pos,
            }],
            10,
        )
        .unwrap();
    let cases = [
        ("lever", "powered"),
        ("stone_button", "powered"),
        ("oak_button", "powered"),
        ("repeater", "powered"),
        ("observer", "powered"),
        ("stone_pressure_plate", "powered"),
        ("redstone_torch", "lit"),
        ("redstone_lamp", "lit"),
    ];
    for (name, property) in cases {
        let mut off = Block::from_name(name).unwrap();
        off.set_properties(std::collections::HashMap::from([(property, "false")]));
        let mut on = off;
        on.set_properties(std::collections::HashMap::from([(property, "true")]));
        world.set_block(pos, off);
        assert_eq!(signal_value(&world, pos), Some(false), "{name}");
        world.set_block(pos, on);
        assert_eq!(signal_value(&world, pos), Some(true), "{name}");
        world.set_block(pos, off);
    }
    let data = world.waveform_page_for_test();
    let records = data["ticks"][0][1].as_array().unwrap();
    assert_eq!(records.len(), cases.len() * 2);
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record[3], u8::from(index % 2 == 0));
    }
}

#[test]
fn ordinary_blocks_follow_direct_and_indirect_power_without_block_state_changes() {
    let mut world = crate::plot::PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let control = BlockPos::new(7, 64, 8);
    let direct = BlockPos::new(8, 64, 8);
    let indirect = BlockPos::new(9, 64, 8);
    let mut off = Block::from_name("lever").unwrap();
    off.set_properties(std::collections::HashMap::from([
        ("face", "wall"),
        ("facing", "west"),
        ("powered", "false"),
    ]));
    let mut on = off;
    on.set_properties(std::collections::HashMap::from([("powered", "true")]));
    world.set_block(control, off);
    world.set_block(direct, Block::Stone {});
    world.set_block(indirect, Block::Stone {});
    world
        .enable_waveform_for_test(
            vec![
                Probe {
                    name: "direct".into(),
                    pos: direct,
                },
                Probe {
                    name: "indirect".into(),
                    pos: indirect,
                },
            ],
            10,
        )
        .unwrap();
    world.set_block(control, on);
    assert_eq!(signal_value(&world, direct), Some(true));
    assert_eq!(signal_value(&world, indirect), Some(true));
    world.set_block(control, off);
    let data = world.waveform_page_for_test();
    assert_eq!(data["ticks"][0][1], json!([[1, 0, 0, 1], [2, 0, 1, 1], [3, 0, 0, 0], [4, 0, 1, 0]]));
    assert_eq!(world.get_block(direct), Block::Stone {});
    assert_eq!(world.get_block(indirect), Block::Stone {});
}

#[test]
fn comparator_entity_changes_are_recorded_even_when_block_state_does_not_change() {
    use mchprs_blocks::block_entities::BlockEntity;
    let mut world = crate::plot::PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let pos = BlockPos::new(8, 64, 8);
    world.set_block(pos, Block::from_name("comparator").unwrap());
    world.enable_waveform_for_test(vec![Probe { name: "compare".into(), pos }], 10).unwrap();
    for strength in [15, 7] {
        world.set_block_entity(pos, BlockEntity::Comparator { output_strength: strength });
    }
    world.delete_block_entity(pos);
    assert_eq!(world.waveform_page_for_test()["ticks"][0][1], json!([[1, 0, 0, 1], [2, 0, 0, 0]]));
}
#[test]
fn overflow_never_changes_chat_success_and_off_releases_capture() {
    let mut recorder = recorder(1000);
    for _ in 0..100 {
        recorder.text(BlockPos::new(2, 3, 4), &json!({"text":"x".repeat(TEXT_BYTES)}).to_string(), 0);
    }
    assert!(!recorder.enabled());
    assert_eq!(page(&recorder, None, 1)["state"], "overflow");
    recorder.off();
    assert!(recorder.capture.is_none());
    assert_eq!(recorder.probes.len(), 1);
}

#[test]
fn admission_failure_preserves_capture_and_u64_ticks_are_exact() {
    let mut recorder = recorder(1000);
    let id = recorder.capture.as_ref().unwrap().id;
    assert!(recorder.on(1001, 0, vec![Some(false)], 0).is_err());
    assert_eq!(recorder.capture.as_ref().unwrap().id, id);
    let tick = (1u64 << 53) + 17;
    recorder.tick(tick, true);
    recorder.changed(recorder.probes[0].pos, Some(true), tick);
    let data = page(&recorder, None, 4096);
    assert_eq!(data["ticks"][0][0], tick.to_string());
    assert_eq!(data["window"]["completed_tick"], (tick - 1).to_string());
    recorder.discontinuity(tick, true, vec![Some(true)]);
    assert_ne!(recorder.capture.as_ref().unwrap().id, id);
    assert!(recorder.capture.as_ref().unwrap().records.is_empty());
    assert_eq!(page(&recorder, None, 4096)["snapshot"]["baseline"], json!([1]));
}

#[test]
fn escaped_text_pages_are_bounded_and_unicode_clipping_is_valid() {
    let mut recorder = recorder(1000);
    recorder.text(BlockPos::new(1, 2, 3), &json!({"text":"\u{1}".repeat(TEXT_BYTES)}).to_string(), 0);
    let first = page(&recorder, None, 4096);
    assert_eq!(first["more"], true);
    let second = page(&recorder, Some(api::Cursor::parse(first["cursor"].as_str().unwrap()).unwrap()), 4096);
    assert_eq!(second["more"], false);
    assert!(serde_json::to_vec(&second).unwrap().len() <= PAGE_BYTES);
    recorder.text(BlockPos::new(1, 2, 3), &json!({"text":"🦄".repeat(TEXT_BYTES)}).to_string(), 0);
    let record = recorder.capture.as_ref().unwrap().records.back().unwrap();
    let super::recorder::Kind::Text(_, text, original) = &record.kind else {
        panic!()
    };
    assert_eq!(*original, TEXT_BYTES * 4);
    assert_eq!(text.len(), TEXT_BYTES);
}
