use std::path::PathBuf;

use thiserror::Error;

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
    #[error("`--set` expects id=value, got `{0}`")]
    BadAssignment(String),
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid TOML: {message}")]
    Parse { path: PathBuf, message: String },
}
