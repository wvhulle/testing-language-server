use std::{collections::HashMap, path::PathBuf, sync::LazyLock};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::AdapterId;

pub static CONFIG: LazyLock<Config> = LazyLock::new(Config::parse);

/// Configuration for a test adapter.
#[derive(Debug, Deserialize, Clone, Serialize, Default)]
pub struct AdapterConfig {
    /// Test runner kind (e.g., "cargo-test", "cargo-nextest", "jest", "vitest",
    /// "go-test", "phpunit", "node-test", "deno")
    pub test_kind: String,
    /// Extra arguments passed to the test command
    #[serde(default)]
    pub extra_arg: Vec<String>,
    /// Environment variables for the test process
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Glob patterns for files to include
    #[serde(default)]
    pub include: Vec<String>,
    /// Glob patterns for files to exclude
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Override workspace directory
    pub workspace_dir: Option<String>,
}

impl AdapterConfig {
    /// Validate configuration and return warnings.
    #[must_use]
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
                "Adapter '{adapter_id}': unknown test_kind '{}'. Valid values are: {}",
                self.test_kind,
                valid_kinds.join(", ")
            ));
        }

        warnings
    }
}

/// Main configuration struct for assert-lsp.
/// Can be loaded from CLI arguments, TOML file, or LSP initialization options.
#[derive(Parser, Debug, Clone, Deserialize, Serialize)]
#[command(name = "assert-lsp")]
#[command(about = "LSP server for showing test failures as diagnostics")]
#[serde(rename_all = "snake_case")]
pub struct Config {
    /// Directory for cache files (defaults to system temp directory)
    #[arg(default_value_os_t = default_cache_dir())]
    #[serde(default = "default_cache_dir")]
    pub cache_dir: PathBuf,

    /// Adapter configurations per test kind
    #[arg(skip)]
    #[serde(default)]
    pub adapter_command: HashMap<AdapterId, AdapterConfig>,
}

fn default_cache_dir() -> PathBuf {
    std::env::temp_dir().join("assert-lsp")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir(),
            adapter_command: HashMap::new(),
        }
    }
}
