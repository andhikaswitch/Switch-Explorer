use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("no Switch Explorer instance is running")]
    NotRunning,
    #[error("another Switch Explorer instance owns the pipe")]
    AlreadyRunning,
    #[error("the other instance did not answer in time")]
    Timeout,
    #[error("message of {0} bytes is larger than allowed")]
    TooLarge(usize),
    #[error("cannot encode message: {0}")]
    Encode(serde_json::Error),
    #[error("cannot decode message: {0}")]
    Decode(serde_json::Error),
    #[error("the connection closed early")]
    Closed,
    #[error("pipe call failed: {0}")]
    Os(#[from] windows::core::Error),
    #[error("cannot start the IPC thread: {0}")]
    Thread(std::io::Error),
}
