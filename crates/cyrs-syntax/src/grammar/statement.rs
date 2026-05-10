//! Statement / `SingleQuery` productions. Spec §4.6, cypher.ungrammar
//! `Statement`, `SingleQuery`, `UnionQuery`.

use crate::SyntaxKind;
use crate::parser::{Parser, syntax_codes as sc};

use super::{CLAUSE_START, clause};

/// `Statement = SingleQuery (UNION ALL? SingleQuery)*` (spec §19).
///
/// `UNION` / `UNION ALL` joins two or more `SingleQuery` branches into one
/// statement node. Each `UNION [ALL] SingleQuery` pair is wrapped in a
/// `UNION_TAIL` child node; the leading `SingleQuery` is parsed inline.
pub(crate) fn statement(p: &mut Parser<'_>) {
    let m = p.start();
    single_query(p);
    // Consume zero or more `UNION [ALL] SingleQuery` tails.
    while p.at(SyntaxKind::UNION_KW) {
        union_tail(p);
    }
    m.complete(p, SyntaxKind::STATEMENT);
}

/// `UnionTail = 'UNION' 'ALL'? SingleQuery`
fn union_tail(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::UNION_KW));
    let m = p.start();
    p.bump(SyntaxKind::UNION_KW);
    p.eat(SyntaxKind::ALL_KW);
    single_query(p);
    m.complete(p, SyntaxKind::UNION_TAIL);
}

/// `SingleQuery = Clause+`. We stop the clause loop when we see a
/// statement terminator (`;`), EOF, `UNION` (handled by the caller),
/// or something that isn't plausibly a clause start (in which case
/// we've already consumed everything this statement can own).
fn single_query(p: &mut Parser<'_>) {
    // Parse at least one clause, tolerating recovery wrapping.
    let mut parsed_any = false;
    loop {
        if p.current() == SyntaxKind::EOF || p.at(SyntaxKind::SEMI) || p.at(SyntaxKind::UNION_KW) {
            break;
        }
        if !p.at_ts(CLAUSE_START) {
            if parsed_any {
                break;
            }
            // We promised a clause but see none — the outer statement
            // recoverer will handle it; just return empty.
            p.error_code(
                sc::EXPECTED_CLAUSE,
                "expected a clause (MATCH/WITH/RETURN/...)",
            );
            return;
        }
        clause::clause(p);
        parsed_any = true;
    }
}
