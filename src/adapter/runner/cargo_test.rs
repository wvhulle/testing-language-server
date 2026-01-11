use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Output;

use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use regex::Regex;
use serde::Deserialize;
use tree_sitter::{Point, Query, QueryCursor};

use crate::adapter::model::Runner;
use crate::adapter::runner::util::{
    detect_workspaces_from_file_list, send_stdout, write_result_log, MAX_CHAR_LENGTH,
};
use crate::error::LSError;
use crate::spec::{
    DetectWorkspaceResult, DiscoverResult, FileDiagnostics, FoundFileTests, RunFileTestResult,
    TestItem,
};

// --- Libtest JSON types ---

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LibtestEvent {
    Suite(SuiteEvent),
    Test(TestEvent),
    Bench(BenchEvent),
}

#[derive(Debug, Deserialize)]
struct SuiteEvent {
    #[allow(dead_code)]
    event: String,
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

#[derive(Debug, Deserialize)]
struct BenchEvent {
    #[allow(dead_code)]
    name: String,
}

// --- Rust module path helpers ---

/// Convert a file path to its Rust module path.
/// e.g., "src/rules/side_effects/mod.rs" -> "rules::side_effects"
/// e.g., "src/rules/side_effects/detect_bad.rs" -> "rules::side_effects::detect_bad"
fn file_path_to_module_path(file_path: &str) -> String {
    let path = Path::new(file_path);
    let components: Vec<_> = path.components().collect();

    let src_idx = components.iter().position(|c| {
        matches!(c, std::path::Component::Normal(s) if s.to_str() == Some("src"))
    });

    let relevant = match src_idx {
        Some(idx) => &components[idx + 1..],
        None => &components[..],
    };

    relevant
        .iter()
        .filter_map(|c| {
            if let std::path::Component::Normal(s) = c {
                let s = s.to_str()?;
                let s = s.strip_suffix(".rs").unwrap_or(s);
                match s {
                    "lib" | "main" | "mod" => None,
                    _ => Some(s.to_string()),
                }
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

// --- Test discovery ---

/// Discover Rust tests in a file using tree-sitter.
fn discover_rust_tests(file_path: &str) -> Result<Vec<TestItem>, LSError> {
    // Query from https://github.com/rouge8/neotest-rust
    let query_str = r#"
        (
  (attribute_item
    [
      (attribute (identifier) @macro_name)
      (attribute
        [
          (identifier) @macro_name
          (scoped_identifier name: (identifier) @macro_name)
        ]
      )
    ]
  )
  [(attribute_item (attribute (identifier))) (line_comment)]*
  .
  (function_item name: (identifier) @test.name) @test.definition
  (#any-of? @macro_name "test" "rstest" "case")
)
(mod_item name: (identifier) @namespace.name)? @namespace.definition
"#;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .expect("Error loading Rust grammar");

    let source_code = std::fs::read_to_string(file_path)?;
    let tree = parser.parse(&source_code, None).unwrap();
    let query = Query::new(&tree_sitter_rust::language(), query_str).expect("Error creating query");

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(tree.root_node().byte_range());
    let source = source_code.as_bytes();
    let matches = cursor.matches(&query, tree.root_node(), source);

    let file_module = file_path_to_module_path(file_path);
    let mut namespace_name = String::new();
    let mut namespace_stack: Vec<(Point, Point)> = vec![];
    let mut test_id_set = HashSet::new();
    let mut test_items = Vec::new();
    let mut test_start = Point::default();
    let mut test_end = Point::default();

    for m in matches {
        for capture in m.captures {
            let name = query.capture_names()[capture.index as usize];
            let value = capture.node.utf8_text(source)?;
            let start = capture.node.start_position();
            let end = capture.node.end_position();

            match name {
                "namespace.definition" => namespace_stack.push((start, end)),
                "namespace.name" => {
                    if let Some((ns_start, ns_end)) = namespace_stack.first() {
                        if start.row >= ns_start.row && end.row <= ns_end.row && !namespace_name.is_empty() {
                            namespace_name = format!("{}::{}", namespace_name, value);
                        } else {
                            namespace_name = value.to_string();
                        }
                    } else {
                        namespace_name = value.to_string();
                    }
                }
                "test.definition" => {
                    if let Some((ns_start, ns_end)) = namespace_stack.first() {
                        if start.row < ns_start.row || end.row > ns_end.row {
                            namespace_stack.remove(0);
                            namespace_name.clear();
                        }
                    }
                    test_start = start;
                    test_end = end;
                }
                "test.name" => {
                    let local_id = if namespace_name.is_empty() {
                        value.to_string()
                    } else {
                        format!("{}::{}", namespace_name, value)
                    };

                    let test_id = if file_module.is_empty() {
                        local_id
                    } else {
                        format!("{}::{}", file_module, local_id)
                    };

                    if test_id_set.insert(test_id.clone()) {
                        test_items.push(TestItem {
                            id: test_id.clone(),
                            name: test_id,
                            path: file_path.to_string(),
                            start_position: Range {
                                start: Position {
                                    line: test_start.row as u32,
                                    character: test_start.column as u32,
                                },
                                end: Position {
                                    line: test_start.row as u32,
                                    character: MAX_CHAR_LENGTH,
                                },
                            },
                            end_position: Range {
                                start: Position {
                                    line: test_end.row as u32,
                                    character: 0,
                                },
                                end: Position {
                                    line: test_end.row as u32,
                                    character: test_end.column as u32,
                                },
                            },
                        });
                    }
                    test_start = Point::default();
                    test_end = Point::default();
                }
                _ => {}
            }
        }
    }

    Ok(test_items)
}

// --- Panic location extraction ---

/// Extract panic location and message from test stdout.
fn extract_panic_location(stdout: &str, workspace_root: &Path) -> (Option<String>, u32, u32, String) {
    let re = Regex::new(r"panicked at ([^:]+):(\d+):(\d+):").unwrap();

    if let Some(caps) = re.captures(stdout) {
        let relative_path = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let line: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
        let col: u32 = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);

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

// --- Libtest JSON parsing ---

/// Parse libtest JSON format output from `cargo test -- -Z unstable-options --format json`
fn parse_libtest_json(
    json_output: &str,
    workspace_root: PathBuf,
    file_paths: &[String],
    test_items: &[TestItem],
) -> RunFileTestResult {
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();

    for line in json_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let event: LibtestEvent = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Failed to parse libtest JSON: {}, error: {}", line, e);
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
                tracing::warn!("Could not find test item for failed test: {}", test_name);
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

    RunFileTestResult {
        data: result_map
            .into_iter()
            .map(|(path, diagnostics)| FileDiagnostics { path, diagnostics })
            .collect(),
        messages: vec![],
    }
}

// --- Runner implementation ---

fn detect_workspaces(file_paths: &[String]) -> DetectWorkspaceResult {
    detect_workspaces_from_file_list(file_paths, &["Cargo.toml".to_string()])
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct CargoTestRunner;

impl Runner for CargoTestRunner {
    #[tracing::instrument(skip(self))]
    fn discover(&self, args: crate::spec::DiscoverArgs) -> Result<(), LSError> {
        let mut results = DiscoverResult { data: vec![] };
        for file_path in args.file_paths {
            let tests = discover_rust_tests(&file_path)?;
            results.data.push(FoundFileTests {
                tests,
                path: file_path,
            });
        }
        send_stdout(&results)?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn run_file_test(&self, args: crate::spec::RunFileTestArgs) -> Result<(), LSError> {
        let discovered_tests: Vec<TestItem> = args
            .file_paths
            .iter()
            .filter_map(|path| discover_rust_tests(path).ok())
            .flatten()
            .collect();

        let test_ids: Vec<String> = discovered_tests.iter().map(|t| t.id.clone()).collect();

        let output = std::process::Command::new("cargo")
            .current_dir(&args.workspace)
            .arg("test")
            .args(&args.extra)
            .arg("--")
            .arg("-Z")
            .arg("unstable-options")
            .arg("--format")
            .arg("json")
            .args(&test_ids)
            .output()?;

        write_result_log("cargo_test.log", &output)?;

        let Output { stdout, stderr, .. } = output;
        let json_output = String::from_utf8(stdout)?;

        if !stderr.is_empty() {
            tracing::debug!("cargo test stderr: {}", String::from_utf8_lossy(&stderr));
        }

        let diagnostics = parse_libtest_json(
            &json_output,
            PathBuf::from(&args.workspace),
            &args.file_paths,
            &discovered_tests,
        );
        send_stdout(&diagnostics)?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn detect_workspaces(&self, args: crate::spec::DetectWorkspaceArgs) -> Result<(), LSError> {
        send_stdout(&detect_workspaces(&args.file_paths))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_file_path_to_module_path() {
        assert_eq!(
            file_path_to_module_path("src/rules/side_effects/detect_bad.rs"),
            "rules::side_effects::detect_bad"
        );
        assert_eq!(file_path_to_module_path("src/lib.rs"), "");
        assert_eq!(
            file_path_to_module_path("src/rules/mod.rs"),
            "rules"
        );
    }

    #[test]
    fn test_parse_libtest_json() {
        let fixture = r#"{"type":"suite","event":"started","test_count":1}
{"type":"test","event":"started","name":"rocks::dependency::tests::parse_dependency"}
{"type":"test","name":"rocks::dependency::tests::parse_dependency","event":"failed","stdout":"thread 'rocks::dependency::tests::parse_dependency' panicked at rocks-lib/src/rocks/dependency.rs:86:64:\ncalled `Result::unwrap()` on an `Err` value: unexpected end of input\n","message":"panicked"}
{"type":"suite","event":"failed","passed":0,"failed":1,"ignored":0,"measured":0,"filtered_out":0}"#;

        let file_paths = vec!["/home/example/projects/rocks-lib/src/rocks/dependency.rs".to_string()];
        let test_items = vec![TestItem {
            id: "rocks::dependency::tests::parse_dependency".to_string(),
            name: "rocks::dependency::tests::parse_dependency".to_string(),
            path: "/home/example/projects/rocks-lib/src/rocks/dependency.rs".to_string(),
            start_position: Range {
                start: Position { line: 85, character: 63 },
                end: Position { line: 85, character: MAX_CHAR_LENGTH },
            },
            end_position: Range {
                start: Position { line: 85, character: 63 },
                end: Position { line: 85, character: MAX_CHAR_LENGTH },
            },
        }];

        let diagnostics = parse_libtest_json(
            fixture,
            PathBuf::from_str("/home/example/projects").unwrap(),
            &file_paths,
            &test_items,
        );

        assert_eq!(diagnostics.data.len(), 1);
        assert_eq!(diagnostics.data[0].diagnostics.len(), 1);
        assert_eq!(diagnostics.data[0].diagnostics[0].source, Some("cargo-test".to_string()));
    }
}
