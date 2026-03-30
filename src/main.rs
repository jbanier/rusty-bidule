mod azure;
mod config;
mod conversation_store;
mod local_tools;
mod logging;
mod mcp_runtime;
mod oauth;
mod orchestrator;
mod recipes;
mod skills;
mod tool_evidence;
mod types;
mod ui;
mod web;

use anyhow::Result;
use config::AppConfig;
use orchestrator::Orchestrator;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::unbounded_channel;
use tracing::{debug, error, info};
use types::UiEvent;
use ui::App;

#[tokio::main]
async fn main() -> Result<()> {
    let options = CliOptions::parse();
    // We need config before we can pass tracing config to init_logging.
    // Load config first (no logging yet — errors go to stderr).
    let config_result = AppConfig::load(&options.config_path);
    let tracing_config = config_result.as_ref().ok().and_then(|c| c.tracing.clone());
    let _log_guard = logging::init_logging(tracing_config.as_ref())?;

    let current_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    info!(
        pid = std::process::id(),
        cwd = %current_dir.display(),
        config_path = %options.config_path.display(),
        config_path_source = options.config_path_source,
        "starting rusty-bidule"
    );
    let config = match config_result {
        Ok(config) => config,
        Err(err) => {
            error!(
                config_path = %options.config_path.display(),
                config_path_source = options.config_path_source,
                error = %err,
                "failed to load application config"
            );
            return Err(err);
        }
    };
    debug!("application config loaded successfully");
    let orchestrator = Orchestrator::new(config)?;

    match options.interface.as_str() {
        "web" => {
            info!("launching web interface");
            let recipes = orchestrator.recipes().clone();
            web::run_web_server(orchestrator, recipes, &options.host, options.port).await
        }
        _ => {
            if let Some(message) = options.once_message {
                info!("running in one-shot mode");
                run_once(orchestrator, options.conversation_id, message).await
            } else {
                info!("launching interactive TUI");
                let app = App::new(orchestrator).await?;
                app.run().await
            }
        }
    }
}

async fn run_once(
    orchestrator: Orchestrator,
    conversation_id: Option<String>,
    message: String,
) -> Result<()> {
    let conversation_id = match conversation_id {
        Some(value) => value,
        None => orchestrator.ensure_default_conversation().await?,
    };
    info!(%conversation_id, "executing one-shot conversation turn");
    let (ui_tx, mut ui_rx) = unbounded_channel();
    let run = orchestrator.run_turn(&conversation_id, message, ui_tx);
    tokio::pin!(run);

    loop {
        tokio::select! {
            result = &mut run => {
                let result = result?;
                println!("{}", result.reply);
                return Ok(());
            }
            Some(event) = ui_rx.recv() => {
                if let UiEvent::Progress(progress) = event {
                    eprintln!("{}: {}", progress.kind, progress.message);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CliOptions {
    config_path: std::path::PathBuf,
    config_path_source: &'static str,
    once_message: Option<String>,
    conversation_id: Option<String>,
    interface: String,
    host: String,
    port: u16,
}

impl CliOptions {
    fn parse() -> Self {
        let env_config_path = std::env::var("RUSTY_BIDULE_CONFIG").ok();
        let mut options = Self {
            config_path: env_config_path
                .as_deref()
                .map(Into::into)
                .unwrap_or_else(default_config_path),
            config_path_source: if env_config_path.is_some() {
                "env"
            } else {
                "default-search"
            },
            once_message: None,
            conversation_id: None,
            interface: "tui".to_string(),
            host: "127.0.0.1".to_string(),
            port: 8080,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "--config" => {
                    if let Some(path) = args.next() {
                        options.config_path = path.into();
                        options.config_path_source = "cli";
                    }
                }
                "--once" => {
                    options.once_message = args.next();
                }
                "--conversation" => {
                    options.conversation_id = args.next();
                }
                "--interface" => {
                    if let Some(iface) = args.next() {
                        options.interface = iface;
                    }
                }
                "--host" => {
                    if let Some(host) = args.next() {
                        options.host = host;
                    }
                }
                "--port" => {
                    if let Some(port_str) = args.next()
                        && let Ok(port) = port_str.parse()
                    {
                        options.port = port;
                    }
                }
                other => {
                    eprintln!("Unknown option: {other}");
                    eprintln!("Run with --help for usage.");
                    std::process::exit(1);
                }
            }
        }
        options
    }
}

fn print_help() {
    println!(
        "\
rusty-bidule — AI assistant for CSIRT investigators

USAGE:
    rusty-bidule [OPTIONS]

OPTIONS:
    -h, --help
            Print this help message and exit.

    --config <PATH>
            Path to the YAML configuration file.
            Default: config/config.local.yaml (searched upward from cwd).
            Can also be set via the RUSTY_BIDULE_CONFIG environment variable.

    --interface <tui|web>
            Select the user interface.
              tui  — interactive terminal UI (default)
              web  — HTTP server with REST API and browser UI

    --host <HOST>
            Bind address for the web interface. [default: 127.0.0.1]
            Only used when --interface web is set.

    --port <PORT>
            TCP port for the web interface. [default: 8080]
            Only used when --interface web is set.

    --once <MESSAGE>
            Run a single agent turn with MESSAGE, print the reply to stdout,
            and exit. No interactive UI is launched.

    --conversation <ID>
            Conversation ID to use with --once.
            If omitted, the most recent conversation is used (or a new one
            is created).

ENVIRONMENT:
    RUSTY_BIDULE_CONFIG
            Overrides the default config file path (same as --config).

EXAMPLES:
    # Launch the interactive TUI
    rusty-bidule

    # Launch the web interface on all interfaces, port 9000
    rusty-bidule --interface web --host 0.0.0.0 --port 9000

    # Run a one-shot query and exit
    rusty-bidule --once \"Scan host 10.0.0.1\"

    # Use a custom config file
    rusty-bidule --config /etc/csirt/bidule.yaml
"
    );
}

fn default_config_path() -> PathBuf {
    discover_project_root()
        .map(|root| root.join("config").join("config.local.yaml"))
        .unwrap_or_else(|| PathBuf::from("config/config.local.yaml"))
}

fn discover_project_root() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok()?;
    current_dir
        .ancestors()
        .find(|candidate| looks_like_project_root(candidate))
        .map(Path::to_path_buf)
}

fn looks_like_project_root(candidate: &Path) -> bool {
    candidate.join("Cargo.toml").is_file() && candidate.join("src").is_dir()
}
