use failure::Error;

use std::collections::BTreeMap as Map;
use std::rc::Rc;
use std::path::{Path, PathBuf};
use std::fs;

use lint::Lint;
use lint::Emitter;
use syntax;
use RunOpt;

/// Parsed and preprocessed source file
#[derive(Debug)]
pub struct Parsed {
    pub imports: Map<PathBuf, syntax::Import>,
    pub definitions: Vec<syntax::Definition>,
    pub usages: Vec<syntax::Usage>,
    pub testcases: Vec<syntax::Testcase>,

    pub source: Rc<str>,

    /// Original, non-resolved path, relative to PWD. Used for error reporting.
    pub original_path: PathBuf,
}

// Manual impl of default required because Rc<str> does not impl Default
impl Default for Parsed {
    fn default() -> Self {
        Parsed {
            imports:       Default::default(),
            definitions:   Default::default(),
            usages:        Default::default(),
            testcases:     Default::default(),
            original_path: Default::default(),
            source:        From::from(""),
        }
    }
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

    // Strip BOM
    // TODO move this to muncher after getting rid of regexes in syntax::parse.
    let source = source.trim_left_matches('\u{feff}');

    if run_opt.debug_parser { println!("Trying to parse {}", path.display()); }
    let file = match syntax::parse(&source, run_opt.debug_parser) {
        Ok(file) => file,
        Err(e)   => {
            e.where_.to_span()
                .in_file_source(path, Rc::from(source))
                .lint(Lint::SyntaxErrors, format!("syntax error: {}", e.what))
                .note(format!("Column {}", e.where_.col))
                .note("If this is valid PowerShell syntax, please file an issue")
                .emit(emitter);
            return Ok(PreprocessOutput::SyntaxErrors);
        }
    };

    let source = Rc::from(source);

    let resolved_imports = match resolve_imports(&source, path, file.imports, emitter)? {
        Some(imports) => imports,
        None => return Ok(PreprocessOutput::InvalidImports),
    };

    Ok(PreprocessOutput::Valid(Parsed {
        imports: resolved_imports,
        definitions: file.definitions,
        usages: file.usages,
        testcases: file.testcases,
        original_path: path.to_owned(),
        source,
    }))
}

/// Verifies imports and canonicalizes their paths
///
/// Returns None if any of imports were not recognized
// TODO the `source` argument is weird here.
// Perhaps the whole in_file_source was a bad idea.
fn resolve_imports(source: &Rc<str>, source_path: &Path, imports: Vec<syntax::Import>, emitter: &mut Emitter)
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

                import.span.in_file_source(source_path, Rc::clone(source))
                    .lint(Lint::UnrecognizedImports, "unrecognized import statement")
                    .note("Note: Recognized imports are `$PSScriptRoot\\..` or `$here\\$sut`")
                    .emit(emitter);

                continue;
            }
        };

        if dest_path.exists() {
            resolved_imports.insert(dest_path.canonicalize()?, import);
        } else {
            import_error = true;

            import.span.in_file_source(source_path, Rc::clone(source))
                .lint(Lint::NonexistingImports, "invalid import")
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
    pub fn functions_and_classes(&self) -> impl Iterator<Item=&syntax::Definition> {
        self.definitions
            .iter()
            .filter(|def| def.item.is_function() || def.item.is_class())
    }
}

