mod call;
mod parse;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use lsp_types::{Position, Range};
use tree_sitter::{Point, Query, QueryCursor};

use crate::{
    Diagnostics, DiscoveredTests, FileTests, MAX_CHAR_LENGTH, TestItem, Workspaces, error::LSError,
    runner::Runner,
};

const DISCOVER_QUERY: &str = include_str!("discover.scm");

/// Convert a file path to its Rust module path.
/// e.g., "src/rules/side_effects/mod.rs" -> "rules::side_effects"
/// e.g., "src/rules/side_effects/detect_bad.rs" ->
/// "rules::side_effects::detect_bad"
fn file_path_to_module_path(file_path: &str) -> String {
    let path = Path::new(file_path);
    let components: Vec<_> = path.components().collect();

    let src_idx = components
        .iter()
        .position(|c| matches!(c, std::path::Component::Normal(s) if s.to_str() == Some("src")));

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

/// Discover Rust tests in a file using tree-sitter.
fn discover_tests(file_path: &str) -> Result<Vec<TestItem>, LSError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .expect("Error loading Rust grammar");

    let source_code = std::fs::read_to_string(file_path)?;
    let tree = parser.parse(&source_code, None).unwrap();
    let query =
        Query::new(&tree_sitter_rust::language(), DISCOVER_QUERY).expect("Error creating query");

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
                        if start.row >= ns_start.row
                            && end.row <= ns_end.row
                            && !namespace_name.is_empty()
                        {
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

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct CargoTestRunner;

impl Runner for CargoTestRunner {
    #[tracing::instrument(skip(self))]
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError> {
        let mut files = Vec::new();
        for file_path in file_paths {
            let tests = discover_tests(file_path)?;
            files.push(FileTests {
                tests,
                path: file_path.to_string(),
            });
        }
        Ok(DiscoveredTests { files })
    }

    #[tracing::instrument(skip(self))]
    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let discovered_tests: Vec<TestItem> = file_paths
            .iter()
            .filter_map(|path| discover_tests(path).ok())
            .flatten()
            .collect();

        let test_ids: Vec<String> = discovered_tests.iter().map(|t| t.id.clone()).collect();

        let output = call::run_cargo_test(workspace, extra_args, &test_ids)?;
        let json_output = String::from_utf8(output.stdout)?;

        Ok(parse::parse_libtest_json(
            &json_output,
            PathBuf::from(workspace),
            file_paths,
            &discovered_tests,
        ))
    }

    #[tracing::instrument(skip(self))]
    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(file_paths, &["Cargo.toml"])
    }
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct CargoNextestRunner;

impl Runner for CargoNextestRunner {
    #[tracing::instrument(skip(self))]
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError> {
        // Nextest uses the same test discovery as cargo test
        let mut files = Vec::new();
        for file_path in file_paths {
            let tests = discover_tests(file_path)?;
            files.push(FileTests {
                tests,
                path: file_path.to_string(),
            });
        }
        Ok(DiscoveredTests { files })
    }

    #[tracing::instrument(skip(self))]
    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let discovered_tests: Vec<TestItem> = file_paths
            .iter()
            .filter_map(|path| discover_tests(path).ok())
            .flatten()
            .collect();

        let test_ids: Vec<String> = discovered_tests.iter().map(|t| t.id.clone()).collect();

        let output = call::run_cargo_nextest(workspace, extra_args, &test_ids)?;

        // Nextest outputs to stderr, and status code 100 means tests failed (not an
        // error)
        let stderr_output = String::from_utf8(output.stderr)?;
        let unexpected_status = output.status.code().map(|code| code != 100 && code != 0);

        if output.stdout.is_empty()
            && !stderr_output.is_empty()
            && unexpected_status.unwrap_or(false)
        {
            return Err(LSError::AdapterError);
        }

        Ok(parse::parse_nextest_output(
            &stderr_output,
            PathBuf::from(workspace),
            file_paths,
            &discovered_tests,
        ))
    }

    #[tracing::instrument(skip(self))]
    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(file_paths, &["Cargo.toml"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_path_to_module_path() {
        assert_eq!(
            file_path_to_module_path("src/rules/side_effects/detect_bad.rs"),
            "rules::side_effects::detect_bad"
        );
        assert_eq!(file_path_to_module_path("src/lib.rs"), "");
        assert_eq!(file_path_to_module_path("src/rules/mod.rs"), "rules");
    }
}
