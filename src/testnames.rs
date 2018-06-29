use std::collections::BTreeMap as Map;
use std::path::PathBuf;

use preprocess::Parsed;
use lint::Lint;
use Emitter;
use EmittedItem;
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
                    EmittedItem {
                        lint: Lint::InvalidTestnameCharacters,
                        kind: Message::Warning,
                        message: "Testname contains invalid characters".to_owned(),
                        location: testcase.location.in_file(&file.original_path),
                        notes: Some(format!(
                            "These characters are invalid in a file name: {:?}",
                            invalid_chars,
                        )),
                    }
                );
            }
        }
    }
}
