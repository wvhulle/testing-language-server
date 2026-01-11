//! LSP test client for integration tests
//!
//! This module provides a simple LSP client that can spawn the server process,
//! send messages, and collect responses for verification.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    thread::{self, JoinHandle},
    time::Duration,
};

fn lsp_message(content: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", content.len(), content)
}

/// A parsed diagnostic from a publishDiagnostics response
#[derive(Debug)]
pub struct ParsedDiagnostic {
    pub uri: String,
    pub message: String,
    pub severity: Option<u32>,
    pub source: Option<String>,
    pub code: Option<String>,
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
}

/// Result of an LSP session, containing all responses and logs
pub struct SessionResult {
    pub responses: Vec<String>,
    pub stderr_lines: Vec<String>,
    pub status: ExitStatus,
}

impl SessionResult {
    /// Get all publishDiagnostics responses
    #[must_use]
    pub fn diagnostic_responses(&self) -> Vec<&String> {
        self.responses
            .iter()
            .filter(|r| r.contains("publishDiagnostics"))
            .collect()
    }

    /// Parse all diagnostics from responses
    #[must_use]
    pub fn parse_diagnostics(&self) -> Vec<ParsedDiagnostic> {
        let mut result = Vec::new();
        for response in &self.responses {
            if !response.contains("publishDiagnostics") {
                continue;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(response) {
                if let Some(params) = json.get("params") {
                    let uri = params
                        .get("uri")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(diagnostics) = params.get("diagnostics").and_then(|d| d.as_array())
                    {
                        for diag in diagnostics {
                            let message = diag
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let severity = diag
                                .get("severity")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32);
                            let source = diag
                                .get("source")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            let range = diag.get("range");
                            let start_line = range
                                .and_then(|r| r.get("start"))
                                .and_then(|s| s.get("line"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0) as u32;
                            let start_char = range
                                .and_then(|r| r.get("start"))
                                .and_then(|s| s.get("character"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0) as u32;
                            let end_line = range
                                .and_then(|r| r.get("end"))
                                .and_then(|s| s.get("line"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0) as u32;
                            let end_char = range
                                .and_then(|r| r.get("end"))
                                .and_then(|s| s.get("character"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0) as u32;
                            let code = diag
                                .get("code")
                                .and_then(|v| v.as_str())
                                .map(String::from);

                            result.push(ParsedDiagnostic {
                                uri: uri.clone(),
                                message,
                                severity,
                                source,
                                code,
                                start_line,
                                start_char,
                                end_line,
                                end_char,
                            });
                        }
                    }
                }
            }
        }
        result
    }

    /// Get diagnostics count
    #[must_use]
    pub fn diagnostic_count(&self) -> usize {
        self.parse_diagnostics().len()
    }

    /// Check if any diagnostic contains the given text
    #[must_use]
    #[allow(dead_code)]
    pub fn has_diagnostic_containing(&self, text: &str) -> bool {
        self.parse_diagnostics()
            .iter()
            .any(|d| d.message.contains(text))
    }

    /// Check if stderr contains the given text
    #[must_use]
    pub fn stderr_contains(&self, text: &str) -> bool {
        self.stderr_lines.iter().any(|l| l.contains(text))
    }

    /// Check if we got a valid initialize response
    #[must_use]
    pub fn has_init_response(&self) -> bool {
        self.responses
            .iter()
            .any(|r| r.contains("capabilities") && r.contains("result"))
    }

    /// Print a summary of the session for debugging
    pub fn print_summary(&self) {
        println!("\n=== Session Summary ===");
        println!("Exit status: {:?}", self.status);
        println!("Stderr lines: {}", self.stderr_lines.len());
        println!("Responses: {}", self.responses.len());
        println!(
            "Diagnostic responses: {}",
            self.diagnostic_responses().len()
        );
        let diagnostics = self.parse_diagnostics();
        println!("Parsed diagnostics: {}", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            println!(
                "  [{i}] line {}: {} (source: {:?})",
                diag.start_line, diag.message, diag.source
            );
        }
    }

    // ========== Assertions ==========

    /// Assert that we got a valid initialize response
    pub fn assert_initialized(&self) {
        assert!(
            self.has_init_response(),
            "Expected initialize response with capabilities"
        );
    }

    /// Assert that auto-detection found a project
    #[allow(dead_code)]
    pub fn assert_auto_detected(&self) {
        assert!(
            self.stderr_contains("Auto-detected projects"),
            "Expected server to auto-detect project type"
        );
    }

    /// Assert that no project was detected
    #[allow(dead_code)]
    pub fn assert_no_project_detected(&self) {
        assert!(
            self.stderr_contains("No project detected"),
            "Expected 'No project detected' message"
        );
    }

    /// Assert exact number of diagnostics
    pub fn assert_diagnostic_count(&self, expected: usize) {
        let actual = self.diagnostic_count();
        assert!(
            actual == expected,
            "Expected {expected} diagnostics, got {actual}"
        );
    }

    /// Assert at least N diagnostics
    pub fn assert_has_diagnostics(&self) {
        let count = self.diagnostic_count();
        assert!(count > 0, "Expected at least one diagnostic, got none");
    }

    /// Assert no diagnostics were published
    pub fn assert_no_diagnostics(&self) {
        let count = self.diagnostic_count();
        assert!(count == 0, "Expected no diagnostics, got {count}");
    }

    /// Assert a diagnostic exists with the given test name in its message
    pub fn assert_diagnostic_for_test(&self, test_name: &str) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics.iter().any(|d| d.message.contains(test_name));
        assert!(
            found,
            "Expected diagnostic for test '{test_name}', found: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// Assert a diagnostic exists with the given source
    pub fn assert_diagnostic_source(&self, expected_source: &str) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics
            .iter()
            .any(|d| d.source.as_deref() == Some(expected_source));
        assert!(
            found,
            "Expected diagnostic with source '{expected_source}', found sources: {:?}",
            diagnostics.iter().map(|d| &d.source).collect::<Vec<_>>()
        );
    }

    /// Assert a diagnostic exists with error severity (1)
    pub fn assert_diagnostic_is_error(&self) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics.iter().any(|d| d.severity == Some(1));
        assert!(
            found,
            "Expected at least one error diagnostic (severity=1), found: {:?}",
            diagnostics.iter().map(|d| d.severity).collect::<Vec<_>>()
        );
    }

    /// Assert a diagnostic exists at the given line
    pub fn assert_diagnostic_at_line(&self, line: u32) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics.iter().any(|d| d.start_line == line);
        assert!(
            found,
            "Expected diagnostic at line {line}, found lines: {:?}",
            diagnostics.iter().map(|d| d.start_line).collect::<Vec<_>>()
        );
    }

    /// Assert a diagnostic message contains the given text
    pub fn assert_diagnostic_message_contains(&self, text: &str) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics.iter().any(|d| d.message.contains(text));
        assert!(
            found,
            "Expected diagnostic message containing '{text}', found messages: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// Assert a diagnostic has the given code
    pub fn assert_diagnostic_code(&self, expected_code: &str) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(expected_code));
        assert!(
            found,
            "Expected diagnostic with code '{expected_code}', found codes: {:?}",
            diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }

    /// Assert that diagnostics were run (based on log output)
    pub fn assert_diagnostics_ran(&self) {
        assert!(
            self.stderr_contains("Running initial workspace diagnostics")
                || self.stderr_contains("diagnose_workspace: starting"),
            "Expected workspace diagnostics to run"
        );
    }

    /// Assert diagnostic URI contains the given path segment
    pub fn assert_diagnostic_uri_contains(&self, path_segment: &str) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics.iter().any(|d| d.uri.contains(path_segment));
        assert!(
            found,
            "Expected diagnostic URI containing '{path_segment}', found URIs: {:?}",
            diagnostics.iter().map(|d| &d.uri).collect::<Vec<_>>()
        );
    }

    /// Assert diagnostic has a valid range (start <= end)
    pub fn assert_diagnostic_has_valid_range(&self) {
        let diagnostics = self.parse_diagnostics();
        for diag in &diagnostics {
            assert!(
                diag.start_line <= diag.end_line,
                "Invalid range: start_line {} > end_line {}",
                diag.start_line,
                diag.end_line
            );
            if diag.start_line == diag.end_line {
                assert!(
                    diag.start_char <= diag.end_char,
                    "Invalid range on line {}: start_char {} > end_char {}",
                    diag.start_line,
                    diag.start_char,
                    diag.end_char
                );
            }
        }
    }

    /// Get the first diagnostic (for detailed assertions)
    #[must_use]
    #[allow(dead_code)]
    pub fn first_diagnostic(&self) -> Option<ParsedDiagnostic> {
        self.parse_diagnostics().into_iter().next()
    }

    /// Assert all diagnostic fields match expected values
    pub fn assert_diagnostic_fields(
        &self,
        expected_source: &str,
        expected_severity: u32,
        expected_line: u32,
    ) {
        let diagnostics = self.parse_diagnostics();
        let found = diagnostics.iter().any(|d| {
            d.source.as_deref() == Some(expected_source)
                && d.severity == Some(expected_severity)
                && d.start_line == expected_line
        });
        assert!(
            found,
            "Expected diagnostic with source='{}', severity={}, line={}, found: {:?}",
            expected_source,
            expected_severity,
            expected_line,
            diagnostics
                .iter()
                .map(|d| format!(
                    "source={:?}, severity={:?}, line={}",
                    d.source, d.severity, d.start_line
                ))
                .collect::<Vec<_>>()
        );
    }
}

/// LSP client for testing the server
pub struct LspClient {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout_handle: JoinHandle<Vec<String>>,
    stderr_handle: JoinHandle<Vec<String>>,
}

impl LspClient {
    /// Spawn a new server process and create a client
    #[must_use]
    pub fn new(server_path: &PathBuf) -> Self {
        let mut child = Command::new(server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUST_LOG", "debug")
            .spawn()
            .expect("Failed to start server");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Read stderr in background thread
        let stderr_handle = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            let mut lines = Vec::new();
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[SERVER STDERR] {line}");
                lines.push(line);
            }
            lines
        });

        // Read stdout in background thread
        let stdout_handle = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut responses = Vec::new();
            let mut buf = String::new();

            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if buf.starts_with("Content-Length:") {
                            let len: usize = buf
                                .trim()
                                .strip_prefix("Content-Length: ")
                                .unwrap()
                                .parse()
                                .unwrap();

                            // Skip empty line
                            reader.read_line(&mut String::new()).ok();

                            // Read content
                            let mut content = vec![0u8; len];
                            reader.read_exact(&mut content).ok();
                            let content_str = String::from_utf8_lossy(&content).to_string();
                            println!("[SERVER RESPONSE] {content_str}");
                            responses.push(content_str);
                        }
                    }
                }
            }
            responses
        });

        Self {
            child,
            stdin,
            stdout_handle,
            stderr_handle,
        }
    }

    /// Send a raw LSP message
    pub fn send(&mut self, content: &str) {
        self.stdin
            .write_all(lsp_message(content).as_bytes())
            .unwrap();
        self.stdin.flush().unwrap();
    }

    /// Send initialize and initialized messages
    pub fn initialize(&mut self, root_uri: &str) {
        let init = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"initialize","params":{{"processId":{},"rootUri":"{}","capabilities":{{"textDocument":{{"publishDiagnostics":{{"relatedInformation":true}}}}}},"workspaceFolders":[{{"uri":"{}","name":"test-project"}}]}}}}"#,
            std::process::id(),
            root_uri,
            root_uri
        );
        println!("Sending initialize...");
        self.send(&init);
        thread::sleep(Duration::from_millis(100));

        println!("Sending initialized...");
        self.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#);
    }

    /// Send shutdown and exit messages
    pub fn shutdown_and_exit(&mut self) {
        println!("Sending shutdown...");
        self.send(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":null}"#);
        thread::sleep(Duration::from_millis(50));

        println!("Sending exit...");
        self.send(r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }

    /// Wait for the server to exit and collect all output
    #[must_use]
    pub fn wait_for_completion(mut self) -> SessionResult {
        drop(self.stdin);

        // Give the server a moment to exit gracefully
        thread::sleep(Duration::from_millis(100));

        // Try to kill the process if it's still running
        let _ = self.child.kill();

        let status = self.child.wait().expect("Failed to wait for child");
        let stderr_lines = self.stderr_handle.join().expect("stderr thread panicked");
        let responses = self.stdout_handle.join().expect("stdout thread panicked");

        SessionResult {
            responses,
            stderr_lines,
            status,
        }
    }
}

/// Get the path to the server binary
#[must_use]
pub fn server_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap();
    cwd.join("target/debug/assert-lsp")
}

/// Assert the server binary exists
pub fn assert_server_exists(path: &PathBuf) {
    assert!(
        path.exists(),
        "Server binary not found at {path:?}. Run `cargo build` first."
    );
}

/// Create a temporary directory for testing
#[must_use]
pub fn create_temp_dir(name: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");
    temp_dir
}

/// Clean up a temporary directory
pub fn cleanup_temp_dir(path: &PathBuf) {
    let _ = fs::remove_dir_all(path);
}

/// Builder for creating test projects
pub struct TestProject {
    path: PathBuf,
}

impl TestProject {
    /// Create a new test project in a temporary directory
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            path: create_temp_dir(name),
        }
    }

    /// Add a Cargo.toml for a Rust project
    #[must_use]
    pub fn with_cargo_toml(self) -> Self {
        let cargo_toml = r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(self.path.join("Cargo.toml"), cargo_toml).expect("Failed to write Cargo.toml");
        self
    }

    /// Add a lib.rs with a passing and failing test
    #[must_use]
    pub fn with_failing_test(self) -> Self {
        fs::create_dir_all(self.path.join("src")).expect("Failed to create src dir");
        let lib_rs = r#"pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_passes() {
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    fn test_add_fails() {
        assert_eq!(add(2, 2), 5, "Expected 5 but got 4");
    }
}
"#;
        fs::write(self.path.join("src/lib.rs"), lib_rs).expect("Failed to write lib.rs");
        self
    }

    /// Add a .assert-lsp.toml config file
    #[must_use]
    #[allow(dead_code)]
    pub fn with_config(self, config: &str) -> Self {
        fs::write(self.path.join(".assert-lsp.toml"), config).expect("Failed to write config");
        self
    }

    /// Add the default Rust adapter config
    #[must_use]
    #[allow(dead_code)]
    pub fn with_rust_config(self) -> Self {
        self.with_config(
            r#"[adapter_command.rust]
test_kind = "cargo-test"
extra_arg = []
include = ["/**/*.rs"]
exclude = ["/target/**/*"]
"#,
        )
    }

    /// Get the project path
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the file:// URI for this project
    #[must_use]
    pub fn uri(&self) -> String {
        format!("file://{}", self.path.to_string_lossy())
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        cleanup_temp_dir(&self.path);
    }
}

/// Run a complete LSP session and return the result
pub fn run_session(project: &TestProject, wait_secs: u64) -> SessionResult {
    let server = server_path();
    assert_server_exists(&server);

    let mut client = LspClient::new(&server);
    client.initialize(&project.uri());

    thread::sleep(Duration::from_secs(wait_secs));

    client.shutdown_and_exit();
    client.wait_for_completion()
}
