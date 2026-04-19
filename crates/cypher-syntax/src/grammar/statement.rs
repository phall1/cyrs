//! Statement / `SingleQuery` productions. Spec §4.6, cypher.ungrammar
//! `Statement`, `SingleQuery`.

use crate::SyntaxKind;
use crate::parser::Parser;

use super::{CLAUSE_START, clause};

/// `Statement = SingleQuery` (v1 scope; `Union` deferred).
pub(crate) fn statement(p: &mut Parser<'_>) {
    let m = p.start();
    single_query(p);
    m.complete(p, SyntaxKind::STATEMENT);
}

/// `SingleQuery = Clause+`. We stop the clause loop when we see a
/// statement terminator (`;`), EOF, or something that isn't plausibly a
/// clause start (in which case we've already consumed everything this
/// statement can own).
fn single_query(p: &mut Parser<'_>) {
    // Parse at least one clause, tolerating recovery wrapping.
    let mut parsed_any = false;
    loop {
        if p.current() == SyntaxKind::EOF || p.at(SyntaxKind::SEMI) {
            break;
        }
        if !p.at_ts(CLAUSE_START) {
            if parsed_any {
                break;
            }
            // We promised a clause but see none — the outer statement
            // recoverer will handle it; just return empty.
            p.error("expected a clause (MATCH/WITH/RETURN/...)");
            return;
        }
        clause::clause(p);
        parsed_any = true;
    }
}
