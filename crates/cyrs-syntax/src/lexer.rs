//! Lexer — `logos`-generated DFA producing a stream of [`LexToken`].
//!
//! Reference: spec 0001 §4.1. Every significant token carries its byte
//! range; trivia (whitespace + comments) is surfaced as regular tokens so
//! the parser can attach it to the enclosing node.
//!
//! Case insensitivity is handled per-keyword via `ignore(case)` on the
//! logos derive; the text of the token preserves the original casing so
//! the formatter can honour user preference when requested.

use logos::Logos;
use smol_str::SmolStr;
use text_size::{TextRange, TextSize};

use crate::SyntaxKind;

/// A single lexed token: kind, original text, and byte range.
#[derive(Debug, Clone)]
pub struct LexToken {
    /// Classification of the token (keyword, identifier, literal, …).
    pub kind: SyntaxKind,
    /// Exact source text the token spans.  Trivia tokens keep their
    /// whitespace / comment text verbatim.
    pub text: SmolStr,
    /// Byte range the token occupies in the source.
    pub range: TextRange,
}

/// A lexer-level diagnostic: a code (matching `DiagCode` discriminants in
/// `cyrs-diag`) plus a message and byte range. Emitted by
/// [`validate_tokens`] for errors that can only be detected after the DFA
/// run — unterminated literals, invalid escape sequences, etc.
///
/// `cyrs-syntax` does not depend on `cyrs-diag` (spec §3.1), so the
/// code is carried as a plain `u16`.
#[derive(Debug, Clone)]
pub struct LexError {
    /// Numeric discriminant of the corresponding `DiagCode` in
    /// `cyrs-diag` (e.g. `4` for `E0004`).
    pub code: u16,
    /// Human-readable message (rustc-style lower-case initial, no
    /// trailing period).
    pub message: String,
    /// Byte range of the offending lexeme.
    pub range: TextRange,
}

/// Scan a token stream for lex-level errors that the DFA cannot express
/// directly:
///
/// - **E0004** — unterminated string literal (a run of ERROR tokens that
///   starts with a quote character but never closes).
/// - **E0005** — unterminated block comment (a run of ERROR tokens that
///   starts with `/*` but has no matching `*/`).
/// - **E0046** — invalid escape sequence inside an otherwise-valid
///   `STRING_LITERAL` token (`\q`, `\X`, etc.).
///
/// Returns a (possibly empty) list of [`LexError`]s in source order. The
/// token stream itself is not modified — every byte remains in a token.
#[must_use]
pub fn validate_tokens(src: &str, tokens: &[LexToken]) -> Vec<LexError> {
    let mut errors: Vec<LexError> = Vec::new();

    // --- E0004 / E0005: unterminated string literal / block comment ---
    //
    // A logos `ERROR` token is produced for any byte (or byte sequence)
    // the DFA cannot recognise. An unterminated string or block comment
    // produces a run of ERROR tokens starting at the opening delimiter.
    // We detect these by scanning error-token runs.
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == SyntaxKind::ERROR {
            let start = tok.range.start();
            let start_usize = usize::from(start);

            // Collect the contiguous run of ERROR tokens.
            let mut run_end = tok.range.end();
            let mut j = i + 1;
            while j < tokens.len() && tokens[j].kind == SyntaxKind::ERROR {
                run_end = tokens[j].range.end();
                j += 1;
            }
            let run_src = &src[start_usize..usize::from(run_end)];

            if run_src.starts_with("/*") {
                // E0005 — unterminated block comment.
                errors.push(LexError {
                    code: super::parser::syntax_codes::UNCLOSED_BLOCK_COMMENT,
                    message: "unterminated block comment".to_owned(),
                    range: TextRange::new(start, run_end),
                });
            } else if run_src.starts_with('"') || run_src.starts_with('\'') {
                // E0004 — unterminated string literal.
                errors.push(LexError {
                    code: super::parser::syntax_codes::UNCLOSED_STRING,
                    message: "unterminated string literal".to_owned(),
                    range: TextRange::new(start, run_end),
                });
            }
            // E0002 / E0006 — unexpected / unrecognised token(s) that
            // are not an unterminated literal.
            if !run_src.starts_with("/*") && !run_src.starts_with('"') && !run_src.starts_with('\'')
            {
                // Heuristic: an error run starting with a digit that
                // contains a base prefix (`0x`, `0o`, `0b`) followed by
                // no valid digits is an invalid numeric literal (E0006).
                let first = run_src.chars().next().unwrap_or('\0');
                let is_bad_numeric = first.is_ascii_digit()
                    && (run_src.starts_with("0x")
                        || run_src.starts_with("0X")
                        || run_src.starts_with("0o")
                        || run_src.starts_with("0O")
                        || run_src.starts_with("0b")
                        || run_src.starts_with("0B"));
                if is_bad_numeric {
                    errors.push(LexError {
                        code: super::parser::syntax_codes::INVALID_NUMERIC_LITERAL,
                        message: format!("invalid numeric literal `{run_src}`"),
                        range: TextRange::new(start, run_end),
                    });
                } else {
                    errors.push(LexError {
                        code: super::parser::syntax_codes::UNEXPECTED_TOKEN,
                        message: format!("unexpected token `{first}`"),
                        range: TextRange::new(start, run_end),
                    });
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }

    // --- E0046: invalid escape sequence in string literal ---------------
    //
    // The DFA matches any `\.` as a valid escape because verifying the
    // escape character would require lookahead inside logos patterns. We
    // perform the semantic check here.
    let valid_escapes: &[char] = &['n', 't', 'r', '\\', '\'', '"', '0', 'b', 'f', 'u', 'U'];
    for tok in tokens {
        if tok.kind != SyntaxKind::STRING_LITERAL {
            continue;
        }
        let text = tok.text.as_str();
        // Strip surrounding quotes.
        let inner = if (text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\''))
        {
            &text[1..text.len() - 1]
        } else {
            continue;
        };
        let mut chars = inner.char_indices().peekable();
        while let Some((byte_off, ch)) = chars.next() {
            if ch == '\\'
                && let Some(&(_, next_ch)) = chars.peek()
            {
                if !valid_escapes.contains(&next_ch) {
                    let abs_start = usize::from(tok.range.start())
                        + 1 // opening quote
                        + byte_off;
                    let abs_end = abs_start + 1 + next_ch.len_utf8();
                    let range = TextRange::new(
                        TextSize::try_from(abs_start).expect("offset fits u32"),
                        TextSize::try_from(abs_end).expect("offset fits u32"),
                    );
                    errors.push(LexError {
                        code: super::parser::syntax_codes::INVALID_ESCAPE,
                        message: format!("invalid escape sequence `\\{next_ch}`"),
                        range,
                    });
                }
                chars.next(); // consume the escape character
            }
        }
    }

    errors
}

/// Tokenise an entire source string. Unknown bytes become [`SyntaxKind::ERROR`]
/// tokens that preserve their range; the lexer never panics on input.
#[must_use]
pub fn lex(src: &str) -> Vec<LexToken> {
    let mut out = Vec::new();
    let mut lex = RawToken::lexer(src);
    while let Some(raw) = lex.next() {
        let range = {
            let span = lex.span();
            let start = TextSize::try_from(span.start).expect("span.start fits u32");
            let end = TextSize::try_from(span.end).expect("span.end fits u32");
            TextRange::new(start, end)
        };
        let text = SmolStr::new(lex.slice());
        let kind = match raw {
            Ok(tok) => tok.to_syntax_kind(),
            Err(()) => SyntaxKind::ERROR,
        };
        out.push(LexToken { kind, text, range });
    }
    out
}

/// Internal logos-generated token enum.
///
/// Keywords use `ignore(case)` per spec §4.1. Identifiers are recognised
/// as a fallback after keywords so `MATCHING` doesn't lex as `MATCH_KW`
/// followed by `ING` — logos resolves to the longest match.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum RawToken {
    // ---- trivia ------------------------------------------------------
    #[regex(r"[ \t\r\n]+")]
    Whitespace,
    #[regex(r"//[^\n\r]*")]
    LineComment,
    // Canonical non-nested C-style block-comment regex. Spec §4.1.
    //   /\*          opening
    //   [^*]*        any non-star run
    //   \*+          one-or-more closing stars
    //   ([^/*] [^*]* \*+)*   non-closing star-runs
    //   /            final slash
    #[regex(r"/\*[^*]*\*+([^/*][^*]*\*+)*/")]
    BlockComment,

    // ---- keywords (case-insensitive) ---------------------------------
    #[token("MATCH", ignore(case))]
    Match,
    #[token("OPTIONAL", ignore(case))]
    Optional,
    #[token("WHERE", ignore(case))]
    Where,
    #[token("WITH", ignore(case))]
    With,
    #[token("RETURN", ignore(case))]
    Return,
    #[token("CREATE", ignore(case))]
    Create,
    #[token("MERGE", ignore(case))]
    Merge,
    #[token("DELETE", ignore(case))]
    Delete,
    #[token("DETACH", ignore(case))]
    Detach,
    #[token("SET", ignore(case))]
    Set,
    #[token("REMOVE", ignore(case))]
    Remove,
    #[token("UNWIND", ignore(case))]
    Unwind,
    #[token("CALL", ignore(case))]
    Call,
    #[token("YIELD", ignore(case))]
    Yield,
    #[token("ON", ignore(case))]
    On,
    #[token("AS", ignore(case))]
    As,
    #[token("AND", ignore(case))]
    And,
    #[token("OR", ignore(case))]
    Or,
    #[token("XOR", ignore(case))]
    Xor,
    #[token("NOT", ignore(case))]
    Not,
    #[token("IN", ignore(case))]
    In,
    #[token("IS", ignore(case))]
    Is,
    #[token("NULL", ignore(case))]
    Null,
    #[token("TRUE", ignore(case))]
    True,
    #[token("FALSE", ignore(case))]
    False,
    #[token("CASE", ignore(case))]
    Case,
    #[token("WHEN", ignore(case))]
    When,
    #[token("THEN", ignore(case))]
    Then,
    #[token("ELSE", ignore(case))]
    Else,
    #[token("END", ignore(case))]
    End,
    #[token("ORDER", ignore(case))]
    Order,
    #[token("BY", ignore(case))]
    By,
    #[token("ASC", ignore(case))]
    Asc,
    #[token("ASCENDING", ignore(case))]
    Ascending,
    #[token("DESC", ignore(case))]
    Desc,
    #[token("DESCENDING", ignore(case))]
    Descending,
    #[token("SKIP", ignore(case))]
    Skip,
    #[token("LIMIT", ignore(case))]
    Limit,
    #[token("DISTINCT", ignore(case))]
    Distinct,
    #[token("UNION", ignore(case))]
    Union,
    #[token("ALL", ignore(case))]
    All,
    #[token("STARTS", ignore(case))]
    Starts,
    #[token("ENDS", ignore(case))]
    Ends,
    #[token("CONTAINS", ignore(case))]
    Contains,
    #[token("DIV", ignore(case))]
    Div,
    #[token("MOD", ignore(case))]
    Mod,
    #[token("COUNT", ignore(case))]
    Count,
    #[token("EXISTS", ignore(case))]
    Exists,
    #[token("shortestPath", ignore(case))]
    ShortestPath,
    #[token("allShortestPaths", ignore(case))]
    AllShortestPaths,
    // List-predicate keywords (cy-8x5). `All` above already carries ALL_KW.
    #[token("ANY", ignore(case))]
    Any,
    #[token("NONE", ignore(case))]
    None,
    #[token("SINGLE", ignore(case))]
    Single,

    // ---- identifiers & parameters ------------------------------------
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", priority = 1)]
    Ident,
    #[regex(r"`(``|[^`])*`")]
    QuotedIdent,
    #[regex(r"\$[A-Za-z_][A-Za-z0-9_]*|\$[0-9]+")]
    Param,

    // ---- numeric literals --------------------------------------------
    // Float first so `1.0` doesn't shadow to `1` + `.` + `0`.
    #[regex(r"[0-9]+\.[0-9]+([eE][+\-]?[0-9]+)?")]
    #[regex(r"[0-9]+[eE][+\-]?[0-9]+")]
    Float,
    #[regex(r"0[xX][0-9A-Fa-f]+")]
    #[regex(r"0[oO][0-7]+")]
    #[regex(r"0[bB][01]+")]
    #[regex(r"[0-9]+")]
    Int,

    // ---- string literals ---------------------------------------------
    #[regex(r#""([^"\\]|\\.)*""#)]
    #[regex(r"'([^'\\]|\\.)*'")]
    String,

    // ---- punctuation -------------------------------------------------
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBrack,
    #[token("]")]
    RBrack,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token("::")]
    DoubleColon,
    #[token(":")]
    Colon,
    #[token("..")]
    DotDot,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
    #[token("*")]
    Star,
    #[token("+")]
    Plus,
    #[token("->")]
    ArrowR,
    #[token("<-")]
    ArrowL,
    #[token("-")]
    Minus,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("^")]
    Caret,
    #[token("<>")]
    Neq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("=~")]
    RegexEq,
    #[token("=")]
    Eq,
    #[token("$")]
    Dollar,
    #[token("!")]
    Bang,
    #[token("&")]
    Amp,
}

impl RawToken {
    fn to_syntax_kind(self) -> SyntaxKind {
        match self {
            Self::Whitespace => SyntaxKind::WHITESPACE,
            Self::LineComment => SyntaxKind::LINE_COMMENT,
            Self::BlockComment => SyntaxKind::BLOCK_COMMENT,

            Self::Match => SyntaxKind::MATCH_KW,
            Self::Optional => SyntaxKind::OPTIONAL_KW,
            Self::Where => SyntaxKind::WHERE_KW,
            Self::With => SyntaxKind::WITH_KW,
            Self::Return => SyntaxKind::RETURN_KW,
            Self::Create => SyntaxKind::CREATE_KW,
            Self::Merge => SyntaxKind::MERGE_KW,
            Self::Delete => SyntaxKind::DELETE_KW,
            Self::Detach => SyntaxKind::DETACH_KW,
            Self::Set => SyntaxKind::SET_KW,
            Self::Remove => SyntaxKind::REMOVE_KW,
            Self::Unwind => SyntaxKind::UNWIND_KW,
            Self::Call => SyntaxKind::CALL_KW,
            Self::Yield => SyntaxKind::YIELD_KW,
            Self::On => SyntaxKind::ON_KW,
            Self::As => SyntaxKind::AS_KW,
            Self::And => SyntaxKind::AND_KW,
            Self::Or => SyntaxKind::OR_KW,
            Self::Xor => SyntaxKind::XOR_KW,
            Self::Not => SyntaxKind::NOT_KW,
            Self::In => SyntaxKind::IN_KW,
            Self::Is => SyntaxKind::IS_KW,
            Self::Null => SyntaxKind::NULL_KW,
            Self::True => SyntaxKind::TRUE_KW,
            Self::False => SyntaxKind::FALSE_KW,
            Self::Case => SyntaxKind::CASE_KW,
            Self::When => SyntaxKind::WHEN_KW,
            Self::Then => SyntaxKind::THEN_KW,
            Self::Else => SyntaxKind::ELSE_KW,
            Self::End => SyntaxKind::END_KW,
            Self::Order => SyntaxKind::ORDER_KW,
            Self::By => SyntaxKind::BY_KW,
            Self::Asc => SyntaxKind::ASC_KW,
            Self::Ascending => SyntaxKind::ASCENDING_KW,
            Self::Desc => SyntaxKind::DESC_KW,
            Self::Descending => SyntaxKind::DESCENDING_KW,
            Self::Skip => SyntaxKind::SKIP_KW,
            Self::Limit => SyntaxKind::LIMIT_KW,
            Self::Distinct => SyntaxKind::DISTINCT_KW,
            Self::Union => SyntaxKind::UNION_KW,
            Self::All => SyntaxKind::ALL_KW,
            Self::Starts => SyntaxKind::STARTS_KW,
            Self::Ends => SyntaxKind::ENDS_KW,
            Self::Contains => SyntaxKind::CONTAINS_KW,
            Self::Div => SyntaxKind::DIV_KW,
            Self::Mod => SyntaxKind::MOD_KW,
            Self::Count => SyntaxKind::COUNT_KW,
            Self::Exists => SyntaxKind::EXISTS_KW,
            Self::ShortestPath => SyntaxKind::SHORTESTPATH_KW,
            Self::AllShortestPaths => SyntaxKind::ALLSHORTESTPATHS_KW,
            Self::Any => SyntaxKind::ANY_KW,
            Self::None => SyntaxKind::NONE_KW,
            Self::Single => SyntaxKind::SINGLE_KW,

            Self::Ident => SyntaxKind::IDENT,
            Self::QuotedIdent => SyntaxKind::QUOTED_IDENT,
            Self::Param => SyntaxKind::PARAM,

            Self::Int => SyntaxKind::INT_LITERAL,
            Self::Float => SyntaxKind::FLOAT_LITERAL,
            Self::String => SyntaxKind::STRING_LITERAL,

            Self::LParen => SyntaxKind::L_PAREN,
            Self::RParen => SyntaxKind::R_PAREN,
            Self::LBrack => SyntaxKind::L_BRACK,
            Self::RBrack => SyntaxKind::R_BRACK,
            Self::LBrace => SyntaxKind::L_BRACE,
            Self::RBrace => SyntaxKind::R_BRACE,
            Self::Comma => SyntaxKind::COMMA,
            Self::Semi => SyntaxKind::SEMI,
            Self::Colon => SyntaxKind::COLON,
            Self::DoubleColon => SyntaxKind::DOUBLE_COLON,
            Self::Dot => SyntaxKind::DOT,
            Self::DotDot => SyntaxKind::DOT_DOT,
            Self::Pipe => SyntaxKind::PIPE,
            Self::Star => SyntaxKind::STAR,
            Self::Plus => SyntaxKind::PLUS,
            Self::Minus => SyntaxKind::MINUS,
            Self::Slash => SyntaxKind::SLASH,
            Self::Percent => SyntaxKind::PERCENT,
            Self::Caret => SyntaxKind::CARET,
            Self::Eq => SyntaxKind::EQ,
            Self::Neq => SyntaxKind::NEQ,
            Self::BangEq => SyntaxKind::BANG_EQ,
            Self::Lt => SyntaxKind::LT,
            Self::Le => SyntaxKind::LE,
            Self::Gt => SyntaxKind::GT,
            Self::Ge => SyntaxKind::GE,
            Self::ArrowR => SyntaxKind::ARROW_R,
            Self::ArrowL => SyntaxKind::ARROW_L,
            Self::RegexEq => SyntaxKind::REGEX_EQ,
            Self::Dollar => SyntaxKind::DOLLAR,
            Self::Bang => SyntaxKind::BANG,
            Self::Amp => SyntaxKind::AMP,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SyntaxKind, lex};
    use text_size::{TextRange, TextSize};

    fn kinds(src: &str) -> Vec<SyntaxKind> {
        lex(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lex_empty() {
        assert!(lex("").is_empty());
    }

    #[test]
    fn tokenises_empty_input_to_zero_tokens() {
        // §4.1: empty input yields no tokens (EOF is synthesised by the
        // parser, not the lexer).
        assert_eq!(lex("").len(), 0);
    }

    #[test]
    fn lex_simple_match() {
        let k = kinds("MATCH (n) RETURN n");
        assert_eq!(
            k,
            vec![
                SyntaxKind::MATCH_KW,
                SyntaxKind::WHITESPACE,
                SyntaxKind::L_PAREN,
                SyntaxKind::IDENT,
                SyntaxKind::R_PAREN,
                SyntaxKind::WHITESPACE,
                SyntaxKind::RETURN_KW,
                SyntaxKind::WHITESPACE,
                SyntaxKind::IDENT,
            ]
        );
    }

    #[test]
    fn keywords_are_case_insensitive() {
        assert_eq!(kinds("match")[0], SyntaxKind::MATCH_KW);
        assert_eq!(kinds("MaTcH")[0], SyntaxKind::MATCH_KW);
    }

    #[test]
    fn keywords_case_insensitive_preserves_case() {
        // §4.1: case-insensitive match, original casing preserved in `text`.
        let toks = lex("match MATCH Match");
        let kw_toks: Vec<_> = toks
            .iter()
            .filter(|t| t.kind == SyntaxKind::MATCH_KW)
            .collect();
        assert_eq!(kw_toks.len(), 3);
        assert_eq!(kw_toks[0].text.as_str(), "match");
        assert_eq!(kw_toks[1].text.as_str(), "MATCH");
        assert_eq!(kw_toks[2].text.as_str(), "Match");
    }

    #[test]
    fn identifier_not_shadowed_by_keyword_prefix() {
        // `MATCHING` must lex as a single IDENT, not MATCH_KW + ING.
        assert_eq!(kinds("MATCHING"), vec![SyntaxKind::IDENT]);
    }

    #[test]
    fn identifiers_vs_keywords() {
        // `matching` is a single IDENT; word-boundary prevents MATCH_KW + ing.
        let toks = lex("matching");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SyntaxKind::IDENT);
        assert_eq!(toks[0].text.as_str(), "matching");
    }

    #[test]
    fn numeric_literals() {
        assert_eq!(kinds("42"), vec![SyntaxKind::INT_LITERAL]);
        assert_eq!(kinds("3.14"), vec![SyntaxKind::FLOAT_LITERAL]);
        assert_eq!(kinds("0xFF"), vec![SyntaxKind::INT_LITERAL]);
        assert_eq!(kinds("0x1f"), vec![SyntaxKind::INT_LITERAL]);
        assert_eq!(kinds("0o17"), vec![SyntaxKind::INT_LITERAL]);
        assert_eq!(kinds("0b10"), vec![SyntaxKind::INT_LITERAL]);
        // Float with exponent.
        assert_eq!(kinds("1.5e10"), vec![SyntaxKind::FLOAT_LITERAL]);
        assert_eq!(kinds("2e-5"), vec![SyntaxKind::FLOAT_LITERAL]);
    }

    #[test]
    fn string_literals() {
        assert_eq!(kinds(r#""hello""#), vec![SyntaxKind::STRING_LITERAL]);
        assert_eq!(kinds("'world'"), vec![SyntaxKind::STRING_LITERAL]);
    }

    #[test]
    fn string_literal_with_escapes() {
        // Single literal covering the full range; escape sequences are
        // syntactically consumed but not decoded at lex time (§4.1).
        let src = "'a\\nb'";
        let toks = lex(src);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SyntaxKind::STRING_LITERAL);
        assert_eq!(toks[0].text.as_str(), src);
        let end = TextSize::try_from(src.len()).expect("len fits u32");
        assert_eq!(toks[0].range, TextRange::new(TextSize::from(0), end));
        // Double-quoted variant with several escapes also lexes as one token.
        let src2 = r#""tab:\t quote:\" backslash:\\""#;
        let toks2 = lex(src2);
        assert_eq!(toks2.len(), 1);
        assert_eq!(toks2[0].kind, SyntaxKind::STRING_LITERAL);
        assert_eq!(toks2[0].text.as_str(), src2);
    }

    #[test]
    fn parameters() {
        assert_eq!(kinds("$foo"), vec![SyntaxKind::PARAM]);
        assert_eq!(kinds("$0"), vec![SyntaxKind::PARAM]);
    }

    #[test]
    fn param_forms() {
        // `$ident` and `$<decimal>` both lex as a single PARAM token.
        let a = lex("$name");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, SyntaxKind::PARAM);
        assert_eq!(a[0].text.as_str(), "$name");
        let b = lex("$0");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, SyntaxKind::PARAM);
        assert_eq!(b[0].text.as_str(), "$0");
    }

    #[test]
    fn quoted_identifier_with_escaped_backtick() {
        // Backtick-delimited, escape by doubling per spec §4.1.
        let src = "`weird``name`";
        let toks = lex(src);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SyntaxKind::QUOTED_IDENT);
        assert_eq!(toks[0].text.as_str(), src);
    }

    #[test]
    fn punctuation_composite() {
        // Each composite operator must lex as exactly ONE token.
        for (src, expected) in [
            ("<>", SyntaxKind::NEQ),
            ("!=", SyntaxKind::BANG_EQ),
            ("<=", SyntaxKind::LE),
            (">=", SyntaxKind::GE),
            ("->", SyntaxKind::ARROW_R),
            ("<-", SyntaxKind::ARROW_L),
            ("::", SyntaxKind::DOUBLE_COLON),
            ("..", SyntaxKind::DOT_DOT),
            ("=~", SyntaxKind::REGEX_EQ),
        ] {
            let toks = lex(src);
            assert_eq!(toks.len(), 1, "expected 1 token for {src:?}");
            assert_eq!(toks[0].kind, expected, "wrong kind for {src:?}");
            assert_eq!(toks[0].text.as_str(), src);
        }
    }

    #[test]
    fn comments() {
        assert_eq!(kinds("// hi"), vec![SyntaxKind::LINE_COMMENT]);
        assert_eq!(kinds("/* hi */"), vec![SyntaxKind::BLOCK_COMMENT]);
    }

    #[test]
    fn block_comment_and_line_comment() {
        // Each comment is a single trivia token spanning its full range.
        let line = lex("// a comment");
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].kind, SyntaxKind::LINE_COMMENT);
        assert_eq!(line[0].text.as_str(), "// a comment");
        let block = lex("/* multi\nline */");
        assert_eq!(block.len(), 1);
        assert_eq!(block[0].kind, SyntaxKind::BLOCK_COMMENT);
        assert_eq!(block[0].text.as_str(), "/* multi\nline */");
    }

    #[test]
    fn losslessness_invariant_sample() {
        let src = "MATCH (n:Person {name: $nm}) // find\nRETURN n";
        let reassembled: String = lex(src).into_iter().map(|t| t.text.to_string()).collect();
        assert_eq!(reassembled, src);
    }

    #[test]
    fn lossless_concat() {
        // §4.4 losslessness invariant at the lexer level: concatenating
        // every token's text reproduces the source byte-for-byte.
        let src = "MATCH (n:Person {name: 'a\\nb', age: 42})\n// trailing\nRETURN n.age + 1";
        let reassembled: String = lex(src).into_iter().map(|t| t.text.to_string()).collect();
        assert_eq!(reassembled, src);
    }

    #[test]
    fn error_token_for_unknown_bytes() {
        // A stray `@` is not a valid token in v1.
        let toks = lex("@");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, SyntaxKind::ERROR);
    }

    #[test]
    fn unknown_byte_becomes_error_token() {
        // A stray multi-byte codepoint (`§`, U+00A7) is not consumable by
        // the DFA; the lexer emits an ERROR token spanning its bytes and
        // does not panic. Losslessness is preserved: text + range round-trip.
        let src = "§";
        let toks = lex(src);
        assert!(!toks.is_empty());
        assert!(toks.iter().all(|t| t.kind == SyntaxKind::ERROR));
        let reassembled: String = toks.iter().map(|t| t.text.to_string()).collect();
        assert_eq!(reassembled, src);
        // First token starts at offset 0.
        assert_eq!(u32::from(toks[0].range.start()), 0);
        // Final token ends at end of input.
        let last_end = usize::from(toks.last().unwrap().range.end());
        assert_eq!(last_end, src.len());
    }
}
