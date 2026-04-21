//! v1 `textDocument/foldingRange` engine (spec §14.2, bead cy-3ik).
//!
//! Cheap CST walk: every node or block-comment token whose text
//! spans two or more lines becomes a `FoldingRange`.  Uses `parse_cst`
//! only — no sema, no plan.
//!
//! Deduplicates overlapping ranges that share the same start line
//! (e.g. a MATCH clause wrapping a pattern that both start on the
//! same line) so the client gets the outer-most fold for any given
//! anchor.

use cypher_db::{Database, FileId};
use cypher_syntax::{LineIndex, SyntaxKind, SyntaxNode, TextRange};
use lsp_types::{FoldingRange, FoldingRangeKind};
use rowan::{NodeOrToken, WalkEvent};

/// Compute folding ranges for the file.  Returns an empty vec (not
/// None) when the parse succeeds and no nodes span multiple lines —
/// clients prefer an explicit empty list over a protocol error.
pub(crate) fn compute(db: &Database, file_id: FileId) -> Vec<FoldingRange> {
    let Ok(source) = db.source_of(file_id) else {
        return Vec::new();
    };
    let line_index = LineIndex::new(&source);
    let Ok(parse) = db.parse_cst(file_id) else {
        return Vec::new();
    };
    let root = parse.parse().syntax();

    let mut out: Vec<FoldingRange> = Vec::new();
    for event in root.preorder_with_tokens() {
        let WalkEvent::Enter(element) = event else {
            continue;
        };
        match element {
            NodeOrToken::Node(node) => {
                if let Some(r) = node_to_folding_range(&node, &line_index) {
                    out.push(r);
                }
            }
            NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::BLOCK_COMMENT
                    && let Some(r) = range_to_folding_range(
                        tok.text_range(),
                        Some(FoldingRangeKind::Comment),
                        &line_index,
                    )
                {
                    out.push(r);
                }
            }
        }
    }

    // Deduplicate by (start_line, end_line): preorder emits parent
    // before children, so keeping the first occurrence retains the
    // outermost fold at any anchor.
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    out.retain(|r| seen.insert((r.start_line, r.end_line)));
    out
}

fn node_to_folding_range(node: &SyntaxNode, line_index: &LineIndex) -> Option<FoldingRange> {
    // Skip the top-level SOURCE_FILE node — its whole-file fold is
    // unhelpful.
    if node.kind() == SyntaxKind::SOURCE_FILE {
        return None;
    }
    range_to_folding_range(node.text_range(), None, line_index)
}

fn range_to_folding_range(
    range: TextRange,
    kind: Option<FoldingRangeKind>,
    line_index: &LineIndex,
) -> Option<FoldingRange> {
    let start = line_index.line_col(range.start());
    let end = line_index.line_col(range.end());
    if end.line <= start.line {
        return None;
    }
    Some(FoldingRange {
        start_line: start.line,
        start_character: None,
        end_line: end.line,
        end_character: None,
        kind,
        collapsed_text: None,
    })
}
