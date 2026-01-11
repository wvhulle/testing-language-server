use crate::config;
use crate::error::LSError;
use chrono::{NaiveDate, Utc};
use regex::Regex;
use std::fs;
use std::io;
use std::process::Output;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;

/// Clean old log files from a directory.
pub fn clean_old_logs(
    log_dir: &str,
    retention_days: i64,
    glob_pattern: &str,
    prefix: &str,
) -> Result<(), LSError> {
    let today = Utc::now().date_naive();
    let retention_threshold = today - chrono::Duration::days(retention_days);

    let walker = globwalk::GlobWalkerBuilder::from_patterns(log_dir, &[glob_pattern])
        .build()
        .unwrap();

    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
            if let Some(date_str) = file_name.strip_prefix(prefix) {
                if let Ok(file_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    if file_date < retention_threshold {
                        fs::remove_file(path)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Write test command output to a log file for debugging.
pub fn write_result_log(file_name: &str, output: &Output) -> io::Result<()> {
    let stdout_str = String::from_utf8(output.stdout.clone()).unwrap_or_default();
    let stderr_str = String::from_utf8(output.stderr.clone()).unwrap_or_default();
    let content = format!("stdout:\n{}\nstderr:\n{}", stdout_str, stderr_str);
    let cache = config::get().cache_dir();
    fs::create_dir_all(&cache)?;
    let log_path = cache.join(file_name);
    fs::write(&log_path, content)?;
    Ok(())
}

/// Clean ANSI escape sequences from text.
pub fn clean_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1B\[([0-9]{1,2}(;[0-9]{1,2})*)?[m|K]").unwrap();
    re.replace_all(input, "").to_string()
}

pub fn init_logging(component: &str) -> Result<WorkerGuard, LSError> {
    let log_dir = config::get().log_dir();
    fs::create_dir_all(&log_dir)?;

    let prefix = format!("{component}.log");

    let file_appender = rolling::daily(&log_dir, &prefix);
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
