use mchprs_core::server::MinecraftServer;
use std::fs;
use std::path::Path;
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

mod logging;

fn main() {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--version" || arg == "-v")
    {
        println!("{}", mchprs_core::server::version_string());
        return;
    }
    // Setup logging
    fs::create_dir_all("./logs").expect("Cannot create log directory");
    let logfile = logging::Log::open(Path::new("./logs/mchprs.log"), 16 * 1024 * 1024)
        .expect("Cannot open server log");
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .with_env_var("MCHPRS_LOG")
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_writer(logfile.and(std::io::stdout))
        .with_thread_names(true)
        .with_env_filter(env_filter)
        .init();

    // Move old log file into logs folder
    let old_log_path = Path::new("./output.log");
    if old_log_path.exists() {
        let dest_path = "./logs/old_output.log";
        fs::rename(old_log_path, "./logs/old_output.log").unwrap();
        debug!(
            "Moving old log file from {old_log_path} to {dest_path}",
            old_log_path = old_log_path.display()
        );
    }

    MinecraftServer::run();
}
