use failure::Error;

use std::collections::BTreeMap as Map;
use std::path::{Path, PathBuf};
use std::fs;

use lint::Lint;
use lint::Emitter;
use syntax;
use RunOpt;
use Line;

/// Parsed and preprocessed source file
#[derive(Debug, Default)]
pub struct Parsed {
    pub imports: Map<PathBuf, syntax::Import>,
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

    /// A file can't be preprocessed since it contains syntax errors
    SyntaxErrors,
}

/// Parses and preprocesses a file for further analysys.
pub fn parse_and_preprocess(path: &Path, run_opt: &RunOpt, emitter: &mut Emitter) -> Result<PreprocessOutput, Error> {
    let source = fs::read_to_string(path)?;

    if run_opt.debug_parser { println!("Trying to parse {}", path.display()); }
    let file = match syntax::parse(&source, run_opt.debug_parser) {
        Ok(file) => file,
        Err(e)   => {
            Line { line: e.where_.find_line(&source).to_owned(), no: e.where_.line }
                .in_file(path)
                .lint(Lint::SyntaxErrors, format!("Syntax error: {}", e.what))
                .note(format!("Column {}", e.where_.col))
                .note("If this is valid PowerShell syntax, please file an issue")
                .emit(emitter);
            return Ok(PreprocessOutput::SyntaxErrors);
        }
    };

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
    -> Result<Option<Map<PathBuf, syntax::Import>>, Error>
{
    let mut import_error = false;
    let mut resolved_imports = Map::new();

    for import in imports {
        use syntax::Importee;

        let dir = source_path.parent().unwrap();
        let filename = source_path.file_name().unwrap().to_str().unwrap();

        let dest_path = match import.importee {
            Importee::Relative(ref relative_path) => dir.join(relative_path),
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
            resolved_imports.insert(dest_path.canonicalize()?, import);
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

impl Parsed {
    pub fn functions(&self) -> impl Iterator<Item=&syntax::Definition> {
        // Currently all definitions are functions, except the
        // pseudoitems, whose names begin with `!`.
        // Pseudoitems are items that are propaged similarly to normal
        // definitions, but they're created by some part of analysis.
        // Eg. we have "uses strict mode" pseudoitem, that gets injected
        // on "Set-StrictMode" and propagates to downstream files.
        self.definitions
            .iter()
            .filter(|def| !def.name.starts_with("!"))
    }
}

