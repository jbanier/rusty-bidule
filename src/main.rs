mod auto_pull;
mod config;
mod conversation_store;
mod doc_sections;
mod llm;
mod local_tools;
mod logging;
mod mcp_runtime;
mod oauth;
mod orchestrator;
mod paths;
mod prompt_expansion;
mod recipes;
mod redaction;
mod schedules;
mod skills;
mod tool_evidence;
mod types;
mod ui;
mod web;
mod workflows;

use anyhow::{Result, bail};
use auto_pull::AutoPullRuntime;
use config::AppConfig;
use orchestrator::Orchestrator;
use prompt_expansion::expand_prompt_file_references;
use std::path::PathBuf;
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

    match options.interface {
        Interface::Web => {
            info!("launching web interface");
            AutoPullRuntime::new(orchestrator.clone()).start();
            schedules::ScheduleRuntime::new(orchestrator.clone()).start();
            let recipes = orchestrator.recipes().clone();
            web::run_web_server(orchestrator, recipes, &options.host, options.port).await
        }
        Interface::Tui => {
            if let Some(message) = options.once_message {
                info!("running in one-shot mode");
                run_once(orchestrator, options.conversation_id, message).await
            } else {
                info!("launching interactive TUI");
                AutoPullRuntime::new(orchestrator.clone()).start();
                schedules::ScheduleRuntime::new(orchestrator.clone()).start();
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
    let conversation = orchestrator.store().load(&conversation_id)?;
    let message = expand_prompt_file_references(
        &message,
        &conversation.agent_permissions,
        paths::discover_project_root().as_deref(),
    )?;
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
                match event {
                    UiEvent::Progress(progress) => {
                        eprintln!("{}: {}", progress.kind, progress.message);
                    }
                    UiEvent::StreamChunk(_chunk) => {
                        // TODO: Print streaming chunks
                    }
                    UiEvent::Finished(_) | UiEvent::CompactionFinished(_) => {}
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
    interface: Interface,
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interface {
    Tui,
    Web,
}

impl Interface {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "tui" => Ok(Self::Tui),
            "web" => Ok(Self::Web),
            other => bail!("invalid --interface value '{other}'; expected 'tui' or 'web'"),
        }
    }
}

impl CliOptions {
    fn parse() -> Self {
        match Self::try_parse_from(std::env::args().skip(1)) {
            Ok(options) => options,
            Err(err) => {
                eprintln!("{err}");
                eprintln!("Run with --help for usage.");
                std::process::exit(1);
            }
        }
    }

    fn try_parse_from<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
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
            interface: Interface::Tui,
            host: "127.0.0.1".to_string(),
            port: 8080,
        };
        let mut args = args.into_iter().map(Into::into);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "--config" => {
                    let path = next_arg_value(&mut args, "--config")?;
                    options.config_path = path.into();
                    options.config_path_source = "cli";
                }
                "--once" => {
                    options.once_message = Some(next_arg_value(&mut args, "--once")?);
                }
                "--conversation" => {
                    options.conversation_id = Some(next_arg_value(&mut args, "--conversation")?);
                }
                "--interface" => {
                    let iface = next_arg_value(&mut args, "--interface")?;
                    options.interface = Interface::parse(&iface)?;
                }
                "--host" => {
                    options.host = next_arg_value(&mut args, "--host")?;
                }
                "--port" => {
                    let port_str = next_arg_value(&mut args, "--port")?;
                    options.port = port_str
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid --port value '{port_str}'"))?;
                }
                other => {
                    bail!("unknown option: {other}");
                }
            }
        }
        Ok(options)
    }
}

fn next_arg_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
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
    paths::discover_project_root()
        .map(|root| root.join("config").join("config.local.yaml"))
        .unwrap_or_else(|| PathBuf::from("config/config.local.yaml"))
}

#[cfg(test)]
mod tests {
    use super::{CliOptions, Interface};

    #[test]
    fn parses_web_interface_and_port() {
        let options = CliOptions::try_parse_from(["--interface", "web", "--port", "9000"]).unwrap();

        assert_eq!(options.interface, Interface::Web);
        assert_eq!(options.port, 9000);
    }

    #[test]
    fn rejects_invalid_interface() {
        let err = CliOptions::try_parse_from(["--interface", "desktop"]).unwrap_err();

        assert!(err.to_string().contains("invalid --interface value"));
    }

    #[test]
    fn rejects_invalid_port() {
        let err = CliOptions::try_parse_from(["--port", "not-a-port"]).unwrap_err();

        assert!(err.to_string().contains("invalid --port value"));
    }

    #[test]
    fn rejects_missing_option_value() {
        let err = CliOptions::try_parse_from(["--once"]).unwrap_err();

        assert!(err.to_string().contains("--once requires a value"));
    }
}
