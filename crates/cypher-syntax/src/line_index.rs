//! `LineIndex` — byte-offset → line/column conversion for diagnostic rendering.
//!
//! Spec 0001 §4.5. Offsets into source code are UTF-8 byte [`TextSize`]
//! values (via `text-size`). [`LineIndex`] pre-computes line start positions
//! so that every later lookup is `O(log lines)`. A separate table of wide
//! characters per line lets us cheaply convert UTF-8 column offsets to
//! UTF-16 code units — the conversion used at the LSP boundary.
//!
//! The implementation mirrors rust-analyzer's `ide_db::line_index` (§23
//! defers to rust-analyzer house style) but is owned in-tree, since
//! rust-analyzer's crate is not a dependency.

use text_size::{TextRange, TextSize};

/// A position in source code: 0-based line and 0-based UTF-8 column (byte
/// offset within the line). Multi-byte characters occupy multiple columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineCol {
    /// 0-based line number.
    pub line: u32,
    /// UTF-8 byte offset within the line (not UTF-16 code units).
    pub col: u32,
}

/// A position in LSP-flavoured UTF-16 code units. Used ONLY at the LSP
/// boundary (spec §4.5). Do not propagate inside the analyser — the rest of
/// the compiler uses [`LineCol`] + UTF-8 offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WideLineCol {
    /// 0-based line number.
    pub line: u32,
    /// UTF-16 code-unit offset within the line.
    pub col: u32,
}

/// Line-start table over the original source.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line. `newlines[0] == 0`.
    newlines: Vec<TextSize>,
    /// For each line, a sorted list of entries for every non-ASCII char on
    /// that line. Empty for pure-ASCII lines.
    wide_chars: Vec<Vec<WideChar>>,
}

#[derive(Debug, Clone, Copy)]
struct WideChar {
    /// UTF-8 byte offset within the line.
    start: TextSize,
    /// UTF-8 byte length of the character.
    len: TextSize,
}

impl LineIndex {
    /// Build a `LineIndex` from full source text. `O(n)` in input length.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut newlines = vec![TextSize::from(0)];
        let mut wide_chars: Vec<Vec<WideChar>> = vec![Vec::new()];
        let mut line_start = TextSize::from(0);

        for (offset, ch) in text.char_indices() {
            let offset = TextSize::try_from(offset).expect("source fits in u32 bytes");
            if ch == '\n' {
                let next_line_start = offset + TextSize::of('\n');
                newlines.push(next_line_start);
                wide_chars.push(Vec::new());
                line_start = next_line_start;
                continue;
            }
            if !ch.is_ascii() {
                let start_in_line = offset - line_start;
                wide_chars
                    .last_mut()
                    .expect("at least one line always exists")
                    .push(WideChar {
                        start: start_in_line,
                        len: TextSize::of(ch),
                    });
            }
        }

        Self {
            newlines,
            wide_chars,
        }
    }

    /// Convert a byte offset to 0-based line/column (UTF-8).
    ///
    /// If `offset` falls inside a multi-byte character the result reports
    /// the mechanical byte-offset within the line — callers that need
    /// char-aligned columns are responsible for snapping.
    ///
    /// # Panics
    /// Panics if the computed line index does not fit in `u32`, which
    /// requires a source file with more than `u32::MAX` lines — impossible
    /// for any input that also fits in a `TextSize`.
    #[must_use]
    pub fn line_col(&self, offset: TextSize) -> LineCol {
        let line = self
            .newlines
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.newlines[line];
        let col = offset - line_start;
        LineCol {
            line: u32::try_from(line).expect("line count fits in u32"),
            col: u32::from(col),
        }
    }

    /// Convert a [`LineCol`] (UTF-8) to the LSP UTF-16 flavour.
    #[must_use]
    pub fn to_utf16(&self, pos: LineCol) -> WideLineCol {
        let line = pos.line as usize;
        if line >= self.wide_chars.len() || self.wide_chars[line].is_empty() {
            return WideLineCol {
                line: pos.line,
                col: pos.col,
            };
        }
        let mut col = pos.col;
        for wc in &self.wide_chars[line] {
            if u32::from(wc.start) >= pos.col {
                break;
            }
            // A non-ASCII char in UTF-8 occupies `wc.len` bytes; in UTF-16
            // it occupies 1 code unit for BMP (len <= 3) and 2 for astral
            // (len == 4).
            let utf8_len = u32::from(wc.len);
            let utf16_len: u32 = if utf8_len == 4 { 2 } else { 1 };
            col = col - utf8_len + utf16_len;
        }
        WideLineCol {
            line: pos.line,
            col,
        }
    }

    /// Convert an LSP UTF-16 position back to a UTF-8 byte [`LineCol`].
    #[must_use]
    pub fn from_utf16(&self, pos: WideLineCol) -> LineCol {
        let line = pos.line as usize;
        if line >= self.wide_chars.len() || self.wide_chars[line].is_empty() {
            return LineCol {
                line: pos.line,
                col: pos.col,
            };
        }
        let mut utf16_seen: u32 = 0;
        let mut col = pos.col;
        for wc in &self.wide_chars[line] {
            let wc_col_utf8 = u32::from(wc.start);
            // Number of ASCII cols between the previous wide char and this
            // one is the same in both encodings — skip past them.
            if wc_col_utf8 + utf16_seen >= col {
                break;
            }
            let utf8_len = u32::from(wc.len);
            let utf16_len: u32 = if utf8_len == 4 { 2 } else { 1 };
            // The caller gave us a UTF-16 col; we advance by
            // `(utf8_len - utf16_len)` extra bytes for this char.
            col = col + utf8_len - utf16_len;
            utf16_seen += utf16_len;
        }
        LineCol {
            line: pos.line,
            col,
        }
    }

    /// Range of the given 0-based line in full-source byte offsets.
    /// Includes the trailing `\n` if the line has one. Returns `None` for
    /// lines past the end.
    ///
    /// For the final line of the input (which has no trailing newline in
    /// the index) the range extends to `TextSize::from(u32::MAX)` as a
    /// sentinel end — callers that care about the "real" tail should clamp
    /// to the input length.
    #[must_use]
    pub fn line_range(&self, line: u32) -> Option<TextRange> {
        let idx = line as usize;
        let start = *self.newlines.get(idx)?;
        let end = self
            .newlines
            .get(idx + 1)
            .copied()
            .unwrap_or(TextSize::from(u32::MAX));
        Some(TextRange::new(start, end))
    }

    /// Number of lines in the input. Always `>= 1`; empty input has a
    /// single empty line.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.newlines.len()).expect("line count fits in u32")
    }
}

#[cfg(test)]
mod tests {
    use super::{LineCol, LineIndex, WideLineCol};
    use pretty_assertions::assert_eq;
    use text_size::{TextRange, TextSize};

    #[test]
    fn empty_input_is_one_line() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
    }

    #[test]
    fn ascii_single_line_line_col() {
        let idx = LineIndex::new("abc");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.line_col(TextSize::from(2)), LineCol { line: 0, col: 2 });
    }

    #[test]
    fn ascii_multi_line_line_col() {
        // "ab\ncde\nf"
        //  0 1  2 3 4 5  6 7
        // line 0: "ab\n" (offsets 0..3)
        // line 1: "cde\n" (offsets 3..7)
        // line 2: "f" (offset 7)
        let idx = LineIndex::new("ab\ncde\nf");
        assert_eq!(idx.line_count(), 3);
        assert_eq!(idx.line_col(TextSize::from(0)), LineCol { line: 0, col: 0 });
        assert_eq!(idx.line_col(TextSize::from(2)), LineCol { line: 0, col: 2 });
        assert_eq!(idx.line_col(TextSize::from(3)), LineCol { line: 1, col: 0 });
        assert_eq!(idx.line_col(TextSize::from(6)), LineCol { line: 1, col: 3 });
        assert_eq!(idx.line_col(TextSize::from(7)), LineCol { line: 2, col: 0 });
    }

    #[test]
    fn utf8_offset_is_bytes_not_chars() {
        // "éllo" — 'é' is 2 UTF-8 bytes (0xC3 0xA9).
        // offset 2 is the byte after 'é' → col 2.
        let idx = LineIndex::new("éllo");
        assert_eq!(idx.line_col(TextSize::from(2)), LineCol { line: 0, col: 2 });
        // Offset 1 lands in the middle of 'é'; we report the mechanical
        // result (col 1) — callers that want char alignment must snap.
        assert_eq!(idx.line_col(TextSize::from(1)), LineCol { line: 0, col: 1 });
    }

    #[test]
    fn utf16_round_trip_bmp() {
        // "abé": 'a'=1B, 'b'=1B, 'é'=2B UTF-8; in UTF-16 all three chars
        // are 1 code unit. After 'é' UTF-8 col is 4, UTF-16 col is 3.
        let idx = LineIndex::new("abé");
        let utf8 = idx.line_col(TextSize::from(4));
        assert_eq!(utf8, LineCol { line: 0, col: 4 });
        let utf16 = idx.to_utf16(utf8);
        assert_eq!(utf16, WideLineCol { line: 0, col: 3 });
        let back = idx.from_utf16(utf16);
        assert_eq!(back, utf8);
    }

    #[test]
    fn utf16_round_trip_astral() {
        // "a\u{1F600}b": 'a'=1B, grin=4B UTF-8 / 2 UTF-16 code units, 'b'=1B.
        // UTF-8 offset after grin is 5; UTF-16 col is 3 (1 + 2).
        let idx = LineIndex::new("a\u{1F600}b");
        let utf8 = idx.line_col(TextSize::from(5));
        assert_eq!(utf8, LineCol { line: 0, col: 5 });
        let utf16 = idx.to_utf16(utf8);
        assert_eq!(utf16, WideLineCol { line: 0, col: 3 });
        let back = idx.from_utf16(utf16);
        assert_eq!(back, utf8);
    }

    #[test]
    fn line_range_last_line_open_ended() {
        // "ab\ncd": line 0 = 0..3, line 1 = 3..u32::MAX sentinel.
        let idx = LineIndex::new("ab\ncd");
        assert_eq!(
            idx.line_range(0),
            Some(TextRange::new(TextSize::from(0), TextSize::from(3)))
        );
        assert_eq!(
            idx.line_range(1),
            Some(TextRange::new(TextSize::from(3), TextSize::from(u32::MAX)))
        );
        assert_eq!(idx.line_range(2), None);
    }
}
