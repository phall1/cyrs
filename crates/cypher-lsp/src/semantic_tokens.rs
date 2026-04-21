//! v1 `textDocument/semanticTokens` engine (spec §14.2, bead cy-oko).
//!
//! Classifies every non-trivia `SyntaxToken` in the CST into one of
//! the LSP standard semantic-token types and emits the LSP delta
//! encoding (5-tuple per token: `deltaLine`, `deltaStart`,
//! `length`, `tokenType`, `tokenModifiers`).
//!
//! v1 scope:
//! * Full-file request (`textDocument/semanticTokens/full`) —
//!   implemented.
//! * Range request (`textDocument/semanticTokens/range`) —
//!   implemented by filtering the same walk to tokens that overlap
//!   the requested byte range.
//!
//! Modifiers are left empty for v1; a follow-up can emit
//! `DECLARATION` / `READONLY` / `MODIFICATION` etc. once the HIR
//! position-mapping work lands.

use cypher_db::{Database, FileId};
use cypher_syntax::{LineIndex, SyntaxKind, TextRange, TextSize};
use lsp_types::{
    Range, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensLegend,
};
use rowan::NodeOrToken;

/// Token-type legend.  Index order here is the `tokenType` integer
/// emitted in the 5-tuples; don't reorder without bumping a follow-up
/// bead that teaches clients to re-resolve the legend.
pub(crate) const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,   // 0
    SemanticTokenType::OPERATOR,  // 1
    SemanticTokenType::STRING,    // 2
    SemanticTokenType::NUMBER,    // 3
    SemanticTokenType::COMMENT,   // 4
    SemanticTokenType::VARIABLE,  // 5 — used for IDENT
    SemanticTokenType::PARAMETER, // 6 — used for $param
];

const TOKEN_KEYWORD: u32 = 0;
const TOKEN_OPERATOR: u32 = 1;
const TOKEN_STRING: u32 = 2;
const TOKEN_NUMBER: u32 = 3;
const TOKEN_COMMENT: u32 = 4;
const TOKEN_VARIABLE: u32 = 5;
const TOKEN_PARAMETER: u32 = 6;

/// The legend the server advertises.  Modifiers list is empty in v1.
pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: Vec::<SemanticTokenModifier>::new(),
    }
}

/// Compute semantic tokens for the entire file.
pub(crate) fn compute_full(db: &Database, file_id: FileId) -> SemanticTokens {
    compute(db, file_id, None)
}

/// Compute semantic tokens restricted to a client-requested range.
pub(crate) fn compute_range(db: &Database, file_id: FileId, range: Range) -> SemanticTokens {
    let Ok(source) = db.source_of(file_id) else {
        return SemanticTokens::default();
    };
    let line_index = LineIndex::new(&source);
    let byte_range = TextRange::new(
        position_to_offset(&line_index, range.start).unwrap_or_else(|| TextSize::from(0)),
        position_to_offset(&line_index, range.end).unwrap_or_else(|| TextSize::from(u32::MAX)),
    );
    compute(db, file_id, Some(byte_range))
}

fn compute(db: &Database, file_id: FileId, filter: Option<TextRange>) -> SemanticTokens {
    let Ok(source) = db.source_of(file_id) else {
        return SemanticTokens::default();
    };
    let line_index = LineIndex::new(&source);
    let Ok(parse) = db.parse_cst(file_id) else {
        return SemanticTokens::default();
    };

    // Walk tokens in source order (lexicographic by text_range).  The
    // delta encoding requires that we track the previous token's line
    // and start column.  A `$param` is surfaced as a single PARAMETER
    // token spanning `$` + the following IDENT; we detect it by
    // looking ahead when we see a DOLLAR.
    //
    // We do not currently emit a DOLLAR type because the LSP
    // standard set has no dedicated punctuation type; operators
    // cover the remaining interesting punctuation.
    let mut data: Vec<SemanticToken> = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;

    let root = parse.parse().syntax();
    let mut iter = root.descendants_with_tokens().peekable();
    while let Some(element) = iter.next() {
        let NodeOrToken::Token(token) = element else {
            continue;
        };
        if token.kind().is_trivia()
            && !matches!(
                token.kind(),
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
            )
        {
            continue;
        }

        let mut range = token.text_range();
        let Some(mut token_type) = classify(token.kind()) else {
            continue;
        };

        // Merge DOLLAR + following IDENT into a single PARAMETER token.
        if token.kind() == SyntaxKind::DOLLAR
            && let Some(next_el) = iter.peek()
            && let NodeOrToken::Token(next_tok) = next_el
            && next_tok.kind() == SyntaxKind::IDENT
            && next_tok.text_range().start() == range.end()
        {
            range = TextRange::new(range.start(), next_tok.text_range().end());
            token_type = TOKEN_PARAMETER;
            // Consume the IDENT so we don't emit it twice.
            iter.next();
        }

        if let Some(filter_range) = filter
            && !ranges_intersect(range, filter_range)
        {
            continue;
        }

        if let Some(encoded) = encode(
            &line_index,
            range,
            token_type,
            &mut prev_line,
            &mut prev_start,
        ) {
            data.push(encoded);
        }
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

fn classify(kind: SyntaxKind) -> Option<u32> {
    if kind.is_keyword() {
        return Some(TOKEN_KEYWORD);
    }
    match kind {
        SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => Some(TOKEN_COMMENT),
        SyntaxKind::STRING_LITERAL => Some(TOKEN_STRING),
        SyntaxKind::INT_LITERAL | SyntaxKind::FLOAT_LITERAL => Some(TOKEN_NUMBER),
        SyntaxKind::IDENT => Some(TOKEN_VARIABLE),
        SyntaxKind::DOLLAR => Some(TOKEN_PARAMETER),
        k if k.is_punct() => Some(TOKEN_OPERATOR),
        _ => None,
    }
}

/// Encode one token as the LSP 5-tuple relative to the previous one.
/// Returns `None` when the token spans multiple lines — the LSP
/// protocol doesn't support multi-line tokens; we skip them rather
/// than emit wrong offsets.  This path mainly affects block
/// comments; clients fall back to their grammar for those.
fn encode(
    line_index: &LineIndex,
    range: TextRange,
    token_type: u32,
    prev_line: &mut u32,
    prev_start: &mut u32,
) -> Option<SemanticToken> {
    let start = line_index.line_col(range.start());
    let end = line_index.line_col(range.end());
    if end.line != start.line {
        return None;
    }
    let start_utf16 = line_index.to_utf16(start);
    let end_utf16 = line_index.to_utf16(end);
    let delta_line = start_utf16.line - *prev_line;
    let delta_start = if delta_line == 0 {
        start_utf16.col - *prev_start
    } else {
        start_utf16.col
    };
    let length = end_utf16.col - start_utf16.col;
    *prev_line = start_utf16.line;
    *prev_start = start_utf16.col;
    Some(SemanticToken {
        delta_line,
        delta_start,
        length,
        token_type,
        token_modifiers_bitset: 0,
    })
}

fn ranges_intersect(a: TextRange, b: TextRange) -> bool {
    a.start() <= b.end() && b.start() <= a.end()
}

fn position_to_offset(line_index: &LineIndex, pos: lsp_types::Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
}
