/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

const LOG_ENV: &str = "RELIX_LOG";

/// Logs to a daily rotated file in the data directory; the terminal itself is owned by
/// the UI. Credentials are never passed to log statements.
pub fn init(logs_dir: &Path) -> WorkerGuard {
    let appender = tracing_appender::rolling::daily(logs_dir, "relix.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
    guard
}
