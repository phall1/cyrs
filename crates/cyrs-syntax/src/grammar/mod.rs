//! Cypher grammar — recursive-descent productions consumed by
//! [`crate::parser`]. Spec references §4.2 (engine), §4.3 (recovery),
//! §4.6 (statement boundaries), and the canonical `cypher.ungrammar` at
//! `crates/cyrs-ast/cypher.ungrammar`.
//!
//! This module is intentionally split across submodules so each
//! production set fits on a page. The top-level entry [`source_file`]
//! handles statement boundaries; everything else delegates downward.
//!
//! # v1 scope (cy-nom)
//!
//! - Statements: `SingleQuery` (`Clause+`) with `;`-separated statement
//!   boundaries. `UNION` is deferred.
//! - Clauses: `MATCH` / `OPTIONAL MATCH`, `WHERE`, `RETURN` (with
//!   `DISTINCT`, `ORDER BY`, `SKIP`, `LIMIT`).
//! - Patterns: nodes with labels + property maps; simple fixed-length
//!   relationships (`-[]-`, `-[]->`, `<-[]-`) with optional detail.
//! - Expressions: full Pratt precedence table, postfix (`.`, `[]`, `()`),
//!   unary (`NOT`, `-`, `+`), all core binops including the string-op
//!   family and `IS [NOT] NULL`.
//!
//! Follow-up beads land: `UNWIND`, `CREATE`, `MERGE`, `SET`, `REMOVE`,
//! `DELETE`, `CALL` / `YIELD`, `UNION`, list/map/comprehensions, `CASE`,
//! path-binders, variable-length rels, pattern predicates, the full
//! recovery table (`cy-2vh`), and diagnostic code reconciliation
//! (`cy-a4d`).

use crate::SyntaxKind;
use crate::parser::{Parser, TokenSet, syntax_codes as sc};

pub(crate) mod catalog;
pub(crate) mod clause;
pub(crate) mod expression;
pub(crate) mod pattern;
pub(crate) mod statement;

/// Clause-starter keywords. Entering a clause means the current token is
/// in this set (modulo the `OPTIONAL MATCH` two-token case).
pub(crate) const CLAUSE_START: TokenSet = TokenSet::new(&[
    SyntaxKind::MATCH_KW,
    SyntaxKind::OPTIONAL_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::WITH_KW,
    SyntaxKind::RETURN_KW,
    SyntaxKind::CREATE_KW,
    SyntaxKind::INSERT_KW,
    SyntaxKind::FILTER_KW,
    SyntaxKind::MERGE_KW,
    SyntaxKind::SET_KW,
    SyntaxKind::REMOVE_KW,
    SyntaxKind::DELETE_KW,
    SyntaxKind::DETACH_KW,
    SyntaxKind::UNWIND_KW,
    SyntaxKind::CALL_KW,
]);

/// Entry point. `SourceFile = (Statement (Separator Statement)* Separator?)?`
/// where `Separator` is `;` or the GQL-distinct `NEXT` keyword (ISO/IEC
/// 39075:2024 §14.14; cy-rgqg).
///
/// Empty input produces a single empty `SOURCE_FILE` node with no
/// children and no errors (spec §4.6: "An empty file parses to an empty
/// tree, not an error.").
pub(crate) fn source_file(p: &mut Parser<'_>) {
    let m = p.start();

    // Leading-junk recovery: if we start with something that is not a
    // clause keyword, a catalog-DDL opener, `;`, or `NEXT`, skip until we
    // see one (or EOF). This gives the `garbage MATCH ...` case a usable
    // tree.
    if p.current() != SyntaxKind::EOF
        && !p.at_ts(CLAUSE_START)
        && !p.at(SyntaxKind::SEMI)
        && !p.at(SyntaxKind::NEXT_KW)
        && !p.at(SyntaxKind::UNION_KW)
        && !catalog::at_catalog_create(p)
    {
        p.error_code(sc::EXPECTED_STATEMENT, "expected statement");
        p.recover_until(TokenSet::EMPTY);
    }

    // Defense-in-depth: track parser position across iterations. If the
    // parser makes no progress (token cursor does not advance), force-bump
    // the current token into an ERROR node and continue. This mirrors the
    // rust-analyzer approach and guarantees the loop always terminates.
    while p.current() != SyntaxKind::EOF {
        let pos_before = p.position();
        if p.at(SyntaxKind::SEMI) {
            // Stray semicolon at top level: consume as its own empty statement
            // boundary. We just bump it as punctuation directly under the root.
            p.bump(SyntaxKind::SEMI);
            continue;
        }
        if p.at(SyntaxKind::NEXT_KW) {
            // GQL `NEXT` separator (cy-rgqg, ISO/IEC 39075:2024 §14.14):
            // peer to `;`, consumed at the same shape so multiple
            // catalog-DDL statements chain naturally.
            p.bump(SyntaxKind::NEXT_KW);
            continue;
        }
        if catalog::at_catalog_create(p) {
            catalog::catalog_statement(p);
        } else {
            statement::statement(p);
        }
        if p.at(SyntaxKind::SEMI) {
            p.bump(SyntaxKind::SEMI);
        } else if p.at(SyntaxKind::NEXT_KW) {
            p.bump(SyntaxKind::NEXT_KW);
        } else if p.current() != SyntaxKind::EOF {
            // No separator and more input: recover to the next clause or
            // semicolon so we don't loop forever.
            if !p.at_ts(CLAUSE_START)
                && !p.at(SyntaxKind::UNION_KW)
                && !catalog::at_catalog_create(p)
            {
                p.error_code(
                    sc::EXPECTED_SEMICOLON_OR_EOF,
                    "expected ';' or end of input",
                );
                p.recover_until(TokenSet::EMPTY);
            }
        }
        // No-progress guard: if this iteration consumed nothing, force-bump
        // one token so the loop cannot spin. This is the last line of defence
        // against any future grammar gap that would otherwise cause a hang.
        if p.position() == pos_before {
            let err = p.start();
            p.error_code(sc::UNEXPECTED_TOKEN, "unexpected token");
            p.bump_any();
            err.complete(p, SyntaxKind::ERROR);
        }
    }

    m.complete(p, SyntaxKind::SOURCE_FILE);
}
