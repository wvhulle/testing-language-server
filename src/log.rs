use crate::error::LSError;
use crate::util::clean_old_logs;
use tracing_appender::non_blocking::WorkerGuard;

pub fn init_logging(component: &str) -> Result<WorkerGuard, LSError> {
    let home_dir = dirs::home_dir().ok_or(LSError::NoHomeDirectory)?;
    let log_dir = home_dir.join(".config/testing_language_server/logs");
    let prefix = format!("{component}.log");

    let file_appender = tracing_appender::rolling::daily(&log_dir, &prefix);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let _ = clean_old_logs(
        log_dir.to_str().unwrap_or_default(),
        30,
        &format!("{prefix}.*"),
        &format!("{prefix}."),
    );

    tracing_subscriber::fmt().with_writer(non_blocking).init();
    Ok(guard)
}
