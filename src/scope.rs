use failure::Error;

use unicase::UniCase;

use std::collections::BTreeMap as Map;
use std::collections::BTreeSet as Set;
use std::path::{Path, PathBuf};

use lint::Emitter;
use lint::Lint;
use preprocess::Parsed;
use syntax;
use syntax::Item;
use ConfigFile;

struct Config<'a> {
    custom_cmdlets: Set<Item<UniCase<&'a str>>>,
}

impl<'a> Config<'a> {
    fn from_config_file(config_file: &ConfigFile) -> Config {
        let custom_cmdlets = config_file.extras.as_ref()
            .and_then(|extras| extras.cmdlets.as_ref())
            .map(|cmdlets|
                cmdlets
                    .iter()
                    .map(|cmdlet| Item::function(UniCase::new(cmdlet.as_str())))
                    .collect()
            )
            .unwrap_or_else(Set::new);

        Config { custom_cmdlets }
    }
}

/// Functions in scope
#[derive(Debug, Clone)]
pub struct Scope<'a> {
    /// All the functions in scope
    items: Map<Item<UniCase<&'a str>>, DefinedItem<'a>>,

    /// Files directly imported by `.`
    direct_imports: Set<&'a Path>,

    /// Current file
    current_file: &'a Path,
}

/// A function, class etc. defined in some file
#[derive(Debug, Copy, Clone)]
pub struct DefinedItem<'a> {
    /// Canonical path to a file containing the definition
    origin: &'a Path,

    /// Original definition of an item
    definition: &'a syntax::Definition,
}

/// Type of function found in scope
#[derive(Debug)]
pub enum Found {
    /// Found in current file or directly imported files
    Direct,

    /// Indirectly imported (through multiple layers of `.`)
    Indirect,
}

impl<'a> Scope<'a> {
    pub fn search(&self, item: &Item<&str>) -> Option<(Found, DefinedItem<'a>)> {
        match self.items.get(&item.as_case_insensitive()) {
            Some(item) => {
                if item.origin == self.current_file
                || self.direct_imports.contains(item.origin) {
                    Some((Found::Direct, *item))
                } else {
                    Some((Found::Indirect, *item))
                }
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
        static ref BUILTINS: Set<Item<UniCase<&'static str>>> =
            include_str!("builtins.txt")
            .split_whitespace()
            .chain(include_str!("extras.txt").split_whitespace())
            .map(UniCase::new)
            .map(Item::function)
            .collect();
    }

    let config = Config::from_config_file(config);

    let mut scopes = Map::new();

    for (path, parsed) in files {
        let scope = get_scope(path, files, &mut scopes)?;

        let mut already_analyzed = Set::new();
        let mut used_dependencies: Set<&Path> = Set::new();

        for usage in &parsed.usages {
            let usage_unicase = usage.item.as_case_insensitive();

            if BUILTINS.contains(&usage_unicase) {
                continue;
            }
            if config.custom_cmdlets.contains(&usage_unicase) {
                continue;
            }
            if already_analyzed.contains(&usage_unicase) {
                continue;
            }

            already_analyzed.insert(usage_unicase);

            let search_result = scope.search(&usage.item.as_ref());
            match search_result {
                None => {
                    // Don't produce errors for unkown classes yet,
                    // because their usage us a big heuristic.
                    if usage.item.is_function() {
                        usage.span.in_file(&parsed)
                            .lint(Lint::UnknownFunctions, "function not in scope")
                            .what(usage.name())
                            .emit(emitter);
                    }
                }
                Some((Found::Indirect, item)) => {
                    let imported_through: Vec<_> = parsed.imports
                        .keys()
                        .filter(|imported_file| {
                            get_cached_scope(imported_file, &scopes)
                                .search(&usage.item.as_ref())
                                .is_some()
                        })
                        .collect();

                    let through_import_bags: Vec<_> =
                        imported_through
                            .iter()
                            .cloned()
                            .filter(|through| files[*through].is_import_bag())
                            .collect();

                    if through_import_bags.is_empty() {
                        used_dependencies.insert(imported_through[0]);

                        usage.span.in_file(&parsed)
                            .lint(Lint::IndirectImports, "indirectly imported")
                            .what(usage.name())
                            .note(format!(
                                "Indirectly imported through {}",
                                files[imported_through[0]].original_path.display()
                            ))
                            .note(format!(
                                "Consider directly importing {}",
                                files[item.origin].original_path.display()
                            ))
                            .emit(emitter);
                    } else {
                        used_dependencies.insert(through_import_bags[0]);
                    }

                }
                _ => ()
            }
            if let Some((_, defined)) = search_result {
                used_dependencies.insert(defined.origin);

                if usage.item != defined.definition.item {
                    usage.span.in_file(&parsed)
                        .lint(Lint::InvalidLetterCasing, "function name differs between usage and definition")
                        .note("Check whether the letter casing is the same")
                        .emit(emitter);
                }
            }
        }

        // TODO perhaps we can move this check out of scope
        // analysis to its own module? That would require
        // scope analysis to save some info.
        if !parsed.is_import_bag() {
            for (imported_file, import) in &parsed.imports {
                if !used_dependencies.contains(&**imported_file) {
                    if files[imported_file].functions_and_classes().next().is_none() {
                        // Temporarily silence unused-imports for weird "empty" files
                        // with no functions and no class definitions to avoid false positives.
                        // TODO Make the parser understand the world beyond functions
                        // and reenable the lint.
                        continue;
                    }

                    import.span.in_file(&parsed)
                        .lint(Lint::UnusedImports, "unused import")
                        .emit(emitter);
                }
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

/// Gets a scope for a file, panics if not computed yet
fn get_cached_scope<'a>(
    file: &Path,
    scopes: &'a Map<&'a Path, ScopeWip<'a>>
) -> &'a Scope<'a> {
    match scopes.get(file).expect("nonexisting cached scope") {
        ScopeWip::Resolved(scope) => scope,
        ScopeWip::Current         => panic!("scope cached but WIP"),
    }
}

/// Computes or retrieves a Scope for a file,
/// errors on recursive or out-of-tree imports.
/// Caches the computed Scope in the `scopes` cache.
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

    let mut scope = Scope {
        items: Map::new(),
        direct_imports: Set::new(),
        current_file: file,
    };

    for import in parsed_file.imports.keys() {
        scope.direct_imports.insert(import);
        let nested = get_scope(&import, files, scopes)?;
        scope.items.extend(&nested.items);
    }

    for definition in &parsed_file.definitions {
        scope.items.insert(
            definition.item.as_case_insensitive(),
            DefinedItem { definition, origin: file },
        );
    }

    scopes.insert(file, ScopeWip::Resolved(scope.clone()));

    Ok(scope)
}

#[cfg(test)]
mod test {
    use syntax::{Span, Definition, Usage, Import, Importee};
    use VecEmitter;
    use MessageKind;
    use lint;
    use super::*;

    /// A helper macro for initializing different collections than a vec.
    macro_rules! collect {
        ( $( $keyval:expr ),* $(,)* ) => {
            vec![$($keyval),*].into_iter().collect()
        };
    }

    fn usage(fun: &str) -> Usage {
        Usage {
            span: Span::dummy(),
            item: Item::function(fun.to_owned()),
        }
    }

    fn class_usage(class: &str) -> Usage {
        Usage {
            span: Span::dummy(),
            item: Item::class(class.to_owned()),
        }
    }

    fn definition(fun: &str) -> Definition {
        Definition {
            span: Span::dummy(),
            item: Item::function(fun.to_owned()),
        }
    }

    fn class_definition(class: &str) -> Definition {
        Definition {
            span: Span::dummy(),
            item: Item::class(class.to_owned()),
        }
    }

    fn import(relpath: &str) -> (PathBuf, Import) {
        (
            PathBuf::from(relpath),
            Import {
                span: Span::dummy(),
                importee: Importee::Relative(relpath.into()),
            }
        )
    }

    #[test]
    fn test_happy() {
        let files = vec![
            (
                "A".into(),
                Parsed {
                    imports: collect![import("B")],
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
            ("A".into(), Parsed { imports: collect![import("B")], ..Parsed::default() }),
            ("B".into(), Parsed { imports: collect![import("A")], ..Parsed::default() }),
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
                    imports: collect![import("B")],
                    ..Parsed::default()
                }
            ),
            (
                "B".into(),
                Parsed {
                    imports: collect![import("C")],
                    // we must use something from C, otherwise unused-imports will complain
                    usages: vec![usage("funC2")],
                    ..Parsed::default()
                }
            ),
            (
                "C".into(),
                Parsed {
                    definitions: vec![definition("funC1"), definition("funC2")],
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

        assert_eq!(emitter.emitted_items.len(), 1);
        assert_eq!(emitter.emitted_items[0].kind, MessageKind::Warning);
        assert_eq!(emitter.emitted_items[0].lint, Lint::IndirectImports);
    }

    #[test]
    fn test_complains_about_unused_import_if_imported_file_doesnt_use_anything_from_its_own_import_even_though_we_use_it() {
        let files = vec![
            (
                "A".into(),
                Parsed {
                    usages: vec![usage("funB1"), usage("funC1")],
                    imports: collect![import("B")],
                    ..Parsed::default()
                }
            ),
            (
                "B".into(),
                Parsed {
                    imports: collect![import("C")],
                    definitions: vec![definition("funB1")],
                    ..Parsed::default()
                }
            ),
            (
                "C".into(),
                Parsed {
                    definitions: vec![definition("funC1")],
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

        let unused_import_lints: Vec<_> = emitter.emitted_items
            .into_iter()
            .filter(|item| item.lint == Lint::UnusedImports)
            .collect();
        assert_eq!(unused_import_lints.len(), 1);
    }

    #[test]
    fn test_unused_imports_complex() {
        /*
            Arrows signify imports

            file_A: uses bar() --> file_B: defines bar{} --> file_C: defines foo{}
                                   ^  ^
            file_D: uses foo() ---/  /
                                    /
            file_E: uses nothing --/
                    (except of builtin functions.
                     TODO: Dont' detect never-imported files as import-bags and revert the test)

            We expect that:
            * file_B complains about unused import file_C and
            * file_E complains about unused import file_B.
        */
        let files = vec![
            ( "file_A".into(), Parsed {
                    imports: collect![import("file_B")],
                    usages: vec![usage("bar"),],
                    ..Parsed::default() }),
            ( "file_B".into(), Parsed {
                    imports: collect![import("file_C")],
                    definitions: vec![definition("bar")],
                    ..Parsed::default()
                }),
            ( "file_C".into(), Parsed {
                    definitions: vec![definition("foo")],
                    ..Parsed::default()
                }),
            ( "file_D".into(), Parsed {
                    imports: collect![import("file_B")],
                    usages: vec![usage("foo"),],
                    ..Parsed::default()
                }),
            ( "file_E".into(), Parsed {
                    imports: collect![import("file_B")],
                    usages: vec![usage("New-Item")],
                    ..Parsed::default()
                }),
        ].into_iter().collect();

        let mut emitter = VecEmitter::new();
        analyze(
            &files,
            &ConfigFile::default(),
            &mut Emitter::new(&mut emitter, lint::Config::default())
        ).unwrap();

        let unused_import_lints: Vec<_> = emitter.emitted_items
            .into_iter()
            .filter(|item| item.lint == Lint::UnusedImports)
            .collect();
        assert_eq!(unused_import_lints.len(), 2);
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
                    imports: collect![import("file_A")],
                    ..Parsed::default()
                }
            ),
            (
                "file_C".into(),
                Parsed {
                    usages: vec![usage("MyFunB"), usage("myFunA")],
                    imports: collect![import("file_B")],
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

    #[test]
    fn test_detects_unused_imports() {
        let files = vec![
            (
                "file_A".into(),
                Parsed {
                    definitions: vec![definition("foo")],
                    ..Parsed::default()
                }
            ),
            (
                "file_B".into(),
                Parsed {
                    imports: collect![import("file_A")],
                    usages: collect![usage("New-Item")],
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
        assert_eq!(emitter.emitted_items.len(), 1);
        assert_eq!(emitter.emitted_items[0].lint, Lint::UnusedImports);
    }

    #[test]
    fn test_using_classes_marks_import_as_used() {
        let files = vec![
            (
                "file_A".into(),
                Parsed {
                    definitions: vec![
                        definition("foo"),
                        class_definition("Car"),
                    ],
                    ..Parsed::default()
                }
            ),
            (
                "file_B".into(),
                Parsed {
                    imports: collect![import("file_A")],
                    usages: vec![class_usage("Car")],
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
        assert_eq!(emitter.emitted_items.len(), 0);
    }
}
