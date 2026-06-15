//! AI error taxonomy (ADR-0023 Decision 8).
//!
//! Independent of [`dbboard_core::DbError`]. AI errors never travel
//! over the desktop ↔ web HTTP contract (ADR-0009's English-prefix
//! translation rule does not apply), so `dbboard-ui` translates each
//! variant directly to its own Fluent key.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    /// Missing API key, malformed model id, or other configuration
    /// problem detected before any request is issued.
    #[error("ai configuration error: {0}")]
    Configuration(String),

    /// Transport-level failure reaching the provider (timeout, TLS,
    /// connection refused). Distinct from `Provider` so the UI can
    /// suggest a retry path.
    #[error("ai network error: {0}")]
    Network(String),

    /// Provider returned an error envelope (rate limit, content
    /// filter, model unavailable) or a malformed response.
    #[error("ai provider error: {0}")]
    Provider(String),

    /// Caller-imposed budget exceeded. Wired for Stage 2; the variant
    /// exists now so adding budget enforcement later is not a breaking
    /// change.
    #[error("ai quota exceeded: {0}")]
    Quota(String),

    /// The in-flight request was cancelled by the user (or by the UI
    /// during shutdown). Surfacing this distinctly lets the panel
    /// avoid showing it as a failure.
    #[error("ai request cancelled")]
    Cancelled,
}

pub type AiResult<T> = Result<T, AiError>;

#[cfg(test)]
mod tests {
    use super::{AiError, AiResult};

    #[test]
    fn display_covers_every_variant() {
        let cases = [
            (
                AiError::Configuration("missing key".into()),
                "ai configuration error: missing key",
            ),
            (
                AiError::Network("timeout".into()),
                "ai network error: timeout",
            ),
            (
                AiError::Provider("rate_limit".into()),
                "ai provider error: rate_limit",
            ),
            (
                AiError::Quota("daily cap".into()),
                "ai quota exceeded: daily cap",
            ),
            (AiError::Cancelled, "ai request cancelled"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn result_alias_round_trips() {
        // Push the values through a `Vec<AiResult<u32>>` so the alias is
        // exercised at a real binding site rather than as a literal that
        // clippy would flag (`unnecessary_literal_unwrap` /
        // `unnecessary_wraps`). The point of the test is that
        // `AiResult<T>` and `Result<T, AiError>` are interchangeable.
        let mut both: Vec<AiResult<u32>> = Vec::new();
        both.push(Ok(7));
        both.push(Err(AiError::Cancelled));

        let plain_ok: Result<u32, AiError> = both.remove(0);
        let plain_err: Result<u32, AiError> = both.remove(0);
        assert_eq!(plain_ok.unwrap(), 7);
        assert!(matches!(plain_err.unwrap_err(), AiError::Cancelled));
    }
}
