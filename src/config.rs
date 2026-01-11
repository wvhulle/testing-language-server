use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::AdapterId;

static CONFIG: OnceLock<Config> = OnceLock::new();

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
#[derive(Parser, Debug, Clone, Deserialize, Serialize, Default)]
#[command(name = "assert-lsp")]
#[command(about = "LSP server for showing test failures as diagnostics")]
#[serde(rename_all = "snake_case")]
pub struct Config {
    /// Directory for log files
    #[arg(long, default_value_t = default_log_dir())]
    #[serde(default = "default_log_dir")]
    pub log_dir: String,

    /// Directory for cache files
    #[arg(long, default_value_t = default_cache_dir())]
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,

    /// Adapter configurations per test kind
    #[arg(skip)]
    #[serde(default)]
    pub adapter_command: HashMap<AdapterId, AdapterConfig>,
}

fn default_log_dir() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("assert-lsp/logs")
        .to_string_lossy()
        .to_string()
}

fn default_cache_dir() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("assert-lsp")
        .to_string_lossy()
        .to_string()
}

impl Config {
    #[must_use]
    pub fn log_dir(&self) -> PathBuf {
        PathBuf::from(&self.log_dir)
    }

    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        PathBuf::from(&self.cache_dir)
    }
}

pub fn init() -> &'static Config {
    CONFIG.get_or_init(Config::parse)
}

pub fn get() -> &'static Config {
    CONFIG
        .get()
        .expect("Config not initialized. Call config::init() first.")
}
