use std::path::PathBuf;

use regex::Regex;
use unicase::UniCase;

mod v2;
pub use self::v2::Span;
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

/// A `.` import
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Import {
    pub span: Span,
    pub importee: Importee,
}

/// An importee pointed by `.` import
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Importee {
    /// `$PSScriptRoot/...`
    Relative(PathBuf),

    /// Points to system under test, namely `$here/$sut`
    HereSut,

    Unrecognized(String),
}

/// An item – function, class, etc.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Item<S: AsRef<str>> {
    pub name: S,
    kind: ItemKind,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ItemKind {
    Function,
    Class,

    /// Pseudoitems are items that are propaged similarly to normal
    /// definitions, but they're created by some part of analysis.
    /// Eg. we have "uses strict mode" pseudoitem, that gets injected
    /// on "Set-StrictMode" and propagates to downstream files.
    Pseudoitem,
}

impl<S: AsRef<str>> Item<S> {
    pub fn as_case_insensitive(&self) -> Item<UniCase<&str>> {
        Item {
            name: UniCase::new(self.name.as_ref()),
            kind: self.kind,
        }
    }

    pub fn as_ref(&self) -> Item<&str> {
        Item {
            name: self.name.as_ref(),
            kind: self.kind,
        }
    }

    pub fn function(name: S) -> Self {
        Item { name, kind: ItemKind::Function, }
    }

    pub fn class(name: S) -> Self {
        Item { name, kind: ItemKind::Class, }
    }

    pub fn pseudo(name: S) -> Self {
        Item { name, kind: ItemKind::Pseudoitem, }
    }

    pub fn is_function(&self) -> bool { self.kind == ItemKind::Function }
    pub fn is_class(&self) -> bool { self.kind == ItemKind::Class }

}

impl<'a> From<Item<&'a str>> for Item<String> {
    fn from(item: Item<&'a str>) -> Self {
        Item {
            name: item.name.into(),
            kind: item.kind,
        }
    }
}

/// Definition of an item
#[derive(Debug)]
pub struct Definition {
    pub span: Span,
    pub item: Item<String>,
}

/// Function/commandlet call / usage of a class
#[derive(Debug)]
pub struct Usage {
    pub span: Span,
    pub item: Item<String>,
}

impl Usage {
    pub fn name(&self) -> &str { &self.item.name }
}

/// `It` testcase
#[derive(Debug)]
pub struct Testcase {
    pub span: Span,
    pub name: String,
}

/// Parses a source file.
///
/// Note: Assumes BOM (byte order mark) is stripped.
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

    let token_tree_stream = v2::parse(source, debug)?;

    let mut definitions = Vec::new();
    let mut usages = Vec::new();
    let mut imports = Vec::new();
    let mut testcases = Vec::new();

    // Gather function definitions and usages
    v2::traverse_streams(&token_tree_stream, |stream, _| {
        let mut is_function_definition = false;
        let mut iter = stream.iter();
        while let Some(tt) = iter.next() {
            match *tt {
                v2::TokenTree::Cmdlet { span, ident } => {
                    let name = ident.cut_from(source).to_owned();

                    if is_function_definition {
                        definitions.push(Definition { span, item: Item::function(name) });
                    } else {
                        if !v2::ident_is_keyword(&name) && !name.ends_with(".exe") {
                            usages.push(Usage { span, item: Item::function(name) });
                        }
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

    // Gather class definitions and usages
    v2::traverse_streams(&token_tree_stream, |stream, delim| {
        match (stream, delim) {
            // TODO: stop representing class names as "fields".
            (&[v2::TokenTree::Field { span, ident }], Some(v2::Delimiter::Bracket)) => {
                // This is just a heuristic – not every [<word in brackets>] is necessarily
                // a class name. But every usage of a class name should be of such form
                let name = ident.cut_from(source).to_owned();
                usages.push(Usage { span, item: Item::class(name) });
            }
            _ => {}
        }

        for window in stream.windows(2) {
            match window {
                &[v2::TokenTree::ClassKeyword { .. }, v2::TokenTree::Field { span, ident }] => {
                    let name = ident.cut_from(source).to_owned();
                    definitions.push(Definition { span, item: Item::class(name) });
                }
                _ => {}
            }
        }
    });

    for (line, line_no) in source.lines().zip(1..) {

        let get_span = |fragment: &str| Span::from_fragment(line_no, fragment, source);

        if let Some(captures) = IMPORT.captures(line) {
            let importee_string = &captures[1];

            let importee = if let Some(captures) = IMPORT_RELATIVE.captures(importee_string) {
                let relative = &captures[1];
                let relative = relative.replace(r"\", "/");
                let relative = relative.trim_matches('/');
                Importee::Relative(relative.into())
            } else if IMPORT_HERESUT.is_match(importee_string) {
                Importee::HereSut
            } else {
                Importee::Unrecognized(importee_string.to_owned())
            };

            imports.push(Import {
                span: get_span(importee_string),
                importee,
            })
        }

        if let Some(captures) = TESTCASE.captures(line) {
            testcases.push(Testcase {
                span: get_span(&captures[1]),
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

        class Car {}
        [Boat] $Foo = 5
    "#;

    let parsed = parse(source, false).unwrap();

    assert_eq!(parsed.imports[0].importee, Importee::HereSut);
    assert_eq!(parsed.imports[1].importee, Importee::HereSut);
    assert_eq!(parsed.imports[2].importee, Importee::Relative("foo/bar".into()));
    assert_eq!(parsed.imports[3].importee, Importee::Relative("foo/quux".into()));
    assert_eq!(parsed.imports[4].importee, Importee::Unrecognized("blablabla".into()));

    assert_eq!(parsed.definitions[0].item.name, "Foo");
    assert_eq!(parsed.definitions[1].item.name, "Bar");
    assert_eq!(parsed.definitions[2].item.as_ref(), Item::class("Car"));

    assert_eq!(parsed.usages[0].item.name, "Fooize-Bar");
    assert_eq!(parsed.usages[1].item.name, "Write-Host");
    assert_eq!(parsed.usages[2].item.name, "Write-Log");
    assert_eq!(parsed.usages[3].item.as_ref(), Item::function("Describe"));
    assert_eq!(parsed.usages[4].item.as_ref(), Item::function("It"));
    assert_eq!(parsed.usages[5].item.as_ref(), Item::class("Boat"));

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
        .map(|def| &def.item.name)
        .collect();

    funs.sort();

    assert_eq!(funs, ["Foo", "Nested"]);
}
