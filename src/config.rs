use std::{path::PathBuf, sync::OnceLock};

use clap::Parser;

static CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Parser, Debug, Clone)]
#[command(name = "testing-language-server")]
#[command(about = "LSP server for showing test failures as diagnostics")]
pub struct Config {
    /// Directory for log files
    #[arg(long, default_value_t = default_log_dir())]
    pub log_dir: String,

    /// Directory for adapter output files
    #[arg(long, default_value_t = default_cache_dir())]
    pub cache_dir: String,
}

fn default_log_dir() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("test-lsp/logs")
        .to_string_lossy()
        .to_string()
}

fn default_cache_dir() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("test-lsp")
        .to_string_lossy()
        .to_string()
}

impl Config {
    pub fn log_dir(&self) -> PathBuf {
        PathBuf::from(&self.log_dir)
    }

    pub fn cache_dir(&self) -> PathBuf {
        PathBuf::from(&self.cache_dir)
    }
}

pub fn init() -> &'static Config {
    CONFIG.get_or_init(|| Config::parse())
}

pub fn get() -> &'static Config {
    CONFIG
        .get()
        .expect("Config not initialized. Call config::init() first.")
}
