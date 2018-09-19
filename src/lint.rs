use std::collections::BTreeMap as Map;
use std::collections::BTreeSet as Set;
use std::str::FromStr;
use std::fmt;

use regex::Regex;
use failure;

use EmittedItem;
use Location;
use MessageKind;
use ConfigFile;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub enum Level {
    Allow,
    Warn,
    Deny,
}

#[derive(Debug)]
pub struct UnknownLevel;

impl FromStr for Level {
    type Err = UnknownLevel;

    fn from_str(s: &str) -> Result<Level, Self::Err> {
        match s {
            "Allow" | "allow" => Ok(Level::Allow),
            "Warn"  | "warn"  => Ok(Level::Warn),
            "Deny"  | "deny"  => Ok(Level::Deny),
            _       => Err(UnknownLevel),
        }
    }
}

macro_rules! lints {
    ( $( #[$attr:meta] $name:ident : $slug:tt => $level:ident ),+ $(,)* ) => {

        /// Lint is a type of error or warning that a linter can emit
        #[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
        pub enum Lint {
            $( #[ $attr ] $name ),+
        }

        impl Lint {
            pub fn default_level(&self) -> Level {
                match self {
                    $( Lint::$name => Level::$level),+
                }
            }

            pub fn slug(&self) -> &'static str {
                match self {
                    $( Lint::$name => $slug ),+
                }
            }

            pub fn lints() -> impl Iterator<Item=Lint> {
                [ $( Lint::$name ),+ ].iter().cloned()
            }
        }

        #[derive(Debug, Clone, Eq, PartialEq)]
        pub struct UnknownLint(String);

        impl ::std::str::FromStr for Lint {
            type Err = UnknownLint;
            fn from_str(s: &str) -> Result<Lint, UnknownLint> {
                match s {
                    $( $slug => Ok(Lint::$name), )+
                    s => Err(UnknownLint(s.to_owned())),
                }
            }
        }
    };
}

lints!{
    /// Imported file not found
    NonexistingImports: "nonexisting-imports" => Deny,

    /// Import in an unrecognized form
    UnrecognizedImports: "unrecognized-imports" => Warn,

    /// Function not in scope
    UnknownFunctions: "unknown-functions" => Deny,

    /// Usage of indirectly imported item (through multiple levels of dot-imports)
    IndirectImports: "indirect-imports" => Warn,

    /// Invalid characters in testname
    InvalidTestnameCharacters: "invalid-testname-characters" => Warn,

    /// Strict mode not enabled
    NoStrictMode: "no-strict-mode" => Warn,

    /// Function name differs between usage and definition
    InvalidLetterCasing: "invalid-letter-casing" => Warn,

    /// Unknown lint allowed in a comment
    UnknownLints: "unknown-lints" => Warn,

    /// Shelly couldn't parse this syntax
    SyntaxErrors: "syntax-errors" => Warn,

    /// File was imported but no direct definitions from it are being uesd
    UnusedImports: "unused-imports" => Deny,
}

impl fmt::Display for UnknownLint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unknown lint: `{}`", self.0)
    }
}

impl Lint {
    pub fn level(&self, config: &Config) -> Level {
        let uncapped_level = config
            .overrides
            .get(self)
            .cloned()
            .unwrap_or(self.default_level());
        uncapped_level.min(config.cap)
    }
}

fn parse_allow_annotation(line: &str) -> Result<Option<(Lint, Option<&str>)>, UnknownLint> {
    lazy_static!(
        static ref RE: Regex = Regex::new(
            r"(?ix) ^ [^\#]* \# \s* (?: shelly:|analyzer:)? \s*
              allow \s* ( [[:word:]-]+ ) (?: \( (.*) \) )? $"
        ).unwrap();
    );

    let captures = match RE.captures(line) {
        Some(c) => c,
        None => return Ok(None),
    };

    let lint = captures.get(1).unwrap().as_str();
    let lint = lint.parse()?;
    let what = captures.get(2).map(|match_| match_.as_str());

    Ok(Some((lint, what)))
}

#[test]
fn test_parse_allow_annotation() {
    assert_eq!(
        parse_allow_annotation("New-Foo # Random comment"),
        Ok(None),
    );
    assert_eq!(
        parse_allow_annotation("New-Foo # allow unicorns"),
        Err(UnknownLint("unicorns".to_owned())),
    );
    assert_eq!(
        parse_allow_annotation("New-Foo # allow unknown-functions"),
        Ok(Some((Lint::UnknownFunctions, None))),
    );
    assert_eq!(
        parse_allow_annotation("New-Foo # allow unknown-functions(New-Foo)"),
        Ok(Some((Lint::UnknownFunctions, Some("New-Foo")))),
    );
    assert_eq!(
        parse_allow_annotation("New-Foo # shelly: allow unknown-functions"),
        Ok(Some((Lint::UnknownFunctions, None))),
    );
    assert_eq!(
        parse_allow_annotation("New-Foo # whatever: allow unknown-functions"),
        Ok(None),
    );
}

pub struct Config {
    /// Overrides default levels for lints
    overrides: Map<Lint, Level>,

    /// Maximal severity level
    cap: Level,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            overrides: Map::default(),
            cap: Level::Deny,
        }
    }
}

impl Config {
    pub fn from_config_file(config_file: &ConfigFile) -> Result<Config, failure::Error> {
        let mut config = Config::default();

        if let Some(levels) = &config_file.levels {
            for (lint_name, level) in levels {
                let lint = lint_name.parse()
                    .map_err(|_| format_err!("Unknown lint name: '{}'", lint_name))?;

                let level = level.parse()
                    .map_err(|_| format_err!("Unknown level '{}' for '{}'", level, lint_name))?;

                let previous_entry = config.overrides.insert(lint, level);
                if previous_entry.is_some() {
                    // Toml parsing gets rid of duplicates for us,
                    // but if we introduce lint-name-aliases, duplicates
                    // may occur.
                    bail!("Duplicated entry for '{}' lint", lint_name);
                }
            }
        }

        Ok(config)
    }

    pub fn with_overrides(mut self, overrides: &Map<Lint, Level>) -> Self {
        self.overrides.extend(overrides);
        self
    }
}

#[test]
fn config_from_string() {
    let cfg_string = r#"
[levels]
nonexisting-imports = "warn"
"#;
    let config = ConfigFile::from_str(cfg_string).unwrap();
    let config = Config::from_config_file(&config).unwrap();
    assert_eq!(config.overrides[&Lint::NonexistingImports], Level::Warn);
}

#[test]
fn misc() {
    let mut config = Config::default();
    assert_eq!(Lint::UnknownFunctions.level(&config), Level::Deny);

    config.overrides.insert("unknown-functions".parse().unwrap(), Level::Warn);
    assert_eq!(Lint::UnknownFunctions.level(&config), Level::Warn);

    config.cap = Level::Allow;
    assert_eq!(Lint::UnknownFunctions.level(&config), Level::Allow);
}

#[test]
fn overrides() {
    let mut config = Config::default();
    config.overrides.insert("unknown-functions".parse().unwrap(), Level::Warn);

    let overrides = ::std::iter::once(
        (Lint::UnknownFunctions, Level::Deny)
    ).collect();

    config = config.with_overrides(&overrides);

    assert_eq!(Lint::UnknownFunctions.level(&config), Level::Deny);
}

#[test]
fn slug_roundtrip() {
    assert!(Lint::lints().count() > 0);
    for lint in Lint::lints() {
        assert_eq!(lint, lint.slug().parse().unwrap());
    }
}

// Emitting

/// Lint Emitter
///
/// This is different Emitter than the main one,
/// as it handles also allow-logic and configuration overrides.
///
/// Use `MessageBuilder::emit` to emit the message.
pub struct Emitter<'e> {
    raw_emitter: &'e mut ::Emitter,
    config: Config,
    encountered_lints: Set<Lint>,
}

impl<'e> Emitter<'e> {
    /// Creates a new emitter.
    pub fn new(emitter: &'e mut ::Emitter, config: Config) -> Emitter<'e> {
        Emitter {
            raw_emitter: emitter,
            config,
            encountered_lints: Set::new(),
        }
    }

    fn emit(&mut self, mut message: MessageBuilder) {
        let kind = match message.lint.level(&self.config) {
            Level::Allow => return,
            Level::Warn => MessageKind::Warning,
            Level::Deny => MessageKind::Error,
        };

        if message.lint != Lint::UnknownLints {
            if let Some(line) = &message.location.line {
                match parse_allow_annotation(&line.line) {
                    Err(unknown_lint) => {
                        message.location.clone()
                            .lint(Lint::UnknownLints, format!("Unknown lint: {}", unknown_lint.0))
                            .note("Use `shelly show-lints` to list available lints")
                            .emit(self);
                    }
                    Ok(Some((allowed_lint, allowed_elem))) if message.lint == allowed_lint => {
                        match (allowed_elem, &message.what) {
                            (Some(allowed_elem), Some(linted_elem)) if allowed_elem == linted_elem => return,
                            (None, _) => return,
                            _ => (),
                        }
                    }
                    _ => (),
                }
            }
        }

        if self.encountered_lints.insert(message.lint) == true
        && message.location.line.is_some()
        && message.lint != Lint::UnknownLints {
            let elem_str = message.what.as_ref()
                .map(|what| format!("({})", what))
                .unwrap_or_else(String::new);
            let note = format!(
                "To allow, add a comment `allow {}{}` in this line",
                message.lint.slug(),
                elem_str,
            );
            message = message.note(note);
        }

        let item = EmittedItem {
            kind,
            lint: message.lint,
            message: message.message,
            location: message.location,
            notes: message.notes,
        };

        self.raw_emitter.emit(item);
    }
}

impl Location {
    pub fn lint(self, lint: Lint, message: impl Into<String>) -> MessageBuilder {
        MessageBuilder {
            location: self,
            lint,
            message: message.into(),
            notes: None,
            what: None,
        }
    }
}

#[must_use = "The message should be emitted with .emit()"]
pub struct MessageBuilder {
    lint: Lint,
    message: String,
    location: Location,
    notes: Option<String>,

    /// Specific syntax element that the lint refers to,
    /// used for allow comment logic. Eg. the function name
    /// for the UnkonwnFunctions lint.
    what: Option<String>,
}

impl MessageBuilder {
    /// Adds a note at the end of the message.
    ///
    /// Could be called multiple times.
    pub fn note(mut self, note: impl Into<String>) -> MessageBuilder {
        let note = note.into();

        match &mut self.notes {
            Some(current_note) => {
                current_note.push('\n');
                current_note.push_str(&note);
            }
            notes => {
                *notes = Some(note)
            }
        }

        self
    }

    /// Specifies the exact syntax element that the lint
    /// refers to (eg. a function name)
    pub fn what(mut self, what: impl Into<String>) -> MessageBuilder  {
        self.what = Some(what.into());
        self
    }

    /// Checks the allow-logic and emits the message
    /// according to overrides used in config.
    pub fn emit(self, emitter: &mut Emitter) {
        emitter.emit(self)
    }
}

#[test]
fn test_ignoring_allowed_messages() {
    let get_location = || Location { file: "foo".into(), line: None };
    let mut raw_emitter = ::VecEmitter::new();

    // Allowed in a config
    {
        let mut emitter = Emitter::new(
            &mut raw_emitter,
            Config { cap: Level::Allow, ..Config::default() },
            );
        get_location().lint(Lint::UnknownFunctions, "Boo").emit(&mut emitter);
    }
    assert!(raw_emitter.emitted_items.is_empty());

    // Not allowed
    {
        let mut emitter = Emitter::new(&mut raw_emitter, Config::default());
        get_location().lint(Lint::UnknownFunctions, "Boo").emit(&mut emitter);
    }
    assert_eq!(raw_emitter.emitted_items.len(), 1);
}
