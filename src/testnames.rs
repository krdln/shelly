use std::collections::BTreeMap as Map;
use std::path::PathBuf;

use preprocess::Parsed;
use lint::Lint;
use Emitter;
use Message;

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
                emitter.emit(
                    Lint::InvalidTestnameCharacters,
                    Message::Warning,
                    "Testname contains invalid characters".to_owned(),
                    testcase.location.in_file(&file.original_path),
                    Some(format!(
                        "These characters are invalid in a file name: {:?}",
                        invalid_chars,
                    )),
                );
            }
        }
    }
}
