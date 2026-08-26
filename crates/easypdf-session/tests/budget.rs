//! Measures the app against the performance budget.
//!
//! `ideas/04-performance-budget.md` says budgets that are not measured
//! automatically are aspirations. These are the ones measurable below the UI;
//! startup time and idle memory need a running app and are measured by
//! `scripts/measure-startup.sh`.
//!
//! Each assertion uses the budget's **hard fail** figure, not its target. The
//! target is what we aim for; the hard fail is what makes the build red. Using
//! the target here would make the suite fail on a loaded machine and train
//! everyone to ignore it.
//!
//! Assertions are enforced **only in release builds** — see `check`. Run them
//! with `cargo test --release -p easypdf-session --test budget`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use easypdf_session::Session;

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

/// Reports a measurement against its budget and fails past the hard limit.
fn check(what: &str, measured: Duration, target: Duration, hard_fail: Duration) {
    let verdict = if measured <= target {
        "within target"
    } else if measured <= hard_fail {
        "OVER TARGET"
    } else {
        "HARD FAIL"
    };
    println!("{what}: {measured:?} (target {target:?}, limit {hard_fail:?}) — {verdict}");

    assert!(measured <= hard_fail, "{what} took {measured:?}, past the {hard_fail:?} hard limit");
}

#[test]
fn a_five_hundred_page_document_becomes_interactive_quickly() {
    // Budget: 1s target, 2s hard fail. "Interactive" means the page count is
    // known and the first page is on screen — not that every page is parsed.
    let session = session_or_skip!();

    let start = Instant::now();
    let info = session.open(&corpus("five-hundred-pages.pdf"), None).unwrap();
    session.render(0, 1.0, 0).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(info.page_count, 500);
    check(
        "500-page open + first page",
        elapsed,
        Duration::from_millis(1000),
        Duration::from_millis(2000),
    );
}

#[test]
fn opening_does_not_scale_with_page_count() {
    // The architectural claim in ideas/02: a 900-page document opens as fast as
    // a 2-page one, because only the cross-reference table is read. If opening
    // ever started walking every page this would catch it.
    let session = session_or_skip!();

    let small = Instant::now();
    session.open(&corpus("minimal.pdf"), None).unwrap();
    let small = small.elapsed();

    let large = Instant::now();
    session.open(&corpus("five-hundred-pages.pdf"), None).unwrap();
    let large = large.elapsed();

    println!("open: 1 page {small:?}, 500 pages {large:?}");
    if cfg!(debug_assertions) {
        return;
    }
    assert!(
        large < small + Duration::from_millis(500),
        "opening 500 pages ({large:?}) took far longer than one page ({small:?}), \
         which suggests open is walking the page tree"
    );
}

#[test]
fn a_cached_page_is_served_within_a_frame() {
    // Budget: 16ms target, 33ms hard fail. This is the number that decides
    // whether paging through a document feels instant.
    let session = session_or_skip!();
    session.open(&corpus("five-hundred-pages.pdf"), None).unwrap();
    session.render(3, 1.0, 0).unwrap(); // warm the cache

    let start = Instant::now();
    session.render(3, 1.0, 0).unwrap();
    let elapsed = start.elapsed();

    check("cached page", elapsed, Duration::from_millis(16), Duration::from_millis(33));
}

#[test]
fn a_zoom_step_re_renders_promptly() {
    // Budget: 100ms target, 250ms hard fail.
    let session = session_or_skip!();
    session.open(&corpus("five-hundred-pages.pdf"), None).unwrap();
    session.render(0, 1.0, 0).unwrap();

    let start = Instant::now();
    session.render(0, 2.0, 0).unwrap();
    let elapsed = start.elapsed();

    check("zoom re-render", elapsed, Duration::from_millis(100), Duration::from_millis(250));
}

#[test]
fn searching_a_large_document_stays_responsive() {
    // Not in the written budget, but it is typed into: the find box debounces
    // at 250ms, so a search slower than that queues up behind itself and the
    // box feels stuck.
    let session = session_or_skip!();
    session.open(&corpus("five-hundred-pages.pdf"), None).unwrap();

    let start = Instant::now();
    let (hits, truncated) = session.search("Page 250", false).unwrap();
    let elapsed = start.elapsed();

    println!("search across 500 pages: {elapsed:?}, {} hits, truncated={truncated}", hits.len());
    assert!(!hits.is_empty(), "should have found the page marker");
    check("search 500 pages", elapsed, Duration::from_millis(250), Duration::from_millis(1000));
}

#[test]
fn paging_through_a_document_stays_cheap() {
    // Simulates reading: forty consecutive pages, cold each time.
    let session = session_or_skip!();
    session.open(&corpus("five-hundred-pages.pdf"), None).unwrap();

    let start = Instant::now();
    for page in 0..40 {
        session.render(page, 1.0, 0).unwrap();
    }
    let average = start.elapsed() / 40;

    check("average cold page", average, Duration::from_millis(16), Duration::from_millis(50));
}
