use std::{
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use crate::{config, error::LSError};

pub fn run_phpunit(
    workspace: &str,
    file_paths: &[String],
    filter_pattern: &str,
) -> Result<(Output, PathBuf), LSError> {
    let log_path = config::CONFIG.cache_dir.join("phpunit.xml");

    let output = Command::new("phpunit")
        .current_dir(workspace)
        .args([
            "--log-junit",
            log_path.to_str().unwrap(),
            "--filter",
            filter_pattern,
        ])
        .args(file_paths)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()?;

    Ok((output, log_path))
}
