mod server;

use std::io::{self, BufRead, Read};

use lsp_types::InitializeParams;
use serde::{Deserialize, de::Error};
use serde_json::{Value, json};
use test_lsp::{config, error::LSError, log::init_logging, protocol};

use crate::server::TestingLS;

fn extract_textdocument_uri(params: &Value) -> Result<String, serde_json::Error> {
    let uri = params["textDocument"]["uri"]
        .as_str()
        .ok_or(serde_json::Error::custom("`textDocument.uri` is not set"))?;
    Ok(protocol::uri_to_path(uri))
}

fn extract_uri(params: &Value) -> Result<String, serde_json::Error> {
    let uri = params["uri"]
        .as_str()
        .ok_or(serde_json::Error::custom("`uri` is not set"))?;
    Ok(protocol::uri_to_path(uri))
}

fn main_loop(server: &mut TestingLS) -> Result<(), LSError> {
    let mut is_workspace_checked = false;
    loop {
        let mut size = 0;
        'read_header: loop {
            let mut buffer = String::new();
            let stdin = io::stdin();
            let mut handle = stdin.lock();
            handle.read_line(&mut buffer)?;

            if buffer.is_empty() {
                tracing::warn!("buffer is empty")
            }

            // The end of header section
            if buffer == "\r\n" {
                break 'read_header;
            }

            let split: Vec<&str> = buffer.split(' ').collect();

            if split.len() != 2 {
                tracing::warn!("unexpected");
            }

            let header_name = split[0].to_lowercase();
            let header_value = split[1].trim();

            match header_name.as_ref() {
                "content-length" => {}
                "content-type:" => {}
                _ => {}
            }

            size = header_value.parse::<usize>().unwrap();
        }

        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut buf = vec![0u8; size];
        handle.read_exact(&mut buf).unwrap();
        let message = String::from_utf8(buf).unwrap();

        let received_json: Value = serde_json::from_str(&message)?;
        tracing::info!("received json={:#?}", received_json);
        let method = &received_json["method"].as_str();
        let params = &received_json["params"];

        if let Some(method) = method {
            match *method {
                "$/cancelRequest" => {}
                "initialized" => {
                    is_workspace_checked = true;
                    server.diagnose_workspace()?;
                }
                "initialize" => {
                    let initialize_params = InitializeParams::deserialize(params)?;
                    let id = received_json["id"].as_i64().unwrap();
                    server.initialize(id, initialize_params)?;
                }
                "shutdown" => {
                    let id = received_json["id"].as_i64().unwrap();
                    server.shutdown(id)?;
                }
                "exit" => {
                    std::process::exit(0);
                }
                "workspace/diagnostic" => {
                    is_workspace_checked = true;
                    server.diagnose_workspace()?;
                }
                "textDocument/diagnostic" | "textDocument/didSave" => {
                    let uri = extract_textdocument_uri(params)?;
                    server.check_file(&uri, false)?;
                }
                "textDocument/didOpen" => {
                    if !is_workspace_checked {
                        is_workspace_checked = true;
                        server.diagnose_workspace()?;
                    }
                    let uri = extract_textdocument_uri(params)?;
                    if server.refreshing_needed(&uri) {
                        server.refresh_workspaces_cache()?;
                    }
                }
                "$/runFileTest" => {
                    let uri = extract_uri(params)?;
                    server.check_file(&uri, false)?;
                }
                "$/runWorkspaceTest" => {
                    server.diagnose_workspace()?;
                }
                "$/discoverFileTest" => {
                    let id = received_json["id"].as_i64().unwrap();
                    let uri = extract_uri(params)?;
                    let result = server.discover_file(&uri)?;
                    protocol::send(&json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result,
                    }))?;
                }
                _ => {
                    // https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#responseMessage
                    let id = received_json["id"].as_i64();
                    if id.is_some() {
                        protocol::send_error(
                            id,
                            -32601, // Method not found
                            format!("method not found: {}", method),
                        )?;
                    }
                }
            }
        }
    }
}

fn main() {
    config::init();
    let mut server = TestingLS::new();
    let _guard = init_logging("server").expect("Failed to initialize logger");
    if let Err(ls_error) = main_loop(&mut server) {
        tracing::error!("Error: {:?}", ls_error);
    }
}
