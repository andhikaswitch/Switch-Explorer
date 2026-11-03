use std::path::PathBuf;

use thiserror::Error;

/// File errors keep the path as a field, out of the message: the message may be logged at any
/// level, the path (it contains the user name) only at debug.
#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("unknown switch `{0}`")]
    UnknownFlag(String),
    #[error("invalid value `{value}` for `{id}`: expected {expected}")]
    InvalidValue {
        id: String,
        value: String,
        expected: String,
    },
    #[error("`{0}` cannot be changed with Hot Switch")]
    NotHotSwitchable(&'static str),
    #[error("`{0}` is experimental and needs Developer Mode")]
    NeedsDeveloperMode(&'static str),
    #[error("unknown preset `{0}`")]
    UnknownPreset(String),
    #[error("`--set` expects id=value, got `{0}`")]
    BadAssignment(String),
    #[error("cannot read the settings file: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write the settings file: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the settings file is not valid TOML: {message}")]
    Parse { path: PathBuf, message: String },
}
