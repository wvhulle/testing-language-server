use std::process::{Command, Output};

use crate::error::LSError;
use crate::log::write_result_log;

pub fn run_go_test(workspace: &str, extra_args: &[String]) -> Result<Output, LSError> {
    let default_args = ["-v", "-json", "", "-count=1", "-timeout=60s"];
    let output = Command::new("go")
        .current_dir(workspace)
        .arg("test")
        .args(default_args)
        .args(extra_args)
        .output()?;
    write_result_log("go.log", &output)?;
    Ok(output)
}
