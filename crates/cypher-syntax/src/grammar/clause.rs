//! Clause productions for v1 cy-nom scope: `MATCH` (with `OPTIONAL`),
//! `WHERE`, `RETURN` (with `DISTINCT`, `ORDER BY`, `SKIP`, `LIMIT`).
//!
//! Spec references: §4.3 (recovery), §4.6 (statement boundaries), and
//! `cypher.ungrammar` `MatchClause` / `WhereClause` / `ReturnClause`.
//! Every other clause in the ungrammar is deferred per cy-nom scope and
//! tagged with `cy-nom: v1 scope` at its stub site in the grammar module.

use crate::SyntaxKind;
use crate::parser::{Parser, TokenSet, syntax_codes as sc};

use super::{expression, pattern};

/// Dispatch on the current token to the appropriate clause production.
/// Caller guarantees `p.at_ts(CLAUSE_START)`.
pub(crate) fn clause(p: &mut Parser<'_>) {
    match p.current() {
        SyntaxKind::MATCH_KW => match_clause(p),
        SyntaxKind::OPTIONAL_KW => optional_match_clause(p),
        SyntaxKind::WHERE_KW => where_clause(p),
        SyntaxKind::RETURN_KW => return_clause(p),
        SyntaxKind::WITH_KW => with_clause(p),
        // cy-nom: v1 scope — UNWIND lands in a follow-up bead.
        SyntaxKind::UNWIND_KW
        // cy-nom: v1 scope — CREATE lands in a follow-up bead.
        | SyntaxKind::CREATE_KW
        // cy-nom: v1 scope — MERGE lands in a follow-up bead.
        | SyntaxKind::MERGE_KW
        // cy-nom: v1 scope — SET lands in a follow-up bead.
        | SyntaxKind::SET_KW
        // cy-nom: v1 scope — REMOVE lands in a follow-up bead.
        | SyntaxKind::REMOVE_KW
        // cy-nom: v1 scope — DELETE / DETACH DELETE land in a follow-up bead.
        | SyntaxKind::DELETE_KW
        | SyntaxKind::DETACH_KW
        // cy-nom: v1 scope — CALL / YIELD land in a follow-up bead.
        | SyntaxKind::CALL_KW => {
            // Until these land: treat the keyword as a single-token stub
            // clause so recovery keeps the rest of the statement useful.
            deferred_clause_stub(p);
        }
        other => unreachable!("clause dispatch on non-clause-start token: {other:?}"),
    }
}

/// Consume the deferred clause's keyword, emit a diagnostic, and skip to
/// the next clause-start / `;` / EOF. Used while only MATCH/WHERE/RETURN
/// are implemented — keeps downstream tests interpretable.
fn deferred_clause_stub(p: &mut Parser<'_>) {
    let m = p.start();
    let kw = p.current();
    p.error_code(
        sc::UNIMPLEMENTED_CLAUSE,
        format!("{kw:?} clause is not implemented in cy-nom"),
    );
    p.bump_any();
    p.recover_until(TokenSet::EMPTY);
    m.complete(p, SyntaxKind::ERROR);
}

/// `MatchClause = 'MATCH' Pattern (',' Pattern)* WhereClause?`
fn match_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::MATCH_KW));
    let m = p.start();
    p.bump(SyntaxKind::MATCH_KW);
    pattern::pattern_list(p);
    if p.at(SyntaxKind::WHERE_KW) {
        where_clause(p);
    }
    m.complete(p, SyntaxKind::MATCH_CLAUSE);
}

/// `OPTIONAL MATCH Pattern (...)` — spec §19, cypher.ungrammar
/// `'OPTIONAL'? 'MATCH' ...`. We emit it under `OPTIONAL_MATCH_CLAUSE`.
fn optional_match_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::OPTIONAL_KW));
    let m = p.start();
    p.bump(SyntaxKind::OPTIONAL_KW);
    if !p.eat(SyntaxKind::MATCH_KW) {
        p.error_code(
            sc::EXPECTED_MATCH_AFTER_OPTIONAL,
            "expected MATCH after OPTIONAL",
        );
    }
    pattern::pattern_list(p);
    if p.at(SyntaxKind::WHERE_KW) {
        where_clause(p);
    }
    m.complete(p, SyntaxKind::OPTIONAL_MATCH_CLAUSE);
}

/// `WhereClause = 'WHERE' Expr`
fn where_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::WHERE_KW));
    let m = p.start();
    p.bump(SyntaxKind::WHERE_KW);
    if expression::expr(p).is_none() {
        p.error_code(sc::EXPECTED_WHERE_EXPR, "expected expression after WHERE");
    }
    m.complete(p, SyntaxKind::WHERE_CLAUSE);
}

/// `WithClause = 'WITH' 'DISTINCT'? ReturnItems WhereClause? OrderBy? Skip? Limit?`
///
/// Cypher §6.4: WITH introduces a projection frame — upstream bindings are
/// replaced by the projected items for downstream clauses. Shape mirrors
/// `RETURN` with an optional inline `WHERE` filter (spec `cypher.ungrammar`
/// `WithClause`).
fn with_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::WITH_KW));
    let m = p.start();
    p.bump(SyntaxKind::WITH_KW);
    p.eat(SyntaxKind::DISTINCT_KW);

    let body = p.start();
    let items = p.start();

    if p.at(SyntaxKind::STAR) {
        let item = p.start();
        p.bump(SyntaxKind::STAR);
        item.complete(p, SyntaxKind::RETURN_ITEM);
    } else {
        return_item(p);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            return_item(p);
        }
    }
    items.complete(p, SyntaxKind::RETURN_ITEMS);

    // Per ungrammar: WITH admits an inline WHERE before the order/skip/limit
    // trailer. Order of trailers follows the spec exactly.
    if p.at(SyntaxKind::WHERE_KW) {
        where_clause(p);
    }
    if p.at(SyntaxKind::ORDER_KW) {
        order_by(p);
    }
    if p.at(SyntaxKind::SKIP_KW) {
        skip_subclause(p);
    }
    if p.at(SyntaxKind::LIMIT_KW) {
        limit_subclause(p);
    }

    body.complete(p, SyntaxKind::RETURN_BODY);
    m.complete(p, SyntaxKind::WITH_CLAUSE);
}

/// `ReturnClause = 'RETURN' 'DISTINCT'? ReturnItem (',' ReturnItem)*
///                 OrderBy? Skip? Limit?`
///
/// Spec defers the full `RETURN *` mixed-list form to cy-nom-follow-ups;
/// we accept `RETURN *` as a standalone star and otherwise require the
/// comma-separated `ReturnItem` list from the ungrammar.
fn return_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::RETURN_KW));
    let m = p.start();
    p.bump(SyntaxKind::RETURN_KW);
    p.eat(SyntaxKind::DISTINCT_KW);

    let body = p.start();
    let items = p.start();

    if p.at(SyntaxKind::STAR) {
        // `RETURN *` — emit as a lone return item containing the star.
        let item = p.start();
        p.bump(SyntaxKind::STAR);
        item.complete(p, SyntaxKind::RETURN_ITEM);
    } else {
        return_item(p);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            return_item(p);
        }
    }
    items.complete(p, SyntaxKind::RETURN_ITEMS);

    if p.at(SyntaxKind::ORDER_KW) {
        order_by(p);
    }
    if p.at(SyntaxKind::SKIP_KW) {
        skip_subclause(p);
    }
    if p.at(SyntaxKind::LIMIT_KW) {
        limit_subclause(p);
    }

    body.complete(p, SyntaxKind::RETURN_BODY);
    m.complete(p, SyntaxKind::RETURN_CLAUSE);
}

/// `ReturnItem = Expr ('AS' IDENT)?`
fn return_item(p: &mut Parser<'_>) {
    let m = p.start();
    if expression::expr(p).is_none() {
        p.error_code(
            sc::EXPECTED_RETURN_EXPR,
            "expected expression in RETURN item",
        );
    }
    if p.eat(SyntaxKind::AS_KW) {
        // Alias: IDENT or QUOTED_IDENT (the `NameDef` variants).
        if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
            p.error_code(sc::EXPECTED_IDENT_AFTER_AS, "expected identifier after AS");
        }
    }
    m.complete(p, SyntaxKind::RETURN_ITEM);
}

fn order_by(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::ORDER_KW));
    let m = p.start();
    p.bump(SyntaxKind::ORDER_KW);
    if !p.eat(SyntaxKind::BY_KW) {
        p.error_code(sc::EXPECTED_BY_AFTER_ORDER, "expected BY after ORDER");
    }
    order_item(p);
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        order_item(p);
    }
    m.complete(p, SyntaxKind::ORDER_BY);
}

fn order_item(p: &mut Parser<'_>) {
    let m = p.start();
    if expression::expr(p).is_none() {
        p.error_code(sc::EXPECTED_ORDERBY_EXPR, "expected expression in ORDER BY");
    }
    // Optional ordering direction.
    match p.current() {
        SyntaxKind::ASC_KW
        | SyntaxKind::ASCENDING_KW
        | SyntaxKind::DESC_KW
        | SyntaxKind::DESCENDING_KW => p.bump_any(),
        _ => {}
    }
    m.complete(p, SyntaxKind::ORDER_ITEM);
}

fn skip_subclause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::SKIP_KW));
    let m = p.start();
    p.bump(SyntaxKind::SKIP_KW);
    if expression::expr(p).is_none() {
        p.error_code(sc::EXPECTED_SKIP_EXPR, "expected expression after SKIP");
    }
    m.complete(p, SyntaxKind::SKIP_SUBCLAUSE);
}

fn limit_subclause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::LIMIT_KW));
    let m = p.start();
    p.bump(SyntaxKind::LIMIT_KW);
    if expression::expr(p).is_none() {
        p.error_code(sc::EXPECTED_LIMIT_EXPR, "expected expression after LIMIT");
    }
    m.complete(p, SyntaxKind::LIMIT_SUBCLAUSE);
}
