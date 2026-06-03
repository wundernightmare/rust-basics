//! Error types for [`crate::ResilientHttpClient`].
//!
//! The `Transient` / `Fatal` split lets callers decide what to do: retry a
//! transient failure later (re-queue, backoff), but log-and-discard a fatal one
//! because retrying cannot help.

use thiserror::Error;

/// Error returned by [`crate::ResilientHttpClient::send`].
#[derive(Debug, Clone, Error)]
pub enum OutboundError {
    /// Temporary failure — retry later. Triggered by: circuit breaker open,
    /// local or upstream rate limit (HTTP 429), timeout / connection refused,
    /// HTTP 5xx, or the client shutting down.
    #[error("transient: {0}")]
    Transient(String),

    /// Permanent failure — retrying will not help. Triggered by: HTTP 4xx
    /// (except 429), TLS/cert failure, or an unparseable URL.
    #[error("fatal: {0}")]
    Fatal(String),
}

impl OutboundError {
    /// Returns `true` if this error is transient (retry-eligible).
    pub fn is_transient(&self) -> bool {
        matches!(self, OutboundError::Transient(_))
    }

    /// Returns `true` if this error is fatal (discard).
    pub fn is_fatal(&self) -> bool {
        matches!(self, OutboundError::Fatal(_))
    }

    /// Canonical error-type string for use as a Prometheus label.
    pub fn error_type(&self) -> &'static str {
        match self {
            OutboundError::Transient(_) => "transient",
            OutboundError::Fatal(_) => "fatal",
        }
    }
}

/// Error returned by [`crate::ResilientHttpClient::shutdown`].
#[derive(Debug, Error)]
pub enum ShutdownError {
    /// The graceful-shutdown deadline elapsed with requests still in flight.
    #[error("shutdown timed out with {in_flight} requests still in flight")]
    Timeout { in_flight: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_and_fatal_classify_correctly() {
        assert!(OutboundError::Transient("x".into()).is_transient());
        assert!(!OutboundError::Transient("x".into()).is_fatal());
        assert!(OutboundError::Fatal("x".into()).is_fatal());
        assert_eq!(OutboundError::Fatal("x".into()).error_type(), "fatal");
    }
}
