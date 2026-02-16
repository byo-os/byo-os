mod batch;
mod config;
mod id;
mod process;
mod router;
mod state;

use std::path::PathBuf;

use tokio::sync::mpsc;
use tracing::{error, info};

use config::Config;
use process::{ProcessId, spawn_process};
use router::{Router, RouterMsg};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("byo-orchestrator starting");

    // Load config.
    let config_path = find_config();
    let config = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("failed to load config: {e}");
            std::process::exit(1);
        }
    };

    info!("loaded config with {} processes", config.process.len());

    // Central router channel — all process reader tasks send here.
    let (router_tx, mut router_rx) = mpsc::channel::<RouterMsg>(1024);

    // Spawn all configured processes and register with the router.
    let mut router = Router::new();
    let mut next_id = 1u32;

    for proc_config in &config.process {
        let pid = ProcessId(next_id);
        next_id += 1;

        info!(
            "spawning process '{}': {} {:?}",
            proc_config.name, proc_config.command, proc_config.args
        );

        match spawn_process(
            pid,
            proc_config.name.clone(),
            &proc_config.command,
            &proc_config.args,
            router_tx.clone(),
        ) {
            Ok(process) => {
                router.add_process(process);
                info!("started process '{}' (id={})", proc_config.name, pid.0);
            }
            Err(e) => {
                error!("failed to spawn '{}': {e}", proc_config.name);
            }
        }
    }

    // Drop our copy of the sender — the router loop only needs the receiver.
    // Reader tasks hold their own clones.
    drop(router_tx);

    // Main router loop.
    info!("entering router loop");
    while let Some(msg) = router_rx.recv().await {
        router.handle(msg).await;

        // If all processes have disconnected, exit.
        if !router.has_processes() {
            info!("all processes disconnected, shutting down");
            break;
        }
    }

    info!("byo-orchestrator exiting");
}

/// Find the config file path. Checks:
/// 1. `BYO_CONFIG` environment variable
/// 2. `config/system.toml` relative to the executable
/// 3. `config/system.toml` relative to the current directory
fn find_config() -> PathBuf {
    if let Ok(path) = std::env::var("BYO_CONFIG") {
        return PathBuf::from(path);
    }

    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));
        let candidate = exe_dir.join("config/system.toml");
        if candidate.exists() {
            return candidate;
        }
    }

    PathBuf::from("config/system.toml")
}
