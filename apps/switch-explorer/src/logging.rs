//! Logging (ARCHITECTURE.md §14): `tracing` into a daily rolling file under
//! `%LOCALAPPDATA%\SwitchExplorer\logs`, with GPUI's `log` records bridged in. User paths are
//! only logged at `debug` (SECURITY.md §8); tokens never.

use std::path::Path;

use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;

/// Keeps the background log writer alive; dropping it flushes the file.
pub struct LogGuard {
    _writer: Option<WorkerGuard>,
}

pub fn init(dir: Option<&Path>, level: &str) -> LogGuard {
    let level = match level {
        "error" => LevelFilter::ERROR,
        "warn" => LevelFilter::WARN,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        _ => LevelFilter::INFO,
    };
    let builder = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(false)
        .with_target(true);
    let Some(dir) = dir.filter(|d| std::fs::create_dir_all(d).is_ok()) else {
        let _ = builder.with_writer(std::io::stderr).try_init();
        return LogGuard { _writer: None };
    };
    let (writer, guard) = tracing_appender::non_blocking(tracing_appender::rolling::daily(
        dir,
        "switch-explorer.log",
    ));
    let _ = builder.with_writer(writer).try_init();
    let default_hook = std::panic::take_hook();
    // The location goes to the error log; the message may carry user data (a path), so it is
    // only logged at debug.
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        tracing::error!(%location, "panic");
        tracing::debug!(panic = %info, "panic");
        default_hook(info);
    }));
    LogGuard {
        _writer: Some(guard),
    }
}
