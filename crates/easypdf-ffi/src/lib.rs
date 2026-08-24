//! Protocol for the sandboxed parse and render worker.
//!
//! Every byte of an untrusted PDF is handled in a separate low-privilege
//! process. This crate defines the narrow, typed channel across that boundary.
//! See `ideas/07-security.md` (T1) and decision D-005.
//!
//! **This is a security boundary, not an abstraction.** Two rules follow:
//!
//! - Messages carry plain data only — no paths the worker could widen into
//!   filesystem access, no URLs, no callbacks.
//! - The host trusts nothing the worker returns. A compromised worker will
//!   send well-formed lies, so every field is validated on arrival.
//!
//! The current state is honest about its limits: the protocol and the limits
//! are defined and tested, but **OS-level sandboxing is not yet applied** —
//! that lands with the worker binary in Phase 0. Until then this is a process
//! boundary, not yet a privilege boundary.

// Tests legitimately assert on known-good values; the panic lints exist to
// keep unwraps out of parsing paths, not out of assertions.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod limits;
pub mod protocol;

pub use limits::Limits;
pub use protocol::{Request, Response, WorkerError};
