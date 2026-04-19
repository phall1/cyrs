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
    pub kind: SyntaxKind,
    pub text: SmolStr,
    pub range: TextRange,
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

    fn kinds(src: &str) -> Vec<SyntaxKind> {
        lex(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lex_empty() {
        assert!(lex("").is_empty());
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
    fn identifier_not_shadowed_by_keyword_prefix() {
        // `MATCHING` must lex as a single IDENT, not MATCH_KW + ING.
        assert_eq!(kinds("MATCHING"), vec![SyntaxKind::IDENT]);
    }

    #[test]
    fn numeric_literals() {
        assert_eq!(kinds("42"), vec![SyntaxKind::INT_LITERAL]);
        assert_eq!(kinds("3.14"), vec![SyntaxKind::FLOAT_LITERAL]);
        assert_eq!(kinds("0xFF"), vec![SyntaxKind::INT_LITERAL]);
    }

    #[test]
    fn string_literals() {
        assert_eq!(kinds(r#""hello""#), vec![SyntaxKind::STRING_LITERAL]);
        assert_eq!(kinds("'world'"), vec![SyntaxKind::STRING_LITERAL]);
    }

    #[test]
    fn parameters() {
        assert_eq!(kinds("$foo"), vec![SyntaxKind::PARAM]);
        assert_eq!(kinds("$0"), vec![SyntaxKind::PARAM]);
    }

    #[test]
    fn comments() {
        assert_eq!(kinds("// hi"), vec![SyntaxKind::LINE_COMMENT]);
        assert_eq!(kinds("/* hi */"), vec![SyntaxKind::BLOCK_COMMENT]);
    }

    #[test]
    fn losslessness_invariant_sample() {
        let src = "MATCH (n:Person {name: $nm}) // find\nRETURN n";
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
}
