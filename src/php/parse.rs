use std::{fs::File, io::BufReader};

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use xml::reader::{ParserConfig, XmlEvent};

use crate::{Diagnostics, FileDiagnostics, MAX_CHAR_LENGTH, error::LSError};

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
                range: Range {
                    start: Position {
                        line: result.line - 1,
                        character: result.col,
                    },
                    end: Position {
                        line: result.line - 1,
                        character: MAX_CHAR_LENGTH,
                    },
                },
                message: result.message,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("phpunit".to_string()),
                code: Some(NumberOrString::String("phpunit-failed".to_string())),
                ..Diagnostic::default()
            }],
        }
    }
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

pub fn parse_phpunit_xml(path: &str) -> Result<Vec<ResultFromXml>, LSError> {
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
                log::error!("XML parse error: {e}");
                return Err(LSError::XmlParse);
            }
            _ => {}
        }
    }

    Ok(results)
}

pub fn to_diagnostics(results: Vec<ResultFromXml>) -> Diagnostics {
    Diagnostics {
        files: results.into_iter().map(std::convert::Into::into).collect(),
        messages: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_phpunit_xml() {
        let mut path = std::env::current_dir().unwrap();
        path.push("demo/phpunit/output.xml");
        let result = parse_phpunit_xml(path.to_str().unwrap()).unwrap();
        assert_eq!(result.len(), 1);
    }
}
