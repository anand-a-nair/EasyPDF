//! Document session: owns the worker, the tile cache, and the open document.
//!
//! This is the only place in the shell that holds state. Everything here is
//! orchestration — no PDF logic, which lives in the crates. See
//! `ideas/02-architecture.md`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use easypdf_ffi::protocol::{Request, Response, WorkerError};
use easypdf_ffi::worker::Worker;
use easypdf_render::cache::{Tile, TileCache, TileKey, ZoomBucket};
use easypdf_render::wire::bgra_to_rgba;

/// Memory ceiling for cached page bitmaps.
///
/// Part of the 150 MB idle RSS budget in `ideas/04-performance-budget.md`. An
/// unbounded cache is how a viewer turns a large scanned document into an
/// out-of-memory crash, so this is set at construction, not after the first
/// bug report.
const CACHE_BUDGET_BYTES: usize = 96 * 1024 * 1024;

/// What the frontend needs to know about an open document.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentInfo {
    /// File name only — never the full path, which the UI has no use for.
    pub name: String,
    /// Number of pages.
    pub page_count: usize,
    /// Whether the document was encrypted.
    pub encrypted: bool,
}

/// A rendered page, ready for the canvas.
pub(crate) struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data, converted from PDFium's BGRA.
    pub pixels: Vec<u8>,
}

impl RenderedPage {
    /// Packs the page for transport. See `easypdf_render::wire`.
    pub(crate) fn into_wire_format(self) -> Vec<u8> {
        easypdf_render::wire::encode_page(self.width, self.height, &self.pixels)
    }
}

struct OpenDocument {
    info: DocumentInfo,
    bytes: Vec<u8>,
}

/// The shell's document state.
pub(crate) struct Session {
    worker_path: PathBuf,
    worker: Mutex<Option<Worker>>,
    cache: Mutex<TileCache>,
    document: Mutex<Option<OpenDocument>>,
}

impl Session {
    /// Creates an empty session. No worker is spawned until a document opens.
    #[must_use]
    pub(crate) fn new(worker_path: PathBuf) -> Self {
        Self {
            worker_path,
            worker: Mutex::new(None),
            cache: Mutex::new(TileCache::new(CACHE_BUDGET_BYTES)),
            document: Mutex::new(None),
        }
    }

    /// Opens a document from disk.
    ///
    /// The host reads the file; the worker never sees a path. See D-019.
    pub(crate) fn open(&self, path: &Path) -> Result<DocumentInfo, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("could not read file: {error}"))?;

        let name = path
            .file_name()
            .map_or_else(|| "document.pdf".to_owned(), |n| n.to_string_lossy().into_owned());

        let response = self
            .send(&Request::OpenDocument { data: bytes.clone(), password: None })
            .map_err(|error| error.to_string())?;

        match response {
            Response::DocumentOpened { page_count, encrypted, .. } => {
                let info = DocumentInfo { name, page_count, encrypted };

                // A new document invalidates every cached tile — they are keyed
                // by page and zoom, not by document.
                self.cache.lock().map_err(|_| "cache lock poisoned")?.clear();
                *self.document.lock().map_err(|_| "document lock poisoned")? =
                    Some(OpenDocument { info: info.clone(), bytes });

                Ok(info)
            }
            Response::Failed(error) => Err(error.to_string()),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    /// Renders a page, serving from cache when possible.
    pub(crate) fn render(&self, page: usize, zoom: f32) -> Result<RenderedPage, String> {
        let key = TileKey { page, zoom: ZoomBucket::from_zoom(zoom), rotation: 0 };

        if let Ok(mut cache) = self.cache.lock()
            && let Some(tile) = cache.get(&key)
        {
            return Ok(RenderedPage {
                width: tile.width,
                height: tile.height,
                pixels: tile.pixels.clone(),
            });
        }

        let response = self
            .send(&Request::RenderPage { page, zoom: key.zoom.zoom(), rotation: 0 })
            .map_err(|error| error.to_string())?;

        match response {
            Response::PageRendered { width, height, pixels } => {
                let pixels = bgra_to_rgba(pixels);

                if let Ok(mut cache) = self.cache.lock() {
                    cache.insert(key, Tile { width, height, pixels: pixels.clone() });
                }

                Ok(RenderedPage { width, height, pixels })
            }
            Response::Failed(error) => Err(error.to_string()),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    /// A page's size in points, without rendering it.
    pub(crate) fn page_size(&self, page: usize) -> Result<(f32, f32), String> {
        let response = self.send(&Request::PageSize { page }).map_err(|error| error.to_string())?;

        match response {
            Response::PageSize { width, height } => Ok((width, height)),
            Response::Failed(error) => Err(error.to_string()),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    /// Closes the document and releases its memory.
    pub(crate) fn close(&self) -> Result<(), String> {
        let _ = self.send(&Request::CloseDocument);
        *self.document.lock().map_err(|_| "document lock poisoned")? = None;
        self.cache.lock().map_err(|_| "cache lock poisoned")?.clear();
        Ok(())
    }

    /// Currently open document, if any.
    pub(crate) fn info(&self) -> Option<DocumentInfo> {
        self.document.lock().ok()?.as_ref().map(|d| d.info.clone())
    }

    /// Sends a request, spawning or restarting the worker as needed.
    ///
    /// A dead worker is replaced rather than reused, and the reopened document
    /// is re-sent — see D-005 and D-017. There is deliberately **no** fallback
    /// to in-process parsing if the worker cannot be started.
    fn send(&self, request: &Request) -> Result<Response, WorkerError> {
        let mut slot =
            self.worker.lock().map_err(|_| WorkerError::Channel("worker lock poisoned".into()))?;

        if slot.is_none() {
            *slot = Some(Worker::spawn(&self.worker_path)?);
        }

        let worker =
            slot.as_mut().ok_or_else(|| WorkerError::Channel("worker unavailable".into()))?;

        match worker.request(request) {
            Ok(response) => Ok(response),
            Err(error) => {
                // The worker died or misbehaved. Replace it, restore the
                // document, and retry once — a single transient failure should
                // not lose the user's document.
                let dead = slot.take();
                drop(dead);

                let mut fresh = Worker::spawn(&self.worker_path)?;

                let reopened = self
                    .document
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().map(|d| d.bytes.clone()));

                if let Some(bytes) = reopened {
                    let _ = fresh.request(&Request::OpenDocument { data: bytes, password: None });
                }

                let result = fresh.request(request);
                *slot = Some(fresh);
                result.map_err(|_| error)
            }
        }
    }
}
