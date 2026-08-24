//! Rendering tests against the vendored PDFium build.
//!
//! Requires `scripts/fetch-pdfium.sh` to have run. Tests skip with a clear
//! message when the library is absent rather than failing confusingly — a
//! missing vendored dependency is a setup problem, not a code defect.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use easypdf_render::cache::{TileKey, ZoomBucket};
use easypdf_render::pdfium::{PdfiumRasterizer, RenderError};

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

fn rasterizer() -> Option<PdfiumRasterizer> {
    let dir = library_dir()?;
    Some(
        PdfiumRasterizer::load_from(&dir)
            .expect("vendored PDFium present but failed to load — run scripts/fetch-pdfium.sh"),
    )
}

fn corpus(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("tests/corpus").join(name))
        .unwrap_or_else(|error| panic!("missing corpus file {name}: {error}"))
}

fn key(page: usize, zoom: f32) -> TileKey {
    TileKey { page, zoom: ZoomBucket::from_zoom(zoom), rotation: 0 }
}

macro_rules! rasterizer_or_skip {
    () => {
        match rasterizer() {
            Some(r) => r,
            None => {
                eprintln!("skipping: vendored PDFium not present (run scripts/fetch-pdfium.sh)");
                return;
            }
        }
    };
}

#[test]
fn renders_a_page_to_pixels() {
    let rasterizer = rasterizer_or_skip!();
    let tile = rasterizer.render(&corpus("minimal.pdf"), None, key(0, 1.0)).unwrap();

    // The fixture is 200x100 points at zoom 1.0.
    assert_eq!(tile.width, 200);
    assert_eq!(tile.height, 100);
    assert_eq!(tile.pixels.len(), 200 * 100 * 4, "BGRA is 4 bytes per pixel");
}

#[test]
fn rendered_page_is_not_blank() {
    // A renderer that returns a correctly sized field of white pixels passes
    // every dimension check while being completely broken.
    let rasterizer = rasterizer_or_skip!();
    let tile = rasterizer.render(&corpus("minimal.pdf"), None, key(0, 1.0)).unwrap();

    let distinct: std::collections::HashSet<_> = tile.pixels.as_chunks::<4>().0.iter().collect();
    assert!(
        distinct.len() > 1,
        "page rendered as a single flat colour — nothing was actually drawn"
    );

    let dark = tile.pixels.as_chunks::<4>().0.iter().filter(|p| p[0] < 128).count();
    assert!(dark > 20, "expected text pixels, found only {dark} dark pixels");
}

#[test]
fn zoom_scales_the_output() {
    let rasterizer = rasterizer_or_skip!();
    let small = rasterizer.render(&corpus("minimal.pdf"), None, key(0, 1.0)).unwrap();
    let large = rasterizer.render(&corpus("minimal.pdf"), None, key(0, 2.0)).unwrap();

    assert_eq!(large.width, small.width * 2);
    assert_eq!(large.height, small.height * 2);
}

#[test]
fn page_dimensions_follow_the_document_not_a_default() {
    let rasterizer = rasterizer_or_skip!();
    let wide = rasterizer.render(&corpus("wide.pdf"), None, key(0, 1.0)).unwrap();

    assert_eq!(wide.width, 400);
    assert_eq!(wide.height, 100);
}

#[test]
fn page_count_is_reported_without_rendering() {
    let rasterizer = rasterizer_or_skip!();
    assert_eq!(rasterizer.page_count(&corpus("minimal.pdf"), None).unwrap(), 1);
}

#[test]
fn out_of_range_page_is_refused_with_both_numbers() {
    let rasterizer = rasterizer_or_skip!();
    let error = rasterizer.render(&corpus("minimal.pdf"), None, key(99, 1.0)).unwrap_err();

    match error {
        RenderError::PageOutOfRange { requested, total } => {
            assert_eq!(requested, 99);
            assert_eq!(total, 1);
        }
        other => panic!("expected PageOutOfRange, got {other:?}"),
    }
}

#[test]
fn non_pdf_input_is_rejected_not_guessed() {
    let rasterizer = rasterizer_or_skip!();
    let result = rasterizer.render(&corpus("not-a-pdf.bin"), None, key(0, 1.0));
    assert!(matches!(result, Err(RenderError::OpenFailed(_))), "{result:?}");
}

#[test]
fn truncated_document_fails_cleanly_without_panicking() {
    // Malformed input must produce an error, never a panic — a panic in the
    // worker is a denial-of-service vector. See ideas/07-security.md (T3).
    let rasterizer = rasterizer_or_skip!();
    let result = rasterizer.render(&corpus("truncated.pdf"), None, key(0, 1.0));
    assert!(result.is_err(), "truncated document should not render successfully");
}

#[test]
fn absurd_zoom_is_refused_rather_than_allocating() {
    let rasterizer = rasterizer_or_skip!();
    let result = rasterizer.render(&corpus("minimal.pdf"), None, key(0, 5000.0));
    assert!(
        matches!(result, Err(RenderError::ImplausibleDimensions { .. })),
        "expected a dimension refusal, got {result:?}"
    );
}

#[test]
fn concurrent_rendering_is_safe() {
    // Regression test for a process abort. Rendering from several threads at
    // once used to crash with SIGTRAP; the library now serializes internally.
    // If this test starts killing the test binary, that serialization was
    // removed.
    let Some(dir) = library_dir() else {
        eprintln!("skipping: vendored PDFium not present");
        return;
    };

    let handles: Vec<_> = (0..8)
        .map(|index| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let rasterizer = PdfiumRasterizer::load_from(&dir).unwrap();
                let document = corpus(if index % 2 == 0 { "minimal.pdf" } else { "wide.pdf" });
                rasterizer.render(&document, None, key(0, 1.0)).unwrap().width
            })
        })
        .collect();

    for handle in handles {
        let width = handle.join().expect("render thread should not panic");
        assert!(width == 200 || width == 400);
    }
}
