//! Messages exchanged with the worker process.

use serde::{Deserialize, Serialize};

/// A request from the trusted host to the sandboxed worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Parse only the cross-reference table and trailer.
    ///
    /// Deliberately minimal: this is what makes a 900-page document open as
    /// fast as a 2-page one. Nothing else is parsed until it is needed.
    /// See `ideas/02-architecture.md`.
    OpenDocument {
        /// Index into the host's table of already-opened file handles.
        ///
        /// Never a path — the worker has no filesystem access and must not be
        /// able to name a file it was not given.
        handle: u32,
        /// Password, if the document is encrypted.
        password: Option<String>,
    },

    /// Rasterize one page.
    RenderPage {
        /// Zero-based page index.
        page: usize,
        /// Zoom factor.
        zoom: f32,
        /// Rotation in degrees clockwise.
        rotation: i32,
    },

    /// Extract text for selection and search.
    ExtractText {
        /// Zero-based page index.
        page: usize,
    },

    /// Release all resources for the current document.
    CloseDocument,
}

/// A reply from the worker.
///
/// The host validates every field before use. A compromised worker sends
/// well-formed lies, so "it deserialized" proves nothing about the contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Response {
    /// The document opened successfully.
    DocumentOpened {
        /// Number of pages.
        page_count: usize,
        /// Whether the document is encrypted.
        encrypted: bool,
        /// Whether the document carries at least one digital signature.
        signed: bool,
    },

    /// A rasterized page.
    PageRendered {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// BGRA pixel data.
        pixels: Vec<u8>,
    },

    /// Extracted text.
    TextExtracted {
        /// The page's text in reading order.
        text: String,
    },

    /// The request completed with nothing to return.
    Ok,

    /// The request failed.
    Failed(WorkerError),
}

/// Why a worker request failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[non_exhaustive]
pub enum WorkerError {
    /// The document could not be parsed.
    #[error("malformed document: {0}")]
    Malformed(String),

    /// A password is required, or the supplied one was wrong.
    #[error("incorrect or missing password")]
    BadPassword,

    /// A safety limit was exceeded. See [`crate::limits`].
    #[error("safety limit exceeded: {0}")]
    LimitExceeded(String),

    /// The feature is recognized but deliberately unsupported.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// The worker died — very possibly while being exploited.
    ///
    /// The host must treat this as potentially adversarial: restart the worker
    /// rather than retrying the same input in the same process.
    #[error("worker process terminated unexpectedly")]
    WorkerDied,
}

impl Response {
    /// Validates a response before the host acts on it.
    ///
    /// Specifically guards against a malicious worker returning a pixel buffer
    /// whose length disagrees with its declared dimensions — the kind of
    /// mismatch that turns into an out-of-bounds read in the consumer.
    #[must_use]
    pub fn is_self_consistent(&self) -> bool {
        match self {
            Self::PageRendered { width, height, pixels } => {
                let expected =
                    (*width as usize).checked_mul(*height as usize).and_then(|n| n.checked_mul(4));
                expected == Some(pixels.len())
            }
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_formed_render_response_is_accepted() {
        let response = Response::PageRendered { width: 2, height: 3, pixels: vec![0; 24] };
        assert!(response.is_self_consistent());
    }

    #[test]
    fn pixel_buffer_shorter_than_declared_size_is_rejected() {
        // A hostile worker under-reporting its buffer is how the host gets an
        // out-of-bounds read.
        let response = Response::PageRendered { width: 100, height: 100, pixels: vec![0; 10] };
        assert!(!response.is_self_consistent());
    }

    #[test]
    fn dimension_overflow_is_rejected_rather_than_wrapping() {
        let response =
            Response::PageRendered { width: u32::MAX, height: u32::MAX, pixels: vec![0; 4] };
        assert!(!response.is_self_consistent());
    }

    #[test]
    fn requests_round_trip_through_serialization() {
        let request = Request::OpenDocument { handle: 7, password: None };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: Request = serde_json::from_str(&encoded).unwrap();
        assert_eq!(request, decoded);
    }

    #[test]
    fn open_document_carries_a_handle_not_a_path() {
        // Structural guarantee: the worker cannot name a file it was not given.
        let encoded =
            serde_json::to_string(&Request::OpenDocument { handle: 3, password: None }).unwrap();
        assert!(encoded.contains("handle"));
        assert!(!encoded.contains("path"));
    }
}
