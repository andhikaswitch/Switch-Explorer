//! Messages between Switch Explorer processes (ARCHITECTURE.md §12): a little-endian `u32`
//! length followed by that many bytes of JSON, at most [`MAX_MESSAGE`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::IpcError;

/// Bumped when a message changes incompatibly; part of the pipe name (`.v1`).
pub const VERSION: u32 = 1;
/// Largest accepted message body (1 MiB).
pub const MAX_MESSAGE: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    Hello {
        version: u32,
        client: String,
    },
    /// Open these folders (or select these items) in the running instance.
    OpenPaths {
        paths: Vec<PathBuf>,
        select: Option<PathBuf>,
        new_window: bool,
    },
    /// Bring the main window to the front.
    Activate,
    Ping,
    Pong,
    /// The request was received and will be handled.
    Ack,
    /// A message type this version does not know (sent by a newer client).
    #[serde(other)]
    Unknown,
}

/// The framed bytes of a message.
pub fn encode(message: &Message) -> Result<Vec<u8>, IpcError> {
    let body = serde_json::to_vec(message).map_err(IpcError::Encode)?;
    if body.len() > MAX_MESSAGE {
        return Err(IpcError::TooLarge(body.len()));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// The body length announced by a frame header; rejects oversized frames before reading them.
pub fn body_len(header: [u8; 4]) -> Result<usize, IpcError> {
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_MESSAGE {
        return Err(IpcError::TooLarge(len));
    }
    Ok(len)
}

pub fn decode(body: &[u8]) -> Result<Message, IpcError> {
    serde_json::from_slice(body).map_err(IpcError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let message = Message::OpenPaths {
            paths: vec![PathBuf::from(r"C:\Users\Raka\日本語")],
            select: None,
            new_window: false,
        };
        let framed = encode(&message).unwrap();
        let len = body_len(framed[..4].try_into().unwrap()).unwrap();
        assert_eq!(len, framed.len() - 4);
        assert_eq!(decode(&framed[4..]).unwrap(), message);
    }

    #[test]
    fn oversized_and_unknown_frames() {
        assert!(matches!(
            body_len((MAX_MESSAGE as u32 + 1).to_le_bytes()),
            Err(IpcError::TooLarge(_))
        ));
        assert_eq!(
            decode(br#"{"type":"FromTheFuture","x":1}"#).unwrap(),
            Message::Unknown
        );
        assert!(decode(b"not json").is_err());
    }
}
