use std::{collections::HashMap, path::PathBuf};

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use regex::Regex;
use serde_json::Value;
use xml::{ParserConfig, reader::XmlEvent};

use crate::{Diagnostics, FileDiagnostics, MAX_CHAR_LENGTH, error::LSError};

/// Clean ANSI escape sequences from text
pub fn clean_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1B\[([0-9]{1,2}(;[0-9]{1,2})*)?[m|K]").unwrap();
    re.replace_all(input, "").to_string()
}

/// Resolve a relative path against a base directory
pub fn resolve_path(base_dir: &std::path::Path, relative_path: &str) -> PathBuf {
    let absolute = if std::path::Path::new(relative_path).is_absolute() {
        PathBuf::from(relative_path)
    } else {
        base_dir.join(relative_path)
    };

    let mut components = Vec::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::Normal(_) | std::path::Component::RootDir => {
                components.push(component);
            }
            _ => {}
        }
    }

    PathBuf::from_iter(components)
}

/// Parse Jest JSON output format
pub fn parse_jest_json(test_result: &str, file_paths: &[String]) -> Result<Diagnostics, LSError> {
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    let json: Value = serde_json::from_str(test_result)?;
    let test_results = json["testResults"].as_array().unwrap();

    for test_result in test_results {
        let file_path = test_result["name"].as_str().unwrap();
        if !file_paths.iter().any(|path| path.contains(file_path)) {
            continue;
        }
        let assertion_results = test_result["assertionResults"].as_array().unwrap();

        'assertion: for assertion_result in assertion_results {
            let status = assertion_result["status"].as_str().unwrap();
            if status != "failed" {
                continue 'assertion;
            }
            let location = assertion_result["location"].as_object().unwrap();
            let failure_messages = assertion_result["failureMessages"].as_array().unwrap();
            let line = location["line"].as_u64().unwrap() - 1;
            let column = location["column"].as_u64().unwrap() - 1;

            failure_messages.iter().for_each(|message| {
                let message = clean_ansi(message.as_str().unwrap());
                let diagnostic = Diagnostic {
                    range: Range {
                        start: Position {
                            line: line as u32,
                            character: column as u32,
                        },
                        end: Position {
                            line: line as u32,
                            character: MAX_CHAR_LENGTH,
                        },
                    },
                    message,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("jest".to_string()),
                    code: Some(NumberOrString::String("jest-failed".to_string())),
                    ..Diagnostic::default()
                };
                result_map
                    .entry(file_path.to_string())
                    .or_default()
                    .push(diagnostic);
            })
        }
    }

    Ok(Diagnostics {
        files: result_map
            .into_iter()
            .map(|(path, diagnostics)| FileDiagnostics { path, diagnostics })
            .collect(),
        messages: vec![],
    })
}

/// Parse Vitest JSON output format (similar to Jest but slightly different
/// column handling)
pub fn parse_vitest_json(
    test_result: &str,
    file_paths: Vec<String>,
) -> Result<Diagnostics, LSError> {
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    let json: Value = serde_json::from_str(test_result)?;
    let test_results = json["testResults"].as_array().unwrap();

    for test_result in test_results {
        let file_path = test_result["name"].as_str().unwrap();
        if !file_paths.iter().any(|path| path.contains(file_path)) {
            continue;
        }
        let assertion_results = test_result["assertionResults"].as_array().unwrap();

        'assertion: for assertion_result in assertion_results {
            let status = assertion_result["status"].as_str().unwrap();
            if status != "failed" {
                continue 'assertion;
            }
            let location = assertion_result["location"].as_object().unwrap();
            let failure_messages = assertion_result["failureMessages"].as_array().unwrap();
            let line = location["line"].as_u64().unwrap() - 1;

            failure_messages.iter().for_each(|message| {
                let message = clean_ansi(message.as_str().unwrap());
                let diagnostic = Diagnostic {
                    range: Range {
                        start: Position {
                            line: line as u32,
                            // Line and column number is slightly incorrect.
                            // Bug in json reporter: https://github.com/vitest-dev/vitest/discussions/5350
                            character: 0,
                        },
                        end: Position {
                            line: line as u32,
                            character: MAX_CHAR_LENGTH,
                        },
                    },
                    message,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("vitest".to_string()),
                    code: Some(NumberOrString::String("vitest-failed".to_string())),
                    ..Diagnostic::default()
                };
                result_map
                    .entry(file_path.to_string())
                    .or_default()
                    .push(diagnostic);
            })
        }
    }

    Ok(Diagnostics {
        files: result_map
            .into_iter()
            .map(|(path, diagnostics)| FileDiagnostics { path, diagnostics })
            .collect(),
        messages: vec![],
    })
}

fn get_deno_position_from_output(line: &str) -> Option<(String, u32, u32)> {
    let re = Regex::new(r"=> (?P<file>.*):(?P<line>\d+):(?P<column>\d+)").unwrap();

    if let Some(captures) = re.captures(line) {
        let file = captures.name("file").unwrap().as_str().to_string();
        let line = captures.name("line").unwrap().as_str().parse().unwrap();
        let column = captures.name("column").unwrap().as_str().parse().unwrap();

        Some((file, line, column))
    } else {
        None
    }
}

/// Parse Deno test output format
pub fn parse_deno_output(
    contents: &str,
    workspace_root: PathBuf,
    file_paths: &[String],
) -> Result<Diagnostics, LSError> {
    let contents = clean_ansi(&contents.replace("\r\n", "\n"));
    let lines = contents.lines();
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    let mut file_name: Option<String> = None;
    let mut lnum: Option<u32> = None;
    let mut message = String::new();
    let mut error_exists = false;

    for line in lines {
        if line.contains("ERRORS") {
            error_exists = true;
        } else if !error_exists {
            continue;
        }

        if let Some(position) = get_deno_position_from_output(line) {
            if file_name.is_some() {
                let diagnostic = Diagnostic {
                    range: Range {
                        start: Position {
                            line: lnum.unwrap(),
                            character: 1,
                        },
                        end: Position {
                            line: lnum.unwrap(),
                            character: MAX_CHAR_LENGTH,
                        },
                    },
                    message: message.clone(),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("deno".to_string()),
                    code: Some(NumberOrString::String("deno-test-failed".to_string())),
                    ..Diagnostic::default()
                };
                let file_path = resolve_path(&workspace_root, file_name.as_ref().unwrap())
                    .to_str()
                    .unwrap()
                    .to_string();
                if file_paths.contains(&file_path) {
                    result_map.entry(file_path).or_default().push(diagnostic);
                }
            }
            file_name = Some(position.0);
            lnum = Some(position.1);
        } else {
            message += line;
        }
    }

    Ok(Diagnostics {
        files: result_map
            .into_iter()
            .map(|(path, diagnostics)| FileDiagnostics { path, diagnostics })
            .collect(),
        messages: vec![],
    })
}

pub struct ResultFromXml {
    pub message: String,
    pub path: String,
    pub line: u32,
    pub col: u32,
}

impl From<ResultFromXml> for FileDiagnostics {
    fn from(result: ResultFromXml) -> Self {
        FileDiagnostics {
            path: result.path,
            diagnostics: vec![Diagnostic {
                message: result.message,
                range: Range {
                    start: Position {
                        line: result.line - 1,
                        character: result.col - 1,
                    },
                    end: Position {
                        line: result.line - 1,
                        character: MAX_CHAR_LENGTH,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("node-test".to_string()),
                code: Some(NumberOrString::String("node-test-failed".to_string())),
                ..Default::default()
            }],
        }
    }
}

fn parse_error_location(error_text: &str, target_file_paths: &[String]) -> Option<ResultFromXml> {
    let re = Regex::new(r"\(([^:]+):(\d+):(\d+)\)").ok()?;
    for line in error_text.lines() {
        if let Some(caps) = re.captures(line) {
            let file_path = caps.get(1)?.as_str();
            if !target_file_paths.contains(&file_path.to_string()) {
                continue;
            }
            return Some(ResultFromXml {
                message: error_text
                    .strip_prefix('\n')
                    .unwrap_or(error_text)
                    .to_string(),
                path: file_path.to_string(),
                line: caps.get(2)?.as_str().parse().ok()?,
                col: caps.get(3)?.as_str().parse().ok()?,
            });
        }
    }
    None
}

/// Parse Node.js test runner XML (JUnit) output format
pub fn parse_node_test_xml(output: &str, target_file_paths: &[String]) -> Vec<ResultFromXml> {
    let mut reader = ParserConfig::default()
        .ignore_root_level_whitespace(false)
        .create_reader(output.as_bytes());

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
                if let Some(result) = parse_error_location(&data, target_file_paths) {
                    results.push(result);
                }
            }
            Ok(XmlEvent::EndDocument) => break,
            Err(e) => {
                log::error!("XML parse error: {e}");
                break;
            }
            _ => {}
        }
    }

    results
}
