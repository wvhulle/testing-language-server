use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LSError {
    // Standard errors with From implementations
    #[error("IO error: {0}")]
    IO(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UTF8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("String UTF8 error: {0}")]
    StringUtf8(#[from] std::string::FromUtf8Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    // Tree-sitter errors
    #[error("Tree-sitter language error: {0}")]
    TreeSitterLanguage(#[from] tree_sitter::LanguageError),

    #[error("Tree-sitter query error: {0}")]
    TreeSitterQuery(#[from] tree_sitter::QueryError),

    #[error("Tree-sitter parse failed")]
    TreeSitterParse,

    // Adapter errors
    #[error("Unknown test kind: {0}")]
    UnknownTestKind(String),

    #[error("Missing --test-kind argument")]
    MissingTestKind,

    #[error("Command spawn failed: {0}")]
    CommandSpawn(String),

    #[error("Adapter produced no output")]
    AdapterNoOutput,

    #[error("Adapter returned error output")]
    AdapterError,

    // Configuration errors
    #[error("No workspace folders found")]
    NoWorkspaceFolders,

    #[error("No home directory found")]
    NoHomeDirectory,

    #[error("Configuration file not found: {0}")]
    ConfigNotFound(PathBuf),

    #[error("XML parse error")]
    XmlParse,
}
