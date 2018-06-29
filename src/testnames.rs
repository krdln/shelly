use std::collections::BTreeMap as Map;
use std::path::PathBuf;

use lint::Lint;
use lint::Emitter;
use preprocess::Parsed;

pub fn analyze(files: &Map<PathBuf, Parsed>, emitter: &mut Emitter) {
    let invalid_chars: &[char] = &['"', '>', '<', '|', ':', '*', '?', '\\', '/'];

    for file in files.values() {
        let uses_pester_logger = file.usages.iter()
            .any(|usage| usage.name == "Initialize-PesterLogger");

        if !uses_pester_logger {
            continue;
        }

        for testcase in &file.testcases {
            if testcase.name.contains(invalid_chars) {
                testcase.location.in_file(&file.original_path)
                    .lint(Lint::InvalidTestnameCharacters, "Testname contains invalid characters")
                    .note(format!("These characters are invalid in a file name: {:?}", invalid_chars))
                    .emit(emitter);
            }
        }
    }
}
