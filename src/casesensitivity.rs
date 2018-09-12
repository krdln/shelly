use std::collections::BTreeMap as Map;
use std::path::PathBuf;

use lint::Lint;
use lint::Emitter;
use preprocess::Parsed;

pub fn analyze(files: &Map<PathBuf, Parsed>, emitter: &mut Emitter) {
    for file in files.values() {
        for funcdef in &file.definitions {
            let funcdefname = funcdef.name.trim();

            for funcusage in &file.usages {
                let funcusagename = funcusage.name.trim();
    
                if (funcdefname.to_lowercase() == funcusagename.to_lowercase()) &&
                   (funcdefname != funcusagename) {
                    funcusage.location.in_file(&file.original_path)
                        .lint(Lint::CaseSensitivity, "Function name differs between usage and definition")
                        .note(format!("Check whether the letter casing is the same"))
                        .emit(emitter);
                }
            }
        }
    }
}



#[cfg(test)]
mod test {
    use syntax::{Line, Definition, Usage};
    use VecEmitter;
    use lint;
    use super::*;

    fn usage(fun: &str) -> Usage {
        Usage {
            location: Line { line: fun.to_owned(), no: 1 },
            name: fun.to_owned(),
        }
    }

    fn definition(fun: &str) -> Definition {
        Definition {
            location: Line { line: fun.to_owned(), no: 1 },
            name: fun.to_owned(),
        }
    }

    #[test]
    fn test_case_insensitive() {
        let files = vec![
            (
                "A".into(),
                Parsed {
                    usages: vec![usage("myfuna"), usage("MyFunB")],
                    definitions: vec![definition("MyFunA"), definition("myfunb")],
                    ..Parsed::default()
                }
            ),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(
            &files,
            &mut Emitter::new(&mut emitter, lint::Config::default())
        );
        assert_eq!(emitter.emitted_items.len(), 2);
        assert_eq!(emitter.emitted_items[0].lint, Lint::CaseSensitivity);
        assert_eq!(emitter.emitted_items[1].lint, Lint::CaseSensitivity);
    }
}
