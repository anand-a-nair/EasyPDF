//! Messages exchanged with the worker process.

use serde::{Deserialize, Serialize};

/// Which kernel resource ceilings the worker managed to apply to itself.
///
/// Not a boolean, for the same reason [`crate::protocol::Response`] validation
/// is not: "limits applied" bundles several independent facts, and platforms
/// differ in which they actually support. Reporting a single `true` when the
/// memory ceiling silently failed would misrepresent the containment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Core dumps are disabled.
    ///
    /// A crash dump of this process would contain the document, which may be
    /// confidential.
    pub core_dumps_disabled: bool,

    /// CPU-seconds after which the kernel terminates the worker.
    pub cpu_seconds: Option<u64>,

    /// Address-space ceiling in bytes, if the platform supports one.
    ///
    /// **`None` on macOS**, which rejects `RLIMIT_AS` and `RLIMIT_DATA` with
    /// `EINVAL`. There is no kernel-level memory backstop there, so memory
    /// containment rests entirely on the accounting in [`crate::limits`].
    pub address_space_bytes: Option<u64>,
}

impl ResourceLimits {
    /// Nothing applied.
    #[must_use]
    pub fn none() -> Self {
        Self { core_dumps_disabled: false, cpu_seconds: None, address_space_bytes: None }
    }

    /// Whether a kernel-enforced memory ceiling is in place.
    #[must_use]
    pub fn memory_capped(&self) -> bool {
        self.address_space_bytes.is_some()
    }
}

/// Whether the worker actually confined itself at startup.
///
/// Reported in [`Response::Ready`] so the host knows what it is talking to.
/// Sandboxing that silently fails is worse than none, because the whole
/// architecture is built on the assumption that it holds — so the worker says
/// plainly what it managed to apply, and the host surfaces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SandboxStatus {
    /// OS-level confinement is active.
    Enforced {
        /// Which mechanism was applied, e.g. "seatbelt".
        mechanism: String,
        /// Which kernel resource ceilings were applied alongside it.
        resource_limits: ResourceLimits,
    },

    /// Not confined, with the reason.
    ///
    /// **This is not a normal operating state.** It means the process handling
    /// untrusted input has ordinary user privileges.
    NotEnforced {
        /// Why confinement could not be applied.
        reason: String,
        /// Which kernel resource ceilings were applied, if any.
        resource_limits: ResourceLimits,
    },
}

impl SandboxStatus {
    /// Whether OS-level confinement is active.
    #[must_use]
    pub fn is_enforced(&self) -> bool {
        matches!(self, Self::Enforced { .. })
    }
}

/// A request from the trusted host to the sandboxed worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Hand the worker a document to open.
    ///
    /// Carries the **bytes**, never a path. This is not a convenience: the
    /// sandbox denies the worker all filesystem access (verified by test), so
    /// it could not open a path even if given one. The host reads the file and
    /// passes the contents across.
    ///
    /// The cost is a copy through the channel, bounded by
    /// [`crate::framing::MAX_FRAME_BYTES`]. Passing a file descriptor instead
    /// would avoid both the copy and the ceiling — inherited descriptors
    /// remain usable inside the sandbox — but needs `SCM_RIGHTS` on Unix and
    /// handle duplication on Windows. Tracked as OQ-010.
    OpenDocument {
        /// The document's raw bytes.
        data: Vec<u8>,
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

    /// Startup handshake. Must be the first message on a new worker.
    Handshake,

    /// Ask the worker to exit cleanly.
    Shutdown,
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

    /// Handshake reply, sent once at startup.
    Ready {
        /// Worker version, checked against the host's own.
        version: String,
        /// What confinement the worker managed to apply to itself.
        sandbox: SandboxStatus,
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

    /// The worker did not answer within the deadline.
    ///
    /// Treated the same as death: the process is killed rather than waited on,
    /// because a worker stuck on a pathological document will not recover.
    #[error("worker did not respond within {timeout_ms}ms")]
    Timeout {
        /// The deadline that elapsed.
        timeout_ms: u64,
    },

    /// The channel to the worker failed.
    #[error("worker channel failed: {0}")]
    Channel(String),
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
        let request = Request::OpenDocument { data: vec![1, 2, 3], password: None };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: Request = serde_json::from_str(&encoded).unwrap();
        assert_eq!(request, decoded);
    }

    #[test]
    fn sandbox_status_distinguishes_enforced_from_not() {
        let enforced = SandboxStatus::Enforced {
            mechanism: "seatbelt".into(),
            resource_limits: ResourceLimits::none(),
        };
        assert!(enforced.is_enforced());

        let not = SandboxStatus::NotEnforced {
            reason: "unsupported platform".into(),
            resource_limits: ResourceLimits::none(),
        };
        assert!(!not.is_enforced(), "unconfined must never report as enforced");
    }

    #[test]
    fn resource_limits_report_memory_gaps_rather_than_hiding_them() {
        // macOS rejects RLIMIT_AS, so this case is real, not hypothetical.
        let partial = ResourceLimits {
            core_dumps_disabled: true,
            cpu_seconds: Some(120),
            address_space_bytes: None,
        };
        assert!(!partial.memory_capped(), "a missing memory ceiling must be visible");
    }

    #[test]
    fn open_document_never_carries_a_path() {
        // Structural guarantee: the worker cannot name a file of its own
        // choosing, because it is never told a filename at all.
        let encoded =
            serde_json::to_string(&Request::OpenDocument { data: vec![37], password: None })
                .unwrap();
        assert!(encoded.contains("data"));
        assert!(!encoded.contains("path"), "a path must never cross this boundary");
    }
}
