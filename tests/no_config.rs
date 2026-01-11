//! Integration tests for projects WITHOUT a .assert-lsp.toml config file
//!
//! These tests verify that the server correctly auto-detects project types
//! and runs diagnostics without requiring explicit configuration.

mod client;

use client::{run_session, TestProject};

#[test]
fn test_auto_detect_rust_project() {
    let project = TestProject::new("no-config-rust")
        .with_cargo_toml()
        .with_failing_test();

    println!("Created test project at: {}", project.path().display());

    let result = run_session(&project, 10);
    result.print_summary();

    // Basic protocol assertions
    result.assert_initialized();
    result.assert_auto_detected();
    result.assert_diagnostics_ran();

    // Diagnostic assertions
    result.assert_has_diagnostics();
    result.assert_diagnostic_count(1);
    result.assert_diagnostic_for_test("test_add_fails");
    result.assert_diagnostic_source("cargo-test");
    result.assert_diagnostic_is_error();
    result.assert_diagnostic_at_line(15); // Line where assert_eq! fails
    result.assert_diagnostic_message_contains("assertion");
}

#[test]
fn test_empty_directory_no_config() {
    let project = TestProject::new("no-config-empty");

    let result = run_session(&project, 1);
    result.print_summary();

    result.assert_initialized();
    result.assert_no_project_detected();
    result.assert_no_diagnostics();
}
