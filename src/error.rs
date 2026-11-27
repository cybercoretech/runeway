//! Library error type.

use std::{io, path::PathBuf};

/// Errors the library reports. The binary turns them into messages.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid {what}: {source}")]
    Json {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("agent-deck: {0}")]
    AgentDeck(String),
    #[error("opencode database: {0}")]
    OpenCode(String),
}

/// Library result.
pub type Result<T> = std::result::Result<T, Error>;
