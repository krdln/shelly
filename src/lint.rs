use std::collections::BTreeMap as Map;

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
