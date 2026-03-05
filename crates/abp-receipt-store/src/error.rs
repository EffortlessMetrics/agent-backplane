// SPDX-License-Identifier: MIT OR Apache-2.0

//! Error types for receipt store operations.

use std::fmt;

/// Errors from receipt store operations.
#[derive(Debug)]
pub enum StoreError {
    /// A receipt with this run ID already exists.
    DuplicateId(String),
    /// The provided ID is not a valid UUID.
    InvalidId(String),
    /// I/O error during file operations.
    Io(std::io::Error),
    /// JSON serialization/deserialization error.
    Json(serde_json::Error),
    /// Generic storage failure.
    Other(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "duplicate receipt id: {id}"),
            Self::InvalidId(id) => write!(f, "invalid receipt id: {id}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Other(msg) => write!(f, "store error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
