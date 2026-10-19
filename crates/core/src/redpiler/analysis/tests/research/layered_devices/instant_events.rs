//! Acceptance ladder for shared-cell composition; wire work remains reported explicitly.
use super::*;

#[test]
#[ignore = "explicit one-kernel COUNTER plus ordinary-memory boundary histories"]
fn counter_compiles_one_event_kernel_and_matches_native() {
    let path = root().join("test_data/instant-pistons-io/fixtures/counter_basic.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    check_manifest_device_mode("counter_event_kernel", &fixture, 1, true);
}

#[test]
#[ignore = "explicit one-kernel Divider and all pinned native histories"]
fn divider_compiles_one_event_kernel_and_matches_native() {
    check_manifest_device_mode("divider_event_kernel", &manifest("fpu_divider"), 8, true);
}
