use std::collections::BTreeMap as Map;
use std::path::PathBuf;

use lint::Lint;
use lint::Emitter;
use preprocess::Parsed;
use syntax::Item;

pub fn analyze(files: &Map<PathBuf, Parsed>, emitter: &mut Emitter) {
    let invalid_chars: &[char] = &['"', '>', '<', '|', ':', '*', '?', '\\', '/'];

    for file in files.values() {
        let uses_pester_logger = file.usages.iter()
            .any(|usage| usage.item.as_ref() == Item::function("Initialize-PesterLogger"));

        if !uses_pester_logger {
            continue;
        }

        for testcase in &file.testcases {
            if testcase.name.contains(invalid_chars) {
                testcase.span.in_file(&file)
                    .lint(Lint::InvalidTestnameCharacters, "testname contains invalid characters")
                    .note(format!("These characters are invalid in a file name: {:?}", invalid_chars))
                    .emit(emitter);
            }
        }
    }
}
