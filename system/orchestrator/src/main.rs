use std::path::PathBuf;

use tokio::sync::mpsc;
use tracing::{error, info};

use byo_orchestrator::config::{Config, ProcessConfig};
use byo_orchestrator::process::{ProcessId, spawn_process};
use byo_orchestrator::router::{Router, RouterMsg};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("byo-orchestrator starting");

    // Parse CLI: `byo-orchestrator [command [args...]]`
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let app_command = if !cli_args.is_empty() {
        Some(cli_args)
    } else {
        None
    };

    // Load config.
    let config_path = find_config();
    let mut config = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("failed to load config: {e}");
            std::process::exit(1);
        }
    };

    // Append CLI app as an additional process.
    if let Some(args) = app_command {
        let command = args[0].clone();
        let extra_args = args[1..].to_vec();

        // Derive a process name from the command (basename without extension).
        // Must be a valid BYO ID: starts with a letter, [a-zA-Z][a-zA-Z0-9_-]*.
        // Uses "app" prefix to avoid collisions with system process names.
        let raw_stem = std::path::Path::new(&command)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("app");
        let name = format!("app-{}", sanitize_id(raw_stem));

        info!("adding CLI app '{name}': {command} {extra_args:?}");
        config.process.push(ProcessConfig {
            name,
            command,
            args: extra_args,
        });
    }

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
/// 4. Compile-time manifest directory (dev builds)
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

    let candidate = PathBuf::from("config/system.toml");
    if candidate.exists() {
        return candidate;
    }

    // Compile-time fallback: relative to the crate's source directory.
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/system.toml");
    if candidate.exists() {
        return candidate;
    }

    PathBuf::from("config/system.toml")
}

/// Sanitize a raw string into valid BYO ID characters (`[a-zA-Z0-9_-]`).
///
/// Keeps only alphanumeric, underscore, and hyphen characters.
/// The caller is responsible for ensuring the final ID starts with a letter
/// (e.g. by prepending a prefix like `"app-"`).
fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}
