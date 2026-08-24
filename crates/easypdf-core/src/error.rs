//! Error types for document operations.

/// Convenient result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong handling a document.
///
/// Variants are deliberately specific: a user-facing message like "this file
/// is damaged" is useless for diagnosis, and a tool that cannot say *what* it
/// failed to understand cannot be trusted with the file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file is not a PDF, or its header is unrecoverably damaged.
    #[error("not a valid PDF document: {reason}")]
    NotAPdf {
        /// What specifically failed to parse.
        reason: String,
    },

    /// The document is encrypted and no valid password was supplied.
    #[error("document is encrypted and requires a password")]
    PasswordRequired,

    /// A page index was outside the document.
    #[error("page {requested} is out of range (document has {total} pages)")]
    PageOutOfRange {
        /// The page that was asked for.
        requested: usize,
        /// How many pages the document actually has.
        total: usize,
    },

    /// A structural limit was exceeded — see `ideas/07-security.md` (T3).
    ///
    /// This is a defense against decompression bombs and pathological object
    /// graphs, not a bug. Hitting it should surface a clear message.
    #[error("document exceeds the {limit} safety limit ({value} > {maximum})")]
    LimitExceeded {
        /// Which limit was hit.
        limit: &'static str,
        /// The value the document asked for.
        value: u64,
        /// The configured maximum.
        maximum: u64,
    },

    /// The operation is understood but deliberately unsupported.
    ///
    /// Used where honest refusal beats a wrong result — XFA forms being the
    /// canonical example. See `ideas/06-features.md`.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// Underlying I/O failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}
