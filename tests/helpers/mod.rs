use tempdir::TempDir;
use failure::Error;
use shelly::{self, Emitter, VecEmitter, EmittedItem};

use std::fs;
use std::path::Path;

pub fn test_dir(dir: impl AsRef<Path>) -> Vec<EmittedItem> {
    let mut emitter = VecEmitter::new();
    let root_path = Path::new("tests").join(dir);
    shelly::run(&root_path, &mut emitter).expect("run failed");
    emitter.emitted_items
}

pub struct Contents<'x>(pub &'x str);

pub fn run_on_file(Contents(data): Contents, emitter: &mut Emitter) -> Result<(), Error> {
    let dir = TempDir::new("shelly")?;
    fs::write(dir.path().join("File.ps1"), data)?;
    shelly::run(dir.path(), emitter)
}

pub fn test_file(file: Contents) -> Vec<EmittedItem> {
    let mut emitter = VecEmitter::new();
    run_on_file(file, &mut emitter).expect("run failed");
    emitter.emitted_items
}
