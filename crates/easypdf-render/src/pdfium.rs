//! PDFium-backed rasterization.
//!
//! Wraps the vendored PDFium library (TD-007). Two things about this module
//! are security-relevant rather than incidental:
//!
//! **The library is loaded from a path, which requires filesystem access.**
//! It must therefore be bound *before* the worker sandboxes itself. That is
//! not a weakening of the sandbox: loading a known, hash-pinned library at
//! startup is not the same as reading untrusted input, and the confinement
//! still lands before the first document byte is touched.
//!
//! **The vendored build has no V8.** `pdf_enable_v8 = false`, verified from
//! `args.gn` and enforced by `scripts/fetch-pdfium.sh`, so document JavaScript
//! cannot execute — there is no engine present to execute it. This makes the
//! policy in `ideas/07-security.md` a structural property rather than a
//! setting someone could flip later.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use pdfium_render::prelude::{
    PdfDocument, PdfRenderConfig, PdfSearchDirection, PdfSearchOptions, Pdfium,
};

use easypdf_core::text::{SearchHit, TextRect};

use crate::cache::{Tile, TileKey};

/// The process-wide PDFium instance.
///
/// **PDFium's library initialization is once per process, not once per
/// object.** `FPDF_InitLibrary` followed by `FPDF_DestroyLibrary` cannot be
/// repeated — a second initialization after teardown crashes with `SIGTRAP`.
/// This was found the hard way: nine tests each constructing their own
/// instance killed the test binary outright.
///
/// So the instance is a singleton, created once and never dropped. The first
/// successful load wins; a later call naming a different directory is ignored
/// rather than reinitializing, because reinitializing is what crashes.
static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Serializes all rendering.
///
/// Even with `pdfium-render`'s `thread_safe` feature enabled, rendering two
/// documents concurrently crashes the process with `SIGTRAP` — measured, not
/// assumed: the render tests pass one at a time and abort the test binary when
/// run in parallel. The feature guards the bindings, not the document
/// lifecycle.
///
/// Serializing here rather than documenting a caveat, because a caveat would
/// be a footgun aimed at whoever next calls this from a thread pool. The cost
/// is nil in practice: the worker is a single process handling one document at
/// a time, and progressive rendering is about *ordering* work, not running it
/// in parallel.
static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// Acquires the render lock, recovering from poisoning.
///
/// A panic during one render must not permanently disable rendering — the
/// guarded data is `()`, so there is no corrupt state to protect.
fn render_lock() -> MutexGuard<'static, ()> {
    RENDER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Why the renderer could not be created or used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    /// The PDFium library could not be found or loaded.
    #[error("could not load PDFium from {path}: {source}")]
    LibraryUnavailable {
        /// Where the library was expected.
        path: PathBuf,
        /// The underlying loader error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The document could not be opened.
    #[error("could not open document: {0}")]
    OpenFailed(String),

    /// A page could not be rendered.
    #[error("could not render page {page}: {reason}")]
    PageFailed {
        /// Zero-based page index.
        page: usize,
        /// What went wrong.
        reason: String,
    },

    /// The requested page does not exist.
    #[error("page {requested} is out of range (document has {total} pages)")]
    PageOutOfRange {
        /// The page that was asked for.
        requested: usize,
        /// How many pages the document has.
        total: usize,
    },

    /// The rendered dimensions were not usable.
    #[error("refusing to render a {width}x{height} page")]
    ImplausibleDimensions {
        /// Computed width.
        width: i32,
        /// Computed height.
        height: i32,
    },
}

/// Largest number of hits returned for one search.
///
/// A one-letter query on a long document can match tens of thousands of times.
/// Returning them all would stall the UI and blow the frame limit, and nobody
/// steps through 40,000 results. The caller is told the count was capped.
pub const MAX_SEARCH_HITS: usize = 500;

/// Largest bitmap edge the renderer will produce, in pixels.
///
/// A page with absurd dimensions combined with high zoom can request a bitmap
/// that would exhaust memory. This is the render-side counterpart to the
/// parsing limits in `easypdf-ffi`.
const MAX_BITMAP_EDGE: i32 = 20_000;

/// Handle to the process-wide PDFium instance.
///
/// Cheap to construct after the first call — it does not own the library, it
/// refers to the singleton described on [`PDFIUM`].
pub struct PdfiumRasterizer {
    pdfium: &'static Pdfium,
}

impl std::fmt::Debug for PdfiumRasterizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfiumRasterizer").finish_non_exhaustive()
    }
}

impl PdfiumRasterizer {
    /// Binds to the PDFium shared library in `directory`.
    ///
    /// Call before applying any sandbox — see the module documentation. Safe
    /// to call repeatedly; only the first call does any work.
    pub fn load_from(directory: &Path) -> Result<Self, RenderError> {
        let instance = PDFIUM.get_or_init(|| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&directory))
                .map(Pdfium::new)
                .map_err(|error| error.to_string())
        });

        match instance {
            Ok(pdfium) => Ok(Self { pdfium }),
            Err(reason) => Err(RenderError::LibraryUnavailable {
                path: directory.to_path_buf(),
                source: reason.clone().into(),
            }),
        }
    }

    /// Parses a document once and keeps it open.
    ///
    /// **This is the difference between a usable viewer and an unusable one.**
    /// Every operation previously re-parsed the whole file: each page render,
    /// each size query, each search. On a large document that is seconds of
    /// work repeated for every scroll tick.
    ///
    /// Takes ownership of the bytes because PDFium reads from that buffer
    /// lazily for the document's whole lifetime — it must not be freed or
    /// moved underneath it.
    pub fn open(
        &self,
        bytes: Vec<u8>,
        password: Option<&str>,
    ) -> Result<OpenDocument, RenderError> {
        let _guard = render_lock();

        let document = self
            .pdfium
            .load_pdf_from_byte_vec(bytes, password)
            .map_err(|error| RenderError::OpenFailed(error.to_string()))?;

        Ok(OpenDocument { document })
    }
}

/// A parsed document, held open for the session.
///
/// Not `Send`: PDFium state belongs to the thread that created it, and the
/// worker is single-threaded by design. The type system enforcing that is a
/// feature, not an inconvenience.
pub struct OpenDocument {
    document: PdfDocument<'static>,
}

impl std::fmt::Debug for OpenDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenDocument").field("pages", &self.page_count()).finish()
    }
}

impl OpenDocument {
    /// Number of pages.
    #[must_use]
    pub fn page_count(&self) -> usize {
        usize::try_from(self.document.pages().len()).unwrap_or(0)
    }

    fn page_index(&self, page: usize) -> Result<i32, RenderError> {
        let total = self.page_count();
        if page >= total {
            return Err(RenderError::PageOutOfRange { requested: page, total });
        }
        i32::try_from(page).map_err(|_| RenderError::PageOutOfRange { requested: page, total })
    }

    /// A page's dimensions in points, without rendering it.
    pub fn page_size(&self, page: usize) -> Result<(f32, f32), RenderError> {
        let _guard = render_lock();
        let index = self.page_index(page)?;

        let handle = self
            .document
            .pages()
            .get(index)
            .map_err(|error| RenderError::PageFailed { page, reason: error.to_string() })?;

        Ok((handle.width().value, handle.height().value))
    }

    /// Rasterizes one page.
    pub fn render(&self, key: TileKey) -> Result<Tile, RenderError> {
        let _guard = render_lock();
        let index = self.page_index(key.page)?;

        let page = self.document.pages().get(index).map_err(|error| RenderError::PageFailed {
            page: key.page,
            reason: error.to_string(),
        })?;

        let zoom = key.zoom.zoom();
        let width = (page.width().value * zoom).round() as i32;
        let height = (page.height().value * zoom).round() as i32;

        if width <= 0 || height <= 0 || width > MAX_BITMAP_EDGE || height > MAX_BITMAP_EDGE {
            return Err(RenderError::ImplausibleDimensions { width, height });
        }

        let config = PdfRenderConfig::new().set_target_size(width, height);

        let bitmap = page.render_with_config(&config).map_err(|error| RenderError::PageFailed {
            page: key.page,
            reason: error.to_string(),
        })?;

        Ok(Tile {
            width: u32::try_from(bitmap.width()).unwrap_or(0),
            height: u32::try_from(bitmap.height()).unwrap_or(0),
            pixels: bitmap.as_raw_bytes(),
        })
    }

    /// Extracts a page's text in reading order.
    pub fn extract_text(&self, page: usize) -> Result<String, RenderError> {
        let _guard = render_lock();
        let index = self.page_index(page)?;

        let handle = self
            .document
            .pages()
            .get(index)
            .map_err(|error| RenderError::PageFailed { page, reason: error.to_string() })?;

        let text = handle
            .text()
            .map_err(|error| RenderError::PageFailed { page, reason: error.to_string() })?;

        Ok(text.all())
    }

    /// Searches the whole document, returning positioned hits.
    ///
    /// Returns the hits and whether the result was truncated at
    /// [`MAX_SEARCH_HITS`]. Truncation is reported rather than hidden — a
    /// silently capped result count is a lie about the document.
    pub fn search(
        &self,
        query: &str,
        match_case: bool,
    ) -> Result<(Vec<SearchHit>, bool), RenderError> {
        if query.is_empty() {
            return Ok((Vec::new(), false));
        }

        let _guard = render_lock();

        let options = PdfSearchOptions::new().match_case(match_case);
        let mut hits = Vec::new();
        let mut truncated = false;

        for (index, page) in self.document.pages().iter().enumerate() {
            let Ok(text) = page.text() else {
                // A page whose text layer cannot be read is skipped rather than
                // failing the whole search — one bad page should not make the
                // other four hundred unsearchable.
                continue;
            };

            let Ok(search) = text.search(query, &options) else {
                continue;
            };

            for segments in search.iter(PdfSearchDirection::SearchForward) {
                if hits.len() >= MAX_SEARCH_HITS {
                    truncated = true;
                    break;
                }

                let rects: Vec<TextRect> = segments
                    .iter()
                    .map(|segment| {
                        let bounds = segment.bounds();
                        TextRect {
                            left: bounds.left().value,
                            bottom: bounds.bottom().value,
                            right: bounds.right().value,
                            top: bounds.top().value,
                        }
                    })
                    .filter(|rect| !rect.is_degenerate())
                    .collect();

                if !rects.is_empty() {
                    hits.push(SearchHit { page: index, rects });
                }
            }

            if truncated {
                break;
            }
        }

        Ok((hits, truncated))
    }
}
