//! EasyPDF desktop shell.
//!
//! Deliberately thin. Window management, native menus, file dialogs, and a
//! typed command layer that forwards to the crates — business logic living
//! here is a code smell. See `ideas/02-architecture.md`.

// Suppress the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use easypdf_core::{Document, Page, PageSize, Rotation};
use serde::Serialize;

/// Reported to the frontend so it can confirm the IPC path works end to end.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreStatus {
    version: String,
    page_count: usize,
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
        .invoke_handler(tauri::generate_handler![core_status])
        .run(tauri::generate_context!())
        .expect("failed to start the EasyPDF window");
}
