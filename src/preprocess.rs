use failure::Error;

use std::path::{Path, PathBuf};
use std::fs;

use lint::Lint;
use lint::Emitter;
use syntax;
use RunOpt;

/// Parsed and preprocessed source file
#[derive(Debug, Default)]
pub struct Parsed {
    pub imports: Vec<PathBuf>,
    pub definitions: Vec<syntax::Definition>,
    pub usages: Vec<syntax::Usage>,
    pub testcases: Vec<syntax::Testcase>,

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
pub fn parse_and_preprocess(path: &Path, run_opt: &RunOpt, emitter: &mut Emitter) -> Result<PreprocessOutput, Error> {
    let source = fs::read_to_string(path)?;

    if run_opt.debug_parser { println!("Trying to parse {}", path.display()); }
    let file = syntax::parse(&source, run_opt.debug_parser);

    let resolved_imports = match resolve_imports(path, file.imports, emitter)? {
        Some(imports) => imports,
        None => return Ok(PreprocessOutput::InvalidImports),
    };

    Ok(PreprocessOutput::Valid(Parsed {
        imports: resolved_imports,
        definitions: file.definitions,
        usages: file.usages,
        testcases: file.testcases,
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

                import.location.in_file(source_path)
                    .lint(Lint::UnrecognizedImports, "Unrecognized import statement")
                    .note("Note: Recognized imports are `$PSScriptRoot\\..` or `$here\\$sut`")
                    .emit(emitter);

                continue;
            }
        };

        if dest_path.exists() {
            resolved_imports.push(dest_path.canonicalize()?)
        } else {
            import_error = true;

            import.location.in_file(source_path)
                .lint(Lint::NonexistingImports, "Invalid import")
                .note(format!("File not found: {}", dest_path.display()))
                .emit(emitter);
        }
    }

    if import_error {
        return Ok(None)
    }

    Ok(Some(resolved_imports))
}

