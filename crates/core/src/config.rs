use crate::permissions::PermissionsConfig;
use crate::velocity::VelocityConfig;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use toml_edit::{value, Document};

static CONFIG_PATH: Lazy<std::path::PathBuf> = Lazy::new(|| {
    std::env::var_os("MCHPRS_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "Config.toml".into())
});

pub static CONFIG: Lazy<ServerConfig> = Lazy::new(|| ServerConfig::load(&CONFIG_PATH));

pub(crate) fn save_history_limit(mib: i64) -> Result<(), String> {
    let text = fs::read_to_string(&*CONFIG_PATH).map_err(|e| e.to_string())?;
    let mut doc = text.parse::<Document>().map_err(|e| e.to_string())?;
    doc["rhistory_memory_limit_mib"] = value(mib);
    mchprs_save_data::atomic::write(&CONFIG_PATH, doc.to_string().as_bytes())
        .map_err(|e| format!("Cannot save history limit: {e}"))
}

trait ConfigSerializeDefault {
    fn fix_config(self, name: &str, doc: &mut Document);
}

macro_rules! impl_simple_default {
    ( $( $type:ty ),* ) => {
        $(
            impl ConfigSerializeDefault for $type {
                fn fix_config(self, name: &str, doc: &mut Document) {
                    doc.entry(name).or_insert_with(|| value(self));
                }
            }
        )*
    }
}

impl_simple_default!(String, i64, bool);

impl ConfigSerializeDefault for u32 {
    fn fix_config(self, name: &str, doc: &mut Document) {
        doc.entry(name).or_insert_with(|| value(i64::from(self)));
    }
}
impl ConfigSerializeDefault for u64 {
    fn fix_config(self, name: &str, doc: &mut Document) {
        doc.entry(name).or_insert_with(|| {
            value(i64::try_from(self).expect("Config default exceeds TOML integer range"))
        });
    }
}

impl<T> ConfigSerializeDefault for Option<T> {
    fn fix_config(self, _: &str, _: &mut Document) {
        assert!(matches!(self, None), "`Some` as default is unimplemented");
    }
}

macro_rules! gen_config {
    (
        $( $name:ident: $type:ty = $default:expr),*
    ) => {
        #[derive(Serialize, Deserialize)]
        pub struct ServerConfig {
            $(
                pub $name: $type,
            )*
        }

        impl ServerConfig {
            fn load(config_file: &std::path::Path) -> ServerConfig {
                let str = fs::read_to_string(config_file).unwrap_or_default();
                let mut doc = str.parse::<Document>().unwrap();

                $(
                    <$type as ConfigSerializeDefault>::fix_config($default, stringify!($name), &mut doc);
                )*

                let patched = doc.to_string();
                if str != patched {
                    let mut file = fs::OpenOptions::new().create(true).truncate(true).write(true).open(config_file).unwrap();
                    write!(file, "{}", patched).unwrap();
                }

                toml::from_str(&patched).unwrap()
            }
        }
    };
}

gen_config! {
    bind_address: String = "0.0.0.0:25565".to_string(),
    motd: String = "§4§lmroww.redstoneFUN.pl §r§71.21.5\n§cMinecraft Redstone o Wysokiej Wydajności".to_string(),
    chat_format: String = "<{username}> {message}".to_string(),
    max_players: i64 = 99999,
    view_distance: i64 = 8,
    bungeecord: bool = false,
    velocity: Option<VelocityConfig> = None,
    whitelist: bool = false,
    schemati: bool = false,
    luckperms: Option<PermissionsConfig> = None,
    block_in_hitbox: bool = true,
    auto_redpiler: bool = false,
    default_tps: u32 = 20,
    fast_render_threshold: i64 = 200,
    fast_render_send_rate: i64 = 10,
    rhistory_memory_limit_mib: i64 = 2048,
    rhistory_work_memory_limit_mib: i64 = 256,
    max_command_ticks: u32 = 10_000,
    command_work_time_ms: u64 = 250,
    worldedit_max_blocks: u64 = 4_194_304,
    worldedit_history_blocks: u64 = 8_388_608
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_patches_the_selected_config_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mchprs-config-{}-{unique}.toml",
            std::process::id()
        ));
        fs::write(
            &path,
            "motd = \"Selected container config\"\nfast_render_send_rate = 10\n",
        )
        .unwrap();
        let config = ServerConfig::load(&path);
        let patched = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(config.motd, "Selected container config");
        assert_eq!(config.fast_render_send_rate, 10);
        assert!(patched.contains("rhistory_memory_limit_mib"));
    }
}
