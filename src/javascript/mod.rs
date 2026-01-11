pub mod call;
pub mod parse;

use std::{collections::HashSet, path::PathBuf, str::FromStr};

use lsp_types::{Position, Range};
use tree_sitter::{Language, Point, Query, QueryCursor};

use crate::{
    Diagnostics, DiscoveredTests, FileDiagnostics, FileTests, MAX_CHAR_LENGTH, TestItem,
    Workspaces, error::LSError, runner::Runner,
};

const DISCOVER_JEST_QUERY: &str = include_str!("discover_jest.scm");
const DISCOVER_DENO_QUERY: &str = include_str!("discover_deno.scm");
const DISCOVER_NODE_TEST_QUERY: &str = include_str!("discover_node_test.scm");

fn discover_with_treesitter(
    file_path: &str,
    language: &Language,
    query: &str,
) -> Result<Vec<TestItem>, LSError> {
    let mut parser = tree_sitter::Parser::new();
    let mut test_items: Vec<TestItem> = vec![];
    parser.set_language(language)?;
    let source_code = std::fs::read_to_string(file_path)?;
    let tree = parser
        .parse(&source_code, None)
        .ok_or(LSError::TreeSitterParse)?;
    let query = Query::new(language, query)?;

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(tree.root_node().byte_range());
    let source = source_code.as_bytes();
    let matches = cursor.matches(&query, tree.root_node(), source);

    let mut namespace_name = String::new();
    let mut namespace_position_stack: Vec<(Point, Point)> = vec![];
    let mut test_id_set = HashSet::new();

    for m in matches {
        let mut test_start_position = Point::default();
        let mut test_end_position = Point::default();

        for capture in m.captures {
            let capture_name = query.capture_names()[capture.index as usize];
            let value = capture.node.utf8_text(source)?;
            let start_position = capture.node.start_position();
            let end_position = capture.node.end_position();

            match capture_name {
                "namespace.definition" => {
                    namespace_position_stack.push((start_position, end_position));
                }
                "namespace.name" => {
                    let current_namespace = namespace_position_stack.first();
                    if let Some((ns_start, ns_end)) = current_namespace {
                        if start_position.row >= ns_start.row
                            && end_position.row <= ns_end.row
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
                    if let Some((ns_start, ns_end)) = namespace_position_stack.first() {
                        if start_position.row < ns_start.row || end_position.row > ns_end.row {
                            namespace_position_stack.remove(0);
                            namespace_name = String::new();
                        }
                    }
                    test_start_position = start_position;
                    test_end_position = end_position;
                }
                "test.name" => {
                    let test_id = if namespace_name.is_empty() {
                        value.to_string()
                    } else {
                        format!("{}::{}", namespace_name, value)
                    };

                    if test_id_set.contains(&test_id) {
                        continue;
                    } else {
                        test_id_set.insert(test_id.clone());
                    }

                    let test_item = TestItem {
                        id: test_id.clone(),
                        name: test_id,
                        path: file_path.to_string(),
                        start_position: Range {
                            start: Position {
                                line: test_start_position.row as u32,
                                character: test_start_position.column as u32,
                            },
                            end: Position {
                                line: test_start_position.row as u32,
                                character: MAX_CHAR_LENGTH,
                            },
                        },
                        end_position: Range {
                            start: Position {
                                line: test_end_position.row as u32,
                                character: 0,
                            },
                            end: Position {
                                line: test_end_position.row as u32,
                                character: test_end_position.column as u32,
                            },
                        },
                    };
                    test_items.push(test_item);
                    test_start_position = Point::default();
                    test_end_position = Point::default();
                }
                _ => {}
            }
        }
    }

    Ok(test_items)
}

// --- Jest Runner ---

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct JestRunner;

impl Runner for JestRunner {
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError> {
        let language = tree_sitter_javascript::language();
        let mut files = Vec::new();

        for file_path in file_paths {
            let tests = discover_with_treesitter(file_path, &language, DISCOVER_JEST_QUERY)?;
            files.push(FileTests {
                tests,
                path: file_path.to_string(),
            });
        }
        Ok(DiscoveredTests { files })
    }

    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        _extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let (_, log_path) = call::run_jest(workspace)?;
        let test_result = std::fs::read_to_string(log_path)?;
        parse::parse_jest_json(&test_result, file_paths.to_vec())
    }

    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(file_paths, &["package.json"])
    }
}

// --- Vitest Runner ---

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct VitestRunner;

impl Runner for VitestRunner {
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError> {
        let language = tree_sitter_javascript::language();
        let mut files = Vec::new();

        for file_path in file_paths {
            // Vitest uses the same query as Jest
            let tests = discover_with_treesitter(file_path, &language, DISCOVER_JEST_QUERY)?;
            files.push(FileTests {
                tests,
                path: file_path.to_string(),
            });
        }
        Ok(DiscoveredTests { files })
    }

    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        _extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let (_, log_path) = call::run_vitest(workspace)?;
        let test_result = std::fs::read_to_string(log_path)?;
        parse::parse_vitest_json(&test_result, file_paths.to_vec())
    }

    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(
            file_paths,
            &[
                "package.json",
                "vitest.config.ts",
                "vitest.config.js",
                "vite.config.ts",
                "vite.config.js",
                "vitest.config.mts",
                "vitest.config.mjs",
                "vite.config.mts",
                "vite.config.mjs",
            ],
        )
    }
}

// --- Deno Runner ---

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct DenoRunner;

impl Runner for DenoRunner {
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError> {
        let language = tree_sitter_javascript::language();
        let mut files = Vec::new();

        for file_path in file_paths {
            let tests = discover_with_treesitter(file_path, &language, DISCOVER_DENO_QUERY)?;
            files.push(FileTests {
                tests,
                path: file_path.to_string(),
            });
        }
        Ok(DiscoveredTests { files })
    }

    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        _extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let output = call::run_deno(workspace, file_paths)?;

        if output.stdout.is_empty() {
            return Err(LSError::AdapterError);
        }

        let test_result = String::from_utf8(output.stdout)?;
        parse::parse_deno_output(
            &test_result,
            PathBuf::from_str(workspace).unwrap(),
            file_paths,
        )
    }

    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(file_paths, &["deno.json"])
    }
}

// --- Node.js Test Runner ---

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct NodeTestRunner;

impl Runner for NodeTestRunner {
    fn discover(&self, file_paths: &[String]) -> Result<DiscoveredTests, LSError> {
        let language = tree_sitter_javascript::language();
        let mut files = Vec::new();

        for file_path in file_paths {
            let tests = discover_with_treesitter(file_path, &language, DISCOVER_NODE_TEST_QUERY)?;
            files.push(FileTests {
                tests,
                path: file_path.to_string(),
            });
        }
        Ok(DiscoveredTests { files })
    }

    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let output = call::run_node_test(workspace, file_paths, extra_args)?;

        if output.stdout.is_empty() && !output.stderr.is_empty() {
            return Err(LSError::AdapterError);
        }

        let stdout = String::from_utf8(output.stdout)?;
        let results = parse::parse_node_test_xml(&stdout, file_paths);
        let result_item: Vec<FileDiagnostics> = results.into_iter().map(|r| r.into()).collect();

        Ok(Diagnostics {
            files: result_item,
            messages: vec![],
        })
    }

    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(file_paths, &["package.json"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_jest() {
        let file_path = "demo/jest/index.spec.js";
        let language = tree_sitter_javascript::language();
        let test_items =
            discover_with_treesitter(file_path, &language, DISCOVER_JEST_QUERY).unwrap();
        assert!(!test_items.is_empty());
    }

    #[test]
    fn test_discover_vitest() {
        let file_path = "demo/vitest/basic.test.ts";
        let language = tree_sitter_javascript::language();
        let test_items =
            discover_with_treesitter(file_path, &language, DISCOVER_JEST_QUERY).unwrap();
        assert!(!test_items.is_empty());
    }

    #[test]
    fn test_discover_deno() {
        let file_path = "demo/deno/main_test.ts";
        let language = tree_sitter_javascript::language();
        let test_items =
            discover_with_treesitter(file_path, &language, DISCOVER_DENO_QUERY).unwrap();
        assert!(!test_items.is_empty());
    }

    #[test]
    fn test_discover_node_test() {
        let file_path = "demo/node-test/index.test.js";
        let language = tree_sitter_javascript::language();
        let test_items =
            discover_with_treesitter(file_path, &language, DISCOVER_NODE_TEST_QUERY).unwrap();
        assert!(!test_items.is_empty());
    }
}
