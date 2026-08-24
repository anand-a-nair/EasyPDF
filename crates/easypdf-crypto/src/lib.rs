//! Encryption, permissions, and digital signatures.
//!
//! Isolated from the rest of the workspace because it needs the most careful
//! review and should change the least. See `ideas/06-features.md` for the
//! distinction between a *visible* signature (cosmetic) and a *digital*
//! signature (cryptographic) — conflating them in the UI misleads users about
//! what has actually been proven.
//!
//! Nothing here is implemented yet. The types encode policy decisions that are
//! already made, so they are worth pinning down before the implementation
//! arrives with Phase 3.

// Tests legitimately assert on known-good values; the panic lints exist to
// keep unwraps out of parsing paths, not out of assertions.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod permissions;

pub use permissions::Permissions;

/// Encryption algorithms defined by the PDF standard security handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Algorithm {
    /// RC4 with a 40-bit key. Broken. Read-only support for legacy documents.
    Rc4_40,
    /// RC4 with a 128-bit key. Broken. Read-only support for legacy documents.
    Rc4_128,
    /// AES-128. Read-only support.
    Aes128,
    /// AES-256. The only algorithm this tool will write.
    Aes256,
}

impl Algorithm {
    /// Whether documents using this algorithm can be opened.
    ///
    /// Everything is readable: refusing to open a legacy document a user
    /// legitimately owns would be user-hostile, and the weakness is in the
    /// document, not in the act of reading it.
    #[must_use]
    pub fn can_read(self) -> bool {
        true
    }

    /// Whether new documents may be encrypted with this algorithm.
    ///
    /// **Only AES-256.** Offering weak ciphers for new documents is a footgun
    /// with no upside — the user cannot evaluate the tradeoff, and there is no
    /// legitimate reason to create a new RC4-encrypted file in 2026.
    #[must_use]
    pub fn can_write(self) -> bool {
        matches!(self, Self::Aes256)
    }

    /// Whether this algorithm is considered cryptographically broken.
    #[must_use]
    pub fn is_broken(self) -> bool {
        matches!(self, Self::Rc4_40 | Self::Rc4_128)
    }
}

/// PAdES conformance levels for digital signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SignatureLevel {
    /// Basic: a PKCS#7 detached signature over the document byte ranges.
    BasicB,
    /// Basic plus an RFC 3161 trusted timestamp.
    BasicT,
}

/// The result of verifying a signature.
///
/// Deliberately **not** a boolean. "Valid" collapses several independent
/// questions, and collapsing them into one green checkmark is exactly how
/// users get misled about what was actually proven. See `ideas/06-features.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// The signed digest matches the document's current bytes.
    pub bytes_unmodified: bool,
    /// The signed byte range covers the whole document.
    ///
    /// When false, content was appended after signing — the incremental-update
    /// attack. The signature may be cryptographically valid while covering only
    /// part of what the user is looking at.
    pub covers_whole_document: bool,
    /// The signing certificate chains to a trusted root.
    pub chain_trusted: bool,
    /// Revocation status was checked and the certificate was not revoked.
    pub revocation_checked: bool,
    /// A trusted timestamp was present and valid.
    pub timestamp_valid: bool,
}

impl Verification {
    /// Whether every check passed.
    ///
    /// Callers should still show the individual fields. This is a convenience
    /// for control flow, **not** something to render as a single checkmark.
    #[must_use]
    pub fn fully_verified(&self) -> bool {
        self.bytes_unmodified
            && self.covers_whole_document
            && self.chain_trusted
            && self.revocation_checked
            && self.timestamp_valid
    }

    /// Whether the user must be warned, even though cryptography checks out.
    ///
    /// The dangerous case: the signature is mathematically valid but does not
    /// cover the whole document.
    #[must_use]
    pub fn needs_warning(&self) -> bool {
        self.bytes_unmodified && !self.covers_whole_document
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_aes256_may_be_written() {
        assert!(Algorithm::Aes256.can_write());
        for weak in [Algorithm::Rc4_40, Algorithm::Rc4_128, Algorithm::Aes128] {
            assert!(!weak.can_write(), "{weak:?} must not be writable");
        }
    }

    #[test]
    fn all_algorithms_remain_readable() {
        // Legacy documents must still open; the weakness is in the file.
        for algorithm in
            [Algorithm::Rc4_40, Algorithm::Rc4_128, Algorithm::Aes128, Algorithm::Aes256]
        {
            assert!(algorithm.can_read());
        }
    }

    #[test]
    fn rc4_is_flagged_as_broken() {
        assert!(Algorithm::Rc4_40.is_broken());
        assert!(Algorithm::Rc4_128.is_broken());
        assert!(!Algorithm::Aes256.is_broken());
    }

    #[test]
    fn partial_coverage_triggers_a_warning_despite_valid_crypto() {
        // The incremental-update attack: valid signature, appended content.
        let v = Verification {
            bytes_unmodified: true,
            covers_whole_document: false,
            chain_trusted: true,
            revocation_checked: true,
            timestamp_valid: true,
        };
        assert!(!v.fully_verified());
        assert!(v.needs_warning(), "partial coverage must warn the user");
    }

    #[test]
    fn full_verification_requires_every_check() {
        let mut v = Verification {
            bytes_unmodified: true,
            covers_whole_document: true,
            chain_trusted: true,
            revocation_checked: true,
            timestamp_valid: true,
        };
        assert!(v.fully_verified());
        v.revocation_checked = false;
        assert!(!v.fully_verified());
    }
}
