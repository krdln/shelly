use failure::Error;
use regex::Regex;

use std::fs;
use std::path::{Path, PathBuf};

use Emitter;
use Message;

/// A `.` import
#[derive(Debug)]
pub struct Import {
    pub line: String,
    pub line_no: u32,
    pub resolved_path: PathBuf,
}

/// Function / commandlet definition
#[derive(Debug)]
pub struct Definition {
    pub line: String,
    pub line_no: u32,
    pub name: String,
}

/// Function / commandlet call
#[derive(Debug)]
pub struct Usage {
    pub line: String,
    pub line_no: u32,
    pub name: String,
}

/// Parsed source file
#[derive(Debug)]
pub struct Parsed {
    pub imports: Vec<Import>,
    pub definitions: Vec<Definition>,
    pub usages: Vec<Usage>,

    /// Original, non-resolved path, relative to PWD
    pub original_path: PathBuf,
}

/// Reads and parses source file
pub fn parse(path: &Path, emitter: &mut Emitter) -> Result<Parsed, Error> {
    lazy_static! {
        static ref IMPORT: Regex = Regex::new(
            r"(?ix) ^ \s* \. \s+ (.*?) \s* (\#.*)? $"
        ).unwrap();

        static ref IMPORT_RELATIVE: Regex = Regex::new(
            r"(?ix) ^ \$ PSScriptRoot (.*?) $"
        ).unwrap();

        static ref IMPORT_HERESUT: Regex = Regex::new(
            r#"(?ix) ^ ["]? \$ here [/\\] \$ sut ["]? $"#
        ).unwrap();

        // Note: it captures also definitions of nested functions,
        // so it's overly optimistic wrt. code correctness.
        static ref DEFINITION: Regex = Regex::new(
            r"(?ix) ^ \s* function \s+ ([a-z][a-z0-9-]*) .* $"
        ).unwrap();

        // For now, conservatively treat only [$x = ] Verb-Foo
        // at the very beginning of line as usage.
        static ref USAGE: Regex = Regex::new(
            r"(?ix) ^ \s* (?: \$\S+ \s*=\s*)? ([[:alpha:]]+-[a-z0-9]+) (?: \s+ .*)? $"
        ).unwrap();

        static ref TESTCASE: Regex = Regex::new(
            r#"(?ix) ^ \s* It \s+ " ([^"]*) " "#
        ).unwrap();
    }

    let file = fs::read_to_string(path)?;

    // Strip BOM
    let file = file.trim_left_matches('\u{feff}');

    let mut definitions = Vec::new();
    let mut usages = Vec::new();
    let mut imports = Vec::new();

    let uses_pester_logger = file.contains("Initialize-PesterLogger");

    for (line, line_no) in file.lines().zip(1..) {
        if let Some(captures) = IMPORT.captures(line) {
            let importee = &captures[1];
            let resolved_path = if let Some(captures) = IMPORT_RELATIVE.captures(importee) {
                let relative = captures[1].replace(r"\", "/");
                let relative = relative.trim_matches('/');
                path.parent().unwrap().join(relative)
            } else if IMPORT_HERESUT.is_match(importee) {
                let pathstr = path.to_str().unwrap();
                pathstr.replace(".Tests.", ".").into()
            } else {
                emitter.emit(
                    Message::Warning,
                    "Unrecognized import statement".to_string(),
                    PathBuf::from(path),
                    line_no,
                    line.to_string(),
                    Some(
                        "Note: Recognized imports are `$PSScriptRoot\\..` or `$here\\$sut`"
                            .to_string(),
                    ),
                );
                continue;
            };
            imports.push(Import {
                line: line.to_owned(),
                resolved_path,
                line_no,
            })
        }

        if let Some(captures) = DEFINITION.captures(line) {
            definitions.push(Definition {
                line: line.to_owned(),
                line_no,
                name: captures[1].to_owned(),
            });
        }

        if let Some(captures) = USAGE.captures(line) {
            usages.push(Usage {
                line: line.to_owned(),
                line_no,
                name: captures[1].to_owned(),
            });
        }

        if let Some(captures) = TESTCASE.captures(line) {
            let invalid_chars: &[char] = &['"', '>', '<', '|', ':', '*', '?', '\\', '/'];
            if uses_pester_logger && captures[1].contains(invalid_chars) {
                emitter.emit(
                    Message::Warning,
                    "Testname contains invalid characters".to_owned(),
                    path.to_owned(),
                    line_no,
                    line.to_owned(),
                    Some(format!(
                        "These characters are invalid in a file name: {:?}",
                        invalid_chars
                    )),
                );
            }
        }
    }

    Ok(Parsed {
        definitions,
        usages,
        imports,
        original_path: path.to_owned(),
    })
}
