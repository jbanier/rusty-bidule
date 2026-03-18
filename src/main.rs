mod azure;
mod config;
mod conversation_store;
mod mcp_runtime;
mod oauth;
mod orchestrator;
mod tool_evidence;
mod types;
mod ui;

use anyhow::{Context, Result};
use config::AppConfig;
use orchestrator::Orchestrator;
use tokio::sync::mpsc::unbounded_channel;
use types::UiEvent;
use ui::App;

#[tokio::main]
async fn main() -> Result<()> {
    let options = CliOptions::parse();
    let config = AppConfig::load(&options.config_path).with_context(|| {
        format!(
            "failed to load config from {}",
            options.config_path.display()
        )
    })?;
    let orchestrator = Orchestrator::new(config)?;
    if let Some(message) = options.once_message {
        run_once(orchestrator, options.conversation_id, message).await
    } else {
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
    once_message: Option<String>,
    conversation_id: Option<String>,
}

impl CliOptions {
    fn parse() -> Self {
        let mut options = Self {
            config_path: std::env::var("RUSTY_BIDULE_CONFIG")
                .map(Into::into)
                .unwrap_or_else(|_| std::path::PathBuf::from("config/config.local.yaml")),
            once_message: None,
            conversation_id: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    if let Some(path) = args.next() {
                        options.config_path = path.into();
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
