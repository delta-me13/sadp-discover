//! 日志：文件 + 可选 stderr。
//!
//! TUI 模式下只写文件，绝不能污染终端画面（否则会撕裂界面）。
//! 日志目录：`%LOCALAPPDATA%\sadp-discover\logs`（macOS 为
//! `~/Library/Application Support/sadp-discover/logs`）。

use std::io;
use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 持有文件 appender 的 worker，drop 时会 flush。
pub struct Guard {
    _worker: Option<WorkerGuard>,
}

/// 初始化全局 subscriber。重复调用只生效一次。
#[must_use]
pub fn init(level: Option<&str>, verbose: u8, to_stderr: bool) -> Guard {
    let level = level.map(str::to_string).unwrap_or_else(|| match verbose {
        0 => "warn".to_string(),
        1 => "info".to_string(),
        2 => "debug".to_string(),
        _ => "trace".to_string(),
    });

    let filter = EnvFilter::try_new(&level).unwrap_or_else(|_| EnvFilter::new("warn"));

    let (file_layer, worker) = match log_dir() {
        Some(dir) if std::fs::create_dir_all(&dir).is_ok() => {
            let appender = tracing_appender::rolling::daily(&dir, "sadp-discover.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (
                Some(
                    tracing_subscriber::fmt::layer()
                        .with_writer(writer)
                        .with_ansi(false)
                        .with_target(true),
                ),
                Some(guard),
            )
        }
        _ => (None, None),
    };

    let stderr_layer = if to_stderr {
        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(io::stderr)
                .with_target(false),
        )
    } else {
        None
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    Guard { _worker: worker }
}

#[must_use]
pub fn log_dir() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|d| d.join("sadp-discover").join("logs"))
}
