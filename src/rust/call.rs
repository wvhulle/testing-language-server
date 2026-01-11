use std::process::{Command, Output};

use crate::error::LSError;
use crate::log::write_result_log;

/// Run cargo test with JSON output format.
pub fn run_cargo_test(workspace: &str, extra_args: &[String], test_ids: &[String]) -> Result<Output, LSError> {
    let output = Command::new("cargo")
        .current_dir(workspace)
        .arg("test")
        .args(extra_args)
        .arg("--")
        .arg("-Z")
        .arg("unstable-options")
        .arg("--format")
        .arg("json")
        .args(test_ids)
        .output()?;

    write_result_log("cargo_test.log", &output)?;

    if !output.stderr.is_empty() {
        tracing::debug!("cargo test stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(output)
}

/// Run cargo nextest with text output format.
pub fn run_cargo_nextest(workspace: &str, extra_args: &[String], test_ids: &[String]) -> Result<Output, LSError> {
    let output = Command::new("cargo")
        .current_dir(workspace)
        .arg("nextest")
        .arg("run")
        .arg("--workspace")
        .arg("--no-fail-fast")
        .args(extra_args)
        .arg("--")
        .args(test_ids)
        .output()?;

    write_result_log("cargo_nextest.log", &output)?;

    Ok(output)
}
