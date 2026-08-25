//! Text geometry shared between the renderer and the worker protocol.
//!
//! These live in `easypdf-core` rather than in either of the crates that use
//! them so there is exactly one definition. Two hand-maintained copies of a
//! wire type drift, and the drift shows up as silently wrong coordinates.

use serde::{Deserialize, Serialize};

/// A rectangle in PDF page coordinates: points, origin at the bottom-left.
///
/// Deliberately not in pixels. The caller knows the zoom, and baking a zoom
/// into stored coordinates means every cached result goes stale the moment the
/// user zooms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextRect {
    /// Left edge, in points from the page's left.
    pub left: f32,
    /// Bottom edge, in points from the page's bottom.
    pub bottom: f32,
    /// Right edge, in points from the page's left.
    pub right: f32,
    /// Top edge, in points from the page's bottom.
    pub top: f32,
}

impl TextRect {
    /// Width in points.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    /// Height in points.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.top - self.bottom
    }

    /// Whether the rectangle encloses any area.
    ///
    /// PDFium occasionally reports zero-area boxes for whitespace; drawing
    /// those as highlights produces invisible artefacts and confusing hit
    /// counts.
    #[must_use]
    pub fn is_degenerate(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }
}

/// One search hit: which page, and the rectangles covering the matched text.
///
/// A list of rectangles rather than one, because a match that wraps across a
/// line occupies more than one box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Zero-based page index.
    pub page: usize,
    /// Rectangles covering the matched text, in page points.
    pub rects: Vec<TextRect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_derived_from_the_edges() {
        let rect = TextRect { left: 10.0, bottom: 20.0, right: 40.0, top: 35.0 };
        assert!((rect.width() - 30.0).abs() < f32::EPSILON);
        assert!((rect.height() - 15.0).abs() < f32::EPSILON);
        assert!(!rect.is_degenerate());
    }

    #[test]
    fn zero_area_rectangles_are_flagged() {
        // PDFium reports these for whitespace; highlighting them draws nothing
        // while still counting as a hit.
        let flat = TextRect { left: 10.0, bottom: 20.0, right: 10.0, top: 35.0 };
        assert!(flat.is_degenerate());

        let thin = TextRect { left: 10.0, bottom: 20.0, right: 40.0, top: 20.0 };
        assert!(thin.is_degenerate());
    }

    #[test]
    fn inverted_rectangles_are_flagged_rather_than_reporting_negative_size() {
        let inverted = TextRect { left: 40.0, bottom: 35.0, right: 10.0, top: 20.0 };
        assert!(inverted.is_degenerate());
    }
}

/// One entry in a document's outline (its bookmarks).
///
/// Flattened rather than nested, carrying an explicit `depth`. A tree would be
/// more faithful to the file, but the UI renders a list and a recursive
/// structure across the worker boundary buys nothing but serialisation
/// complexity — and PDF outlines can contain cycles, which a flat walk with a
/// visit budget handles safely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlineEntry {
    /// Entry title as shown to the user.
    pub title: String,
    /// Nesting depth, zero for top-level entries.
    pub depth: u32,
    /// Destination page, if the entry resolves to one.
    ///
    /// `None` for entries whose action is not a simple page jump — those are
    /// shown but not clickable, rather than silently sending the user to
    /// page 1.
    pub page: Option<usize>,
}

/// One character and where it sits on the page.
///
/// Selection is done on the frontend from a cached copy of this list, so a
/// drag never costs an IPC round trip per mouse move. That trade is the whole
/// reason the geometry is shipped up front rather than queried per gesture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharBox {
    /// The character. A string rather than a `char` because it crosses a JSON
    /// boundary, and because PDFium can report a value that is not a scalar.
    pub text: String,
    /// Where it sits, in unrotated page points.
    pub rect: TextRect,
}

/// A page's characters in reading order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextLayout {
    /// Characters in the order PDFium reports them.
    pub chars: Vec<CharBox>,
    /// Whether the list was capped.
    ///
    /// Reported rather than hidden, on the same principle as search truncation:
    /// a silently shortened page is a lie about the document.
    pub truncated: bool,
}
