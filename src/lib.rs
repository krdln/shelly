extern crate regex;
extern crate walkdir;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;

use walkdir::WalkDir;

use failure::Error;
use failure::ResultExt;

use regex::Regex;

use std::collections::BTreeMap as Map;
use std::collections::BTreeSet as Set;
use std::fs;
use std::path::{Path, PathBuf};

pub trait Emitter {
    fn emit(
        &mut self,
        kind: Message,
        message: String,
        file: PathBuf,
        line_no: u32,
        line: String,
        notes: Option<String>,
    );
}

pub struct EmittedItem {
    pub kind: Message,
    pub message: String,
    pub file: PathBuf,
    pub line_no: u32,
    pub line: String,
    pub notes: Option<String>,
}

pub struct VecEmitter {
    pub emitted_items: Vec<EmittedItem>,
}

impl VecEmitter {
    pub fn new() -> VecEmitter { VecEmitter { emitted_items: Vec::new() }}
}

impl Emitter for VecEmitter {
    fn emit(
        &mut self,
        kind: Message,
        message: String,
        file: PathBuf,
        line_no: u32,
        line: String,
        notes: Option<String>,
    ) {
        let to_emit: EmittedItem = EmittedItem {
            kind: kind,
            message: message,
            file: file,
            line_no: line_no,
            line: line,
            notes: notes,
        };
        self.emitted_items.push(to_emit)
    }
}

pub fn run(root_path: &str, emitter: &mut Emitter) -> Result<(), Error> {
    if !Path::new(".git").exists() {
        eprintln!("warning: not a root of a repository");
    }

    let mut files = Map::new();

    for entry in WalkDir::new(root_path) {
        let entry = entry.context("traversing")?;
        if entry.path().to_str().unwrap_or("").contains("_Old_Tests") {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("ps1") {
            continue;
        }

        let mut import_error = false;
        let mut parsed = parse(entry.path(), emitter)?;
        for import in &parsed.imports {
            if !import.resolved_path.exists() {
                import_error = true;
                emitter.emit(
                    Message::Error,
                    "Invalid import".to_string(),
                    PathBuf::from(entry.path()),
                    import.line_no,
                    import.line.to_string(),
                    Some(format!(
                        "File not found: {}",
                        import.resolved_path.display()
                    )),
                );
            }
        }
        if import_error {
            continue;
        }

        for import in &mut parsed.imports {
            let path = std::mem::replace(&mut import.resolved_path, PathBuf::new());
            import.resolved_path = path.canonicalize()?;
        }

        let path = entry.path().canonicalize()?;
        files.insert(path, parsed);
    }

    analyze(&files, emitter).context("analyzing")?;

    Ok(())
}

/// Functions in scope
#[derive(Debug, Clone, Default)]
struct Scope<'a> {
    /// Functions defined in this scopeanalyze
    defined: Set<&'a str>,
    /// Defined by a file imported by `analyze
    directly_imported: Set<&'a str>,
    /// All the functions in scope
    all: Set<&'a str>,
    /// All the files imported (directlanalyzely)
    files: Set<&'a Path>,
}

/// Type of function found in scope
#[derive(Debug)]
enum Found {
    /// Found in Scope::defined or Scope::indirectly_imported
    Direct,
    /// Indirectly imported (through multiple layers of `.`)
    Indirect,
}

impl<'a> Scope<'a> {
    fn search(&self, name: &str) -> Option<Found> {
        if self.all.contains(name) {
            if self.defined.contains(name) || self.directly_imported.contains(name) {
                Some(Found::Direct)
            } else {
                Some(Found::Indirect)
            }
        } else {
            None
        }
    }
}

/// State of scope computation
#[derive(Debug, Clone)]
enum ScopeWip<'a> {
    /// Done
    Resolved(Scope<'a>),

    /// The scope is being currently computed
    /// (used to detect import loop)
    Current,
}

fn analyze(files: &Map<PathBuf, Parsed>, emitter: &mut Emitter) -> Result<(), Error> {
    lazy_static! {
        static ref BUILTINS: Set<&'static str> = include_str!("builtins.txt")
            .split_whitespace()
            .chain(include_str!("extras.txt").split_whitespace())
            .collect();
    }

    let mut scopes = Map::new();

    for path in files.keys() {
        let scope = get_scope(path, files, &mut scopes)?;

        let parsed = &files[path];

        let mut already_analyzed = Set::new();

        for usage in &parsed.usages {
            if BUILTINS.contains(usage.name.as_str()) {
                continue;
            }
            if is_allowed(&usage.line, &usage.name) {
                continue;
            }
            if already_analyzed.contains(usage.name.as_str()) {
                continue;
            }

            already_analyzed.insert(usage.name.as_str());

            match scope.search(&usage.name) {
                None => emitter.emit(
                    Message::Error,
                    format!("Not in scope: {}", usage.name),
                    PathBuf::from(parsed.original_path.clone()),
                    usage.line_no,
                    usage.line.to_string(),
                    None,
                ),
                Some(Found::Indirect) => emitter.emit(
                    Message::Warning,
                    format!("Indirectly imported: {}", usage.name),
                    PathBuf::from(parsed.original_path.clone()),
                    usage.line_no,
                    usage.line.to_string(),
                    None,
                ),
                _ => (),
            }
        }
    }

    Ok(())
}

fn is_allowed(line: &str, what: &str) -> bool {
    let mut chunks = line.splitn(2, "#");

    match chunks.next() {
        Some(_before_comment) => (),
        None => return false,
    }

    match chunks.next() {
        Some(comment) => comment.to_lowercase().contains("allow") && comment.contains(what),
        None => false,
    }
}

fn get_scope<'a>(
    file: &'a Path,
    files: &'a Map<PathBuf, Parsed>,
    scopes: &mut Map<&'a Path, ScopeWip<'a>>,
) -> Result<Scope<'a>, Error> {
    match scopes.get(file) {
        Some(ScopeWip::Current) => bail!("Recursive import of {}", file.display()),
        Some(ScopeWip::Resolved(scope)) => return Ok(scope.clone()),
        _ => (),
    };
    scopes.insert(file, ScopeWip::Current);

    let parsed_file = files.get(file).ok_or_else(|| {
        format_err!(
            "List of elements in scope of file {} was requested, \
             but not available due to previous import error",
            file.display()
        )
    })?;

    let mut scope = Scope::default();

    for import in &parsed_file.imports {
        let nested = get_scope(&import.resolved_path, files, scopes)?;
        scope.directly_imported.extend(&nested.defined);
        scope.all.extend(&nested.all);
        scope.files.extend(&nested.files);
    }

    for definition in &parsed_file.definitions {
        scope.defined.insert(&definition.name);
        scope.all.insert(&definition.name);
    }

    scope.files.insert(file);

    scopes.insert(file, ScopeWip::Resolved(scope.clone()));

    Ok(scope)
}

/// Kind of error message
#[derive(Debug)]
pub enum Message {
    Error,
    Warning,
}

/// A `.` import
#[derive(Debug)]
struct Import {
    line: String,
    line_no: u32,
    resolved_path: PathBuf,
}

/// Function / commandlet definition
#[derive(Debug)]
struct Definition {
    line: String,
    line_no: u32,
    name: String,
}

/// Function / commandlet call
#[derive(Debug)]
struct Usage {
    line: String,
    line_no: u32,
    name: String,
}

/// Parsed source file
#[derive(Debug)]
struct Parsed {
    imports: Vec<Import>,
    definitions: Vec<Definition>,
    usages: Vec<Usage>,

    /// Original, non-resolved path, relative to PWD
    original_path: PathBuf,
}

/// Reads and parses source file
fn parse(path: &Path, emitter: &mut Emitter) -> Result<Parsed, Error> {
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
