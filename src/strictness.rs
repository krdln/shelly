use std::collections::BTreeSet as Set;
use std::collections::BTreeMap as Map;
use std::path::{Path, PathBuf};

use Emitter;
use EmittedItem;
use MessageKind;
use Location;
use preprocess::Parsed;
use scope::Scope;
use lint::Lint;

const STRICT_MODE_PSEUDOITEM_NAME: &str = "!EnablesStrictMode";

pub fn preprocess(file: &mut Parsed) {
    // We treat setting strict mode as defining
    // a "!EnablesStrictMode" pseudo-item.
    // TODO make Definition a proper enum to support this case.
    for usage in &file.usages {
        if usage.name == "Set-StrictMode" {
            file.definitions.push(::syntax::Definition {
                name: STRICT_MODE_PSEUDOITEM_NAME.into(),
                location: usage.location.clone()
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
        for importee in &parsed.imports {
            importees.insert(importee);
        }
    }

    let all_files: Set<&Path> = scopes.keys().cloned().collect();

    let root_files: Set<&Path> = all_files.difference(&importees).cloned().collect();

    for file in &root_files {
        if !scopes[file].all.contains(STRICT_MODE_PSEUDOITEM_NAME) {
            emitter.emit(
                EmittedItem {
                    lint: Lint::NoStrictMode,
                    kind: MessageKind::Warning,
                    message: "Strict mode not enabled for this file".to_owned(),
                    location: Location::whole_file(file),
                    notes: None,
                }
            );
        }
    }
}
