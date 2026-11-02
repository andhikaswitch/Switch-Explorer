//! Single instance and messages between Switch Explorer processes (named pipe, ARCHITECTURE §12).
//!
//! Platform crate: `unsafe` is allowed here, with a `// SAFETY:` comment on every block
//! (docs/CODING-STANDARDS.md §5).

pub mod error;
pub mod pipe;
pub mod protocol;

pub use error::IpcError;
pub use pipe::{Server, ServerHandle, send};
pub use protocol::{Message, VERSION};
