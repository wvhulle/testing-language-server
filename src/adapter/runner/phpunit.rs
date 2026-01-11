use crate::error::LSError;
use crate::spec::{
    DetectWorkspaceResult, DiscoverResult, FileDiagnostics, FoundFileTests, RunFileTestResult,
    TestItem,
};
use std::fs::File;
use std::io::BufReader;
use std::process::Output;
use xml::reader::{ParserConfig, XmlEvent};

use crate::adapter::model::Runner;

use super::util::{
    detect_workspaces_from_file_list, discover_with_treesitter, send_stdout, ResultFromXml,
    LOG_LOCATION,
};

fn detect_workspaces(file_paths: Vec<String>) -> DetectWorkspaceResult {
    detect_workspaces_from_file_list(&file_paths, &["composer.json".to_string()])
}

fn parse_failure_characters(characters: &str) -> Option<ResultFromXml> {
    let mut split = characters.split("\n\n");
    let message = split
        .next()?
        .trim_start_matches("Failed asserting that ")
        .trim_end_matches(".")
        .to_string();
    let location = split.next()?;
    let mut parts = location.split(':');
    let path = parts.next()?.to_string();
    let line = parts.next()?.parse().ok()?;
    Some(ResultFromXml {
        message,
        path,
        line,
        col: 1,
    })
}

fn parse_xml_results(path: &str) -> Result<Vec<ResultFromXml>, LSError> {
    let file = File::open(path)?;
    let mut reader = ParserConfig::default()
        .ignore_root_level_whitespace(false)
        .create_reader(BufReader::new(file));

    let mut in_failure = false;
    let mut results = Vec::new();

    loop {
        match reader.next() {
            Ok(XmlEvent::StartElement { name, .. }) if name.local_name.starts_with("failure") => {
                in_failure = true;
            }
            Ok(XmlEvent::EndElement { .. }) => {
                in_failure = false;
            }
            Ok(XmlEvent::Characters(data)) if in_failure => {
                if let Some(result) = parse_failure_characters(&data) {
                    results.push(result);
                }
            }
            Ok(XmlEvent::EndDocument) => break,
            Err(e) => {
                tracing::error!("XML parse error: {e}");
                return Err(LSError::XmlParse);
            }
            _ => {}
        }
    }

    Ok(results)
}

fn discover(file_path: &str) -> Result<Vec<TestItem>, LSError> {
    // from https://github.com/olimorris/neotest-phpunit/blob/bbd79d95e927ccd16f0e1d765060058d34838e2e/lua/neotest-phpunit/init.lua#L111
    // license: https://github.com/olimorris/neotest-phpunit/blob/bbd79d95e927ccd16f0e1d765060058d34838e2e/LICENSE
    let query = r#"
    ((class_declaration
      name: (name) @namespace.name (#match? @namespace.name "Test")
    )) @namespace.definition

    ((method_declaration
      (attribute_list
        (attribute_group
            (attribute) @test_attribute (#match? @test_attribute "Test")
        )
      )
      (
        (visibility_modifier)
        (name) @test.name
      ) @test.definition
     ))

    ((method_declaration
      (name) @test.name (#match? @test.name "test")
    )) @test.definition

    (((comment) @test_comment (#match? @test_comment "\\@test") .
      (method_declaration
        (name) @test.name
      ) @test.definition
    ))
        "#;
    discover_with_treesitter(file_path, &tree_sitter_php::language_php(), query)
}

#[derive(Eq, PartialEq, Debug)]
pub struct PhpunitRunner;

impl Runner for PhpunitRunner {
    #[tracing::instrument(skip(self))]
    fn discover(&self, args: crate::spec::DiscoverArgs) -> Result<(), LSError> {
        let file_paths = args.file_paths;
        let mut discover_results: DiscoverResult = DiscoverResult { data: vec![] };
        for file_path in file_paths {
            discover_results.data.push(FoundFileTests {
                tests: discover(&file_path)?,
                path: file_path,
            })
        }
        send_stdout(&discover_results)?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn run_file_test(&self, args: crate::spec::RunFileTestArgs) -> Result<(), LSError> {
        let file_paths = args.file_paths;
        let workspace_root = args.workspace;
        let log_path = LOG_LOCATION.join("phpunit.xml");
        let tests = file_paths
            .iter()
            .map(|path| {
                discover(path).map(|test_items| {
                    test_items
                        .into_iter()
                        .map(|item| item.id)
                        .collect::<Vec<String>>()
                })
            })
            .filter_map(Result::ok)
            .flatten()
            .collect::<Vec<_>>();
        let test_names = tests.join("|");
        let filter_pattern = format!("/{test_names}/");
        let output = std::process::Command::new("phpunit")
            .current_dir(&workspace_root)
            .args([
                "--log-junit",
                log_path.to_str().unwrap(),
                "--filter",
                &filter_pattern,
            ])
            .args(file_paths)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();
        let Output { stdout, stderr, .. } = output;
        if stdout.is_empty() && !stderr.is_empty() {
            return Err(LSError::AdapterError);
        }
        let result_from_xml = parse_xml_results(log_path.to_str().unwrap())?;
        let result_item: Vec<FileDiagnostics> = result_from_xml
            .into_iter()
            .map(|result_from_xml| {
                let result_item: FileDiagnostics = result_from_xml.into();
                result_item
            })
            .collect();
        let result = RunFileTestResult {
            data: result_item,
            messages: vec![],
        };
        send_stdout(&result)?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn detect_workspaces(&self, args: crate::spec::DetectWorkspaceArgs) -> Result<(), LSError> {
        let file_paths = args.file_paths;
        let detect_result = detect_workspaces(file_paths);
        send_stdout(&detect_result)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{Position, Range};

    use crate::adapter::runner::util::MAX_CHAR_LENGTH;

    use super::*;

    #[test]
    fn parse_xml() {
        let mut path = std::env::current_dir().unwrap();
        path.push("../../demo/phpunit/output.xml");
        let result = parse_xml_results(path.to_str().unwrap()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].message,
            "Tests\\CalculatorTest::testFail1\nFailed asserting that 8 matches expected 1"
        );
        assert_eq!(
            result[0].path,
            "/home/kbwo/testing-language-server/demo/phpunit/src/CalculatorTest.php"
        );
        assert_eq!(result[0].line, 28);
    }

    #[test]
    fn test_discover() {
        let file_path = "../../demo/phpunit/src/CalculatorTest.php";
        let test_items = discover(file_path).unwrap();
        assert_eq!(test_items.len(), 3);
        assert_eq!(
            test_items,
            [
                TestItem {
                    id: "CalculatorTest::testAdd".to_string(),
                    name: "CalculatorTest::testAdd".to_string(),
                    path: file_path.to_string(),
                    start_position: Range {
                        start: Position {
                            line: 9,
                            character: 4
                        },
                        end: Position {
                            line: 9,
                            character: MAX_CHAR_LENGTH
                        }
                    },
                    end_position: Range {
                        start: Position {
                            line: 14,
                            character: 0
                        },
                        end: Position {
                            line: 14,
                            character: 5
                        }
                    }
                },
                TestItem {
                    id: "CalculatorTest::testSubtract".to_string(),
                    name: "CalculatorTest::testSubtract".to_string(),
                    path: file_path.to_string(),
                    start_position: Range {
                        start: Position {
                            line: 16,
                            character: 4
                        },
                        end: Position {
                            line: 16,
                            character: MAX_CHAR_LENGTH
                        }
                    },
                    end_position: Range {
                        start: Position {
                            line: 21,
                            character: 0
                        },
                        end: Position {
                            line: 21,
                            character: 5
                        }
                    }
                },
                TestItem {
                    id: "CalculatorTest::testFail1".to_string(),
                    name: "CalculatorTest::testFail1".to_string(),
                    path: file_path.to_string(),
                    start_position: Range {
                        start: Position {
                            line: 23,
                            character: 4
                        },
                        end: Position {
                            line: 23,
                            character: MAX_CHAR_LENGTH
                        }
                    },
                    end_position: Range {
                        start: Position {
                            line: 28,
                            character: 0
                        },
                        end: Position {
                            line: 28,
                            character: 5
                        }
                    }
                }
            ]
        )
    }
}
