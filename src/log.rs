use std::{fs, io, process::Output};

use regex::Regex;

use crate::config;

/// Write test command output to a log file for debugging.
pub fn write_result_log(file_name: &str, output: &Output) -> io::Result<()> {
    let stdout_str = String::from_utf8(output.stdout.clone()).unwrap_or_default();
    let stderr_str = String::from_utf8(output.stderr.clone()).unwrap_or_default();
    let content = format!("stdout:\n{stdout_str}\nstderr:\n{stderr_str}");
    let cache = &config::CONFIG.cache_dir;
    fs::create_dir_all(cache)?;
    let log_path = cache.join(file_name);
    fs::write(&log_path, content)?;
    Ok(())
}

/// Clean ANSI escape sequences from text.
pub fn clean_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1B\[([0-9]{1,2}(;[0-9]{1,2})*)?[m|K]").unwrap();
    re.replace_all(input, "").to_string()
}
