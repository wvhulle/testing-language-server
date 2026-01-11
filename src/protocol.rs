//! LSP protocol communication utilities.

use std::io::{Write, stdout};

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value, json};

use crate::error::LSError;

/// Send a JSON-RPC message to stdout with Content-Length header.
pub fn send<T>(message: &T) -> Result<(), LSError>
where
    T: ?Sized + Serialize + std::fmt::Debug,
{
    tracing::info!("send stdout: {:#?}", message);
    let msg = serde_json::to_string(message)?;
    let mut stdout = stdout().lock();
    write!(stdout, "Content-Length: {}\r\n\r\n{}", msg.len(), msg)?;
    stdout.flush()?;
    Ok(())
}

/// JSON-RPC error message.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorMessage {
    jsonrpc: String,
    id: Option<Number>,
    pub error: Value,
}

impl ErrorMessage {
    pub fn new<N: Into<Number>>(id: Option<N>, error: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: id.map(|i| i.into()),
            error,
        }
    }
}

/// Send a JSON-RPC error response.
pub fn send_error<S: Into<String>>(id: Option<i64>, code: i64, msg: S) -> Result<(), LSError> {
    send(&ErrorMessage::new(
        id,
        json!({ "code": code, "message": msg.into() }),
    ))
}

/// Convert a file:// URI to a file path.
pub fn uri_to_path(uri: &str) -> String {
    uri.replace("file://", "")
}
