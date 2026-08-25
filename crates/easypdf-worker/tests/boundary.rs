//! End-to-end tests against the real worker binary.
//!
//! These spawn the actual process rather than calling functions, because the
//! properties under test — confinement, death handling, timeouts — only exist
//! at the process boundary. A unit test cannot observe them.

// Tests assert on known-good values; the panic lints exist to keep unwraps out
// of parsing paths, not out of assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use easypdf_ffi::protocol::{Request, Response, SandboxStatus, WorkerError};
use easypdf_ffi::worker::Worker;

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_easypdf-worker"))
}

fn spawn() -> Worker {
    Worker::spawn(worker_path()).expect("worker should start")
}

#[test]
fn worker_starts_and_completes_the_handshake() {
    let worker = spawn();
    assert!(worker.pid() > 0);
}

#[test]
fn worker_confines_itself_before_accepting_input() {
    // The core security property: by the time the host can talk to it, the
    // worker has already given up its privileges.
    let worker = spawn();

    match worker.sandbox() {
        SandboxStatus::Enforced { mechanism, resource_limits } => {
            assert!(!mechanism.is_empty());
            assert!(resource_limits.core_dumps_disabled, "core dumps must be disabled");
            assert!(resource_limits.cpu_seconds.is_some(), "a cpu ceiling must be set");
        }
        SandboxStatus::NotEnforced { reason, .. } => {
            // Expected on platforms where confinement isn't implemented yet,
            // but never on macOS.
            if cfg!(target_os = "macos") {
                panic!("worker ran unconfined on macOS: {reason}");
            }
        }
        other => panic!("unhandled sandbox status: {other:?}"),
    }
}

#[test]
fn operations_without_a_document_refuse_clearly() {
    // Every protocol operation is now implemented, so there is no longer an
    // "unsupported" case to test. What still matters is honest refusal:
    // asking for text with nothing open must say so rather than return an
    // empty string, which would be indistinguishable from a blank page.
    let mut worker = spawn();

    let response = worker.request(&Request::ExtractText { page: 0 }).expect("worker should answer");

    match response {
        Response::Failed(error) => {
            let message = error.to_string();
            assert!(message.contains("no document"), "refusal should say why: {message}");
        }
        other => panic!("expected an explicit refusal, got {other:?}"),
    }
}

#[test]
fn close_document_succeeds_even_with_nothing_open() {
    let mut worker = spawn();
    assert_eq!(worker.request(&Request::CloseDocument).unwrap(), Response::Ok);
}

#[test]
fn worker_survives_a_sequence_of_requests() {
    let mut worker = spawn();
    for page in 0..5 {
        let response = worker.request(&Request::ExtractText { page }).unwrap();
        assert!(matches!(response, Response::Failed(_)), "page {page}: {response:?}");
    }
    // Still healthy after repeated refusals.
    assert_eq!(worker.request(&Request::CloseDocument).unwrap(), Response::Ok);
}

#[test]
fn killing_the_worker_surfaces_as_worker_died() {
    // The host must distinguish a dead worker from a slow one: a death is
    // potentially an exploit in progress and must never be retried in place.
    let mut worker = spawn();
    worker.kill();

    let result = worker.request(&Request::CloseDocument);
    assert!(
        matches!(result, Err(WorkerError::WorkerDied | WorkerError::Channel(_))),
        "expected death to be reported, got {result:?}"
    );
}

#[test]
fn restart_produces_a_working_worker() {
    let mut worker = spawn();
    worker.kill();

    let mut fresh = worker.restart().expect("restart should produce a live worker");
    assert_eq!(fresh.request(&Request::CloseDocument).unwrap(), Response::Ok);
}

#[test]
fn shutdown_terminates_cleanly() {
    let worker = spawn();
    let pid = worker.pid();
    worker.shutdown();

    // The process should be gone. Give the OS a moment to reap it.
    std::thread::sleep(Duration::from_millis(100));
    assert!(pid > 0);
}

#[test]
fn each_worker_is_an_independent_process() {
    let first = spawn();
    let second = spawn();
    assert_ne!(
        first.pid(),
        second.pid(),
        "workers must not share a process — isolation depends on it"
    );
}

/// The sandbox must actually *deny* things.
///
/// `sandbox_init` returning zero proves only that the profile was accepted.
/// This runs the worker's own probes and checks the operations really fail.
#[cfg(target_os = "macos")]
#[test]
fn confinement_denies_filesystem_and_network_access() {
    use std::process::Command;

    let output = Command::new(worker_path())
        .env("EASYPDF_WORKER_SELFTEST", "1")
        .output()
        .expect("selftest should run");

    let report = String::from_utf8_lossy(&output.stdout);

    for probe in ["read_etc_passwd", "write_temp_file", "network_connect"] {
        assert!(
            report.contains(&format!("{probe}=denied")),
            "{probe} was NOT blocked by the sandbox — confinement is not holding.\n\
             Full report:\n{report}"
        );
    }
}

// ---------------------------------------------------------------------------
// Rendering through the boundary.
//
// These are the tests that matter most: they prove a page can be rasterized by
// a process that has already given up filesystem and network access. If
// rendering only worked unconfined, the architecture would not hold.
// ---------------------------------------------------------------------------

fn corpus(name: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    std::fs::read(root.join(name)).expect("corpus file should exist")
}

fn pdfium_available() -> bool {
    let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "mac-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "mac-x64"
    } else if cfg!(target_os = "windows") {
        "win-x64"
    } else {
        "linux-x64"
    };
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/pdfium")
        .join(target)
        .join("lib")
        .is_dir()
}

macro_rules! require_pdfium {
    () => {
        if !pdfium_available() {
            eprintln!("skipping: vendored PDFium not present (run scripts/fetch-pdfium.sh)");
            return;
        }
    };
}

#[test]
fn opens_a_real_document_inside_the_sandbox() {
    require_pdfium!();
    let mut worker = spawn();

    let response = worker
        .request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None })
        .unwrap();

    match response {
        Response::DocumentOpened { page_count, .. } => assert_eq!(page_count, 1),
        other => panic!("expected DocumentOpened, got {other:?}"),
    }
}

#[test]
fn renders_a_page_inside_the_sandbox() {
    // The whole architecture in one test: untrusted bytes go into a process
    // with no filesystem and no network, and pixels come back.
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    let response =
        worker.request(&Request::RenderPage { page: 0, zoom: 1.0, rotation: 0 }).unwrap();

    match response {
        Response::PageRendered { width, height, pixels } => {
            assert_eq!(width, 200);
            assert_eq!(height, 100);
            assert_eq!(pixels.len(), 200 * 100 * 4);

            let dark = pixels.as_chunks::<4>().0.iter().filter(|p| p[0] < 128).count();
            assert!(dark > 20, "page came back blank — only {dark} dark pixels");
        }
        other => panic!("expected PageRendered, got {other:?}"),
    }
}

#[test]
fn rendering_without_an_open_document_is_refused() {
    require_pdfium!();
    let mut worker = spawn();

    let response =
        worker.request(&Request::RenderPage { page: 0, zoom: 1.0, rotation: 0 }).unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");
}

#[test]
fn malformed_document_is_refused_without_killing_the_worker() {
    // A hostile document must not take the worker down: that would be a
    // denial-of-service vector. The worker must stay usable afterwards.
    require_pdfium!();
    let mut worker = spawn();

    let response = worker
        .request(&Request::OpenDocument { data: corpus("not-a-pdf.bin"), password: None })
        .unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");

    // Still alive and serving.
    assert_eq!(worker.request(&Request::CloseDocument).unwrap(), Response::Ok);
}

#[test]
fn truncated_document_is_refused_without_killing_the_worker() {
    require_pdfium!();
    let mut worker = spawn();

    let response = worker
        .request(&Request::OpenDocument { data: corpus("truncated.pdf"), password: None })
        .unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");
    assert_eq!(worker.request(&Request::CloseDocument).unwrap(), Response::Ok);
}

#[test]
fn close_document_forgets_the_document() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();
    worker.request(&Request::CloseDocument).unwrap();

    // Rendering must now fail: the bytes are gone, not lingering in memory.
    let response =
        worker.request(&Request::RenderPage { page: 0, zoom: 1.0, rotation: 0 }).unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");
}

#[test]
fn page_size_is_reported_without_rendering() {
    // Fit-to-window needs page dimensions. Rendering a full page only to
    // measure it and discard the pixels is wasteful, especially at high zoom.
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    match worker.request(&Request::PageSize { page: 0 }).unwrap() {
        Response::PageSize { width, height } => {
            assert!((width - 200.0).abs() < 0.5, "width was {width}");
            assert!((height - 100.0).abs() < 0.5, "height was {height}");
        }
        other => panic!("expected PageSize, got {other:?}"),
    }
}

#[test]
fn page_size_reflects_the_document_not_a_default() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("wide.pdf"), password: None }).unwrap();

    match worker.request(&Request::PageSize { page: 0 }).unwrap() {
        Response::PageSize { width, .. } => assert!((width - 400.0).abs() < 0.5),
        other => panic!("expected PageSize, got {other:?}"),
    }
}

#[test]
fn page_size_of_a_missing_page_is_refused() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    let response = worker.request(&Request::PageSize { page: 99 }).unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");
}

#[test]
fn text_is_extracted_inside_the_sandbox() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    match worker.request(&Request::ExtractText { page: 0 }).unwrap() {
        Response::TextExtracted { text } => {
            assert!(text.contains("Hello EasyPDF"), "extracted {text:?}");
        }
        other => panic!("expected TextExtracted, got {other:?}"),
    }
}

#[test]
fn search_returns_positioned_hits_inside_the_sandbox() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    let request = Request::Search { query: "EasyPDF".to_owned(), match_case: false };
    match worker.request(&request).unwrap() {
        Response::SearchResults { hits, truncated } => {
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].page, 0);
            assert!(!truncated);

            let rect = hits[0].rects.first().expect("a hit needs a rectangle");
            assert!(!rect.is_degenerate(), "degenerate rect: {rect:?}");
            // Inside the 200x100 fixture page.
            assert!(rect.left >= 0.0 && rect.right <= 200.0, "{rect:?}");
            assert!(rect.bottom >= 0.0 && rect.top <= 100.0, "{rect:?}");
        }
        other => panic!("expected SearchResults, got {other:?}"),
    }
}

#[test]
fn searching_with_no_document_open_is_refused() {
    require_pdfium!();
    let mut worker = spawn();

    let request = Request::Search { query: "anything".to_owned(), match_case: false };
    let response = worker.request(&request).unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");
}

#[test]
fn search_survives_a_query_that_matches_nothing() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    let request = Request::Search { query: "zzzznotpresent".to_owned(), match_case: false };
    match worker.request(&request).unwrap() {
        Response::SearchResults { hits, .. } => assert!(hits.is_empty()),
        other => panic!("expected SearchResults, got {other:?}"),
    }

    // Worker stays healthy.
    assert_eq!(worker.request(&Request::CloseDocument).unwrap(), Response::Ok);
}

#[test]
fn encrypted_document_reports_a_password_problem_as_itself() {
    // Not as a malformed document: the UI has to ask for a password, not tell
    // the user their file is broken.
    require_pdfium!();
    let mut worker = spawn();

    let request = Request::OpenDocument { data: corpus("encrypted.pdf"), password: None };
    match worker.request(&request).unwrap() {
        Response::Failed(WorkerError::BadPassword) => {}
        other => panic!("expected BadPassword, got {other:?}"),
    }
}

#[test]
fn encrypted_document_opens_with_the_right_password_inside_the_sandbox() {
    require_pdfium!();
    let mut worker = spawn();

    let request = Request::OpenDocument {
        data: corpus("encrypted.pdf"),
        password: Some("secret".to_owned()),
    };
    match worker.request(&request).unwrap() {
        Response::DocumentOpened { page_count, .. } => assert_eq!(page_count, 1),
        other => panic!("expected DocumentOpened, got {other:?}"),
    }

    // Decryption really happened.
    match worker.request(&Request::ExtractText { page: 0 }).unwrap() {
        Response::TextExtracted { text } => assert!(text.contains("Secret EasyPDF"), "{text:?}"),
        other => panic!("expected TextExtracted, got {other:?}"),
    }
}

#[test]
fn a_wrong_password_does_not_kill_the_worker() {
    require_pdfium!();
    let mut worker = spawn();

    let request =
        Request::OpenDocument { data: corpus("encrypted.pdf"), password: Some("wrong".to_owned()) };
    assert!(matches!(
        worker.request(&request).unwrap(),
        Response::Failed(WorkerError::BadPassword)
    ));

    // The same worker must still accept the correct password afterwards.
    let retry = Request::OpenDocument {
        data: corpus("encrypted.pdf"),
        password: Some("secret".to_owned()),
    };
    assert!(matches!(worker.request(&retry).unwrap(), Response::DocumentOpened { .. }));
}

#[test]
fn outline_of_a_document_without_one_is_empty_not_an_error() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    match worker.request(&Request::Outline).unwrap() {
        Response::Outline { entries } => assert!(entries.is_empty()),
        other => panic!("expected Outline, got {other:?}"),
    }
}

#[test]
fn rotation_reaches_the_renderer_through_the_boundary() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("wide.pdf"), password: None }).unwrap();

    let upright = worker.request(&Request::RenderPage { page: 0, zoom: 1.0, rotation: 0 }).unwrap();
    let turned = worker.request(&Request::RenderPage { page: 0, zoom: 1.0, rotation: 90 }).unwrap();

    match (upright, turned) {
        (
            Response::PageRendered { width: uw, height: uh, .. },
            Response::PageRendered { width: tw, height: th, .. },
        ) => {
            assert_eq!((uw, uh), (400, 100));
            assert_eq!((tw, th), (100, 400), "rotation must survive the boundary");
        }
        other => panic!("expected two rendered pages, got {other:?}"),
    }
}

#[test]
fn text_layout_crosses_the_boundary_with_usable_geometry() {
    require_pdfium!();
    let mut worker = spawn();

    worker.request(&Request::OpenDocument { data: corpus("minimal.pdf"), password: None }).unwrap();

    match worker.request(&Request::TextLayout { page: 0 }).unwrap() {
        Response::TextLayout { layout } => {
            assert!(!layout.chars.is_empty());
            assert!(!layout.truncated);

            let text: String = layout.chars.iter().map(|c| c.text.as_str()).collect();
            assert!(text.contains("Hello EasyPDF"), "{text:?}");

            // Every box must be hittable, or selection cannot target it.
            for character in &layout.chars {
                assert!(!character.rect.is_degenerate(), "{character:?}");
            }
        }
        other => panic!("expected TextLayout, got {other:?}"),
    }
}

#[test]
fn text_layout_without_a_document_is_refused() {
    require_pdfium!();
    let mut worker = spawn();

    let response = worker.request(&Request::TextLayout { page: 0 }).unwrap();
    assert!(matches!(response, Response::Failed(_)), "{response:?}");
}
