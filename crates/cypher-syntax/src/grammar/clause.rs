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
        SyntaxKind::UNWIND_KW => unwind_clause(p),
        SyntaxKind::CREATE_KW => create_clause(p),
        SyntaxKind::MERGE_KW => merge_clause(p),
        SyntaxKind::SET_KW => set_clause(p),
        SyntaxKind::REMOVE_KW => remove_clause(p),
        SyntaxKind::DELETE_KW | SyntaxKind::DETACH_KW => delete_clause(p),
        SyntaxKind::CALL_KW => call_clause(p),
        other => unreachable!("clause dispatch on non-clause-start token: {other:?}"),
    }
}

/// `CallClause = 'CALL' QualifiedName ('(' ArgList? ')')? YieldClause?`
/// — spec §14 / §19 row "CALL <proc> YIELD ..." (cy-4mg, child of cy-lve).
///
/// Standalone (non-subquery) form. The block form `CALL { <subquery> }`
/// is deferred per spec §19 / §20 D1 and recovered by emitting an
/// `UNIMPLEMENTED_CLAUSE` diagnostic and swallowing the balanced braces
/// — mirrors the EXISTS-block treatment in
/// `expression::exists_block_deferred` (cy-lve tranche A).
///
/// `QualifiedName` is `IDENT ('.' IDENT)*` and is wrapped in a
/// `PROCEDURE_NAME` node for the typed AST.  `YieldClause` is wrapped in
/// a `YIELD_SUBCLAUSE` node containing one or more `YIELD_ITEM` nodes
/// (each a `NameRef` with an optional `AS NameDef` alias).
fn call_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::CALL_KW));
    let m = p.start();
    p.bump(SyntaxKind::CALL_KW);

    // Block-subquery form `CALL { ... }` — deferred per §19 / §20 D1.
    // We surface this with the existing UNIMPLEMENTED_CLAUSE code (E0044)
    // pending a dedicated dialect gate; a follow-up bead may upgrade it.
    if p.at(SyntaxKind::L_BRACE) {
        p.error_code(
            sc::UNIMPLEMENTED_CLAUSE,
            "CALL { ... } block subqueries are deferred per spec §19 / §20 D1",
        );
        consume_balanced_braces(p);
        m.complete(p, SyntaxKind::ERROR);
        return;
    }

    // Procedure name: `IDENT ('.' IDENT)*`.
    if !(p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(
            sc::EXPECTED_PROCEDURE_NAME,
            "expected procedure name after CALL",
        );
        // Recover up to a YIELD or the next clause boundary.
        p.recover_until(TokenSet::new(&[SyntaxKind::YIELD_KW]));
    } else {
        let pn = p.start();
        p.bump_any();
        while p.at(SyntaxKind::DOT) {
            p.bump(SyntaxKind::DOT);
            if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
                p.error_code(
                    sc::EXPECTED_PROCEDURE_NAME,
                    "expected identifier after \'.\' in procedure name",
                );
                break;
            }
        }
        pn.complete(p, SyntaxKind::PROCEDURE_NAME);
    }

    // Optional argument list: `'(' ArgList? ')'`.
    if p.at(SyntaxKind::L_PAREN) {
        p.bump(SyntaxKind::L_PAREN);
        if !p.at(SyntaxKind::R_PAREN) {
            let args = p.start();
            if expression::expr(p).is_none() {
                p.error_code(sc::EXPECTED_CALL_ARG, "expected procedure argument");
            }
            while p.at(SyntaxKind::COMMA) {
                p.bump(SyntaxKind::COMMA);
                if expression::expr(p).is_none() {
                    p.error_code(sc::EXPECTED_CALL_ARG, "expected procedure argument");
                    break;
                }
            }
            args.complete(p, SyntaxKind::ARG_LIST);
        }
        if !p.eat(SyntaxKind::R_PAREN) {
            p.error_code(
                sc::EXPECTED_RPAREN_CALL_ARGS,
                "expected \')\' to close CALL argument list",
            );
        }
    }

    // Optional YIELD subclause.
    if p.at(SyntaxKind::YIELD_KW) {
        yield_subclause(p);
    }

    m.complete(p, SyntaxKind::CALL_CLAUSE);
}

/// `YieldClause = 'YIELD' YieldItem (',' YieldItem)*` — standalone-form
/// scope for cy-4mg.  `YIELD *` and trailing `WHERE` are deferred — the
/// non-subquery `CALL` form rarely uses them in TCK clauses/call
/// scenarios; explicit yield-item lists are the dominant shape.
fn yield_subclause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::YIELD_KW));
    let m = p.start();
    p.bump(SyntaxKind::YIELD_KW);
    yield_item(p);
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        yield_item(p);
    }
    m.complete(p, SyntaxKind::YIELD_SUBCLAUSE);
}

/// `YieldItem = NameRef ('AS' NameDef)?`.  Wraps the name in a `NAME`
/// node so HIR resolution can point at the binder span; the optional
/// alias is a trailing `NAME` after `AS`.
fn yield_item(p: &mut Parser<'_>) {
    let m = p.start();
    if !(p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_YIELD_ITEM, "expected identifier in YIELD item");
        m.complete(p, SyntaxKind::YIELD_ITEM);
        return;
    }
    name_binder(p);
    if p.eat(SyntaxKind::AS_KW) {
        if !(p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT)) {
            p.error_code(sc::EXPECTED_IDENT_AFTER_AS, "expected identifier after AS");
        } else {
            name_binder(p);
        }
    }
    m.complete(p, SyntaxKind::YIELD_ITEM);
}

/// Skip a brace-delimited block, tracking nesting so inner maps don\'t
/// prematurely close the skip. Used to recover from the deferred
/// `CALL { ... }` block-subquery form.
fn consume_balanced_braces(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    p.bump(SyntaxKind::L_BRACE);
    let mut depth: u32 = 1;
    while depth > 0 && !p.at(SyntaxKind::EOF) {
        match p.current() {
            SyntaxKind::L_BRACE => {
                depth += 1;
                p.bump(SyntaxKind::L_BRACE);
            }
            SyntaxKind::R_BRACE => {
                depth -= 1;
                p.bump(SyntaxKind::R_BRACE);
            }
            _ => p.bump_any(),
        }
    }
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

/// `UnwindClause = 'UNWIND' Expr 'AS' NameDef` — spec ungrammar `UnwindClause`.
///
/// UNWIND introduces a Value-kind binding that the downstream clauses treat
/// like a per-row variable; HIR lowering allocates the `VarId` on the
/// trailing identifier (spec §6.3).
fn unwind_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::UNWIND_KW));
    let m = p.start();
    p.bump(SyntaxKind::UNWIND_KW);
    if expression::expr(p).is_none() {
        p.error_code(sc::EXPECTED_UNWIND_EXPR, "expected expression after UNWIND");
    }
    if !p.eat(SyntaxKind::AS_KW) {
        p.error_code(
            sc::EXPECTED_AS_UNWIND,
            "expected AS after UNWIND expression",
        );
    }
    name_binder(p);
    m.complete(p, SyntaxKind::UNWIND_CLAUSE);
}

/// A plain `IDENT` / `QUOTED_IDENT` wrapped in a `NAME` node — used for
/// bind positions (UNWIND target, etc.) that mirror `NodePattern`'s binder.
fn name_binder(p: &mut Parser<'_>) {
    let m = p.start();
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_IDENT, "expected identifier");
    }
    m.complete(p, SyntaxKind::NAME);
}

/// `CreateClause = 'CREATE' Pattern (',' Pattern)*` — spec ungrammar
/// `CreateClause`. Write clause; HIR materialises a `Clause::Create`.
fn create_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::CREATE_KW));
    let m = p.start();
    p.bump(SyntaxKind::CREATE_KW);
    if p.at(SyntaxKind::L_PAREN) {
        pattern::pattern_list(p);
    } else {
        p.error_code(sc::EXPECTED_CREATE_PATTERN, "expected pattern after CREATE");
    }
    m.complete(p, SyntaxKind::CREATE_CLAUSE);
}

/// `MergeClause = 'MERGE' Pattern MergeAction*` — spec ungrammar
/// `MergeClause`.
fn merge_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::MERGE_KW));
    let m = p.start();
    p.bump(SyntaxKind::MERGE_KW);
    if p.at(SyntaxKind::L_PAREN) {
        pattern::pattern(p);
    } else {
        p.error_code(sc::EXPECTED_MERGE_PATTERN, "expected pattern after MERGE");
    }
    while p.at(SyntaxKind::ON_KW) {
        merge_action(p);
    }
    m.complete(p, SyntaxKind::MERGE_CLAUSE);
}

/// `MergeAction = 'ON' ('CREATE' | 'MATCH') SetClause`.
fn merge_action(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::ON_KW));
    let m = p.start();
    p.bump(SyntaxKind::ON_KW);
    if !(p.eat(SyntaxKind::CREATE_KW) || p.eat(SyntaxKind::MATCH_KW)) {
        p.error_code(
            sc::EXPECTED_ON_ACTION,
            "expected CREATE or MATCH after ON in MERGE action",
        );
    }
    if p.at(SyntaxKind::SET_KW) {
        set_clause(p);
    }
    m.complete(p, SyntaxKind::MERGE_ACTION);
}

/// `SetClause = 'SET' SetItem (',' SetItem)*` — spec ungrammar `SetClause`.
fn set_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::SET_KW));
    let m = p.start();
    p.bump(SyntaxKind::SET_KW);
    set_item(p);
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        set_item(p);
    }
    m.complete(p, SyntaxKind::SET_CLAUSE);
}

/// `SetItem` covers four shapes per the ungrammar: property assign,
/// label add, whole-node replace, whole-node merge. We disambiguate on
/// the token that follows the initial target: `:` (label add) → consume
/// the label expression; `=` (property-or-whole-replace) → consume one
/// trailing expression.
///
/// We parse the target with the dedicated [`set_target`] helper — a
/// postfix-only chain over `IDENT (. IDENT | [Expr])*` — so the
/// top-level `=` token is not swallowed by the expression parser's
/// comparison branch (priority 5, `n.x = true` would otherwise parse
/// as a single `BINARY_EXPR`).
fn set_item(p: &mut Parser<'_>) {
    let m = p.start();
    if !set_target(p) {
        p.error_code(sc::EXPECTED_SET_ITEM, "expected SET item");
        m.complete(p, SyntaxKind::SET_ITEM);
        return;
    }
    if p.at(SyntaxKind::COLON) {
        label_expr_inline(p);
    } else if p.eat(SyntaxKind::EQ) {
        if expression::expr(p).is_none() {
            p.error_code(sc::EXPECTED_PROP_VALUE, "expected expression for SET value");
        }
    } else {
        p.error_code(
            sc::EXPECTED_SET_ITEM,
            "expected ':', '=', or '+=' in SET item",
        );
    }
    m.complete(p, SyntaxKind::SET_ITEM);
}

/// Parse a `SET` / `REMOVE` target: `ident ('.' ident | '[' expr ']')*`.
/// Yields either a `VAR_EXPR` (`a`) or a `PROP_ACCESS_EXPR` / `SUBSCRIPT_EXPR`
/// chain. Returns `false` if no identifier was at the cursor.
fn set_target(p: &mut Parser<'_>) -> bool {
    if !(p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT)) {
        return false;
    }
    let mut head = {
        let m = p.start();
        p.bump_any();
        m.complete(p, SyntaxKind::VAR_EXPR)
    };
    loop {
        match p.current() {
            SyntaxKind::DOT => {
                let m = head.precede(p);
                p.bump(SyntaxKind::DOT);
                if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
                    p.error_code(
                        sc::EXPECTED_PROP_KEY_AFTER_DOT,
                        "expected property key after '.'",
                    );
                }
                head = m.complete(p, SyntaxKind::PROP_ACCESS_EXPR);
            }
            SyntaxKind::L_BRACK => {
                let m = head.precede(p);
                p.bump(SyntaxKind::L_BRACK);
                if expression::expr(p).is_none() {
                    p.error_code(sc::EXPECTED_INDEX_EXPR, "expected index expression");
                }
                if !p.eat(SyntaxKind::R_BRACK) {
                    p.error_code(
                        sc::EXPECTED_RBRACK_INDEX,
                        "expected ']' to close index expression",
                    );
                }
                head = m.complete(p, SyntaxKind::SUBSCRIPT_EXPR);
            }
            _ => break,
        }
    }
    let _ = head;
    true
}

/// `RemoveClause = 'REMOVE' RemoveItem (',' RemoveItem)*`.
fn remove_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::REMOVE_KW));
    let m = p.start();
    p.bump(SyntaxKind::REMOVE_KW);
    remove_item(p);
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        remove_item(p);
    }
    m.complete(p, SyntaxKind::REMOVE_CLAUSE);
}

fn remove_item(p: &mut Parser<'_>) {
    let m = p.start();
    if !set_target(p) {
        p.error_code(sc::EXPECTED_REMOVE_ITEM, "expected REMOVE item");
        m.complete(p, SyntaxKind::REMOVE_ITEM);
        return;
    }
    if p.at(SyntaxKind::COLON) {
        label_expr_inline(p);
    }
    m.complete(p, SyntaxKind::REMOVE_ITEM);
}

/// `(':' IDENT)+` — label expression consumed inline from a SET or REMOVE
/// item. Duplicates the pattern-side helper but lives here so the clause
/// module stays self-contained; identical semantics.
fn label_expr_inline(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::COLON));
    let m = p.start();
    while p.at(SyntaxKind::COLON) {
        p.bump(SyntaxKind::COLON);
        if p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT) {
            continue;
        }
        if p.current().is_keyword() {
            p.bump_any();
            continue;
        }
        p.error_code(sc::EXPECTED_LABEL, "expected label after ':'");
        break;
    }
    m.complete(p, SyntaxKind::LABEL_EXPR);
}

/// `DeleteClause = 'DETACH'? 'DELETE' Expr (',' Expr)*`.
fn delete_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::DETACH_KW) || p.at(SyntaxKind::DELETE_KW));
    let m = p.start();
    if p.eat(SyntaxKind::DETACH_KW) && !p.eat(SyntaxKind::DELETE_KW) {
        p.error_code(
            sc::EXPECTED_DELETE_AFTER_DETACH,
            "expected DELETE after DETACH",
        );
    } else if p.at(SyntaxKind::DELETE_KW) {
        p.bump(SyntaxKind::DELETE_KW);
    }
    if expression::expr(p).is_none() {
        p.error_code(sc::EXPECTED_DELETE_EXPR, "expected expression after DELETE");
    }
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        if expression::expr(p).is_none() {
            p.error_code(sc::EXPECTED_DELETE_EXPR, "expected expression after DELETE");
            break;
        }
    }
    m.complete(p, SyntaxKind::DELETE_CLAUSE);
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
