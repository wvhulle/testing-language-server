use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::str::FromStr;
use std::sync::LazyLock;

use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use regex::Regex;
use serde::Serialize;
use tree_sitter::{Language, Point, Query, QueryCursor};

use crate::error::LSError;
use crate::spec::{DetectWorkspaceResult, FileDiagnostics, TestItem};

pub static LOG_LOCATION: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .unwrap()
        .join(".config/testing_language_server/adapter/")
});

/// If the character value is greater than the line length it defaults back to the line length.
pub const MAX_CHAR_LENGTH: u32 = 10000;

#[derive(Debug)]
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
                ..Default::default()
            }],
        }
    }
}

/// Determine if a particular file is the root of workspace based on marker files.
fn detect_workspace_from_file(file_path: PathBuf, file_names: &[String]) -> Option<String> {
    let parent = file_path.parent()?;
    if file_names
        .iter()
        .any(|file_name| parent.join(file_name).exists())
    {
        Some(parent.to_string_lossy().to_string())
    } else {
        detect_workspace_from_file(parent.to_path_buf(), file_names)
    }
}

pub fn detect_workspaces_from_file_list(
    target_file_paths: &[String],
    file_names: &[String],
) -> DetectWorkspaceResult {
    let mut result_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut file_paths = target_file_paths.to_vec();
    file_paths.sort_by_key(|b| b.len());

    for file_path in file_paths {
        let existing_workspace = result_map
            .iter()
            .find(|(workspace_root, _)| file_path.contains(workspace_root.as_str()));

        if let Some((workspace_root, _)) = existing_workspace {
            result_map
                .entry(workspace_root.to_string())
                .or_default()
                .push(file_path.clone());
        }

        let workspace =
            detect_workspace_from_file(PathBuf::from_str(&file_path).unwrap(), file_names);
        if let Some(workspace) = workspace {
            if result_map
                .get(&workspace)
                .map(|v| !v.contains(&file_path))
                .unwrap_or(true)
            {
                result_map
                    .entry(workspace)
                    .or_default()
                    .push(file_path.clone());
            }
        }
    }

    DetectWorkspaceResult { data: result_map }
}

pub fn send_stdout<T>(value: &T) -> Result<(), LSError>
where
    T: ?Sized + Serialize + std::fmt::Debug,
{
    tracing::info!("adapter stdout: {:#?}", value);
    serde_json::to_writer(std::io::stdout(), &value)?;
    Ok(())
}

pub fn write_result_log(file_name: &str, output: &Output) -> io::Result<()> {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap_or_default();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap_or_default();
    let content = format!("stdout:\n{}\nstderr:\n{}", stdout, stderr);
    let log_path = LOG_LOCATION.join(file_name);
    std::fs::write(&log_path, content)?;
    Ok(())
}

pub fn clean_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1B\[([0-9]{1,2}(;[0-9]{1,2})*)?[m|K]").unwrap();
    re.replace_all(input, "").to_string()
}

pub fn resolve_path(base_dir: &Path, relative_path: &str) -> PathBuf {
    let absolute = if Path::new(relative_path).is_absolute() {
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

pub fn discover_with_treesitter(
    file_path: &str,
    language: &Language,
    query: &str,
) -> Result<Vec<TestItem>, LSError> {
    let mut parser = tree_sitter::Parser::new();
    let mut test_items: Vec<TestItem> = vec![];
    parser
        .set_language(language)
        .expect("Error loading grammar");
    let source_code = std::fs::read_to_string(file_path)?;
    let tree = parser.parse(&source_code, None).unwrap();
    let query = Query::new(language, query).expect("Error creating query");

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
