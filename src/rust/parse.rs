use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use regex::Regex;
use serde::Deserialize;

use crate::{Diagnostics, FileDiagnostics, MAX_CHAR_LENGTH, TestItem};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LibtestEvent {
    Suite(()),
    Test(TestEvent),
    Bench(()),
}

#[derive(Debug, Deserialize)]
struct TestEvent {
    event: String,
    name: String,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Extract panic location and message from test stdout.
fn extract_panic_location(
    stdout: &str,
    workspace_root: &Path,
) -> (Option<String>, u32, u32, String) {
    let re = Regex::new(r"panicked at ([^:]+):(\d+):(\d+):").unwrap();

    if let Some(caps) = re.captures(stdout) {
        let relative_path = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let line: u32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let col: u32 = caps
            .get(3)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);

        let absolute_path = workspace_root.join(relative_path);
        let file_path = absolute_path
            .exists()
            .then(|| absolute_path.to_string_lossy().to_string());

        let message = stdout
            .find(":\n")
            .map(|pos| stdout[pos + 2..].trim().to_string())
            .unwrap_or_default();

        (file_path, line, col, message)
    } else {
        (None, 1, 1, stdout.to_string())
    }
}

/// Parse cargo nextest text output (from stderr)
pub fn parse_nextest_output(
    contents: &str,
    workspace_root: PathBuf,
    file_paths: &[String],
    test_items: &[TestItem],
) -> Diagnostics {
    let contents = contents.replace("\r\n", "\n");
    let lines: Vec<&str> = contents.lines().collect();
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();

    let panic_re = Regex::new(r"thread '([^']+)' panicked at ([^:]+):(\d+):(\d+):").unwrap();

    for (i, line) in lines.iter().enumerate() {
        if let Some(m) = panic_re.captures(line) {
            let id_with_file = m.get(1).unwrap().as_str().to_string();
            let relative_file_path = m.get(2).unwrap().as_str().to_string();
            let lnum = m
                .get(3)
                .unwrap()
                .as_str()
                .parse::<u32>()
                .unwrap_or(1)
                .saturating_sub(1);
            let col = m
                .get(4)
                .unwrap()
                .as_str()
                .parse::<u32>()
                .unwrap_or(1)
                .saturating_sub(1);

            // Collect message from subsequent lines until empty line
            let mut message = String::new();
            let mut next_i = i + 1;
            while next_i < lines.len() && !lines[next_i].is_empty() {
                message.push_str(lines[next_i]);
                message.push('\n');
                next_i += 1;
            }

            let absolute_path = workspace_root.join(&relative_file_path);
            let file_path = file_paths
                .iter()
                .find(|p| p.contains(absolute_path.to_str().unwrap_or("")));

            if let Some(file_path) = file_path {
                // Find matching test item
                let matched_test_item = test_items.iter().find(|item| {
                    id_with_file.ends_with(&item.id) || item.id.ends_with(&id_with_file)
                });

                let diagnostic = Diagnostic {
                    range: Range {
                        start: Position {
                            line: lnum,
                            character: col,
                        },
                        end: Position {
                            line: lnum,
                            character: MAX_CHAR_LENGTH,
                        },
                    },
                    message: message.clone(),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("cargo-nextest".to_string()),
                    ..Diagnostic::default()
                };

                // Add diagnostic at panic location
                result_map
                    .entry(file_path.to_string())
                    .or_default()
                    .push(diagnostic);

                // Also add diagnostic at test definition if found
                if let Some(test_item) = matched_test_item {
                    let test_diagnostic = Diagnostic {
                        range: test_item.start_position,
                        message: format!(
                            "`{}` failed at {}:{}:{}\n{}",
                            test_item.name,
                            relative_file_path,
                            lnum + 1,
                            col + 1,
                            message
                        ),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some("cargo-nextest".to_string()),
                        ..Diagnostic::default()
                    };
                    result_map
                        .entry(test_item.path.clone())
                        .or_default()
                        .push(test_diagnostic);
                }
            }
        }
    }

    Diagnostics {
        files: result_map
            .into_iter()
            .map(|(path, diagnostics)| FileDiagnostics { path, diagnostics })
            .collect(),
        messages: vec![],
    }
}

/// Parse libtest JSON format output from `cargo test -- -Z unstable-options
/// --format json`
pub fn parse_libtest_json(
    json_output: &str,
    workspace_root: PathBuf,
    file_paths: &[String],
    test_items: &[TestItem],
) -> Diagnostics {
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();

    for line in json_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let event: LibtestEvent = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                log::debug!("Failed to parse libtest JSON: {}, error: {}", line, e);
                continue;
            }
        };

        if let LibtestEvent::Test(test_event) = event {
            if test_event.event != "failed" {
                continue;
            }

            let test_name = &test_event.name;
            let stdout = test_event.stdout.unwrap_or_default();
            let message = test_event.message.unwrap_or_default();

            let Some(test_item) = test_items
                .iter()
                .find(|item| item.id == *test_name || item.name == *test_name)
            else {
                log::warn!("Could not find test item for failed test: {}", test_name);
                continue;
            };

            let (panic_file, panic_line, panic_col, panic_message) =
                extract_panic_location(&stdout, &workspace_root);

            // Build diagnostic message with short test name
            let base_message = if !panic_message.is_empty() {
                panic_message.clone()
            } else if !message.is_empty() {
                message
            } else {
                "test failed".to_string()
            };
            let short_name = test_name.rsplit("::").next().unwrap_or(test_name);
            let diagnostic_message = format!("[{}] {}", short_name, base_message);

            // Related information pointing to test definition
            let related_info = lsp_types::DiagnosticRelatedInformation {
                location: lsp_types::Location {
                    uri: lsp_types::Url::from_file_path(&test_item.path)
                        .unwrap_or_else(|_| lsp_types::Url::parse("file:///unknown").unwrap()),
                    range: test_item.start_position,
                },
                message: format!("test `{}` defined here", test_name),
            };

            // Determine primary diagnostic location
            let (primary_file, primary_range) = if let Some(ref pf) = panic_file {
                (
                    pf.clone(),
                    Range {
                        start: Position {
                            line: panic_line.saturating_sub(1),
                            character: panic_col.saturating_sub(1),
                        },
                        end: Position {
                            line: panic_line.saturating_sub(1),
                            character: MAX_CHAR_LENGTH,
                        },
                    },
                )
            } else {
                (test_item.path.clone(), test_item.start_position)
            };

            let diagnostic = Diagnostic {
                range: primary_range,
                message: diagnostic_message,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("cargo-test".to_string()),
                related_information: Some(vec![related_info]),
                ..Diagnostic::default()
            };

            // Find target file and deduplicate
            let target_file = file_paths
                .iter()
                .find(|p| p.contains(&primary_file) || primary_file.contains(*p))
                .cloned()
                .unwrap_or_else(|| test_item.path.clone());

            let diagnostics = result_map.entry(target_file).or_default();
            if !diagnostics
                .iter()
                .any(|d| d.range == diagnostic.range && d.message == diagnostic.message)
            {
                diagnostics.push(diagnostic);
            }
        }
    }

    Diagnostics {
        files: result_map
            .into_iter()
            .map(|(path, diagnostics)| FileDiagnostics { path, diagnostics })
            .collect(),
        messages: vec![],
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_parse_libtest_json() {
        let fixture = r#"{"type":"suite","event":"started","test_count":1}
{"type":"test","event":"started","name":"rocks::dependency::tests::parse_dependency"}
{"type":"test","name":"rocks::dependency::tests::parse_dependency","event":"failed","stdout":"thread 'rocks::dependency::tests::parse_dependency' panicked at rocks-lib/src/rocks/dependency.rs:86:64:\ncalled `Result::unwrap()` on an `Err` value: unexpected end of input\n","message":"panicked"}
{"type":"suite","event":"failed","passed":0,"failed":1,"ignored":0,"measured":0,"filtered_out":0}"#;

        let file_paths =
            vec!["/home/example/projects/rocks-lib/src/rocks/dependency.rs".to_string()];
        let test_items = vec![TestItem {
            id: "rocks::dependency::tests::parse_dependency".to_string(),
            name: "rocks::dependency::tests::parse_dependency".to_string(),
            path: "/home/example/projects/rocks-lib/src/rocks/dependency.rs".to_string(),
            start_position: Range {
                start: Position {
                    line: 85,
                    character: 63,
                },
                end: Position {
                    line: 85,
                    character: MAX_CHAR_LENGTH,
                },
            },
            end_position: Range {
                start: Position {
                    line: 85,
                    character: 63,
                },
                end: Position {
                    line: 85,
                    character: MAX_CHAR_LENGTH,
                },
            },
        }];

        let diagnostics = parse_libtest_json(
            fixture,
            PathBuf::from_str("/home/example/projects").unwrap(),
            &file_paths,
            &test_items,
        );

        assert_eq!(diagnostics.files.len(), 1);
        assert_eq!(diagnostics.files[0].diagnostics.len(), 1);
        assert_eq!(
            diagnostics.files[0].diagnostics[0].source,
            Some("cargo-test".to_string())
        );
    }
}
