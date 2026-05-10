//! CST-walking pretty-printer (spec 0001 §13.1).
//!
//! # Design
//!
//! The printer collects all tokens in document order, then emits them with
//! normalised spacing. Trivia tokens (`WHITESPACE`, `LINE_COMMENT`, `BLOCK_COMMENT`)
//! receive special handling:
//!
//! - `WHITESPACE`: normalised — single space between same-line tokens, up to
//!   one blank line between logical groups.
//! - `LINE_COMMENT` / `BLOCK_COMMENT`: preserved verbatim.
//!
//! Keywords are uppercased / lowercased per `FmtOptions::keyword_casing`.
//!
//! Clause keywords (`MATCH`, `WHERE`, `WITH`, `RETURN`, …) start a new line
//! when preceded by any content on the same line.
//!
//! ## Partial-input tolerance
//!
//! `ERROR` nodes emit their token text verbatim. The printer never panics.
//!
//! ## `cypher-fmt: off/on`
//!
//! A `LINE_COMMENT` `// cypher-fmt: off` suspends formatting until
//! `// cypher-fmt: on` (spec §13.4). The suspended region is emitted
//! verbatim.
//!
//! ## Determinism
//!
//! Single left-to-right token-stream pass; no `HashMap` iteration.
//! (AGENTS.md §8 / spec §17.14.)

use cypher_syntax::{SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, WalkEvent};

use crate::{FormatOptions, KeywordCasing};

/// State carried while printing.
#[derive(Debug)]
pub struct Printer {
    opts: FormatOptions,
    /// Finished output.
    out: String,
    /// True when nothing has been emitted on the current line yet.
    at_line_start: bool,
    /// Pending whitespace to emit before the next significant token.
    /// `None` = no pending space; `Some(true)` = pending newline;
    /// `Some(false)` = pending space (same line).
    pending: Pending,
    /// Pending blank lines (collapsed to ≤ 1).
    pending_blank: bool,
    /// `cypher-fmt: off` region state (spec §13.4).
    fmt_state: FmtState,
    /// Buffer for raw (unformatted) content while in `FmtState::Off*`.
    raw_buf: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Space,
    Newline,
}

/// Tri-state for the `// cypher-fmt: off` / `on` region (spec §13.4).
///
/// `OffJustEntered` is the brief state immediately after emitting an
/// off-directive; the next raw chunk must drop one leading `\n` to keep
/// `fmt(fmt(s)) == fmt(s)` (cy-eu2). `emit_comment` always writes exactly
/// one `\n` after the directive, so without this strip the buffered blank
/// line grows by one `\n` per formatting pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FmtState {
    On,
    OffJustEntered,
    Off,
}

impl Printer {
    /// Construct a fresh `Printer` initialised with the given options.
    pub fn new(opts: FormatOptions) -> Self {
        Self {
            opts,
            out: String::new(),
            at_line_start: true,
            pending: Pending::Nothing,
            pending_blank: false,
            fmt_state: FmtState::On,
            raw_buf: String::new(),
        }
    }

    /// Walk the entire subtree rooted at `node` and emit formatted output.
    pub fn print_node(&mut self, node: &SyntaxNode) {
        for event in node.preorder_with_tokens() {
            match event {
                WalkEvent::Enter(NodeOrToken::Token(tok)) => {
                    self.handle_token(tok);
                }
                WalkEvent::Enter(NodeOrToken::Node(_)) | WalkEvent::Leave(_) => {}
            }
        }
    }

    fn handle_token(&mut self, tok: cypher_syntax::SyntaxToken) {
        let kind = tok.kind();
        let text = tok.text();

        // ----------------------------------------------------------------
        // Check for fmt:off / fmt:on in any LINE_COMMENT (even inside off).
        // ----------------------------------------------------------------
        if kind == SyntaxKind::LINE_COMMENT {
            let t = text.trim();
            let is_off =
                t == "// cypher-fmt: off" || t == "//cypher-fmt:off" || t == "// cypher-fmt:off";
            let is_on =
                t == "// cypher-fmt: on" || t == "//cypher-fmt:on" || t == "// cypher-fmt:on";

            if is_off && self.fmt_state == FmtState::On {
                // Emit the comment itself (formatted), then suppress.
                // `emit_comment` pushes one `\n` after the directive, so the
                // first raw chunk we buffer must drop one leading `\n` to
                // keep `fmt(fmt(s)) == fmt(s)` (cy-eu2).
                self.emit_comment(text, kind);
                self.fmt_state = FmtState::OffJustEntered;
                return;
            }
            if is_on && self.fmt_state != FmtState::On {
                self.fmt_state = FmtState::On;
                // Flush raw buffer first.
                if !self.raw_buf.is_empty() {
                    let raw = std::mem::take(&mut self.raw_buf);
                    self.out.push_str(&raw);
                }
                self.emit_comment(text, kind);
                return;
            }
        }

        // ----------------------------------------------------------------
        // When formatting is off: accumulate verbatim.
        // ----------------------------------------------------------------
        if self.fmt_state != FmtState::On {
            // Drop one leading `\n` from the first raw chunk after the
            // off-directive so the trailer emission is idempotent (cy-eu2).
            // `emit_comment` already wrote exactly one `\n` after the
            // directive; without this strip, a second formatter pass would
            // re-add a `\n` on top of the buffered blank line that the
            // first pass produced.
            let to_push = if self.fmt_state == FmtState::OffJustEntered {
                self.fmt_state = FmtState::Off;
                text.strip_prefix('\n').unwrap_or(text)
            } else {
                text
            };
            self.raw_buf.push_str(to_push);
            return;
        }

        // ----------------------------------------------------------------
        // Dispatch.
        // ----------------------------------------------------------------
        match kind {
            SyntaxKind::WHITESPACE => self.handle_whitespace(text),
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                self.emit_comment(text, kind);
            }
            _ => self.emit_significant(kind, text),
        }
    }

    // ------------------------------------------------------------------
    // Whitespace handling
    // ------------------------------------------------------------------

    fn handle_whitespace(&mut self, text: &str) {
        let newlines: usize = text.chars().filter(|&c| c == '\n').count();
        if newlines == 0 {
            // Horizontal space — record as pending space (only if not at
            // start of line).
            if !self.at_line_start && self.pending == Pending::Nothing {
                self.pending = Pending::Space;
            }
        } else {
            // At least one newline → we will want a line break.
            // Upgrade any existing pending to newline.
            self.pending = Pending::Newline;
            // Blank line = 2+ newlines.
            if newlines > 1 {
                self.pending_blank = true;
            }
        }
    }

    // ------------------------------------------------------------------
    // Comment emission
    // ------------------------------------------------------------------

    fn emit_comment(&mut self, text: &str, kind: SyntaxKind) {
        if kind == SyntaxKind::LINE_COMMENT {
            // Spec §13.2 I13.3: if there is at least one newline in the trivia
            // BEFORE this comment token (pending == Newline), treat it as a
            // *leading* comment for the next significant token — emit it on its
            // own line. Otherwise (same-line trivia only), treat it as a
            // trailing comment on the current line.
            let is_leading = self.at_line_start || self.pending == Pending::Newline;
            if is_leading {
                if !self.at_line_start {
                    // We have content on the current line; start a new one first.
                    self.out.push('\n');
                    self.at_line_start = true;
                }
                self.flush_pending_newlines();
            } else {
                self.flush_pending_as_space();
            }
            self.out.push_str(text);
            self.out.push('\n');
            self.at_line_start = true;
            self.pending = Pending::Nothing;
            self.pending_blank = false;
        } else {
            // BLOCK_COMMENT — treat like an inline token.
            if self.at_line_start {
                self.flush_pending_newlines();
            } else {
                self.flush_pending_as_space();
            }
            self.out.push_str(text);
            self.at_line_start = false;
            // Record space after block comment.
            self.pending = Pending::Space;
            self.pending_blank = false;
        }
    }

    // ------------------------------------------------------------------
    // Significant token emission
    // ------------------------------------------------------------------

    fn emit_significant(&mut self, kind: SyntaxKind, text: &str) {
        // Clause starters force a new line.
        let clause_start = Self::is_clause_starter(kind);

        if self.at_line_start {
            // We're already on a fresh line (or at the very start).
            // Emit any pending blank line.
            self.flush_pending_newlines();
            // No leading indent in this minimal formatter (depth=0).
        } else if clause_start {
            // Start a new line for this clause keyword.
            self.out.push('\n');
            // Emit blank line if one was pending.
            if self.pending_blank {
                self.out.push('\n');
            }
            self.pending = Pending::Nothing;
            self.pending_blank = false;
            self.at_line_start = false; // we will immediately emit text
        } else if self.pending == Pending::Newline {
            // Spec §17.3.3 P17.3.3 (cy-nu1): a real newline between two
            // non-clause-starter significant tokens must survive the pass,
            // otherwise idempotence breaks the moment a prior pass emits a
            // newline that is re-tokenised as plain whitespace on round-trip
            // (classic case: ERROR-recovery regions around an unterminated
            // string literal). Preserve the break instead of collapsing it
            // to a single space.
            self.out.push('\n');
            if self.pending_blank {
                self.out.push('\n');
            }
            self.pending = Pending::Nothing;
            self.pending_blank = false;
            self.at_line_start = false; // we will immediately emit text
        } else {
            // Inline: apply pending space if this token wants one.
            let want_space_before = !Self::no_space_before(kind);
            if want_space_before && self.pending != Pending::Nothing {
                self.out.push(' ');
            }
            self.pending = Pending::Nothing;
            self.pending_blank = false;
        }

        let emitted = self.apply_casing(kind, text);
        self.out.push_str(&emitted);
        self.at_line_start = false;
        self.pending = Pending::Nothing;
        self.pending_blank = false;

        // After SEMI: force a newline (multi-statement boundary).
        if kind == SyntaxKind::SEMI {
            self.out.push('\n');
            self.at_line_start = true;
        }

        // After opening brackets: next token is tight (no space needed).
        if Self::no_space_after(kind) {
            self.pending = Pending::Nothing;
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// Flush pending state as whitespace when we're not at line start.
    fn flush_pending_as_space(&mut self) {
        if self.pending != Pending::Nothing {
            self.out.push(' ');
            self.pending = Pending::Nothing;
            self.pending_blank = false;
        }
    }

    /// Flush pending newlines (when we ARE at line start).
    fn flush_pending_newlines(&mut self) {
        // Never emit leading blank lines before any output — only emit blank
        // lines between non-empty content.
        if self.pending_blank && !self.out.is_empty() {
            self.out.push('\n');
            self.pending_blank = false;
        }
        self.pending_blank = false;
        self.pending = Pending::Nothing;
        self.at_line_start = false;
    }

    /// Clause-level keywords that start a new output line.
    fn is_clause_starter(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::MATCH_KW
                | SyntaxKind::OPTIONAL_KW
                | SyntaxKind::WHERE_KW
                | SyntaxKind::WITH_KW
                | SyntaxKind::RETURN_KW
                | SyntaxKind::CREATE_KW
                | SyntaxKind::MERGE_KW
                | SyntaxKind::SET_KW
                | SyntaxKind::REMOVE_KW
                | SyntaxKind::DELETE_KW
                | SyntaxKind::DETACH_KW
                | SyntaxKind::UNWIND_KW
                | SyntaxKind::CALL_KW
                | SyntaxKind::ORDER_KW
                | SyntaxKind::SKIP_KW
                | SyntaxKind::LIMIT_KW
                | SyntaxKind::UNION_KW
        )
    }

    /// Tokens where no space is wanted before them.
    fn no_space_before(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::R_PAREN
                | SyntaxKind::R_BRACK
                | SyntaxKind::R_BRACE
                | SyntaxKind::COMMA
                | SyntaxKind::SEMI
                | SyntaxKind::DOT
                | SyntaxKind::COLON
                | SyntaxKind::DOUBLE_COLON
        )
    }

    /// Tokens after which no space should follow (next token is tight).
    fn no_space_after(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::L_PAREN | SyntaxKind::L_BRACK | SyntaxKind::L_BRACE | SyntaxKind::DOT
        )
    }

    fn apply_casing(&self, kind: SyntaxKind, text: &str) -> String {
        if !kind.is_keyword() {
            return text.to_string();
        }
        match self.opts.keyword_casing {
            KeywordCasing::Upper => text.to_uppercase(),
            KeywordCasing::Lower => text.to_lowercase(),
            KeywordCasing::Preserve => text.to_string(),
        }
    }

    /// Consume the printer and return the finished output string.
    pub fn finish(mut self) -> String {
        // Flush any trailing raw buffer (from fmt:off without matching on).
        if !self.raw_buf.is_empty() {
            let raw = std::mem::take(&mut self.raw_buf);
            self.out.push_str(&raw);
        }
        // Strip trailing horizontal whitespace, then strip excess trailing
        // newlines (keep at most one).
        trim_trailing_blank_lines(self.out.trim_end_matches([' ', '\t']))
    }
}

/// Collapse trailing blank lines to a single trailing newline (or none for
/// empty output).
fn trim_trailing_blank_lines(s: &str) -> String {
    let content = s.trim_end_matches('\n');
    if content.is_empty() {
        return String::new();
    }
    format!("{content}\n")
}

#[cfg(test)]
mod printer_tests {
    use crate::{FormatOptions, format, format_with};

    #[test]
    fn no_double_space() {
        let out = format("MATCH  (n)   RETURN  n");
        // Should not have "  " in the output.
        assert!(!out.contains("  "), "double space in output: {out:?}");
    }

    #[test]
    fn clause_on_new_line() {
        let out = format("MATCH (n) WHERE n.x > 1 RETURN n");
        let lines: Vec<_> = out.lines().collect();
        assert!(lines.len() >= 2, "expected multiple lines, got: {out:?}");
    }

    #[test]
    fn keyword_uppercase() {
        let out = format("match (n) return n");
        assert!(out.contains("MATCH"), "expected MATCH uppercase in {out:?}");
        assert!(
            out.contains("RETURN"),
            "expected RETURN uppercase in {out:?}"
        );
    }

    #[test]
    fn keyword_lowercase_option() {
        let opts = FormatOptions {
            keyword_casing: crate::KeywordCasing::Lower,
            ..Default::default()
        };
        let out = format_with("MATCH (n) RETURN n", &opts).unwrap();
        assert!(out.contains("match"), "expected lowercase match in {out:?}");
        assert!(
            out.contains("return"),
            "expected lowercase return in {out:?}"
        );
    }

    #[test]
    fn dot_is_tight() {
        let out = format("MATCH (n) RETURN n.name");
        assert!(out.contains("n.name"), "dot should be tight: {out:?}");
    }

    #[test]
    fn paren_is_tight() {
        let out = format("MATCH (n) RETURN n");
        assert!(out.contains("(n)"), "parens should be tight: {out:?}");
    }
}
