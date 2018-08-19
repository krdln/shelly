use std::path::PathBuf;

use regex::Regex;

mod v2;
pub use self::v2::Error;
pub use self::v2::Result;

/// Parsed source file
#[derive(Debug)]
pub struct File {
    pub imports: Vec<Import>,
    pub definitions: Vec<Definition>,
    pub usages: Vec<Usage>,
    pub testcases: Vec<Testcase>,
}

/// A source file's line with its location
#[derive(Debug, Clone)]
pub struct Line {
    pub line: String,
    pub no: u32,
}

/// A `.` import
#[derive(Debug)]
pub struct Import {
    pub location: Line,
    pub importee: Importee,
}

/// An importee pointed by `.` import
#[derive(Debug, Eq, PartialEq)]
pub enum Importee {
    /// `$PSScriptRoot/...`
    Relative(PathBuf),

    /// Points to system under test, namely `$here/$sut`
    HereSut,

    Unrecognized(String),
}

/// Function / commandlet definition
#[derive(Debug)]
pub struct Definition {
    pub location: Line,
    pub name: String,
}

/// Function / commandlet call
#[derive(Debug)]
pub struct Usage {
    pub location: Line,
    pub name: String,
}

/// `It` testcase
#[derive(Debug)]
pub struct Testcase {
    pub location: Line,
    pub name: String,
}

/// Parses a source file
pub fn parse(source: &str, debug: bool) -> Result<File> {
    lazy_static! {
        // TODO rewrite import parsing from regexes to token streams
        static ref IMPORT: Regex = Regex::new(
            r"(?ix) ^ \s* \. \s+ (.*?) \s* (\#.*)? $"
        ).unwrap();

        static ref IMPORT_RELATIVE: Regex = Regex::new(
            r"(?ix) ^ \$ PSScriptRoot (.*?) $"
        ).unwrap();

        static ref IMPORT_HERESUT: Regex = Regex::new(
            r#"(?ix) ^ ["]? \$ here [/\\] \$ sut ["]? $"#
        ).unwrap();

        // TODO rewrite testcase parsing to token streams
        static ref TESTCASE: Regex = Regex::new(
            r#"(?ix) ^ \s* It \s+ " ([^"]*) " "#
        ).unwrap();
    }

    // Strip BOM
    let source = source.trim_left_matches('\u{feff}');

    let token_tree_stream = v2::parse(source, debug)?;

    let mut definitions = Vec::new();
    let mut usages = Vec::new();
    let mut imports = Vec::new();
    let mut testcases = Vec::new();

    v2::traverse_streams(&token_tree_stream, |stream| {
        let mut is_function_definition = false;
        for tt in stream {
            match *tt {
                v2::TokenTree::Cmdlet { span, ident } => {
                    let location = Line {
                        line: span.start.find_line(source).to_owned(),
                        no:   span.start.line,
                    };
                    let name = ident.cut_from(source).to_owned();

                    if is_function_definition {
                        definitions.push(Definition { location, name });
                    } else {
                        usages.push(Usage { location, name });
                    }
                }
                _ => {}
            }

            is_function_definition = match *tt {
                v2::TokenTree::FunctionKeyword { .. } => true,
                _                                     => false,
            };
        }
    });

    for (line, line_no) in source.lines().zip(1..) {

        let get_location = || Line { line: line.to_owned(), no: line_no };

        if let Some(captures) = IMPORT.captures(line) {
            let importee = &captures[1];

            let importee = if let Some(captures) = IMPORT_RELATIVE.captures(importee) {
                let relative = &captures[1];
                let relative = relative.replace(r"\", "/");
                let relative = relative.trim_matches('/');
                Importee::Relative(relative.into())
            } else if IMPORT_HERESUT.is_match(importee) {
                Importee::HereSut
            } else {
                Importee::Unrecognized(importee.to_owned())
            };

            imports.push(Import {
                location: get_location(),
                importee,
            })
        }

        if let Some(captures) = TESTCASE.captures(line) {
            testcases.push(Testcase {
                location: get_location(),
                name: captures[1].to_owned(),
            });
        }
    }

    Ok(File {
        definitions,
        usages,
        imports,
        testcases,
    })
}

#[test]
fn test_basics() {
    let source = r#"
        . $here/$sut
        . "$here/$sut"
        . $PSScriptRoot/foo/bar
        . $PSScriptRoot/foo/quux # Because
        . blablabla

        function Foo {
        }

        function Bar() {}

        Fooize-Bar -Baz "quux"
        $A = Write-Host
        $X.Field = Write-Log

        Describe "something" {
            It "works" {}
        }
    "#;

    let parsed = parse(source, false).unwrap();

    assert_eq!(parsed.imports[0].importee, Importee::HereSut);
    assert_eq!(parsed.imports[1].importee, Importee::HereSut);
    assert_eq!(parsed.imports[2].importee, Importee::Relative("foo/bar".into()));
    assert_eq!(parsed.imports[3].importee, Importee::Relative("foo/quux".into()));
    assert_eq!(parsed.imports[4].importee, Importee::Unrecognized("blablabla".into()));

    assert_eq!(parsed.definitions[0].name, "Foo");
    assert_eq!(parsed.definitions[1].name, "Bar");

    assert_eq!(parsed.usages[0].name, "Fooize-Bar");
    assert_eq!(parsed.usages[1].name, "Write-Host");
    assert_eq!(parsed.usages[2].name, "Write-Log");

    assert_eq!(parsed.testcases[0].name, "works");
}

// This test should stop to pass
// when the parser will be implemented correctly.
#[test]
fn test_nested() {
    let source = r#"
        function Foo {
            function Nested {
            }
        }
    "#;

    let parsed = parse(source, false).unwrap();

    let mut funs: Vec<_> = parsed.definitions
        .iter()
        .map(|def| &def.name)
        .collect();

    funs.sort();

    assert_eq!(funs, ["Foo", "Nested"]);
}
