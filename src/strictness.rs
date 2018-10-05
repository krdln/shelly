use std::collections::BTreeSet as Set;
use std::collections::BTreeMap as Map;
use std::path::{Path, PathBuf};

use lint::Emitter;
use lint::Lint;
use Location;
use preprocess::Parsed;
use scope::Scope;
use syntax::Item;

// This should be a constant, not a constant-returning
// function, but constants are currently a little limited on stable Rust.
fn strict_mode_pseudoitem() -> Item<&'static str> {
    Item::pseudo("!EnablesStrictMode")
}

pub fn preprocess(file: &mut Parsed) {
    // We treat setting strict mode as defining
    // a "!EnablesStrictMode" pseudo-item.
    for usage in &file.usages {
        if usage.item.as_ref() == Item::function("Set-StrictMode") {
            file.definitions.push(::syntax::Definition {
                item: strict_mode_pseudoitem().into(),
                span: usage.span.clone()
            });
            break;
        }
    }
}

pub fn analyze<'a>(
    files: &'a Map<PathBuf, Parsed>,
    scopes: &Map<&'a Path, Scope<'a>>,
    emitter: &mut Emitter,
) {
    let mut importees: Set<&Path> = Set::new();

    for parsed in files.values() {
        for importee in parsed.imports.keys() {
            importees.insert(importee);
        }
    }

    let all_files: Set<&Path> = scopes.keys().cloned().collect();

    let root_files: Set<&Path> = all_files.difference(&importees).cloned().collect();

    for &file in &root_files {
        if scopes[file].search(&strict_mode_pseudoitem()).is_none() {
            Location::whole_file(&files[file])
                .lint(Lint::NoStrictMode, "strict mode not enabled for this file")
                .emit(emitter);
        }
    }
}
