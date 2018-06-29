extern crate regex;
extern crate walkdir;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;

pub mod lint;
mod syntax;
mod preprocess;
mod scope;
mod strictness;
mod testnames;

use walkdir::WalkDir;

use failure::Error;
use failure::ResultExt;

use std::collections::BTreeMap as Map;
use std::path::{Path, PathBuf};

use lint::Lint;

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
            PreprocessOutput::Valid(mut parsed) => {
                let path = entry.path().canonicalize()?;

                strictness::preprocess(&mut parsed);

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

    let scopes = scope::analyze(&files, emitter).context("analyzing")?;

    strictness::analyze(&files, &scopes, emitter);
    testnames::analyze(&files, emitter);

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
pub enum MessageKind {
    Warning,
    Error,
}

impl Default for MessageKind {
    fn default() -> MessageKind { MessageKind::Error }
}

pub use syntax::Line;

/// Location of a message
#[derive(Debug)]
pub struct Location {
    pub file: PathBuf,
    pub line: Option<Line>,
}

impl Location {
    fn whole_file(file: &Path) -> Location {
        Location {
            line: None,
            file: file.to_owned(),
        }
    }
}

impl Line {
    fn in_file(&self, file: &Path) -> Location {
        Location {
            line: Some(self.to_owned()),
            file: file.to_owned(),
        }
    }
}

pub trait Emitter {
    fn emit(&mut self, item: EmittedItem);
}

#[derive(Debug)]
pub struct EmittedItem {
    pub lint: Lint,
    pub kind: MessageKind,
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
    fn emit(&mut self, to_emit: EmittedItem) {
        self.emitted_items.push(to_emit)
    }
}
