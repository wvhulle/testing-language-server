//! Testing Language Server - LSP for running tests and showing diagnostics.

use std::collections::HashMap;

use lsp_types::{Diagnostic, Range, ShowMessageParams};
use serde::{Deserialize, Serialize};

pub mod config;
pub mod error;
pub mod log;
pub mod protocol;
pub mod runner;
pub mod workspace;

// Language-specific modules
pub mod go;
pub mod javascript;
pub mod php;
pub mod rust;

/// If the character value is greater than the line length it defaults back to
/// the line length.
pub const MAX_CHAR_LENGTH: u32 = 10000;

// --- Core Types ---

pub type FilePath = String;
pub type WorkspacePath = String;
pub type AdapterId = String;

/// A single test item discovered in a file.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct TestItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub start_position: Range,
    pub end_position: Range,
}

/// Tests found in a single file.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct FileTests {
    pub path: String,
    pub tests: Vec<TestItem>,
}

/// Collection of discovered tests across files.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Default)]
pub struct DiscoveredTests {
    pub files: Vec<FileTests>,
}

/// Diagnostics for a single file.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct FileDiagnostics {
    pub path: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Test run diagnostics across files.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Default)]
pub struct Diagnostics {
    pub files: Vec<FileDiagnostics>,
    #[serde(default)]
    pub messages: Vec<ShowMessageParams>,
}

/// Map of workspace roots to their contained files.
#[derive(Debug, Serialize, Clone, Deserialize, Default)]
pub struct Workspaces {
    pub map: HashMap<WorkspacePath, Vec<FilePath>>,
}

// --- Configuration Types ---

/// Configuration for a test adapter.
#[derive(Debug, Deserialize, Clone, Serialize, Default)]
pub struct AdapterConfiguration {
    /// Test runner kind (e.g., "cargo-test", "cargo-nextest", "jest", "vitest",
    /// "go-test", "phpunit", "node-test", "deno")
    pub test_kind: String,
    #[serde(default)]
    pub extra_arg: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub workspace_dir: Option<String>,
}

impl AdapterConfiguration {
    /// Validate configuration and return warnings.
    pub fn validate(&self, adapter_id: &str) -> Vec<String> {
        let mut warnings = Vec::new();

        let valid_kinds = [
            "cargo-test",
            "cargo-nextest",
            "jest",
            "vitest",
            "go-test",
            "phpunit",
            "node-test",
            "deno",
        ];
        if !valid_kinds.contains(&self.test_kind.as_str()) {
            warnings.push(format!(
                "Adapter '{}': unknown test_kind '{}'. Valid values are: {}",
                adapter_id,
                self.test_kind,
                valid_kinds.join(", ")
            ));
        }

        if self.include.is_empty() {
            warnings.push(format!(
                "Adapter '{}': no include patterns specified",
                adapter_id
            ));
        }

        warnings
    }
}

/// Analysis result for a workspace with its adapter configuration.
#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceAnalysis {
    pub adapter_config: AdapterConfiguration,
    pub workspaces: Workspaces,
}

impl WorkspaceAnalysis {
    pub fn new(adapter_config: AdapterConfiguration, workspaces: Workspaces) -> Self {
        Self {
            adapter_config,
            workspaces,
        }
    }
}
