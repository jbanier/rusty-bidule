use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{TracingConfig, TracingProvider};

pub fn init_logging(tracing_config: Option<&TracingConfig>) -> Result<WorkerGuard> {
    let project_root = discover_project_root()
        .or_else(|| std::env::current_dir().ok())
        .context("failed to determine current directory")?;
    let log_dir = project_root.join("var");
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::never(log_dir, "bidule.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "rusty_bidule=debug,reqwest=warn,hyper=warn,hyper_util=warn,mio=warn,tower=warn",
        )
    });

    let provider = tracing_config
        .map(|c| &c.provider)
        .unwrap_or(&TracingProvider::None);

    let use_console = matches!(provider, TracingProvider::Console);

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true)
                .with_level(true)
                .with_thread_ids(false)
                .with_thread_names(false)
                .with_file(false)
                .with_line_number(false),
        )
        .with(
            use_console.then(|| {
                fmt::layer()
                    .with_writer(std::io::stdout)
                    .with_ansi(true)
                    .with_target(true)
                    .with_level(true)
            }),
        );

    registry
        .try_init()
        .context("failed to initialize file logging")?;

    if matches!(provider, TracingProvider::Phoenix) {
        eprintln!("WARNING: Phoenix tracing requires the 'tracing' feature flag; currently not compiled in");
    }

    tracing::event!(
        Level::INFO,
        log_path = %log_path().display(),
        "file logging initialized"
    );

    Ok(guard)
}

pub fn log_path() -> PathBuf {
    discover_project_root()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("var")
        .join("bidule.log")
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

