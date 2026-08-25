//! Core document model for EasyPDF.
//!
//! This crate owns the in-memory representation of a PDF and every mutation
//! that can be applied to it. It has no knowledge of rendering or of the UI.
//!
//! Two invariants matter more than anything else here:
//!
//! 1. **Never silently corrupt a document.** Operations that cannot be
//!    performed correctly return an error rather than guessing.
//! 2. **Every mutation is reversible.** All changes go through [`Command`],
//!    which is what makes undo/redo a property of the design rather than a
//!    feature bolted on later.
//!
//! See `ideas/02-architecture.md` for how this fits the wider system.

// Tests legitimately assert on known-good values; the panic lints exist to
// keep unwraps out of parsing paths, not out of assertions.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod command;
pub mod document;
pub mod error;
pub mod text;

pub use command::{Command, CommandStack};
pub use document::{Document, Page, PageIndex, PageSize, Rotation};
pub use error::{Error, Result};
pub use text::{OutlineEntry, SearchHit, TextRect};
