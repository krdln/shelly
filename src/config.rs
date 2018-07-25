use std::collections::BTreeMap as Map;
use std::str::FromStr;

use toml;

/// ConfigFile describes a TOML-structure of a shelly.toml config.
///
/// Each module should use a separate config with proper types.
/// See eg. lint::Config.
#[derive(Debug, Default, Deserialize)]
pub struct ConfigFile {
    /// Lint levels overrides
    pub(crate) levels: Option<Map<String, String>>,

    /// Custom commandlets that are assumed to exist
    /// (in addition to the ones defined in builtins.txt)
    pub(crate) cmdlets: Option<Vec<String>>,
}

impl FromStr for ConfigFile {
    type Err = toml::de::Error;

    fn from_str(source: &str) -> Result<ConfigFile, toml::de::Error> {
        toml::de::from_str(source)
    }
}

