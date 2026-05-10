//! Extension trait sugar for [`TextRange`].
//!
//! cyrs uses byte offsets for every span (spec §4); line/column conversion
//! is owned by [`LineIndex`](crate::LineIndex) and only happens at LSP and
//! diagnostic-render boundaries. The helpers in this module deduplicate the
//! handful of one-line conversions that consumer crates were rolling
//! themselves (`TextRange` → `Range<usize>`, `TextRange` → `Range<u32>`,
//! and the half-open intersection predicate).

use crate::TextRange;

/// Convenience conversions on top of [`TextRange`].
///
/// `TextRange` already exposes [`start()`](TextRange::start) and
/// [`end()`](TextRange::end) returning `TextSize`, plus `Into<u32>` /
/// `Into<usize>` on `TextSize`. This trait just bundles the two-line
/// adapter that consumers kept duplicating into a single call site.
pub trait TextRangeExt {
    /// Convert to a half-open byte range suitable for slicing `&str` or
    /// passing to `codespan_reporting`. Equivalent to
    /// `usize::from(self.start())..usize::from(self.end())`.
    fn as_byte_range(&self) -> std::ops::Range<usize>;

    /// Convert to a half-open `u32` range. Useful for FFI / wire formats
    /// that prefer `u32` offsets over `usize`.
    fn as_u32_range(&self) -> std::ops::Range<u32>;

    /// Returns `true` if the two ranges share at least one byte position
    /// under the half-open convention. Zero-width ranges are handled
    /// correctly — a caret at `10..10` intersects `10..15` but not
    /// `5..9`.
    fn intersects(&self, other: TextRange) -> bool;
}

impl TextRangeExt for TextRange {
    #[inline]
    fn as_byte_range(&self) -> std::ops::Range<usize> {
        usize::from(self.start())..usize::from(self.end())
    }

    #[inline]
    fn as_u32_range(&self) -> std::ops::Range<u32> {
        u32::from(self.start())..u32::from(self.end())
    }

    #[inline]
    fn intersects(&self, other: TextRange) -> bool {
        self.start() <= other.end() && other.start() <= self.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextSize;

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::from(start), TextSize::from(end))
    }

    #[test]
    fn as_byte_range_matches_manual_conversion() {
        let r = range(3, 17);
        assert_eq!(r.as_byte_range(), 3usize..17usize);
        // Equivalent to the inline helper consumers used to roll.
        assert_eq!(
            r.as_byte_range(),
            usize::from(r.start())..usize::from(r.end())
        );
    }

    #[test]
    fn as_u32_range_matches_manual_conversion() {
        let r = range(0, 42);
        assert_eq!(r.as_u32_range(), 0u32..42u32);
    }

    #[test]
    fn intersects_overlapping_ranges() {
        assert!(range(0, 10).intersects(range(5, 15)));
        assert!(range(5, 15).intersects(range(0, 10)));
    }

    #[test]
    fn intersects_touching_endpoints_is_true() {
        // Half-open: [0,10) and [10,20) touch at 10. The legacy helpers
        // returned true for this and we preserve that.
        assert!(range(0, 10).intersects(range(10, 20)));
    }

    #[test]
    fn intersects_disjoint_returns_false() {
        assert!(!range(0, 5).intersects(range(6, 10)));
        assert!(!range(6, 10).intersects(range(0, 5)));
    }

    #[test]
    fn zero_width_caret_intersects_containing_range() {
        // Caret at 10..10 inside 10..15 → true.
        assert!(range(10, 10).intersects(range(10, 15)));
        // Caret at 10..10 outside 5..9 → false.
        assert!(!range(10, 10).intersects(range(5, 9)));
    }
}
