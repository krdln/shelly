extern crate regex;
extern crate walkdir;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;
extern crate toml;
#[macro_use]
extern crate serde_derive;
extern crate yansi;

pub mod lint;
mod config;
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
use std::fs;

use lint::Lint;

pub use config::ConfigFile;

pub fn run(root_path: impl AsRef<Path>, run_opt: RunOpt, emitter: &mut Emitter) -> Result<(), Error> {
    run_(root_path.as_ref(), run_opt, emitter)
}

fn run_(root_path: &Path, run_opt: RunOpt, raw_emitter: &mut Emitter) -> Result<(), Error> {
    use preprocess::PreprocessOutput;

    let config = load_config_from_dir(root_path).context("Loading shelly config")?;
    let lint_config = lint::Config::from_config_file(&config).context("Loading lint levels config")?;

    let mut emitter = lint::Emitter::new(raw_emitter, lint_config);

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

        match preprocess::parse_and_preprocess(entry.path(), &run_opt, &mut emitter)? {
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

    let scopes = scope::analyze(&files, &config, &mut emitter).context("analyzing")?;

    strictness::analyze(&files, &scopes, &mut emitter);
    testnames::analyze(&files, &mut emitter);

    Ok(())
}

#[derive(Default)]
pub struct RunOpt {
    pub debug_parser: bool,
}

pub fn load_config_from_dir(dir_path: &Path) -> Result<ConfigFile, Error> {
    for &filename in &["shelly.toml", "Shelly.toml"] {
        let config_path = dir_path.join(filename);
        if config_path.exists() {
            let config_str = fs::read_to_string(config_path)?;
            return Ok(config_str.parse()?);
        }
    }
    Ok(ConfigFile::default())
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
#[derive(Debug, Clone)]
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
