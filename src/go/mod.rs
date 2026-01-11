pub mod call;
pub mod parse;

use std::{path::PathBuf, str::FromStr};

use lsp_types::{Position, Range};
use tree_sitter::{Query, QueryCursor};

use crate::{
    Diagnostics, DiscoveredTests, FileTests, MAX_CHAR_LENGTH, TestItem, Workspaces, error::LSError,
    runner::Runner,
};

const DISCOVER_QUERY: &str = include_str!("discover.scm");

fn discover_tests(file_path: &str) -> Result<Vec<TestItem>, LSError> {
    let source_code = std::fs::read_to_string(file_path)?;
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_go::language();
    parser.set_language(&language)?;

    let tree = parser
        .parse(&source_code, None)
        .ok_or_else(|| LSError::TreeSitterParse)?;

    let query = Query::new(&language, DISCOVER_QUERY)?;
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    let mut tests = Vec::new();
    let name_idx = query
        .capture_index_for_name("test.name")
        .ok_or_else(|| LSError::TreeSitterParse)?;
    let def_idx = query
        .capture_index_for_name("test.definition")
        .ok_or_else(|| LSError::TreeSitterParse)?;

    for m in matches {
        let mut name: Option<String> = None;
        let mut start_point = None;
        let mut end_point = None;

        for capture in m.captures {
            if capture.index == name_idx {
                let text = capture.node.utf8_text(source_code.as_bytes()).unwrap_or("");
                // Remove quotes from string literals
                name = Some(text.trim_matches('"').to_string());
            }
            if capture.index == def_idx {
                start_point = Some(capture.node.start_position());
                end_point = Some(capture.node.end_position());
            }
        }

        if let (Some(test_name), Some(start), Some(end)) = (name, start_point, end_point) {
            tests.push(TestItem {
                id: test_name.clone(),
                name: test_name,
                path: file_path.to_string(),
                start_position: Range {
                    start: Position {
                        line: start.row as u32,
                        character: start.column as u32,
                    },
                    end: Position {
                        line: start.row as u32,
                        character: MAX_CHAR_LENGTH,
                    },
                },
                end_position: Range {
                    start: Position {
                        line: end.row as u32,
                        character: 0,
                    },
                    end: Position {
                        line: end.row as u32,
                        character: end.column as u32,
                    },
                },
            });
        }
    }

    Ok(tests)
}

#[derive(Eq, PartialEq, Hash, Debug)]
pub struct GoTestRunner;

impl Runner for GoTestRunner {
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
        let output = call::run_go_test(workspace, extra_args)?;

        if output.stdout.is_empty() && !output.stderr.is_empty() {
            return Err(LSError::AdapterError);
        }

        let json_output = String::from_utf8(output.stdout)?;
        parse::parse_go_test_json(
            &json_output,
            PathBuf::from_str(workspace).unwrap(),
            file_paths,
        )
    }

    #[tracing::instrument(skip(self))]
    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(file_paths, &["go.mod"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover() {
        let file_path = "demo/go/cases_test.go";
        let test_items = discover_tests(file_path).unwrap();
        assert!(!test_items.is_empty());
    }
}
