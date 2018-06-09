extern crate regex;
extern crate walkdir;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;

mod syntax;
mod preprocess;
mod scope;

use walkdir::WalkDir;

use failure::Error;
use failure::ResultExt;

use std::collections::BTreeMap as Map;
use std::path::{Path, PathBuf};

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

    scope::analyze(&files, emitter).context("analyzing")?;

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
#[derive(Debug, Eq, PartialEq)]
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
