use std::collections::BTreeMap as Map;

use EmittedItem;
use Location;
use MessageKind;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub enum Level {
    Allow,
    Warn,
    Deny,
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

        #[derive(Debug, Copy, Clone)]
        pub struct UnknownLint;

        impl ::std::str::FromStr for Lint {
            type Err = UnknownLint;
            fn from_str(s: &str) -> Result<Lint, UnknownLint> {
                match s {
                    $( $slug => Ok(Lint::$name), )+
                    _ => Err(UnknownLint),
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
}

impl<'e> Emitter<'e> {
    /// Creates a new emitter.
    pub fn new(emitter: &'e mut ::Emitter, config: Config) -> Emitter<'e> {
        Emitter { raw_emitter: emitter, config }
    }

    fn emit(&mut self, message: MessageBuilder) {
        let kind = match message.lint.level(&self.config) {
            Level::Allow => return,
            Level::Warn => MessageKind::Warning,
            Level::Deny => MessageKind::Error,
        };

        // TODO handle allow logic here

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
        }
    }
}

#[must_use = "The message should be emitted with .emit()"]
pub struct MessageBuilder {
    lint: Lint,
    message: String,
    location: Location,
    notes: Option<String>,
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

    {
        let mut emitter = Emitter::new(
            &mut raw_emitter,
            Config { cap: Level::Allow, ..Config::default() },
            );
        get_location().lint(Lint::UnknownFunctions, "Boo").emit(&mut emitter);
    }
    assert!(raw_emitter.emitted_items.is_empty());

    {
        let mut emitter = Emitter::new(&mut raw_emitter, Config::default());
        get_location().lint(Lint::UnknownFunctions, "Boo").emit(&mut emitter);
    }
    assert_eq!(raw_emitter.emitted_items.len(), 1);
}
