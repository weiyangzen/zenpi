//! Error types shared by the two zenpi entry points.

use std::io;

use thiserror::Error;

use crate::{
    backend::BackendError, config::ConfigError, core::AgentError, protocol::ProtocolError,
    session::SessionError,
};

/// The top-level error returned by the command-line binary.
#[derive(Debug, Error)]
pub enum ZenpiError {
    #[error("invalid arguments: {0}")]
    Arguments(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error("{0}")]
    Message(String),
}

impl ZenpiError {
    pub fn arguments(message: impl Into<String>) -> Self {
        Self::Arguments(message.into())
    }
}
