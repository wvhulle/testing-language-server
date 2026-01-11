use crate::adapter::runner::{
    cargo_nextest::CargoNextestRunner, cargo_test::CargoTestRunner, deno::DenoRunner,
    go::GoTestRunner, jest::JestRunner, node_test::NodeTestRunner, phpunit::PhpunitRunner,
    vitest::VitestRunner,
};
use crate::error::LSError;
use crate::spec::{DetectWorkspaceArgs, DiscoverArgs, RunFileTestArgs};

pub trait Runner: Send + Sync {
    fn discover(&self, args: DiscoverArgs) -> Result<(), LSError>;
    fn run_file_test(&self, args: RunFileTestArgs) -> Result<(), LSError>;
    fn detect_workspaces(&self, args: DetectWorkspaceArgs) -> Result<(), LSError>;
}

pub fn get_runner(test_kind: &str) -> Result<Box<dyn Runner>, LSError> {
    match test_kind {
        "cargo-test" => Ok(Box::new(CargoTestRunner)),
        "cargo-nextest" => Ok(Box::new(CargoNextestRunner)),
        "jest" => Ok(Box::new(JestRunner)),
        "vitest" => Ok(Box::new(VitestRunner)),
        "deno" => Ok(Box::new(DenoRunner)),
        "go-test" => Ok(Box::new(GoTestRunner)),
        "phpunit" => Ok(Box::new(PhpunitRunner)),
        "node-test" => Ok(Box::new(NodeTestRunner)),
        _ => Err(LSError::UnknownTestKind(test_kind.to_string())),
    }
}
