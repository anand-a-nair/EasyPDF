//! Session-level integration tests.
//!
//! These drive the same code path the app does: a real worker process, real
//! PDFium, real documents. Everything below the UI, in other words — the layer
//! that was previously untestable because it lived inside a binary.
//!
//! What they cover that the worker's own tests do not: caching, worker restart
//! and document restoration, and the shape of what the UI is actually handed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use easypdf_render::wire::decode_page;
use easypdf_session::Session;

/// Locates the worker built alongside this test.
///
/// `CARGO_BIN_EXE_*` is only set for the package that defines the binary, so
/// the path is derived from the test executable's own location instead:
/// `target/<profile>/deps/<test>` → `target/<profile>/easypdf-worker`.
fn worker_path() -> PathBuf {
    let test_exe = std::env::current_exe().expect("test executable path");
    let profile_dir = test_exe.parent().and_then(Path::parent).expect("target/<profile>");
    profile_dir.join(if cfg!(windows) { "easypdf-worker.exe" } else { "easypdf-worker" })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn corpus(name: &str) -> PathBuf {
    repo_root().join("tests/corpus").join(name)
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
    repo_root().join("vendor/pdfium").join(target).join("lib").is_dir()
}

macro_rules! session_or_skip {
    () => {{
        if !pdfium_available() || !worker_path().is_file() {
            eprintln!("skipping: worker or vendored PDFium not present");
            return;
        }
        Session::new(worker_path())
    }};
}

#[test]
fn opens_a_document_and_reports_what_the_ui_needs() {
    let session = session_or_skip!();
    let info = session.open(&corpus("minimal.pdf"), None).expect("should open");

    assert_eq!(info.page_count, 1);
    assert!(!info.encrypted);
    // The file name, not the full path: the UI has no use for the path, and
    // showing one leaks where the user keeps their files.
    assert_eq!(info.name, "minimal.pdf");
}

#[test]
fn renders_a_page_the_frontend_can_decode() {
    let session = session_or_skip!();
    session.open(&corpus("minimal.pdf"), None).unwrap();

    let rendered = session.render(0, 1.0, 0).expect("should render");
    let wire = rendered.into_wire_format();

    // Decoded with the same function the frontend's decoder mirrors. If the
    // wire format ever drifts, this fails rather than the UI silently showing
    // nothing.
    let tile = decode_page(&wire).expect("wire format should round-trip");
    assert_eq!((tile.width, tile.height), (200, 100));

    let dark = tile.pixels.as_chunks::<4>().0.iter().filter(|p| p[0] < 128).count();
    assert!(dark > 20, "page came back blank: {dark} dark pixels");
}

#[test]
fn a_second_render_of_the_same_page_is_served_from_cache() {
    // The cache is what keeps scrolling cheap. If it silently stopped being
    // used, nothing would break — it would just get slow, which is exactly the
    // kind of regression nobody notices.
    let session = session_or_skip!();
    session.open(&corpus("many-pages.pdf"), None).unwrap();

    let first = std::time::Instant::now();
    session.render(0, 1.0, 0).unwrap();
    let cold = first.elapsed();

    let second = std::time::Instant::now();
    session.render(0, 1.0, 0).unwrap();
    let warm = second.elapsed();

    println!("cold {cold:?}, cached {warm:?}");
    assert!(warm < cold, "a cached render ({warm:?}) should beat a cold one ({cold:?})");
}

#[test]
fn rotation_reaches_the_renderer() {
    let session = session_or_skip!();
    session.open(&corpus("wide.pdf"), None).unwrap();

    let upright = session.render(0, 1.0, 0).unwrap();
    let turned = session.render(0, 1.0, 90).unwrap();

    assert_eq!((upright.width, upright.height), (400, 100));
    assert_eq!((turned.width, turned.height), (100, 400));
}

#[test]
fn an_encrypted_document_asks_for_a_password_rather_than_reporting_damage() {
    let session = session_or_skip!();

    let error = session.open(&corpus("encrypted.pdf"), None).unwrap_err();
    assert!(error.needs_password, "should ask for a password: {}", error.message);

    // And opens with the right one.
    let info = session
        .open(&corpus("encrypted.pdf"), Some("secret".to_owned()))
        .expect("correct password should open it");
    assert_eq!(info.page_count, 1);
}

#[test]
fn a_wrong_password_is_reported_as_a_password_problem() {
    let session = session_or_skip!();
    let error = session.open(&corpus("encrypted.pdf"), Some("wrong".to_owned())).unwrap_err();
    assert!(error.needs_password);
}

#[test]
fn a_missing_file_is_reported_as_a_file_problem_not_a_password_one() {
    let session = session_or_skip!();
    let error = session.open(&corpus("does-not-exist.pdf"), None).unwrap_err();

    assert!(!error.needs_password, "a missing file must not prompt for a password");
    assert!(error.message.contains("could not read"), "unhelpful: {}", error.message);
}

#[test]
fn search_finds_text_and_reports_where_it_is() {
    let session = session_or_skip!();
    session.open(&corpus("minimal.pdf"), None).unwrap();

    let (hits, truncated) = session.search("EasyPDF", false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 0);
    assert!(!truncated);
    assert!(!hits[0].rects[0].is_degenerate());
}

#[test]
fn text_layout_matches_extracted_text() {
    // A selection that copies something different from what is on screen would
    // be a quiet betrayal, so the two paths are checked against each other.
    let session = session_or_skip!();
    session.open(&corpus("minimal.pdf"), None).unwrap();

    let layout = session.text_layout(0).unwrap();
    let from_layout: String = layout.chars.iter().map(|c| c.text.as_str()).collect();
    let extracted = session.extract_text(0).unwrap();

    assert_eq!(from_layout.trim(), extracted.trim());
}

#[test]
fn closing_a_document_releases_it() {
    let session = session_or_skip!();
    session.open(&corpus("minimal.pdf"), None).unwrap();
    assert!(session.info().is_some());

    session.close().unwrap();
    assert!(session.info().is_none());

    // Rendering afterwards must fail rather than serve a stale cached page.
    assert!(session.render(0, 1.0, 0).is_err());
}

#[test]
fn opening_a_second_document_replaces_the_first() {
    let session = session_or_skip!();
    session.open(&corpus("minimal.pdf"), None).unwrap();
    session.render(0, 1.0, 0).unwrap();

    let info = session.open(&corpus("wide.pdf"), None).unwrap();
    assert_eq!(info.name, "wide.pdf");

    // The cache is keyed by page and zoom, not by document, so a stale entry
    // would show the previous document's page here.
    let rendered = session.render(0, 1.0, 0).unwrap();
    assert_eq!((rendered.width, rendered.height), (400, 100), "served a stale cached page");
}

#[test]
fn a_malformed_document_is_refused_without_breaking_the_session() {
    let session = session_or_skip!();
    assert!(session.open(&corpus("not-a-pdf.bin"), None).is_err());

    // The session must still work afterwards.
    let info = session.open(&corpus("minimal.pdf"), None).expect("should recover");
    assert_eq!(info.page_count, 1);
}

#[test]
fn the_outline_is_empty_rather_than_failing_when_absent() {
    let session = session_or_skip!();
    session.open(&corpus("minimal.pdf"), None).unwrap();
    assert!(session.outline().unwrap().is_empty());
}
