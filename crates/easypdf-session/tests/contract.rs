//! Contract fixtures: the exact JSON shapes the frontend receives.
//!
//! The browser harness stubs Tauri, which makes the frontend testable without
//! a running app — but a stub can drift from what Rust actually sends, and
//! then the harness cheerfully passes while the app is broken. That is not
//! hypothetical: the harness once returned unrotated dimensions from a render
//! stub, so the rotation path would have passed its own test while doing
//! nothing.
//!
//! These tests write one fixture per command payload. `scripts/check-contracts.mjs`
//! checks the harness stubs against them, so drift in either direction fails.
//!
//! The fixtures are committed. A change here shows up as a diff, which is the
//! point: changing what the frontend receives should be visible in review.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use easypdf_core::text::{CharBox, OutlineEntry, SearchHit, TextLayout, TextRect};
use easypdf_session::{DocumentInfo, OpenError, PageDimensions, SearchResults, WorkerStatus};

fn contract_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/contract")
}

/// Writes a fixture and returns its JSON, so the test can also assert on it.
fn write_fixture<T: serde::Serialize>(name: &str, value: &T) -> serde_json::Value {
    let json = serde_json::to_value(value).expect("payload should serialize");
    let pretty = serde_json::to_string_pretty(&json).unwrap();

    let dir = contract_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), pretty + "\n").unwrap();

    json
}

/// The key set of a JSON object, sorted.
///
/// The *set* is the contract, not the order: JSON objects are unordered and
/// `serde_json::Value` sorts them anyway. What must not change silently is
/// which keys exist.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> =
        value.as_object().expect("expected an object").keys().cloned().collect();
    names.sort();
    names
}

/// Sorted expectation, so call sites can list keys in a readable order.
fn expect(names: &[&str]) -> Vec<String> {
    let mut sorted: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
    sorted.sort();
    sorted
}

#[test]
fn document_info_contract() {
    let json = write_fixture(
        "open_document",
        &DocumentInfo { name: "example.pdf".into(), page_count: 3, encrypted: false },
    );
    assert_eq!(keys(&json), expect(&["name", "pageCount", "encrypted"]));
}

#[test]
fn open_error_contract() {
    // The frontend branches on `needsPassword`. If that key were ever renamed,
    // every encrypted document would report as a broken file instead of
    // prompting.
    let json = write_fixture("open_document_error", &OpenError::password_required());
    assert_eq!(keys(&json), expect(&["needsPassword", "message"]));
    assert_eq!(json["needsPassword"], serde_json::json!(true));
}

#[test]
fn page_size_contract() {
    let json = write_fixture("page_size", &PageDimensions { width: 200.0, height: 100.0 });
    assert_eq!(keys(&json), expect(&["width", "height"]));
}

#[test]
fn worker_status_contract() {
    let json = write_fixture(
        "worker_status",
        &WorkerStatus {
            running: true,
            sandboxed: true,
            detail: "confined via seatbelt".into(),
            memory_capped: false,
            engine_available: true,
        },
    );
    assert_eq!(
        keys(&json),
        expect(&["running", "sandboxed", "detail", "memoryCapped", "engineAvailable"])
    );
}

#[test]
fn search_results_contract() {
    let json = write_fixture(
        "search",
        &SearchResults {
            hits: vec![SearchHit {
                page: 0,
                rects: vec![TextRect { left: 20.0, bottom: 40.0, right: 90.0, top: 64.0 }],
            }],
            truncated: false,
        },
    );
    assert_eq!(keys(&json), expect(&["hits", "truncated"]));
    assert_eq!(keys(&json["hits"][0]), expect(&["page", "rects"]));
    // Rect field order is the frontend's coordinate contract; getting these
    // confused puts highlights on the wrong side of the page.
    assert_eq!(keys(&json["hits"][0]["rects"][0]), expect(&["left", "bottom", "right", "top"]));
}

#[test]
fn outline_contract() {
    let json = write_fixture(
        "outline",
        &vec![
            OutlineEntry { title: "Introduction".into(), depth: 0, page: Some(0) },
            OutlineEntry { title: "Unlinked".into(), depth: 1, page: None },
        ],
    );
    assert_eq!(keys(&json[0]), expect(&["title", "depth", "page"]));
    // A null page means "shown but not clickable". If it serialized as 0 the
    // UI would send the user to page one instead of disabling the entry.
    assert_eq!(json[1]["page"], serde_json::Value::Null);
}

#[test]
fn text_layout_contract() {
    let json = write_fixture(
        "text_layout",
        &TextLayout {
            chars: vec![CharBox {
                text: "H".into(),
                rect: TextRect { left: 20.0, bottom: 40.0, right: 32.0, top: 64.0 },
            }],
            truncated: false,
        },
    );
    assert_eq!(keys(&json), expect(&["chars", "truncated"]));
    assert_eq!(keys(&json["chars"][0]), expect(&["text", "rect"]));
}
