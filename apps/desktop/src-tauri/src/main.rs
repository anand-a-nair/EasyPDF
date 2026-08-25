//! EasyPDF desktop shell.
//!
//! Deliberately thin. Window management, native menus, file dialogs, and a
//! typed command layer that forwards to the crates — business logic living
//! here is a code smell. See `ideas/02-architecture.md`.

// Suppress the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod session;

use std::path::PathBuf;

use easypdf_core::{Document, Page, PageSize, Rotation};
use easypdf_ffi::protocol::SandboxStatus;
use easypdf_ffi::worker::Worker;
use serde::Serialize;
use session::{DocumentInfo, Session};
use tauri::State;
use tauri::ipc::Response as IpcResponse;

/// Reported to the frontend so it can confirm the IPC path works end to end.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreStatus {
    version: String,
    page_count: usize,
}

/// What the shell knows about the sandboxed worker.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerStatus {
    running: bool,
    sandboxed: bool,
    /// Human-readable detail: the mechanism, or why confinement is absent.
    detail: String,
    /// Whether a kernel-enforced memory ceiling is in place.
    ///
    /// False on macOS, which provides no working `RLIMIT_AS`. Surfaced rather
    /// than hidden so the gap is visible.
    memory_capped: bool,
}

/// Locates the worker binary, which sits beside the app executable.
fn worker_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    let name = if cfg!(windows) { "easypdf-worker.exe" } else { "easypdf-worker" };
    Some(directory.join(name))
}

/// Starts a worker, reports how well it confined itself, and shuts it down.
///
/// The host surfaces this rather than assuming it. A worker running with
/// ordinary user privileges is a condition the user deserves to know about —
/// see `ideas/07-security.md`.
#[tauri::command]
fn worker_status() -> WorkerStatus {
    let Some(path) = worker_path() else {
        return WorkerStatus {
            running: false,
            sandboxed: false,
            detail: "could not locate the worker executable".to_owned(),
            memory_capped: false,
        };
    };

    match Worker::spawn(&path) {
        Ok(worker) => {
            let status = match worker.sandbox() {
                SandboxStatus::Enforced { mechanism, resource_limits } => WorkerStatus {
                    running: true,
                    sandboxed: true,
                    detail: format!("confined via {mechanism}"),
                    memory_capped: resource_limits.memory_capped(),
                },
                SandboxStatus::NotEnforced { reason, resource_limits } => WorkerStatus {
                    running: true,
                    sandboxed: false,
                    detail: format!("UNCONFINED: {reason}"),
                    memory_capped: resource_limits.memory_capped(),
                },
                other => WorkerStatus {
                    running: true,
                    sandboxed: false,
                    detail: format!("unrecognized sandbox status: {other:?}"),
                    memory_capped: false,
                },
            };
            worker.shutdown();
            status
        }
        Err(error) => WorkerStatus {
            running: false,
            sandboxed: false,
            detail: format!("worker failed to start: {error}"),
            memory_capped: false,
        },
    }
}

/// Opens a document from a path chosen by the user.
///
/// The path comes from the native file dialog on the frontend, never from
/// document content. The host reads the bytes; the worker is never told a
/// filename (D-019).
#[tauri::command]
fn open_document(path: String, session: State<'_, Session>) -> Result<DocumentInfo, String> {
    session.open(std::path::Path::new(&path))
}

/// Renders a page and returns raw pixels.
///
/// Returns [`IpcResponse`] rather than a JSON value: a single page is hundreds
/// of kilobytes of pixels, and base64 inside JSON would inflate that by a third
/// and cost a parse on both sides for every tile.
///
/// Layout: `u32` width, `u32` height, then RGBA bytes — both little-endian.
#[tauri::command]
fn render_page(page: usize, zoom: f32, session: State<'_, Session>) -> Result<IpcResponse, String> {
    let rendered = session.render(page, zoom)?;
    Ok(IpcResponse::new(rendered.into_wire_format()))
}

/// A page's dimensions in points, without rendering it.
///
/// Lets the frontend compute a fit-to-window zoom without paying for a render
/// it is going to discard.
#[tauri::command]
fn page_size(page: usize, session: State<'_, Session>) -> Result<PageDimensions, String> {
    let (width, height) = session.page_size(page)?;
    Ok(PageDimensions { width, height })
}

/// Page dimensions in points.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PageDimensions {
    width: f32,
    height: f32,
}

/// Extracts a page's text.
#[tauri::command]
fn extract_text(page: usize, session: State<'_, Session>) -> Result<String, String> {
    session.extract_text(page)
}

/// Searches the whole document, returning positioned hits.
#[tauri::command]
fn search(
    query: String,
    match_case: bool,
    session: State<'_, Session>,
) -> Result<SearchResults, String> {
    let (hits, truncated) = session.search(&query, match_case)?;
    Ok(SearchResults { hits, truncated })
}

/// Search results, with an honest truncation flag.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResults {
    hits: Vec<easypdf_core::text::SearchHit>,
    /// Whether the hit list was capped. Shown to the user rather than hidden.
    truncated: bool,
}

/// Closes the open document and frees its cached tiles.
#[tauri::command]
fn close_document(session: State<'_, Session>) -> Result<(), String> {
    session.close()
}

/// The currently open document, if any.
#[tauri::command]
fn document_info(session: State<'_, Session>) -> Option<DocumentInfo> {
    session.info()
}

/// Returns proof that the shell can reach the core document model.
///
/// A placeholder in the sense that it does nothing useful yet, but a real
/// check: it exercises TypeScript to Tauri IPC to `easypdf-core` and back.
/// If this breaks, the whole architecture is disconnected.
#[tauri::command]
fn core_status() -> CoreStatus {
    let fixture = Document::from_pages(vec![
        Page { size: PageSize::A4, rotation: Rotation::None },
        Page { size: PageSize::LETTER, rotation: Rotation::Clockwise90 },
    ]);

    CoreStatus { version: env!("CARGO_PKG_VERSION").to_owned(), page_count: fixture.page_count() }
}

// If the window cannot be created there is nothing useful to fall back to, and
// no UI in which to report it. Aborting with a clear message is the honest
// outcome. The lint exists to keep unwraps out of parsing paths, not here.
#[allow(clippy::expect_used)]
fn main() {
    let worker = worker_path().unwrap_or_else(|| PathBuf::from("easypdf-worker"));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Session::new(worker))
        .invoke_handler(tauri::generate_handler![
            core_status,
            worker_status,
            open_document,
            render_page,
            page_size,
            extract_text,
            search,
            close_document,
            document_info
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the EasyPDF window");
}
