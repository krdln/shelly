extern crate regex;
extern crate walkdir;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;

mod syntax;
mod preprocess;

use walkdir::WalkDir;

use failure::Error;
use failure::ResultExt;

use std::collections::BTreeMap as Map;
use std::collections::BTreeSet as Set;
use std::path::{Path, PathBuf};

use preprocess::Parsed;

pub fn run(root_path: impl AsRef<Path>, emitter: &mut Emitter) -> Result<(), Error> {
    run_(root_path.as_ref(), emitter)
}

fn run_(root_path: &Path, emitter: &mut Emitter) -> Result<(), Error> {
    use preprocess::PreprocessOutput;

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

        match preprocess::parse_and_preprocess(entry.path(), emitter)? {
            PreprocessOutput::Valid(parsed) => {
                let path = entry.path().canonicalize()?;
                files.insert(path, parsed);
            }
            PreprocessOutput::InvalidImports => {
                eprintln!(
                    "Stopping analysis for this file because of import errors: {}\n",
                    entry.path().display()
                );
            }
        };
    }

    analyze(&files, emitter).context("analyzing")?;

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

/// Kind of error message
#[derive(Debug)]
pub enum Message {
    Error,
    Warning,
}

impl Default for Message {
    fn default() -> Message { Message::Error }
}

pub use syntax::Line;

/// Location of a message
#[derive(Default, Debug)]
pub struct Location {
    pub file: PathBuf,
    pub line: Line,
}

impl Line {
    fn in_file(&self, file: &Path) -> Location {
        Location {
            line: self.to_owned(),
            file: file.to_owned(),
        }
    }
}

pub trait Emitter {
    fn emit(
        &mut self,
        kind: Message,
        message: String,
        location: Location,
        notes: Option<String>,
    );
}

#[derive(Default)]
pub struct EmittedItem {
    pub kind: Message,
    pub message: String,
    pub location: Location,
    pub notes: Option<String>,
}

pub struct VecEmitter {
    pub emitted_items: Vec<EmittedItem>,
}

impl VecEmitter {
    pub fn new() -> VecEmitter {
        VecEmitter { emitted_items: Vec::new() }
    }
}

impl Emitter for VecEmitter {
    fn emit(
        &mut self,
        kind: Message,
        message: String,
        location: Location,
        notes: Option<String>,
    ) {
        let to_emit = EmittedItem { kind, message, location, notes };
        self.emitted_items.push(to_emit)
    }
}

// ---- Scope analysis -------------------------------------------------------

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
            if is_allowed(&usage.location.line, &usage.name) {
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
                    usage.location.in_file(&parsed.original_path),
                    None,
                ),
                Some(Found::Indirect) => emitter.emit(
                    Message::Warning,
                    format!("Indirectly imported: {}", usage.name),
                    usage.location.in_file(&parsed.original_path),
                    None,
                ),
                _ => (),
            }
        }
    }

    Ok(())
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
        let nested = get_scope(&import, files, scopes)?;
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
