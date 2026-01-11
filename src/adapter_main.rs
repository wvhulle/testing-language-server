use clap::Parser;
use std::io::{self, Write};
use testing_language_server::adapter::model::get_runner;
use testing_language_server::error::LSError;
use testing_language_server::log::init_logging;
use testing_language_server::spec::{
    AdapterCommands, DetectWorkspaceArgs, DiscoverArgs, RunFileTestArgs,
};

fn extract_test_kind(extra: &mut Vec<String>) -> Result<String, LSError> {
    let index = extra
        .iter()
        .position(|arg| arg.starts_with("--test-kind="))
        .ok_or(LSError::MissingTestKind)?;

    let test_kind = extra.remove(index).replace("--test-kind=", "");
    Ok(test_kind)
}

fn handle(commands: AdapterCommands) -> Result<(), LSError> {
    match commands {
        AdapterCommands::Discover(mut args) => {
            let test_kind = extract_test_kind(&mut args.extra)?;
            let runner = get_runner(&test_kind)?;
            runner.discover(DiscoverArgs {
                extra: args.extra,
                file_paths: args.file_paths,
            })
        }
        AdapterCommands::RunFileTest(mut args) => {
            let test_kind = extract_test_kind(&mut args.extra)?;
            let runner = get_runner(&test_kind)?;
            runner.run_file_test(RunFileTestArgs {
                extra: args.extra,
                file_paths: args.file_paths,
                workspace: args.workspace,
            })
        }
        AdapterCommands::DetectWorkspace(mut args) => {
            let test_kind = extract_test_kind(&mut args.extra)?;
            let runner = get_runner(&test_kind)?;
            runner.detect_workspaces(DetectWorkspaceArgs {
                extra: args.extra,
                file_paths: args.file_paths,
            })
        }
    }
}

fn main() {
    let _guard = init_logging("adapter").expect("Failed to initialize logger");
    let args = AdapterCommands::parse();
    tracing::info!("adapter args={:#?}", args);

    if let Err(error) = handle(args) {
        let _ = io::stderr().write_all(format!("{:#?}", error).as_bytes());
    }
}
