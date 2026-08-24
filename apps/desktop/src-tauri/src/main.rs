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
use serde::Serialize;

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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![core_status, worker_status])
        .run(tauri::generate_context!())
        .expect("failed to start the EasyPDF window");
}
