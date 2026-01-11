use std::{collections::HashMap, path::Path};

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use regex::Regex;
use serde::Deserialize;

use crate::{Diagnostics, FileDiagnostics, MAX_CHAR_LENGTH, error::LSError};

#[derive(Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Action {
    Start,
    Run,
    Output,
    Fail,
    Pass,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TestResultLine {
    time: String,
    action: Action,
    package: String,
    test: Option<String>,
    output: Option<String>,
}

fn get_position_from_output(output: &str) -> Option<(String, u32)> {
    let pattern = r"^\s{4}(.*_test\.go):(\d+):";
    let re = Regex::new(pattern).unwrap();
    if let Some(captures) = re.captures(output)
        && let (Some(file_name), Some(lnum)) = (captures.get(1), captures.get(2))
    {
        return Some((
            file_name.as_str().to_string(),
            lnum.as_str().parse::<u32>().unwrap() - 1,
        ));
    }
    None
}

fn get_log_from_output(output: &str) -> String {
    output.replace("        ", "")
}

pub fn parse_go_test_json(
    contents: &str,
    workspace_root: &Path,
    file_paths: &[String],
) -> Result<Diagnostics, LSError> {
    let contents = contents.replace("\r\n", "\n");
    let lines = contents.lines();
    let mut result_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    let mut file_name: Option<String> = None;
    let mut lnum: Option<u32> = None;
    let mut message = String::new();
    let mut last_action: Option<Action> = None;

    for line in lines {
        let value: TestResultLine = serde_json::from_str(line)?;
        match value.action {
            Action::Run => {
                file_name = None;
                message = String::new();
            }
            Action::Output => {
                let output = &value.output.unwrap();
                if let Some((detected_file_name, detected_lnum)) = get_position_from_output(output)
                {
                    file_name = Some(detected_file_name);
                    lnum = Some(detected_lnum);
                    message = String::new();
                } else {
                    message += &get_log_from_output(output);
                }
            }
            _ => {}
        }
        let current_action = value.action;
        let is_action_changed = last_action.as_ref() != Some(&current_action);
        if is_action_changed {
            last_action = Some(current_action);
        } else {
            continue;
        }

        if let (Some(detected_fn), Some(detected_lnum)) = (&file_name, lnum) {
            let diagnostic = Diagnostic {
                range: Range {
                    start: Position {
                        line: detected_lnum,
                        character: 1,
                    },
                    end: Position {
                        line: detected_lnum,
                        character: MAX_CHAR_LENGTH,
                    },
                },
                message: message.clone(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("go-test".to_string()),
                code: Some(NumberOrString::String("go-test-failed".to_string())),
                ..Diagnostic::default()
            };
            let file_path = workspace_root
                .join(detected_fn)
                .to_str()
                .unwrap()
                .to_owned();
            if file_paths.contains(&file_path) {
                result_map.entry(file_path).or_default().push(diagnostic);
            }
            file_name = None;
            lnum = None;
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

#[cfg(test)]
mod tests {
    use std::{fs::read_to_string, path::PathBuf, str::FromStr};

    use super::*;

    #[test]
    fn test_parse_go_test_json() {
        let current_dir = std::env::current_dir().unwrap();
        let test_file_path = current_dir.join("tests/go-test.txt");
        let contents = read_to_string(test_file_path).unwrap();
        let workspace = PathBuf::from_str("/home/demo/test/go/src/test").unwrap();
        let target_file_path = "/home/demo/test/go/src/test/cases_test.go";
        let result =
            parse_go_test_json(&contents, &workspace, &[target_file_path.to_string()]).unwrap();
        let result = result.files.first().unwrap();
        assert_eq!(result.path, target_file_path);
        let diagnostic = result.diagnostics.first().unwrap();
        assert_eq!(diagnostic.range.start.line, 30);
        assert_eq!(diagnostic.range.start.character, 1);
        assert_eq!(diagnostic.range.end.line, 30);
    }
}
