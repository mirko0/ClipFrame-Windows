//! File logging (daily rolling, 7 files kept) plus stderr in debug builds.

use std::path::Path;

use anyhow::Context as _;
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

const MAX_LOG_FILES: usize = 7;

/// Initialises the global subscriber. Keep the returned guard alive until exit so buffered
/// log lines are flushed.
pub fn init(log_dir: &Path) -> anyhow::Result<WorkerGuard> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("clipframe")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .with_context(|| format!("creating log files in {}", log_dir.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let level = if cfg!(debug_assertions) {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    let file_layer = fmt::layer().with_writer(writer).with_ansi(false);
    let stderr_layer = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));

    tracing_subscriber::registry()
        .with(level)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .context("installing the log subscriber")?;
    Ok(guard)
}

/// Routes panics into the log file before the default hook runs.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));
}
