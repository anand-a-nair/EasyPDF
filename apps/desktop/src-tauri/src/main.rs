//! EasyPDF desktop shell.
//!
//! Deliberately thin. Window management, native menus, file dialogs, and a
//! typed command layer that forwards to the crates — business logic living
//! here is a code smell. See `ideas/02-architecture.md`.

// Suppress the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use easypdf_core::{Document, Page, PageSize, Rotation};
use easypdf_ffi::protocol::SandboxStatus;
use easypdf_ffi::worker::Worker;
use easypdf_session::{
    DocumentInfo, OpenError, PageDimensions, SearchResults, Session, WorkerStatus,
};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Response as IpcResponse;

/// Reported to the frontend so it can confirm the IPC path works end to end.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreStatus {
    version: String,
    page_count: usize,
}

/// Locates the worker binary.
///
/// In a bundle the worker is a Tauri sidecar and lands beside the app
/// executable with the target triple stripped. In development it is in
/// `target/<profile>/`, which is also beside the app executable — so one rule
/// covers both, and the triple-suffixed name is checked as a fallback in case
/// the bundler ever stops stripping it.
///
/// **There is deliberately no fallback to in-process parsing** if this returns
/// nothing (D-017). A missing worker means no document, with a clear message —
/// falling back would silently discard the entire security model at exactly
/// the moment something is already wrong.
fn worker_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;

    let extension = if cfg!(windows) { ".exe" } else { "" };
    let triple = env!("EASYPDF_TARGET_TRIPLE");

    let candidates = [
        directory.join(format!("easypdf-worker{extension}")),
        directory.join(format!("easypdf-worker-{triple}{extension}")),
    ];

    candidates.into_iter().find(|path| path.is_file())
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
            engine_available: false,
        };
    };

    match Worker::spawn(&path) {
        Ok(worker) => {
            let engine_available = worker.engine_available();
            let status = match worker.sandbox() {
                SandboxStatus::Enforced { mechanism, resource_limits } => WorkerStatus {
                    running: true,
                    sandboxed: true,
                    detail: format!("confined via {mechanism}"),
                    memory_capped: resource_limits.memory_capped(),
                    engine_available,
                },
                SandboxStatus::NotEnforced { reason, resource_limits } => WorkerStatus {
                    running: true,
                    sandboxed: false,
                    detail: format!("UNCONFINED: {reason}"),
                    memory_capped: resource_limits.memory_capped(),
                    engine_available,
                },
                other => WorkerStatus {
                    running: true,
                    sandboxed: false,
                    detail: format!("unrecognized sandbox status: {other:?}"),
                    memory_capped: false,
                    engine_available,
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
            engine_available: false,
        },
    }
}

/// Opens a document from a path chosen by the user.
///
/// The path comes from the native file dialog on the frontend, never from
/// document content. The host reads the bytes; the worker is never told a
/// filename (D-019).
#[tauri::command]
fn open_document(
    path: String,
    password: Option<String>,
    session: State<'_, Session>,
) -> Result<DocumentInfo, OpenError> {
    session.open(std::path::Path::new(&path), password)
}

/// A page's characters and their positions, for text selection.
#[tauri::command]
fn text_layout(
    page: usize,
    session: State<'_, Session>,
) -> Result<easypdf_core::text::TextLayout, String> {
    session.text_layout(page)
}

/// The document outline (bookmarks). Empty when the document has none.
#[tauri::command]
fn outline(session: State<'_, Session>) -> Result<Vec<easypdf_core::text::OutlineEntry>, String> {
    session.outline()
}

/// Renders a page and returns raw pixels.
///
/// Returns [`IpcResponse`] rather than a JSON value: a single page is hundreds
/// of kilobytes of pixels, and base64 inside JSON would inflate that by a third
/// and cost a parse on both sides for every tile.
///
/// Layout: `u32` width, `u32` height, then RGBA bytes — both little-endian.
#[tauri::command]
fn render_page(
    page: usize,
    zoom: f32,
    rotation: i32,
    session: State<'_, Session>,
) -> Result<IpcResponse, String> {
    let rendered = session.render(page, zoom, rotation)?;
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
    // Measured from the top of main to the window being ready. Misses process
    // exec and dynamic linking, which is why the measuring script also records
    // wall-clock time from launch — the two together bound the real figure.
    let started = std::time::Instant::now();
    let measuring = std::env::var_os("EASYPDF_MEASURE_STARTUP").is_some();

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
            outline,
            text_layout,
            close_document,
            document_info
        ])
        .setup(move |_app| {
            if measuring {
                // stderr so it cannot be confused with application output.
                eprintln!("EASYPDF_STARTUP_MS={:.1}", started.elapsed().as_secs_f64() * 1000.0);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start the EasyPDF window");
}
