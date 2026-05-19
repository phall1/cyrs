//! Clause productions for v1 cy-nom scope: `MATCH` (with `OPTIONAL`),
//! `WHERE`, `RETURN` (with `DISTINCT`, `ORDER BY`, `SKIP`, `LIMIT`).
//!
//! Spec references: §4.3 (recovery), §4.6 (statement boundaries), and
//! `cypher.ungrammar` `MatchClause` / `WhereClause` / `ReturnClause`.
//! Every other clause in the ungrammar is deferred per cy-nom scope and
//! tagged with `cy-nom: v1 scope` at its stub site in the grammar module.

use crate::SyntaxKind;
use crate::parser::{Parser, syntax_codes as sc};

use super::{expression, pattern};

/// Dispatch on the current token to the appropriate clause production.
/// Caller guarantees `p.at_ts(CLAUSE_START)`.
pub(crate) fn clause(p: &mut Parser<'_>) {
    match p.current() {
        SyntaxKind::MATCH_KW => match_clause(p),
        SyntaxKind::OPTIONAL_KW => optional_clause(p),
        SyntaxKind::WHERE_KW => where_clause(p),
        SyntaxKind::RETURN_KW => return_clause(p),
        SyntaxKind::WITH_KW => with_clause(p),
        SyntaxKind::UNWIND_KW => unwind_clause(p),
        SyntaxKind::CREATE_KW => create_clause(p),
        SyntaxKind::INSERT_KW => insert_clause(p),
        SyntaxKind::MERGE_KW => merge_clause(p),
        SyntaxKind::SET_KW => set_clause(p),
        SyntaxKind::REMOVE_KW => remove_clause(p),
        SyntaxKind::DELETE_KW | SyntaxKind::DETACH_KW => delete_clause(p),
        SyntaxKind::CALL_KW => call_clause(p),
        other => unreachable!("clause dispatch on non-clause-start token: {other:?}"),
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

/// `OPTIONAL MATCH ...` or `OPTIONAL CALL ...` — covers both GQL/Cypher
/// shapes that begin with the `OPTIONAL` qualifier:
///
/// - `OPTIONAL MATCH Pattern (...)` — spec §19, cypher.ungrammar
///   `'OPTIONAL'? 'MATCH' ...`. Emitted as `OPTIONAL_MATCH_CLAUSE`.
/// - `OPTIONAL CALL Procedure (...) [YIELD ...]` — ISO/IEC 39075:2024
///   §14.11.3 (cy-tdl). Emitted as `OPTIONAL_CALL_CLAUSE`. Reuses the
///   exact body shape of `call_clause` so HIR lowering can share the
///   procedure-call path; the discriminant between the two is the wrap
///   kind plus the leading `OPTIONAL_KW` child token.
///
/// On any other token after `OPTIONAL`, emits `E0042` and falls through
/// to the MATCH path so downstream recovery is unaffected.
fn optional_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::OPTIONAL_KW));
    let m = p.start();
    p.bump(SyntaxKind::OPTIONAL_KW);
    if p.at(SyntaxKind::CALL_KW) {
        // OPTIONAL CALL — GQL-distinct. Body shape mirrors call_clause:
        // CALL kw + procedure name + optional arg list + optional YIELD.
        p.bump(SyntaxKind::CALL_KW);
        procedure_name(p);
        if p.at(SyntaxKind::L_PAREN) {
            call_arg_list(p);
        }
        if p.at(SyntaxKind::YIELD_KW) {
            yield_subclause(p);
        }
        m.complete(p, SyntaxKind::OPTIONAL_CALL_CLAUSE);
        return;
    }
    if !p.eat(SyntaxKind::MATCH_KW) {
        p.error_code(
            sc::EXPECTED_MATCH_OR_CALL_AFTER_OPTIONAL,
            "expected MATCH or CALL after OPTIONAL",
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

/// `InsertClause = 'INSERT' ('NODE' | 'EDGE')? Pattern (',' Pattern)*`
/// — GQL-distinct write clause (cy-8z3; ISO/IEC 39075:2024 §13.4).
///
/// GQL spells data-creation as `INSERT NODE (n:Person {…})` /
/// `INSERT EDGE (a)-[:KNOWS]->(b)`; openCypher spells the equivalent
/// `CREATE`. Parser-side we accept both qualifier spellings and let the
/// pattern's own shape (node vs relationship) carry the semantic
/// distinction — the qualifier is documentation, not a discriminator.
///
/// The qualifier (`NODE` / `EDGE`) is recognised contextually (it stays
/// as an `IDENT` in the lexer so that openCypher's `:NODE` / `:EDGE`
/// label spellings continue to lex as patterns). Both qualifiers are
/// optional: bare `INSERT (n:Person)` parses the same way, because the
/// pattern alone determines node-vs-edge creation.
///
/// Dialect gating is enforced in cyrs-sema (an OpenCypherV9-mode
/// `INSERT_CLAUSE` emits an E4xxx diagnostic; v1 scope leaves that
/// to a follow-up bead).
fn insert_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::INSERT_KW));
    let m = p.start();
    p.bump(SyntaxKind::INSERT_KW);
    // Optional `NODE` / `EDGE` qualifier — contextual IDENT, not a
    // reserved keyword.
    if p.at_contextual("NODE") || p.at_contextual("EDGE") {
        p.bump_any();
    }
    if p.at(SyntaxKind::L_PAREN) {
        pattern::pattern_list(p);
    } else {
        p.error_code(sc::EXPECTED_INSERT_PATTERN, "expected pattern after INSERT");
    }
    m.complete(p, SyntaxKind::INSERT_CLAUSE);
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

/// `ReturnClause = 'RETURN' ('DISTINCT' | 'ALL')? ReturnItem (',' ReturnItem)*
///                 ReturnExclude? OrderBy? Skip? Limit?`
///
/// Spec defers the full `RETURN *` mixed-list form to cy-nom-follow-ups;
/// we accept `RETURN *` as a standalone star and otherwise require the
/// comma-separated `ReturnItem` list from the ungrammar.
///
/// GQL adds two surface variations on top of the openCypher form
/// (ISO/IEC 39075:2024 §14.13, cy-auh):
///
/// * `RETURN ALL ...` — explicit multiset semantics (the default).
///   Accepted as a leading modifier in the same slot as `DISTINCT`;
///   the two are mutually exclusive. `ALL_KW` is also the head of the
///   list-predicate `ALL(x IN xs WHERE p)` in expression position, but
///   that form never appears bare immediately after `RETURN` (it would
///   need a following `(`), so the ambiguity is resolved by context —
///   the modifier is only eaten here at the clause level.
/// * `RETURN <items> EXCLUDE <id> (, <id>)*` — projection minus a list
///   of named fields (§14.13.4). Parsed as a `RETURN_EXCLUDE` node
///   nested inside the `RETURN_CLAUSE`, after the items and before
///   the `ORDER BY` / `SKIP` / `LIMIT` trailers.
fn return_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::RETURN_KW));
    let m = p.start();
    p.bump(SyntaxKind::RETURN_KW);
    // `DISTINCT` and `ALL` occupy the same modifier slot (§14.13.2) and
    // are mutually exclusive. `ALL` is also the head of a list predicate
    // (`ALL(x IN xs WHERE p)`), so it is only consumed as a modifier
    // here when *not* followed by `(` — otherwise it belongs to the
    // first `RETURN_ITEM`'s expression.
    if !p.eat(SyntaxKind::DISTINCT_KW)
        && p.at(SyntaxKind::ALL_KW)
        && p.nth(1) != SyntaxKind::L_PAREN
    {
        p.bump(SyntaxKind::ALL_KW);
    }

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

    if p.at(SyntaxKind::EXCLUDE_KW) {
        return_exclude(p);
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
    m.complete(p, SyntaxKind::RETURN_CLAUSE);
}

/// `ReturnExclude = 'EXCLUDE' NameRef (',' NameRef)*` — GQL-distinct
/// field-exclusion trailer on a `RETURN` clause (cy-auh; ISO/IEC
/// 39075:2024 §14.13.4). Each excluded field name is wrapped in a
/// `NAME` node, mirroring the existing `name_binder` helper, so the
/// HIR can address the list as a typed accessor once lowering lands.
fn return_exclude(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::EXCLUDE_KW));
    let m = p.start();
    p.bump(SyntaxKind::EXCLUDE_KW);
    if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
        name_binder(p);
    } else {
        p.error_code(
            sc::EXPECTED_EXCLUDE_ITEM,
            "expected identifier after EXCLUDE",
        );
    }
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
            name_binder(p);
        } else {
            p.error_code(
                sc::EXPECTED_EXCLUDE_ITEM,
                "expected identifier in EXCLUDE list",
            );
            break;
        }
    }
    m.complete(p, SyntaxKind::RETURN_EXCLUDE);
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

/// `CallClause = 'CALL' ProcedureName ArgList? YieldSubclause?` (cy-4mg)
///
/// Spec §14 / §19 row "CALL <proc> YIELD ...". Parses the standalone
/// form only — the block form `CALL { <subquery> }` is a separate
/// follow-up bead.
fn call_clause(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::CALL_KW));
    let m = p.start();
    p.bump(SyntaxKind::CALL_KW);
    procedure_name(p);
    if p.at(SyntaxKind::L_PAREN) {
        call_arg_list(p);
    }
    if p.at(SyntaxKind::YIELD_KW) {
        yield_subclause(p);
    }
    m.complete(p, SyntaxKind::CALL_CLAUSE);
}

/// `IDENT ('.' IDENT)*` wrapped in a single `PROCEDURE_NAME` node.
fn procedure_name(p: &mut Parser<'_>) {
    let m = p.start();
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(
            sc::EXPECTED_PROCEDURE_NAME,
            "expected procedure name after CALL",
        );
        m.complete(p, SyntaxKind::PROCEDURE_NAME);
        return;
    }
    while p.at(SyntaxKind::DOT) {
        p.bump(SyntaxKind::DOT);
        if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
            p.error_code(
                sc::EXPECTED_PROCEDURE_NAME,
                "expected identifier after `.` in procedure name",
            );
            break;
        }
    }
    m.complete(p, SyntaxKind::PROCEDURE_NAME);
}

/// `'(' Expr (',' Expr)* ')'?` — argument list including the empty
/// `()` form. Reuses the expression parser for each argument.
fn call_arg_list(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_PAREN));
    p.bump(SyntaxKind::L_PAREN);
    if !p.at(SyntaxKind::R_PAREN) && expression::expr(p).is_some() {
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            if expression::expr(p).is_none() {
                break;
            }
        }
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_CALL_ARGS,
            "expected `)` to close CALL argument list",
        );
    }
}

/// `'YIELD' YieldItem (',' YieldItem)*`
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

/// `YieldItem = IDENT ('AS' IDENT)?`
fn yield_item(p: &mut Parser<'_>) {
    let m = p.start();
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_YIELD_ITEM, "expected identifier in YIELD item");
        m.complete(p, SyntaxKind::YIELD_ITEM);
        return;
    }
    if p.eat(SyntaxKind::AS_KW) && !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_IDENT_AFTER_AS, "expected identifier after AS");
    }
    m.complete(p, SyntaxKind::YIELD_ITEM);
}
