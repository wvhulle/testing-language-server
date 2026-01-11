use std::path::PathBuf;
use std::process::{Command, Output};

use crate::config;
use crate::error::LSError;
use crate::log::write_result_log;

pub fn run_jest(workspace: &str) -> Result<(Output, PathBuf), LSError> {
    let log_path = PathBuf::from(config::get().cache_dir()).join("jest.json");

    let output = Command::new("jest")
        .current_dir(workspace)
        .args([
            "--testLocationInResults",
            "--forceExit",
            "--no-coverage",
            "--verbose",
            "--json",
            "--outputFile",
            log_path.to_str().unwrap(),
        ])
        .output()?;

    write_result_log("jest.log", &output)?;
    Ok((output, log_path))
}

pub fn run_vitest(workspace: &str) -> Result<(Output, PathBuf), LSError> {
    let log_path = PathBuf::from(config::get().cache_dir()).join("vitest.json");

    let output = Command::new("vitest")
        .current_dir(workspace)
        .args([
            "--watch=false",
            "--reporter=json",
            &format!("--outputFile={}", log_path.display()),
        ])
        .output()?;

    write_result_log("vitest.log", &output)?;
    Ok((output, log_path))
}

pub fn run_deno(workspace: &str, file_paths: &[String]) -> Result<Output, LSError> {
    let output = Command::new("deno")
        .current_dir(workspace)
        .args(["test", "--no-prompt"])
        .args(file_paths)
        .output()?;

    write_result_log("deno.log", &output)?;
    Ok(output)
}

pub fn run_node_test(
    workspace: &str,
    file_paths: &[String],
    extra_args: &[String],
) -> Result<Output, LSError> {
    let output = Command::new("node")
        .current_dir(workspace)
        .args(["--test", "--test-reporter", "junit"])
        .args(extra_args)
        .args(file_paths)
        .output()?;

    write_result_log("node-test.xml", &output)?;
    Ok(output)
}
