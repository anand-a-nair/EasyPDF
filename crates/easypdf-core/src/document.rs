//! The document model: pages, geometry, and metadata.

use crate::error::{Error, Result};

/// Zero-based index of a page within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageIndex(pub usize);

/// Page dimensions in PDF points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

impl PageSize {
    /// US Letter, 612 x 792 points.
    pub const LETTER: Self = Self { width: 612.0, height: 792.0 };
    /// ISO A4, 595 x 842 points.
    pub const A4: Self = Self { width: 595.0, height: 842.0 };

    /// Returns the size as it appears after `rotation` is applied.
    #[must_use]
    pub fn rotated(self, rotation: Rotation) -> Self {
        if rotation.swaps_axes() { Self { width: self.height, height: self.width } } else { self }
    }
}

/// Page rotation. PDF permits only right-angle multiples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// No rotation.
    #[default]
    None,
    /// 90 degrees clockwise.
    Clockwise90,
    /// 180 degrees.
    Half,
    /// 270 degrees clockwise (90 counter-clockwise).
    Clockwise270,
}

impl Rotation {
    /// Builds a rotation from a PDF `/Rotate` value.
    ///
    /// The spec requires a multiple of 90, but real documents contain
    /// negative values and values beyond 360, so normalize rather than reject:
    /// refusing to open a readable document over this would be pedantry.
    /// Genuinely non-right-angle values are an error.
    pub fn from_degrees(degrees: i32) -> Result<Self> {
        match degrees.rem_euclid(360) {
            0 => Ok(Self::None),
            90 => Ok(Self::Clockwise90),
            180 => Ok(Self::Half),
            270 => Ok(Self::Clockwise270),
            other => Err(Error::NotAPdf {
                reason: format!("page rotation {other} is not a multiple of 90"),
            }),
        }
    }

    /// The rotation in degrees clockwise.
    #[must_use]
    pub fn degrees(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Clockwise90 => 90,
            Self::Half => 180,
            Self::Clockwise270 => 270,
        }
    }

    /// Whether this rotation exchanges width and height.
    #[must_use]
    pub fn swaps_axes(self) -> bool {
        matches!(self, Self::Clockwise90 | Self::Clockwise270)
    }

    /// Composes two rotations.
    #[must_use]
    pub fn then(self, other: Self) -> Self {
        // Both operands are already valid multiples of 90, so the sum is too.
        Self::from_degrees(self.degrees() + other.degrees()).unwrap_or(Self::None)
    }
}

/// A single page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Intrinsic size before rotation.
    pub size: PageSize,
    /// Rotation applied for display.
    pub rotation: Rotation,
}

/// An open document.
///
/// Currently holds only page geometry. Parsing is not yet implemented — the
/// object graph arrives with Phase 1. The shape is established now so the
/// command layer and the worker protocol have something real to target.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pages: Vec<Page>,
}

impl Document {
    /// Creates a document from a list of pages.
    #[must_use]
    pub fn from_pages(pages: Vec<Page>) -> Self {
        Self { pages }
    }

    /// Number of pages.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Returns the page at `index`, or an error if it is out of range.
    pub fn page(&self, index: PageIndex) -> Result<&Page> {
        self.pages
            .get(index.0)
            .ok_or(Error::PageOutOfRange { requested: index.0, total: self.pages.len() })
    }

    /// Mutable access to a page.
    pub fn page_mut(&mut self, index: PageIndex) -> Result<&mut Page> {
        let total = self.pages.len();
        self.pages.get_mut(index.0).ok_or(Error::PageOutOfRange { requested: index.0, total })
    }

    /// Moves a page to a new position, shifting the others.
    pub fn move_page(&mut self, from: PageIndex, to: PageIndex) -> Result<()> {
        let total = self.pages.len();
        if from.0 >= total {
            return Err(Error::PageOutOfRange { requested: from.0, total });
        }
        if to.0 >= total {
            return Err(Error::PageOutOfRange { requested: to.0, total });
        }
        let page = self.pages.remove(from.0);
        self.pages.insert(to.0, page);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(n: usize) -> Document {
        Document::from_pages(
            (0..n).map(|_| Page { size: PageSize::A4, rotation: Rotation::None }).collect(),
        )
    }

    #[test]
    fn rotation_normalizes_out_of_range_and_negative_values() {
        // Real documents contain these; refusing to open them would be wrong.
        assert_eq!(Rotation::from_degrees(-90).unwrap(), Rotation::Clockwise270);
        assert_eq!(Rotation::from_degrees(450).unwrap(), Rotation::Clockwise90);
        assert_eq!(Rotation::from_degrees(0).unwrap(), Rotation::None);
    }

    #[test]
    fn rotation_rejects_non_right_angles() {
        assert!(Rotation::from_degrees(45).is_err());
    }

    #[test]
    fn rotation_composes() {
        assert_eq!(Rotation::Clockwise90.then(Rotation::Clockwise270), Rotation::None);
        assert_eq!(Rotation::Clockwise90.then(Rotation::Clockwise90), Rotation::Half);
    }

    #[test]
    fn quarter_turns_swap_page_dimensions() {
        let landscape = PageSize::A4.rotated(Rotation::Clockwise90);
        assert_eq!(landscape.width, PageSize::A4.height);
        assert_eq!(landscape.height, PageSize::A4.width);
        assert_eq!(PageSize::A4.rotated(Rotation::Half), PageSize::A4);
    }

    #[test]
    fn out_of_range_page_reports_both_numbers() {
        // The error must say what was asked for AND what exists — a bare
        // "invalid page" is useless for diagnosis.
        let err = doc(3).page(PageIndex(7)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains('7'), "{msg}");
        assert!(msg.contains('3'), "{msg}");
    }

    #[test]
    fn move_page_reorders_without_losing_pages() {
        let mut d = doc(4);
        d.page_mut(PageIndex(0)).unwrap().rotation = Rotation::Half;
        d.move_page(PageIndex(0), PageIndex(3)).unwrap();
        assert_eq!(d.page_count(), 4);
        assert_eq!(d.page(PageIndex(3)).unwrap().rotation, Rotation::Half);
    }

    #[test]
    fn move_page_rejects_out_of_range_targets() {
        let mut d = doc(2);
        assert!(d.move_page(PageIndex(0), PageIndex(9)).is_err());
        assert_eq!(d.page_count(), 2, "failed move must not mutate the document");
    }
}
