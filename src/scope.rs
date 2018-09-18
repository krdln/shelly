use failure::Error;

use unicase::UniCase;

use std::collections::BTreeMap as Map;
use std::collections::BTreeSet as Set;
use std::path::{Path, PathBuf};

use lint::Emitter;
use lint::Lint;
use preprocess::Parsed;
use ConfigFile;

struct Config {
    custom_cmdlets: Set<UniCase<String>>,
}

impl Config {
    fn from_config_file(config_file: &ConfigFile) -> Config {
        let custom_cmdlets = config_file.extras.as_ref()
            .and_then(|extras| extras.cmdlets.as_ref())
            .map(|cmdlets|
                cmdlets
                    .iter()
                    .cloned()
                    .map(|cmdlet| UniCase::new(cmdlet))
                    .collect()
            )
            .unwrap_or_else(Set::new);

        Config { custom_cmdlets }
    }
}

/// Functions in scope
#[derive(Debug, Clone, Default)]
pub struct Scope<'a> {
    /// Functions defined in this scope
    defined: Set<UniCase<&'a str>>,
    /// Defined by a file imported by `.`
    directly_imported: Set<UniCase<&'a str>>,
    /// All the functions in scope
    pub all: Set<UniCase<&'a str>>,
    /// All the files imported (directly and indirectly)
    files: Set<&'a Path>,
    /// All defined or directly imported functions
    directly_imported_or_defined: Set<UniCase<&'a str>>,
}

/// Type of function found in scope
#[derive(Debug)]
enum Found {
    /// Found in Scope::defined or Scope::indirectly_imported
    Direct,
    /// Indirectly imported (through multiple layers of `.`)
    Indirect,
}

/// Determines whether function usage matches function definitions letter-case wise
enum Casing {
    /// The same as in definition
    Original,
    /// Letter-casing differs from definition
    Different,
}

impl<'a> Scope<'a> {
    fn search(&self, name: &str) -> Option<(Found, Casing)> {
        let case_insensitive_name = UniCase::new(name);

        match self.all.get(&case_insensitive_name) {
            Some(original_name) => {
                let directness = if self.directly_imported_or_defined.contains(&case_insensitive_name) {
                    Found::Direct
                } else {
                    Found::Indirect
                };

                let casing = if name == &***original_name {
                    Casing::Original
                } else {
                    Casing::Different
                };

                Some((directness, casing))
            }
            None => None
        }
    }
}

/// State of scope computation
#[derive(Debug, Clone)]
enum ScopeWip<'a> {
    /// Done
    Resolved(Scope<'a>),

    /// The scope is being currently computed
    /// (used to detect import loop)
    Current,
}

pub fn analyze<'a>(files: &'a Map<PathBuf, Parsed>, config: &ConfigFile, emitter: &mut Emitter)
    -> Result<Map<&'a Path, Scope<'a>>, Error>
{
    lazy_static! {
        static ref BUILTINS: Set<UniCase<&'static str>> = include_str!("builtins.txt")
            .split_whitespace()
            .chain(include_str!("extras.txt").split_whitespace())
            .map(UniCase::new)
            .collect();
    }

    let config = Config::from_config_file(config);

    let mut scopes = Map::new();

    for path in files.keys() {
        let scope = get_scope(path, files, &mut scopes)?;

        let parsed = &files[path];

        let mut already_analyzed = Set::new();

        for usage in &parsed.usages {
            if BUILTINS.contains(&UniCase::new(&usage.name)) {
                continue;
            }
            if config.custom_cmdlets.contains(&UniCase::new(usage.name.clone())) {
                continue;
            }
            if already_analyzed.contains(&UniCase::new(&usage.name)) {
                continue;
            }

            already_analyzed.insert(UniCase::new(&usage.name));

            let search_result = scope.search(&usage.name);
            match search_result {
                None => {
                    usage.location.in_file(&parsed.original_path)
                        .lint(Lint::UnknownFunctions, format!("Not in scope: {}", usage.name))
                        .what(usage.name.clone())
                        .emit(emitter);
                }
                Some((Found::Indirect, _)) => {
                    usage.location.in_file(&parsed.original_path)
                        .lint(Lint::IndirectImports, format!("Indirectly imported: {}", usage.name))
                        .what(usage.name.clone())
                        .emit(emitter);
                }
                _ => ()
            }
            if let Some((_, Casing::Different)) = search_result {
                usage.location.in_file(&parsed.original_path)
                    .lint(Lint::InvalidLetterCasing, "Function name differs between usage and definition")
                    .note(format!("Check whether the letter casing is the same"))
                    .emit(emitter);
            }
        }
    }

    let scopes = scopes.into_iter()
        .map(
            |(file, scope_wip)| {
                match scope_wip {
                    ScopeWip::Resolved(scope) => (file, scope),
                    ScopeWip::Current => unreachable!(),
                }
            }
        )
        .collect();

    Ok(scopes)
}

fn get_scope<'a>(
    file: &'a Path,
    files: &'a Map<PathBuf, Parsed>,
    scopes: &mut Map<&'a Path, ScopeWip<'a>>,
) -> Result<Scope<'a>, Error> {
    match scopes.get(file) {
        Some(ScopeWip::Current) => bail!("Recursive import of {}", file.display()),
        Some(ScopeWip::Resolved(scope)) => return Ok(scope.clone()),
        _ => (),
    };
    scopes.insert(file, ScopeWip::Current);

    let parsed_file = files.get(file).ok_or_else(|| {
        format_err!(
            "List of elements in scope of file {} was requested, \
             but not available due to previous import error",
            file.display()
        )
    })?;

    let mut scope = Scope::default();

    for import in &parsed_file.imports {
        let nested = get_scope(&import, files, scopes)?;
        scope.directly_imported.extend(&nested.defined);
        scope.directly_imported_or_defined.extend(&nested.defined);
        scope.all.extend(&nested.all);
        scope.files.extend(&nested.files);
    }

    for definition in &parsed_file.definitions {
        scope.defined.insert(UniCase::new(&definition.name));
        scope.all.insert(UniCase::new(&definition.name));
        scope.directly_imported_or_defined.insert(UniCase::new(&definition.name));
    }

    scope.files.insert(file);

    scopes.insert(file, ScopeWip::Resolved(scope.clone()));

    Ok(scope)
}

#[cfg(test)]
mod test {
    use syntax::{Line, Definition, Usage};
    use VecEmitter;
    use MessageKind;
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
    fn test_happy() {
        let files = vec![
            (
                "A".into(),
                Parsed {
                    imports: vec!["B".into()],
                    usages: vec![usage("funA1"), usage("funB1")],
                    definitions: vec![definition("funA1")],
                    ..Parsed::default()
                }
            ),
            (
                "B".into(),
                Parsed {
                    definitions: vec![definition("funB1")],
                    ..Parsed::default()
                }
            ),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(
            &files,
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        ).unwrap();

        assert!(emitter.emitted_items.is_empty());
    }

    #[test]
    fn test_loop() {
        let files = vec![
            ("A".into(), Parsed { imports: vec!["B".into()], ..Parsed::default() }),
            ("B".into(), Parsed { imports: vec!["A".into()], ..Parsed::default() }),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        let res = analyze(
            &files,
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        );

        assert!(res.is_err());
    }

    #[test]
    fn test_errors_when_function_is_used_but_not_defined_anywhere() {
        let files = vec![
            ("A".into(), Parsed { usages: vec![usage("fun")], ..Parsed::default() }),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(
            &files,
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        ).unwrap();

        assert_eq!(emitter.emitted_items.len(), 1);
        assert_eq!(emitter.emitted_items[0].kind, MessageKind::Error);
        assert_eq!(emitter.emitted_items[0].lint, Lint::UnknownFunctions);
    }

    #[test]
    fn test_warns_when_function_is_defined_not_directly_in_imported_file_but_deeper() {
        let files = vec![
            (
                "A".into(),
                Parsed {
                    usages: vec![usage("funC1")],
                    imports: vec!["B".into()],
                    ..Parsed::default()
                }
            ),
            ("B".into(), Parsed { imports: vec!["C".into()], ..Parsed::default() }),
            ("C".into(), Parsed { definitions: vec![definition("funC1")], ..Parsed::default() }),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(
            &files,
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        ).unwrap();

        assert_eq!(emitter.emitted_items.len(), 1);
        assert_eq!(emitter.emitted_items[0].kind, MessageKind::Warning);
        assert_eq!(emitter.emitted_items[0].lint, Lint::IndirectImports);
    }

    #[test]
    fn test_can_detect_invalid_letter_casing() {
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
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        ).unwrap();
        assert_eq!(emitter.emitted_items.len(), 2);
        assert_eq!(emitter.emitted_items[0].lint, Lint::InvalidLetterCasing);
        assert_eq!(emitter.emitted_items[1].lint, Lint::InvalidLetterCasing);
    }

    #[test]
    fn test_detecting_invalid_letter_casing_works_for_multiple_files() {
        let files = vec![
            (
                "file_A".into(),
                Parsed {
                    definitions: vec![definition("myfunb")],
                    ..Parsed::default()
                }
            ),
            (
                "file_B".into(),
                Parsed {
                    usages: vec![usage("MyFunB")],
                    definitions: vec![definition("MyFunA")],
                    imports: vec!["file_A".into()],
                    ..Parsed::default()
                }
            ),
            (
                "file_C".into(),
                Parsed {
                    usages: vec![usage("MyFunB"), usage("myFunA")],
                    imports: vec!["file_B".into()],
                    ..Parsed::default()
                }
            ),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(
            &files,
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        ).unwrap();
        let invalid_casing_lints: Vec<_> = emitter.emitted_items
            .into_iter()
            .filter(|item| item.lint == Lint::InvalidLetterCasing)
            .collect();
        assert_eq!(invalid_casing_lints.len(), 3);
    }
}
