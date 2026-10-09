use std::process::Command;

#[test]
fn production_pool_defaults_to_four_and_respects_overrides() {
    const EXPECTED_WORKERS: &str = "MCHPRS_RAYON_TEST_EXPECTED_WORKERS";
    if let Ok(expected) = std::env::var(EXPECTED_WORKERS) {
        super::init_inference_pool();
        assert_eq!(
            rayon::current_num_threads(),
            expected.parse::<usize>().unwrap()
        );
        return;
    }

    // Global pools cannot be reconfigured; each setting needs a fresh process.
    for (setting, expected) in [
        (None, 4),
        (Some("1"), 1),
        (Some("2"), 2),
        (Some("invalid"), 4),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "server::inference_pool_tests::production_pool_defaults_to_four_and_respects_overrides",
                "--test-threads=1",
            ])
            .env(EXPECTED_WORKERS, expected.to_string())
            .env_remove("RAYON_NUM_THREADS");
        if let Some(setting) = setting {
            child.env("RAYON_NUM_THREADS", setting);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "setting={setting:?}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
