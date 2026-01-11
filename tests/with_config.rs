//! Integration tests for projects WITH a .assert-lsp.toml config file
//!
//! These tests verify that the server correctly reads and uses explicit
//! configuration from the .assert-lsp.toml file.

mod client;

use client::{run_session, TestProject};

#[test]
fn test_with_config_detects_failing_test() {
    let project = TestProject::new("with-config-rust")
        .with_cargo_toml()
        .with_failing_test()
        .with_rust_config();

    println!("Created test project at: {}", project.path().display());

    let result = run_session(&project, 5);
    result.print_summary();

    // Basic protocol assertions
    result.assert_initialized();
    result.assert_diagnostics_ran();

    // Diagnostic count
    result.assert_has_diagnostics();
    result.assert_diagnostic_count(1);

    // Diagnostic content
    result.assert_diagnostic_for_test("test_add_fails");
    result.assert_diagnostic_message_contains("Expected 5 but got 4");

    // Diagnostic metadata
    result.assert_diagnostic_source("cargo-test");
    result.assert_diagnostic_is_error();

    // Diagnostic location
    result.assert_diagnostic_at_line(15);
    result.assert_diagnostic_uri_contains("lib.rs");
    result.assert_diagnostic_has_valid_range();

    // Combined field check
    result.assert_diagnostic_fields("cargo-test", 1, 15);

    // Check all fields of first diagnostic
    let diag = result.first_diagnostic().expect("Expected at least one diagnostic");
    assert!(diag.uri.ends_with("lib.rs"), "URI should end with lib.rs: {}", diag.uri);
    assert_eq!(diag.severity, Some(1), "Severity should be Error (1)");
    assert_eq!(diag.source, Some("cargo-test".to_string()), "Source should be cargo-test");
    assert_eq!(diag.start_line, 15, "Start line should be 15");
    assert!(diag.message.contains("test_add_fails"), "Message should contain test name");
}

#[test]
fn test_protocol_flow_with_config() {
    let project = TestProject::new("protocol-flow")
        .with_cargo_toml()
        .with_rust_config();

    let result = run_session(&project, 2);
    result.print_summary();

    result.assert_initialized();
    result.assert_diagnostics_ran();
    // No test files, so no diagnostics expected
    result.assert_no_diagnostics();
}
