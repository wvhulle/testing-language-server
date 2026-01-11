//! Integration test for the LSP server
//!
//! These tests spawn the server and send LSP messages to verify diagnostics work.
//! All tests use temporary directories with minimal test projects - no external dependencies.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

fn lsp_message(content: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", content.len(), content)
}

/// Creates a temporary Rust project with a failing test
fn create_temp_project() -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("assert-lsp-test-{}", std::process::id()));

    // Clean up if exists
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    // Create Cargo.toml
    let cargo_toml = r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#;
    fs::write(temp_dir.join("Cargo.toml"), cargo_toml).expect("Failed to write Cargo.toml");

    // Create src directory
    fs::create_dir_all(temp_dir.join("src")).expect("Failed to create src dir");

    // Create lib.rs with a failing test
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
        // This test intentionally fails
        assert_eq!(add(2, 2), 5, "Expected 5 but got 4");
    }
}
"#;
    fs::write(temp_dir.join("src/lib.rs"), lib_rs).expect("Failed to write lib.rs");

    // Create .assert-lsp.toml config
    let config = r#"enable_workspace_diagnostics = true

[adapter_command.rust]
test_kind = "cargo-test"
extra_arg = []
include = ["/**/*.rs"]
exclude = ["/target/**/*"]
"#;
    fs::write(temp_dir.join(".assert-lsp.toml"), config).expect("Failed to write config");

    temp_dir
}

fn cleanup_temp_project(path: &PathBuf) {
    let _ = fs::remove_dir_all(path);
}

struct LspClient {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout_handle: thread::JoinHandle<Vec<String>>,
    stderr_handle: thread::JoinHandle<Vec<String>>,
}

impl LspClient {
    fn new(server_path: &PathBuf) -> Self {
        let mut child = Command::new(server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUST_LOG", "info")
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

    fn send(&mut self, content: &str) {
        self.stdin
            .write_all(lsp_message(content).as_bytes())
            .unwrap();
        self.stdin.flush().unwrap();
    }

    fn initialize(&mut self, root_uri: &str) {
        let init = format!(
            r#"{{"jsonrpc":"2.0","id":0,"method":"initialize","params":{{"processId":{},"rootUri":"{}","capabilities":{{"textDocument":{{"publishDiagnostics":{{"relatedInformation":true}}}}}},"workspaceFolders":[{{"uri":"{}","name":"test-project"}}]}}}}"#,
            std::process::id(),
            root_uri,
            root_uri
        );
        println!("Sending initialize...");
        self.send(&init);
        thread::sleep(Duration::from_millis(500));

        println!("Sending initialized...");
        self.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#);
    }

    fn shutdown_and_exit(&mut self) {
        println!("Sending shutdown...");
        self.send(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":null}"#);
        thread::sleep(Duration::from_millis(200));

        println!("Sending exit...");
        self.send(r#"{"jsonrpc":"2.0","method":"exit","params":null}"#);
    }

    fn wait_for_completion(mut self) -> (Vec<String>, Vec<String>, std::process::ExitStatus) {
        drop(self.stdin);

        // Give the server a moment to exit gracefully
        thread::sleep(Duration::from_millis(500));

        // Try to kill the process if it's still running
        let _ = self.child.kill();

        let status = self.child.wait().expect("Failed to wait for child");
        let stderr_lines = self.stderr_handle.join().expect("stderr thread panicked");
        let responses = self.stdout_handle.join().expect("stdout thread panicked");

        (responses, stderr_lines, status)
    }
}

#[test]
fn test_lsp_server_detects_failing_test() {
    let cwd = std::env::current_dir().unwrap();
    let server_path = cwd.join("target/debug/assert-lsp");

    assert!(
        server_path.exists(),
        "Server binary not found at {server_path:?}. Run `cargo build` first."
    );

    // Create a temporary project with a failing test
    let test_project = create_temp_project();
    println!("Created test project at: {test_project:?}");

    let root_uri = format!("file://{}", test_project.to_string_lossy());

    let mut client = LspClient::new(&server_path);
    client.initialize(&root_uri);

    // Wait for tests to run and diagnostics to be published
    println!("Waiting for diagnostics...");
    thread::sleep(Duration::from_secs(5));

    client.shutdown_and_exit();

    let (responses, stderr_lines, status) = client.wait_for_completion();

    // Cleanup
    cleanup_temp_project(&test_project);

    println!("\n=== Summary ===");
    println!("Server exited with status: {status:?}");
    println!("Stderr lines: {}", stderr_lines.len());
    println!("Responses: {}", responses.len());

    // Find diagnostic responses
    let diagnostic_responses: Vec<_> = responses
        .iter()
        .filter(|r| r.contains("publishDiagnostics"))
        .collect();

    println!("Diagnostic responses: {}", diagnostic_responses.len());

    // Check for the failing test diagnostic
    let has_failing_test_diagnostic = diagnostic_responses.iter().any(|r| {
        r.contains("test_add_fails") || r.contains("Expected 5 but got 4") || r.contains("FAILED")
    });

    // Print all diagnostic responses for debugging
    for resp in &diagnostic_responses {
        println!("\nDiagnostic: {resp}");
    }

    // Check stderr for test failure detection
    let found_failed_test = stderr_lines
        .iter()
        .any(|l| l.contains("test_add_fails") || l.contains("FAILED"));
    println!("\nFound failed test in logs: {found_failed_test}");
    println!("Has failing test diagnostic: {has_failing_test_diagnostic}");

    // The test should detect the failing test
    assert!(
        has_failing_test_diagnostic || found_failed_test,
        "Expected to find diagnostic for failing test 'test_add_fails'"
    );
}

#[test]
fn test_lsp_server_protocol_flow() {
    // Simple test to verify basic protocol flow works
    let cwd = std::env::current_dir().unwrap();
    let server_path = cwd.join("target/debug/assert-lsp");

    assert!(
        server_path.exists(),
        "Server binary not found at {server_path:?}. Run `cargo build` first."
    );

    // Use an empty temp dir - no tests to run
    let test_dir = std::env::temp_dir().join(format!("assert-lsp-empty-{}", std::process::id()));
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).expect("Failed to create temp dir");

    let root_uri = format!("file://{}", test_dir.to_string_lossy());

    let mut client = LspClient::new(&server_path);
    client.initialize(&root_uri);

    thread::sleep(Duration::from_secs(1));

    client.shutdown_and_exit();

    let (responses, _stderr_lines, status) = client.wait_for_completion();

    // Cleanup
    let _ = fs::remove_dir_all(&test_dir);

    // Check that we got an initialize response
    let has_init_response = responses
        .iter()
        .any(|r| r.contains("capabilities") && r.contains("result"));

    assert!(has_init_response, "Expected initialize response");
    println!("Server exited with status: {status:?}");
}
