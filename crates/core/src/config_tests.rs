use super::*;

#[test]
fn migrates_old_chat_switch_without_overriding_explicit_native_mode() {
    for (index, (text, expected)) in [
        ("signed_velocity = true\nproxy_chat = true\n", false),
        ("signed_velocity = false\n", true),
        ("signed_velocity = true\nnative_chat = true\n", true),
        ("signed_velocity = false\nnative_chat = false\n", false),
    ]
    .into_iter()
    .enumerate()
    {
        let path = std::env::temp_dir().join(format!("mchprs-chat-migration-{}-{index}.toml", std::process::id()));
        fs::write(&path, text).unwrap();
        assert_eq!(ServerConfig::load(&path).native_chat, expected);
        let patched = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(!patched.contains("signed_velocity"));
        assert!(!patched.contains("proxy_chat"));
    }
}

#[test]
fn reads_and_patches_the_selected_config_file() {
    let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("mchprs-config-{}-{unique}.toml", std::process::id()));
    fs::write(&path, "motd = \"Selected container config\"\nfast_render_send_rate = 10\n").unwrap();
    let config = ServerConfig::load(&path);
    let patched = fs::read_to_string(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert_eq!(config.motd, "Selected container config");
    assert_eq!(config.fast_render_send_rate, 10);
    assert!(config.native_chat);
    assert_eq!(config.neighbor_update_interval_ms, 2000);
    assert_eq!(config.git_work_memory_mib, 1024);
    assert_eq!(config.git_default_plot_storage_mib, 100);
    assert_eq!(config.worldedit_max_blocks, 67_108_864);
    assert_eq!(config.worldedit_history_blocks, 134_217_728);
    assert_eq!(config.schematic_save_prefix, "");
    assert!(patched.contains("schematic_save_prefix = \"\""));
    assert!(patched.contains("neighbor_update_interval_ms"));
    assert!(patched.contains("rhistory_memory_limit_mib"));
}

#[test]
fn preserves_configured_schematic_save_prefix() {
    let path = std::env::temp_dir().join(format!("mchprs-schematic-prefix-{}.toml", std::process::id()));
    for prefix in ["./mroww/", "aaa@", "./mroww/aaa@"] {
        fs::write(&path, format!("schematic_save_prefix = {prefix:?}\n")).unwrap();
        assert_eq!(ServerConfig::load(&path).schematic_save_prefix, prefix);
        assert_eq!(ServerConfig::load(&path).schematic_save_prefix, prefix);
    }
    fs::remove_file(path).unwrap();
}
