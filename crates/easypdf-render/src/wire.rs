//! Pixel format conversion and the transport encoding for rendered pages.
//!
//! Lives here rather than in the desktop shell: the shell is meant to be thin
//! orchestration, and logic that touches every pixel of every page is neither
//! thin nor untestable-by-accident. See `ideas/02-architecture.md`.

use crate::cache::Tile;

/// Bytes of header prefixed to a page: `u32` width, `u32` height, little-endian.
pub const HEADER_BYTES: usize = 8;

/// Converts PDFium's BGRA output to the RGBA a canvas expects.
///
/// Done in Rust rather than JavaScript because it touches every byte of every
/// tile; a per-pixel loop in JS would be the slowest step in the render path.
#[must_use]
pub fn bgra_to_rgba(mut pixels: Vec<u8>) -> Vec<u8> {
    for chunk in pixels.as_chunks_mut::<4>().0 {
        chunk.swap(0, 2);
    }
    pixels
}

/// Packs a tile for transport: header then pixels.
///
/// Raw bytes rather than JSON. A single page is hundreds of kilobytes; base64
/// inside a JSON string would inflate that by a third and cost a parse on both
/// sides, for every tile, forever.
#[must_use]
pub fn encode_page(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + pixels.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(pixels);
    out
}

/// Reverses [`encode_page`].
///
/// Returns `None` when the buffer is too short or its length contradicts the
/// declared dimensions — the same self-consistency check the host applies to
/// worker responses, for the same reason.
#[must_use]
pub fn decode_page(buffer: &[u8]) -> Option<Tile> {
    if buffer.len() < HEADER_BYTES {
        return None;
    }

    let width = u32::from_le_bytes(buffer[0..4].try_into().ok()?);
    let height = u32::from_le_bytes(buffer[4..8].try_into().ok()?);
    let pixels = &buffer[HEADER_BYTES..];

    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if pixels.len() != expected {
        return None;
    }

    Some(Tile { width, height, pixels: pixels.to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_becomes_rgba() {
        // Getting this backwards swaps red and blue on every page, which looks
        // plausible enough on greyscale documents to ship unnoticed.
        assert_eq!(bgra_to_rgba(vec![10, 20, 30, 40]), vec![30, 20, 10, 40]);
    }

    #[test]
    fn conversion_leaves_green_and_alpha_in_place() {
        let converted = bgra_to_rgba(vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(converted[1], 2);
        assert_eq!(converted[3], 4);
        assert_eq!(converted[5], 6);
        assert_eq!(converted[7], 8);
    }

    #[test]
    fn conversion_is_its_own_inverse() {
        let original = vec![9, 8, 7, 6, 5, 4, 3, 2];
        assert_eq!(bgra_to_rgba(bgra_to_rgba(original.clone())), original);
    }

    #[test]
    fn pages_round_trip_through_the_wire_format() {
        let pixels: Vec<u8> = (0..(3 * 2 * 4)).map(|n| n as u8).collect();
        let encoded = encode_page(3, 2, &pixels);

        let decoded = decode_page(&encoded).expect("round trip should succeed");
        assert_eq!(decoded.width, 3);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.pixels, pixels);
    }

    #[test]
    fn short_buffer_is_rejected() {
        assert!(decode_page(&[1, 2, 3]).is_none());
    }

    #[test]
    fn length_contradicting_the_header_is_rejected() {
        // Same class of bug as a hostile worker under-reporting a buffer.
        let mut encoded = encode_page(100, 100, &[0; 40_000]);
        encoded.truncate(HEADER_BYTES + 10);
        assert!(decode_page(&encoded).is_none());
    }

    #[test]
    fn dimension_overflow_is_rejected_rather_than_wrapping() {
        let encoded = encode_page(u32::MAX, u32::MAX, &[0; 4]);
        assert!(decode_page(&encoded).is_none());
    }
}
