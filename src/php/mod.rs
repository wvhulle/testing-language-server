pub mod call;
pub mod parse;

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
    let language = tree_sitter_php::language_php();
    parser.set_language(&language)?;

    let tree = parser
        .parse(&source_code, None)
        .ok_or_else(|| LSError::TreeSitterParse)?;

    let query = Query::new(&language, DISCOVER_QUERY)?;
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    let mut tests = Vec::new();
    let name_idx = query.capture_index_for_name("test.name");
    let def_idx = query.capture_index_for_name("test.definition");

    for m in matches {
        let mut name: Option<String> = None;
        let mut start_point = None;
        let mut end_point = None;

        for capture in m.captures {
            if Some(capture.index) == name_idx {
                let text = capture.node.utf8_text(source_code.as_bytes()).unwrap_or("");
                name = Some(text.to_string());
            }
            if Some(capture.index) == def_idx {
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
pub struct PhpunitRunner;

impl Runner for PhpunitRunner {
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

    fn run_tests(
        &self,
        file_paths: &[String],
        workspace: &str,
        extra_args: &[String],
    ) -> Result<Diagnostics, LSError> {
        let filter_pattern = extra_args.first().map(|s| s.as_str()).unwrap_or(".*");

        let (_, log_path) = call::run_phpunit(workspace, file_paths, filter_pattern)?;

        let results = parse::parse_phpunit_xml(log_path.to_str().unwrap())?;
        Ok(parse::to_diagnostics(results))
    }

    fn detect_workspaces(&self, file_paths: &[String]) -> Workspaces {
        crate::workspace::detect_from_files(
            file_paths,
            &["phpunit.xml", "phpunit.xml.dist", "composer.json"],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover() {
        let file_path = "demo/phpunit/src/CalculatorTest.php";
        let test_items = discover_tests(file_path).unwrap();
        assert!(!test_items.is_empty());
    }
}
