//! Test runner trait and registry.

use crate::{Diagnostics, DiscoveredTests, Workspaces, error::LSError, go, javascript, php, rust};

/// Trait for test runners.
///
/// Each language/framework implements this trait to provide test discovery,
/// execution, and workspace detection.
pub trait Runner: Send + Sync {
    /// Discover tests in the given files.
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError>;

    /// Run tests and return diagnostics.
    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        extra_args: &[String],
    ) -> Result<Diagnostics, LSError>;

    /// Detect workspaces containing the given files.
    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces;
}

/// Get a runner by test kind identifier.
pub fn get(test_kind: &str) -> Result<Box<dyn Runner>, LSError> {
    match test_kind {
        "cargo-test" => Ok(Box::new(rust::CargoTestRunner)),
        "cargo-nextest" => Ok(Box::new(rust::CargoNextestRunner)),
        "go-test" => Ok(Box::new(go::GoTestRunner)),
        "phpunit" => Ok(Box::new(php::PhpunitRunner)),
        "jest" => Ok(Box::new(javascript::JestRunner)),
        "vitest" => Ok(Box::new(javascript::VitestRunner)),
        "deno" => Ok(Box::new(javascript::DenoRunner)),
        "node-test" => Ok(Box::new(javascript::NodeTestRunner)),
        _ => Err(LSError::UnknownTestKind(test_kind.to_string())),
    }
}
