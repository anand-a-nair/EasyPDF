//! Measures the cost of keeping a document open versus re-parsing it.
//!
//! Not a strict benchmark — it runs in the normal test suite and asserts a
//! generous bound, so it catches a regression that reintroduces per-request
//! parsing without becoming flaky on a loaded machine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Instant;

use easypdf_render::cache::{TileKey, ZoomBucket};
use easypdf_render::pdfium::PdfiumRasterizer;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn library_dir() -> Option<PathBuf> {
    let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "mac-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "mac-x64"
    } else if cfg!(target_os = "windows") {
        "win-x64"
    } else {
        "linux-x64"
    };
    let dir = repo_root().join("vendor/pdfium").join(target).join("lib");
    dir.is_dir().then_some(dir)
}

fn corpus(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("tests/corpus").join(name)).unwrap()
}

fn key(page: usize) -> TileKey {
    TileKey { page, zoom: ZoomBucket::from_zoom(1.0), rotation: 0 }
}

#[test]
fn holding_a_document_open_beats_reparsing_it() {
    let Some(dir) = library_dir() else {
        eprintln!("skipping: vendored PDFium not present");
        return;
    };
    let rasterizer = PdfiumRasterizer::load_from(&dir).unwrap();
    let bytes = corpus("many-pages.pdf");
    const PAGES: usize = 40;

    let document = rasterizer.open(bytes.clone(), None).unwrap();

    // First page access carries a one-time cost: PDFium builds its page index
    // then, not at open. Measured separately rather than folded into the
    // steady-state figure, because charging warm-up to one arm of a comparison
    // is how you conclude the wrong thing.
    let warm_up = Instant::now();
    document.render(key(0)).unwrap();
    let warm_up = warm_up.elapsed();

    let start = Instant::now();
    for page in 0..PAGES {
        document.render(key(page)).unwrap();
    }
    let held_open = start.elapsed();

    // Re-open per render — what the worker used to do.
    let start = Instant::now();
    for page in 0..PAGES {
        let fresh = rasterizer.open(bytes.clone(), None).unwrap();
        fresh.render(key(page)).unwrap();
    }
    let reparsed = start.elapsed();

    println!(
        "warm-up {warm_up:?} | {PAGES} pages: held open {held_open:?}, \
         re-parsed {reparsed:?} ({:.1}x faster)",
        reparsed.as_secs_f64() / held_open.as_secs_f64().max(f64::EPSILON)
    );

    assert!(
        held_open < reparsed,
        "steady-state renders should be faster on a held-open document: \
         {held_open:?} vs {reparsed:?}"
    );
}

#[test]
fn warm_up_cost_is_not_paid_per_page() {
    // The property that matters: PDFium's page-index build must happen once,
    // not on every render. If it repeated, every scroll tick would cost tens
    // of milliseconds and the viewer would feel broken.
    //
    // Asserted as an absolute bound rather than a ratio against the first
    // render. Tests share a process and PDFium is a process-wide singleton, so
    // by the time this runs the library may already be warm and "first" is
    // then indistinguishable from "later" — a ratio assertion fails for a
    // reason that has nothing to do with the behaviour under test.
    let Some(dir) = library_dir() else {
        eprintln!("skipping: vendored PDFium not present");
        return;
    };
    let rasterizer = PdfiumRasterizer::load_from(&dir).unwrap();
    let document = rasterizer.open(corpus("many-pages.pdf"), None).unwrap();

    document.render(key(0)).unwrap(); // absorb any warm-up

    let start = Instant::now();
    for page in 1..20 {
        document.render(key(page)).unwrap();
    }
    let average = start.elapsed() / 19;

    // Warm-up is tens of milliseconds; a warm render is tens of microseconds.
    // Five milliseconds sits far below the former and far above the latter,
    // even in a debug build on a loaded machine.
    println!("average warm render: {average:?}");
    assert!(
        average < std::time::Duration::from_millis(5),
        "renders averaged {average:?}, which suggests per-page index rebuilding"
    );
}

#[test]
fn opening_a_two_hundred_page_document_is_fast() {
    // ideas/04-performance-budget.md: a 500-page document must reach an
    // interactive state within a second. Opening must not walk every page.
    let Some(dir) = library_dir() else {
        eprintln!("skipping: vendored PDFium not present");
        return;
    };
    let rasterizer = PdfiumRasterizer::load_from(&dir).unwrap();

    let start = Instant::now();
    let document = rasterizer.open(corpus("many-pages.pdf"), None).unwrap();
    let page_count = document.page_count();
    let elapsed = start.elapsed();

    println!("opened {page_count} pages in {elapsed:?}");
    assert_eq!(page_count, 200);
    assert!(elapsed.as_millis() < 1000, "opening took {elapsed:?}");
}
