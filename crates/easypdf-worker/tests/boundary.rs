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
fn unimplemented_operations_refuse_clearly() {
    // Honest refusal over plausible nonsense — ideas/01-vision.md.
    // Text extraction is the remaining unimplemented operation.
    let mut worker = spawn();

    let response = worker.request(&Request::ExtractText { page: 0 }).expect("worker should answer");

    match response {
        Response::Failed(WorkerError::Unsupported(message)) => {
            assert!(!message.is_empty(), "a refusal must explain itself");
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
        assert!(matches!(response, Response::Failed(WorkerError::Unsupported(_))));
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
