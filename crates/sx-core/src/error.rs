use thiserror::Error;

/// Errors of the core domain layer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("command `{0}` is registered twice")]
    DuplicateCommand(&'static str),
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
}
