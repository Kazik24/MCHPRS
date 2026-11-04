use super::program::{PISTON_HEAD_NOTIFICATION, notification_component};
use crate::{
    plot::worldedit::{load_schematic, paste_clipboard},
    redpiler::{Compiler, CompilerOptions, PublicationMode},
    world::World,
};
use mchprs_blocks::{BlockPos, blocks::Block};
use serde_json::json;

#[test]
fn saved_minimal_bud_piston_receives_unlock_update() {
    minimal_bud_piston_receives_unlock_update(false, false);
}

#[test]
fn llvm_saved_minimal_bud_piston_receives_unlock_update() {
    minimal_bud_piston_receives_unlock_update(true, false);
}

#[test]
fn minimal_bud_piston_unlocks_with_lower_lever_still_high() {
    minimal_bud_piston_receives_unlock_update(false, true);
}

#[test]
fn llvm_minimal_bud_piston_unlocks_with_lower_lever_still_high() {
    minimal_bud_piston_receives_unlock_update(true, true);
}

fn minimal_bud_piston_receives_unlock_update(llvm: bool, lower_held: bool) {
    let origin = BlockPos::new(16, 16, 16);
    let mut clip =
        load_schematic(std::fs::File::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/minimal_repo.schem")).unwrap())
            .unwrap();
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let min = origin - BlockPos::new(2, 2, 2);
    let max = origin + BlockPos::new(clip.size_x as i32 + 1, clip.size_y as i32 + 1, clip.size_z as i32 + 1);
    let top = origin + BlockPos::new(0, 2, 2);
    let bottom = origin + BlockPos::new(0, 1, 4);
    let piston = origin + BlockPos::new(2, 1, 3);
    let mut native = super::tests::host();
    paste_clipboard(&mut native, &clip, origin, false);
    let mut compiled = native.activation_snapshot();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &compiled,
            (min, max),
            CompilerOptions {
                llvm,
                publication: Some(PublicationMode::All),
                ..Default::default()
            },
            vec![],
            Default::default(),
        )
        .unwrap();
    let mut rows = vec![];
    let mut sequence = vec![("top_on", top), ("bottom_on", bottom)];
    if !lower_held {
        sequence.push(("bottom_off", bottom));
    }
    sequence.push(("top_off", top));
    for (stage, lever) in sequence {
        let events = crate::redstone::instant_piston_tests::capture_all(|| {
            crate::interaction::use_redstone(native.get_block(lever), &mut native, lever, None);
        });
        let compiled_events = super::pm1_causal::trace(|| compiler.try_on_use_block(lever).unwrap());
        let mut differences = vec![];
        for tick in 0..=6 {
            compiler.try_flush(&mut compiled).unwrap();
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    for x in min.x..=max.x {
                        let pos = BlockPos::new(x, y, z);
                        let expected = native.get_block(pos);
                        let actual = compiled.get_block(pos);
                        if expected != actual {
                            differences.push(json!({"tick":tick,"pos":pos,"native":format!("{expected:?}"),"compiled":format!("{actual:?}")}));
                        }
                    }
                }
            }
            if tick != 6 {
                native.tick_interpreted();
                compiler.tick().unwrap();
            }
        }
        let watched = compiler.runtime.as_ref().map(|runtime|runtime.program.watched_wires.iter().map(|wire| json!({"wire":runtime.program.positions[wire.wire],"watchers":wire.watchers.iter().map(|&route|json!({"pos":runtime.program.positions[notification_component(route)],"via_head":route & PISTON_HEAD_NOTIFICATION != 0})).collect::<Vec<_>>()})).collect::<Vec<_>>());
        let compiled_input = compiler
            .runtime
            .as_ref()
            .map(|runtime| runtime.input_strength(runtime.program.component_indices[&piston]));
        eprintln!(
            "{stage}: native={:?}, compiled={:?}, differences={}",
            native.get_block(piston),
            compiled.get_block(piston),
            differences.len()
        );
        rows.push(json!({"stage":stage,"native_events":events,"compiled_events":compiled_events,"differences":differences,"watched":watched,"compiled_piston_input":compiled_input,"native_pending":native.scheduler().iter_entries().collect::<Vec<_>>()}));
        if stage == "top_on" {
            assert!(matches!(native.get_block(piston),Block::Piston {piston} if !piston.extended));
        }
        if stage == "bottom_off" {
            assert!(matches!(native.get_block(piston),Block::Piston {piston} if piston.extended));
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/minimal-bud-piston-repro-{}{}.json",
        if llvm { "llvm" } else { "rust" },
        if lower_held { "-lower-held" } else { "" }
    ));
    std::fs::write(path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    assert!(
        matches!(native.get_block(piston),Block::Piston {piston} if !piston.extended),
        "native must update and release the quasi-powered piston"
    );
    if lower_held {
        assert!(matches!(compiled.get_block(bottom), Block::Lever { lever } if lever.powered));
    }
    assert_eq!(
        compiled.get_block(piston),
        native.get_block(piston),
        "unlock must physically update the piston"
    );
    assert!(
        rows.iter().all(|row| row["differences"].as_array().unwrap().is_empty()),
        "complete block states must match each phase"
    );
}

#[test]
fn saved_bigger_bud_pistons_receive_unlock_update() {
    bigger_bud_pistons_receive_unlock_update(false);
}

#[test]
fn llvm_saved_bigger_bud_pistons_receive_unlock_update() {
    bigger_bud_pistons_receive_unlock_update(true);
}

fn bigger_bud_pistons_receive_unlock_update(llvm: bool) {
    let origin = BlockPos::new(16, 16, 16);
    let mut clip =
        load_schematic(std::fs::File::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/bigger_repro.schem")).unwrap())
            .unwrap();
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let min = origin - BlockPos::new(2, 2, 2);
    let max = origin + BlockPos::new(clip.size_x as i32 + 1, clip.size_y as i32 + 1, clip.size_z as i32 + 1);
    let mut rows = vec![];
    for (name, control) in [
        ("main_off", BlockPos::new(2, 3, 6)),
        ("middle_off", BlockPos::new(8, 4, 7)),
        ("top_off", BlockPos::new(7, 6, 3)),
    ] {
        let mut native = super::tests::host();
        paste_clipboard(&mut native, &clip, origin, false);
        let mut compiled = native.activation_snapshot();
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &compiled,
                (min, max),
                CompilerOptions {
                    llvm,
                    publication: Some(PublicationMode::All),
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap();
        let control = origin + control;
        let mut differences = vec![];
        let events = crate::redstone::instant_piston_tests::capture_all(|| {
            crate::interaction::use_redstone(native.get_block(control), &mut native, control, None);
            compiler.try_on_use_block(control).unwrap();
            for tick in 0..=12 {
                compiler.try_flush(&mut compiled).unwrap();
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        for x in min.x..=max.x {
                            let pos = BlockPos::new(x, y, z);
                            let expected = native.get_block(pos);
                            let actual = compiled.get_block(pos);
                            if expected != actual {
                                differences.push(json!({"tick":tick,"pos":pos,"native":format!("{expected:?}"),"compiled":format!("{actual:?}")}));
                            }
                        }
                    }
                }
                if tick != 12 {
                    native.tick_interpreted();
                    compiler.tick().unwrap();
                }
            }
        });
        eprintln!("bigger {name}: differences={}, first={:?}", differences.len(), differences.first());
        rows.push(json!({"control":name,"native_events":events,"differences":differences}));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/bigger-bud-piston-repro-{}.json",
        if llvm { "llvm" } else { "rust" }
    ));
    std::fs::write(path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    assert!(
        rows.iter().all(|row| row["differences"].as_array().unwrap().is_empty()),
        "bigger reproduction diverges; see captured phases"
    );
}
