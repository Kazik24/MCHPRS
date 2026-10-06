//! Behavioral coverage for the separately versioned lever/consumer fixtures.
use super::*;

fn io_pack() -> PathBuf {
    root().join("test_data/instant-pistons-io")
}

#[test]
fn strict_io_imports_decode_and_preserve_their_saved_ports() {
    for m in manifests_at(&io_pack()) {
        let (w, _, d) = load(&m, 0);
        assert!(quiet(&w));
        assert!(!observe(&w, &m, d, 0).as_object().unwrap().is_empty());
    }
}
