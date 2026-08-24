//! Rasterization and the tile cache.
//!
//! Rendering is delegated to PDFium (see `ideas/03-tech-decisions.md`, TD-003),
//! vendored as a hash-pinned prebuilt per TD-007 and installed by
//! `scripts/fetch-pdfium.sh`.
//!
//! The cache exists from the start because an unbounded one is how a viewer
//! turns a 400 MB scanned book into an out-of-memory crash. The budget in
//! `ideas/04-performance-budget.md` assumes a hard ceiling.

// Tests legitimately assert on known-good values; the panic lints exist to
// keep unwraps out of parsing paths, not out of assertions.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod cache;
pub mod pdfium;
pub mod wire;

pub use cache::{Tile, TileCache, TileKey, ZoomBucket};
pub use pdfium::{PdfiumRasterizer, RenderError};
pub use wire::{bgra_to_rgba, decode_page, encode_page};

/// Something that can rasterize pages.
///
/// Implemented by the PDFium-backed engine, and by fakes in tests so the cache
/// can be exercised without a real renderer.
pub trait Rasterizer {
    /// Error type produced by this rasterizer.
    type Error;

    /// Renders one page at the given zoom, returning raw BGRA pixels.
    fn rasterize(&self, key: TileKey) -> Result<Tile, Self::Error>;
}
