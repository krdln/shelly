use failure::Error;

use std::path::{Path, PathBuf};
use std::fs;

use syntax;
use Emitter;
use Message;

/// Parsed and preprocessed source file
#[derive(Debug, Default)]
pub struct Parsed {
    pub imports: Vec<PathBuf>,
    pub definitions: Vec<syntax::Definition>,
    pub usages: Vec<syntax::Usage>,

    /// Original, non-resolved path, relative to PWD. Used for error reporting.
    pub original_path: PathBuf,
}

#[derive(Debug)]
pub enum PreprocessOutput {
    /// Parsed and preprocessed file
    Valid(Parsed),

    /// A file can't be preprocessed since it contains invalid imports
    InvalidImports,
}

/// Parses and preprocesses a file for further analysys.
pub fn parse_and_preprocess(path: &Path, emitter: &mut Emitter) -> Result<PreprocessOutput, Error> {
    let source = fs::read_to_string(path)?;
    let file = syntax::parse(&source);

    for testcase in file.testcases {
        let invalid_chars: &[char] = &['"', '>', '<', '|', ':', '*', '?', '\\', '/'];

        if file.uses_pester_logger && testcase.name.contains(invalid_chars) {
            emitter.emit(
                None,
                Message::Warning,
                "Testname contains invalid characters".to_owned(),
                testcase.location.in_file(path),
                Some(format!(
                    "These characters are invalid in a file name: {:?}",
                    invalid_chars,
                )),
            );
        }
    }

    let resolved_imports = match resolve_imports(path, file.imports, emitter)? {
        Some(imports) => imports,
        None => return Ok(PreprocessOutput::InvalidImports),
    };

    Ok(PreprocessOutput::Valid(Parsed {
        imports: resolved_imports,
        definitions: file.definitions,
        usages: file.usages,
        original_path: path.to_owned(),
    }))
}

/// Verifies imports and canonicalizes their paths
///
/// Returns None if any of imports were not recognized
fn resolve_imports(source_path: &Path, imports: Vec<syntax::Import>, emitter: &mut Emitter)
    -> Result<Option<Vec<PathBuf>>, Error>
{
    let mut import_error = false;
    let mut resolved_imports = Vec::new();

    for import in imports {
        use syntax::Importee;

        let dir = source_path.parent().unwrap();
        let filename = source_path.file_name().unwrap().to_str().unwrap();

        let dest_path = match import.importee {
            Importee::Relative(relative_path) => dir.join(relative_path),
            Importee::HereSut => dir.join(filename.replace(".Tests", "")),
            Importee::Unrecognized(_) => {
                // Should we treat unrecognized import as an error also?
                // This will stop processing the file further and will result in
                // less spammy output, because we'll probably get some
                // "Not in scope" errors later on.
                // import_error = true;

                emitter.emit(
                    None,
                    Message::Warning,
                    "Unrecognized import statement".to_string(),
                    import.location.in_file(source_path),
                    Some(
                        "Note: Recognized imports are `$PSScriptRoot\\..` or `$here\\$sut`"
                            .to_string(),
                    ),
                );

                continue;
            }
        };

        if dest_path.exists() {
            resolved_imports.push(dest_path.canonicalize()?)
        } else {
            import_error = true;

            emitter.emit(
                None,
                Message::Error,
                "Invalid import".to_string(),
                import.location.in_file(source_path),
                Some(format!(
                    "File not found: {}",
                    dest_path.display()
                )),
            );
        }
    }

    if import_error {
        return Ok(None)
    }

    Ok(Some(resolved_imports))
}

