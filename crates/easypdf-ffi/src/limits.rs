//! Hard limits protecting against resource exhaustion.
//!
//! Mitigation for threat T3 in `ideas/07-security.md`: decompression bombs,
//! pathological object graphs, and content streams that render forever.
//!
//! Every limit lives here rather than being scattered through the parser, so
//! they can be audited and tuned in one place.

/// Resource ceilings applied to a single document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted decompressed stream, in bytes.
    pub max_decompressed_bytes: u64,
    /// Largest accepted ratio of decompressed to compressed size.
    ///
    /// The classic decompression bomb is small on disk and enormous in memory,
    /// so the ratio matters independently of the absolute size.
    pub max_decompression_ratio: u32,
    /// Maximum object-graph nesting depth.
    pub max_object_depth: u32,
    /// Maximum number of indirect objects.
    pub max_object_count: u64,
    /// Maximum wall-clock milliseconds spent rendering one page.
    pub max_page_render_ms: u32,
    /// Maximum pages in a document.
    pub max_pages: u32,
}

impl Default for Limits {
    /// Defaults chosen to admit every legitimate document encountered in the
    /// test corpus while refusing pathological ones.
    ///
    /// Erring generous: a false positive means refusing a document the user
    /// legitimately owns, which is a worse failure than a slow render.
    fn default() -> Self {
        Self {
            max_decompressed_bytes: 2 * 1024 * 1024 * 1024,
            max_decompression_ratio: 1000,
            max_object_depth: 256,
            max_object_count: 10_000_000,
            max_page_render_ms: 10_000,
            max_pages: 100_000,
        }
    }
}

impl Limits {
    /// Tighter limits for untrusted contexts such as thumbnail generation.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            max_decompressed_bytes: 256 * 1024 * 1024,
            max_decompression_ratio: 100,
            max_object_depth: 64,
            max_object_count: 1_000_000,
            max_page_render_ms: 2_000,
            max_pages: 10_000,
        }
    }

    /// Checks a decompression against both the absolute and ratio limits.
    ///
    /// Returns the name of the limit that was exceeded, or `None` if allowed.
    #[must_use]
    pub fn check_decompression(&self, compressed: u64, decompressed: u64) -> Option<&'static str> {
        if decompressed > self.max_decompressed_bytes {
            return Some("max_decompressed_bytes");
        }
        // Zero-length input cannot be assigned a meaningful ratio; the absolute
        // check above already covers it.
        if let Some(ratio) = decompressed.checked_div(compressed)
            && ratio > u64::from(self.max_decompression_ratio)
        {
            return Some("max_decompression_ratio");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_compression_is_allowed() {
        let limits = Limits::default();
        assert_eq!(limits.check_decompression(1_000_000, 4_000_000), None);
    }

    #[test]
    fn decompression_bomb_is_caught_by_ratio() {
        // 1 KB expanding to 1 GB: small absolute input, catastrophic output.
        let limits = Limits::default();
        assert_eq!(
            limits.check_decompression(1024, 1024 * 1024 * 1024),
            Some("max_decompression_ratio")
        );
    }

    #[test]
    fn absolute_ceiling_catches_bombs_with_innocent_ratios() {
        let limits = Limits::strict();
        // Ratio is only 2:1, but the absolute size is far past the ceiling.
        assert_eq!(
            limits.check_decompression(1024 * 1024 * 1024, 2 * 1024 * 1024 * 1024),
            Some("max_decompressed_bytes")
        );
    }

    #[test]
    fn zero_length_input_does_not_divide_by_zero() {
        let limits = Limits::default();
        assert_eq!(limits.check_decompression(0, 0), None);
        assert_eq!(limits.check_decompression(0, u64::MAX), Some("max_decompressed_bytes"));
    }

    #[test]
    fn strict_is_tighter_than_default_on_every_axis() {
        let (s, d) = (Limits::strict(), Limits::default());
        assert!(s.max_decompressed_bytes < d.max_decompressed_bytes);
        assert!(s.max_decompression_ratio < d.max_decompression_ratio);
        assert!(s.max_object_depth < d.max_object_depth);
        assert!(s.max_object_count < d.max_object_count);
        assert!(s.max_page_render_ms < d.max_page_render_ms);
        assert!(s.max_pages < d.max_pages);
    }
}
