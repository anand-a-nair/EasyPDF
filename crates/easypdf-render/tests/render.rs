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

/// Opens a corpus document, parsed once.
fn open(rasterizer: &PdfiumRasterizer, name: &str) -> easypdf_render::pdfium::OpenDocument {
    rasterizer.open(corpus(name), None).expect("corpus document should open")
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
    let tile = open(&rasterizer, "minimal.pdf").render(key(0, 1.0)).unwrap();

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
    let tile = open(&rasterizer, "minimal.pdf").render(key(0, 1.0)).unwrap();

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
    let small = open(&rasterizer, "minimal.pdf").render(key(0, 1.0)).unwrap();
    let large = open(&rasterizer, "minimal.pdf").render(key(0, 2.0)).unwrap();

    assert_eq!(large.width, small.width * 2);
    assert_eq!(large.height, small.height * 2);
}

#[test]
fn page_dimensions_follow_the_document_not_a_default() {
    let rasterizer = rasterizer_or_skip!();
    let wide = open(&rasterizer, "wide.pdf").render(key(0, 1.0)).unwrap();

    assert_eq!(wide.width, 400);
    assert_eq!(wide.height, 100);
}

#[test]
fn page_count_is_reported_without_rendering() {
    let rasterizer = rasterizer_or_skip!();
    assert_eq!(open(&rasterizer, "minimal.pdf").page_count(), 1);
}

#[test]
fn out_of_range_page_is_refused_with_both_numbers() {
    let rasterizer = rasterizer_or_skip!();
    let error = open(&rasterizer, "minimal.pdf").render(key(99, 1.0)).unwrap_err();

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
    let result = rasterizer.open(corpus("not-a-pdf.bin"), None);
    assert!(matches!(result, Err(RenderError::OpenFailed(_))), "expected refusal");
}

#[test]
fn truncated_document_fails_cleanly_without_panicking() {
    // Malformed input must produce an error, never a panic — a panic in the
    // worker is a denial-of-service vector. See ideas/07-security.md (T3).
    let rasterizer = rasterizer_or_skip!();
    let result = rasterizer.open(corpus("truncated.pdf"), None);
    assert!(result.is_err(), "truncated document should not open successfully");
}

#[test]
fn absurd_zoom_is_refused_rather_than_allocating() {
    let rasterizer = rasterizer_or_skip!();
    let result = open(&rasterizer, "minimal.pdf").render(key(0, 5000.0));
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
                let name = if index % 2 == 0 { "minimal.pdf" } else { "wide.pdf" };
                let document = rasterizer.open(corpus(name), None).unwrap();
                document.render(key(0, 1.0)).unwrap().width
            })
        })
        .collect();

    for handle in handles {
        let width = handle.join().expect("render thread should not panic");
        assert!(width == 200 || width == 400);
    }
}

/// The full viewer pipeline on real output.
///
/// Renders a page, converts BGRA to RGBA, encodes it for transport, decodes it
/// back, and checks the image survived. This is the path every displayed page
/// takes; unit tests on synthetic buffers would not catch a mistake that only
/// shows up on real pixel data.
#[test]
fn rendered_page_survives_the_full_display_pipeline() {
    use easypdf_render::wire::{bgra_to_rgba, decode_page, encode_page};

    let rasterizer = rasterizer_or_skip!();
    let tile = open(&rasterizer, "minimal.pdf").render(key(0, 1.0)).unwrap();

    let original = tile.pixels.clone();
    let rgba = bgra_to_rgba(tile.pixels);

    let encoded = encode_page(tile.width, tile.height, &rgba);
    let decoded = decode_page(&encoded).expect("a freshly encoded page must decode");

    assert_eq!(decoded.width, 200);
    assert_eq!(decoded.height, 100);
    assert_eq!(decoded.pixels.len(), 200 * 100 * 4);

    // Red and blue swapped, green and alpha untouched.
    for (index, (before, after)) in original.iter().zip(decoded.pixels.iter()).enumerate() {
        match index % 4 {
            1 | 3 => assert_eq!(before, after, "green/alpha changed at byte {index}"),
            _ => {}
        }
    }
    assert_eq!(decoded.pixels[0], original[2], "red should come from the blue slot");
    assert_eq!(decoded.pixels[2], original[0], "blue should come from the red slot");

    // And it is still a page with text on it, not a flat field.
    let dark = decoded.pixels.as_chunks::<4>().0.iter().filter(|p| p[0] < 128).count();
    assert!(dark > 20, "pipeline produced a blank page: {dark} dark pixels");
}

// ---------------------------------------------------------------------------
// Text extraction and search
// ---------------------------------------------------------------------------

#[test]
fn extracts_text_from_a_page() {
    let rasterizer = rasterizer_or_skip!();
    let text = open(&rasterizer, "minimal.pdf").extract_text(0).unwrap();
    assert!(text.contains("Hello EasyPDF"), "extracted {text:?}");
}

#[test]
fn extraction_of_a_missing_page_is_refused() {
    let rasterizer = rasterizer_or_skip!();
    assert!(open(&rasterizer, "minimal.pdf").extract_text(42).is_err());
}

#[test]
fn search_finds_text_and_reports_where_it_is() {
    let rasterizer = rasterizer_or_skip!();
    let (hits, truncated) = open(&rasterizer, "minimal.pdf").search("EasyPDF", false).unwrap();

    assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    assert_eq!(hits[0].page, 0);
    assert!(!truncated);

    // The rectangle must be real and inside the 200x100 page.
    let rect = hits[0].rects.first().expect("a hit must carry at least one rect");
    assert!(rect.right > rect.left, "degenerate rect: {rect:?}");
    assert!(rect.top > rect.bottom, "degenerate rect: {rect:?}");
    assert!(rect.left >= 0.0 && rect.right <= 200.0, "outside the page: {rect:?}");
    assert!(rect.bottom >= 0.0 && rect.top <= 100.0, "outside the page: {rect:?}");
}

#[test]
fn search_is_case_insensitive_by_default_and_exact_when_asked() {
    let rasterizer = rasterizer_or_skip!();
    let document = open(&rasterizer, "minimal.pdf");

    let (insensitive, _) = document.search("easypdf", false).unwrap();
    assert_eq!(insensitive.len(), 1, "case-insensitive search should match");

    let (sensitive, _) = document.search("easypdf", true).unwrap();
    assert!(sensitive.is_empty(), "case-sensitive search should not match");
}

#[test]
fn search_for_absent_text_returns_nothing() {
    let rasterizer = rasterizer_or_skip!();
    let (hits, _) = open(&rasterizer, "minimal.pdf").search("zzzznotpresent", false).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn empty_query_returns_nothing_rather_than_everything() {
    // PDFium errors on an empty search string; returning "no results" is the
    // sane response to an empty search box.
    let rasterizer = rasterizer_or_skip!();
    let (hits, _) = open(&rasterizer, "minimal.pdf").search("", false).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn search_on_a_document_with_no_text_layer_does_not_fail() {
    // A page whose text cannot be read must be skipped, not fail the search:
    // one bad page should not make the rest of the document unsearchable.
    let rasterizer = rasterizer_or_skip!();
    let result = open(&rasterizer, "wide.pdf").search("nothing here", false);
    assert!(result.is_ok(), "{result:?}");
}
