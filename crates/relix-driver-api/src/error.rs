/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Configuration,
    Connection,
    Authentication,
    Tls,
    Timeout,
    Cancelled,
    Query,
    ConcurrentModification,
    AmbiguousRow,
    Unsupported,
    Protocol,
    Io,
    Internal,
}

impl ErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Configuration => "configuration error",
            Self::Connection => "connection error",
            Self::Authentication => "authentication failed",
            Self::Tls => "TLS error",
            Self::Timeout => "timed out",
            Self::Cancelled => "cancelled",
            Self::Query => "query error",
            Self::ConcurrentModification => "concurrent modification",
            Self::AmbiguousRow => "ambiguous row",
            Self::Unsupported => "unsupported",
            Self::Protocol => "protocol error",
            Self::Io => "I/O error",
            Self::Internal => "internal error",
        }
    }

    pub fn is_connection_loss(self) -> bool {
        matches!(self, Self::Connection | Self::Io)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub struct DriverError {
    pub kind: ErrorKind,
    pub message: String,
    pub detail: Option<String>,
    pub position: Option<usize>,
}

impl DriverError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            detail: None,
            position: None,
        }
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Configuration, message)
    }

    pub fn connection(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Connection, message)
    }

    pub fn query(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Query, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_position(mut self, position: usize) -> Self {
        self.position = Some(position);
        self
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.label(), self.message)?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_kind_and_detail() {
        let error = DriverError::query("syntax error").with_detail("near FROM");
        assert_eq!(error.to_string(), "query error: syntax error (near FROM)");
    }
}
