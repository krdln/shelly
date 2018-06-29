use failure::Error;

use std::collections::BTreeMap as Map;
use std::collections::BTreeSet as Set;
use std::path::{Path, PathBuf};

use lint::Emitter;
use lint::Lint;
use preprocess::Parsed;
use is_allowed;

/// Functions in scope
#[derive(Debug, Clone, Default)]
pub struct Scope<'a> {
    /// Functions defined in this scope
    defined: Set<&'a str>,
    /// Defined by a file imported by `.`
    directly_imported: Set<&'a str>,
    /// All the functions in scope
    pub all: Set<&'a str>,
    /// All the files imported (directly and indirectly)
    files: Set<&'a Path>,
}

/// Type of function found in scope
#[derive(Debug)]
enum Found {
    /// Found in Scope::defined or Scope::indirectly_imported
    Direct,
    /// Indirectly imported (through multiple layers of `.`)
    Indirect,
}

impl<'a> Scope<'a> {
    fn search(&self, name: &str) -> Option<Found> {
        if self.all.contains(name) {
            if self.defined.contains(name) || self.directly_imported.contains(name) {
                Some(Found::Direct)
            } else {
                Some(Found::Indirect)
            }
        } else {
            None
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

pub fn analyze<'a>(files: &'a Map<PathBuf, Parsed>, emitter: &mut Emitter)
    -> Result<Map<&'a Path, Scope<'a>>, Error>
{
    lazy_static! {
        static ref BUILTINS: Set<&'static str> = include_str!("builtins.txt")
            .split_whitespace()
            .chain(include_str!("extras.txt").split_whitespace())
            .collect();
    }

    let mut scopes = Map::new();

    for path in files.keys() {
        let scope = get_scope(path, files, &mut scopes)?;

        let parsed = &files[path];

        let mut already_analyzed = Set::new();

        for usage in &parsed.usages {
            if BUILTINS.contains(usage.name.as_str()) {
                continue;
            }
            if is_allowed(&usage.location.line, &usage.name) {
                continue;
            }
            if already_analyzed.contains(usage.name.as_str()) {
                continue;
            }

            already_analyzed.insert(usage.name.as_str());

            match scope.search(&usage.name) {
                None => {
                    usage.location.in_file(&parsed.original_path)
                        .lint(Lint::UnknownFunctions, format!("Not in scope: {}", usage.name))
                        .emit(emitter);
                }
                Some(Found::Indirect) => {
                    usage.location.in_file(&parsed.original_path)
                        .lint(Lint::IndirectImports, format!("Indirectly imported: {}", usage.name))
                        .emit(emitter);
                }
                _ => (),
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
        scope.all.extend(&nested.all);
        scope.files.extend(&nested.files);
    }

    for definition in &parsed_file.definitions {
        scope.defined.insert(&definition.name);
        scope.all.insert(&definition.name);
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
    use lint::Config;
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
        analyze(&files, &mut Emitter::new(&mut emitter, Config::default())).unwrap();

        assert!(emitter.emitted_items.is_empty());
    }

    #[test]
    fn test_loop() {
        let files = vec![
            ("A".into(), Parsed { imports: vec!["B".into()], ..Parsed::default() }),
            ("B".into(), Parsed { imports: vec!["A".into()], ..Parsed::default() }),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        let res = analyze(&files, &mut Emitter::new(&mut emitter, Config::default()));

        assert!(res.is_err());
    }

    #[test]
    fn test_errors_when_function_is_used_but_not_defined_anywhere() {
        let files = vec![
            ("A".into(), Parsed { usages: vec![usage("fun")], ..Parsed::default() }),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(&files, &mut Emitter::new(&mut emitter, Config::default())).unwrap();

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
        analyze(&files, &mut Emitter::new(&mut emitter, Config::default())).unwrap();

        assert_eq!(emitter.emitted_items.len(), 1);
        assert_eq!(emitter.emitted_items[0].kind, MessageKind::Warning);
        assert_eq!(emitter.emitted_items[0].lint, Lint::IndirectImports);
    }
}
