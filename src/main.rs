mod azure;
mod config;
mod conversation_store;
mod logging;
mod mcp_runtime;
mod oauth;
mod orchestrator;
mod tool_evidence;
mod types;
mod ui;

use anyhow::{Context, Result};
use config::AppConfig;
use orchestrator::Orchestrator;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::unbounded_channel;
use tracing::{debug, error, info};
use types::UiEvent;
use ui::App;

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = logging::init_logging()?;
    let options = CliOptions::parse();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    info!(
        pid = std::process::id(),
        cwd = %current_dir.display(),
        config_path = %options.config_path.display(),
        config_path_source = options.config_path_source,
        "starting rusty-bidule"
    );
    let config = AppConfig::load(&options.config_path).with_context(|| {
        format!(
            "failed to load config from {}",
            options.config_path.display()
        )
    });
    let config = match config {
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
    if let Some(message) = options.once_message {
        info!("running in one-shot mode");
        run_once(orchestrator, options.conversation_id, message).await
    } else {
        info!("launching interactive TUI");
        let app = App::new(orchestrator).await?;
        app.run().await
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
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
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
                _ => {}
            }
        }
        options
    }
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
