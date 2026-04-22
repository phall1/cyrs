//! Expression parser — Pratt operator precedence over the openCypher /
//! GQL operator table. Spec §4.2 ("hand-written event-based recursive-
//! descent with Pratt precedence for expressions").
//!
//! # Precedence table (loose to tight)
//!
//! Numeric priority is the binding power of the *left* operand; an
//! operator binds the right operand at `priority + 1` for left
//! associativity, `priority` for right associativity. Match openCypher /
//! GQL canonical ordering:
//!
//! | Priority | Operators                                                  | Assoc |
//! | -------: | ---------------------------------------------------------- | ----- |
//! |        1 | `OR`                                                       | left  |
//! |        2 | `XOR`                                                      | left  |
//! |        3 | `AND`                                                      | left  |
//! |        4 | unary `NOT`                                                | prefix|
//! |        5 | `=` `<>` `!=` `<` `<=` `>` `>=` `IS [NOT] NULL` `STARTS WITH` `ENDS WITH` `CONTAINS` `=~` `IN` | non-assoc |
//! |        6 | `+` `-`                                                    | left  |
//! |        7 | `*` `/` `%`                                                | left  |
//! |        8 | `^`                                                        | right |
//! |        9 | unary `-` / unary `+`                                      | prefix|
//! |       10 | postfix: `.`, `[]`, `()`                                   | left  |
//! |     atom | identifier / literal / parenthesised / parameter           | —     |
//!
//! # cy-nom scope
//!
//! Implemented: every operator in the table above. Atoms cover
//! identifier, int/float/string/bool/null literal, parameter, and
//! `(Expr)`. Postfix covers property access, function call, and index.
//!
//! Deferred (each tagged with `cy-nom: v1 scope` at its stub):
//! list literals `[...]`, map literals `{...}`, list/pattern
//! comprehensions, `CASE` expressions, pattern predicates in expressions,
//! `EXISTS(...)`, `COUNT(*)` standalone form.

use crate::SyntaxKind;
use crate::parser::{CompletedMarker, Marker, Parser, TokenSet, syntax_codes as sc};

use super::pattern;

/// Parse an expression and return a handle to the completed root node.
/// Returns `None` if the current token starts nothing expression-like —
/// the caller should emit its own "expected expression" diagnostic.
pub(crate) fn expr(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    expr_bp(p, 0)
}

/// Recursion-safety cap. Pathological inputs like nested parens cannot
/// exceed this depth. Protects fuzz against stack overflow.
const MAX_EXPR_DEPTH: u32 = 256;

/// Pratt loop. `min_bp` is the minimum binding power a binary operator
/// must exceed to continue the expression on the right. Unary operators
/// and atoms are parsed in `lhs`.
fn expr_bp(p: &mut Parser<'_>, min_bp: u8) -> Option<CompletedMarker> {
    expr_bp_depth(p, min_bp, 0)
}

fn expr_bp_depth(p: &mut Parser<'_>, min_bp: u8, depth: u32) -> Option<CompletedMarker> {
    if depth > MAX_EXPR_DEPTH {
        p.error_code(
            sc::EXPR_NESTING_LIMIT,
            "expression nesting exceeds parser limit",
        );
        return None;
    }

    // --- Prefix / unary --------------------------------------------------
    let mut lhs = if let Some(prefix_bp) = prefix_bp(p.current()) {
        let m = p.start();
        let op_kind = p.current();
        p.bump_any();
        // Right binding power drives recursion. Unary is right-associative.
        if expr_bp_depth(p, prefix_bp, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_UNARY_OPERAND,
                format!("expected operand after unary {op_kind:?}"),
            );
        }
        m.complete(p, SyntaxKind::UNARY_EXPR)
    } else {
        atom(p, depth)?
    };

    // --- Postfix + infix loop -------------------------------------------
    loop {
        // Postfix operators always bind tightest.
        if let Some(postfix) = postfix_op(p) {
            if postfix.bp < min_bp {
                break;
            }
            lhs = apply_postfix(p, lhs, postfix, depth);
            continue;
        }

        // `IS [NOT] NULL` — postfix, priority 5 (comparison-level).
        if p.at(SyntaxKind::IS_KW) {
            let null_check_bp = 10;
            if null_check_bp < min_bp {
                break;
            }
            let m = lhs.precede(p);
            p.bump(SyntaxKind::IS_KW);
            p.eat(SyntaxKind::NOT_KW);
            if !p.eat(SyntaxKind::NULL_KW) {
                p.error_code(sc::EXPECTED_NULL_AFTER_IS, "expected NULL after IS");
            }
            lhs = m.complete(p, SyntaxKind::IS_NULL_EXPR);
            continue;
        }

        // Infix binary operators.
        if let Some(op) = infix_op(p) {
            if op.left_bp < min_bp {
                break;
            }
            let m = lhs.precede(p);
            // Consume the operator token(s).
            consume_infix_op(p, op.kind);
            // Right-hand side parses at right_bp.
            if expr_bp_depth(p, op.right_bp, depth + 1).is_none() {
                p.error_code(
                    sc::EXPECTED_BINOP_RHS,
                    "expected right-hand side of binary expression",
                );
            }
            lhs = m.complete(p, op.node);
            continue;
        }

        break;
    }

    Some(lhs)
}

// --------------------------------------------------------------------------
// Atoms
// --------------------------------------------------------------------------

fn atom(p: &mut Parser<'_>, depth: u32) -> Option<CompletedMarker> {
    let kind = p.current();
    Some(match kind {
        SyntaxKind::INT_LITERAL | SyntaxKind::FLOAT_LITERAL | SyntaxKind::STRING_LITERAL => {
            literal_atom(p, SyntaxKind::LITERAL_EXPR)
        }
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => {
            literal_keyword_atom(p, SyntaxKind::LITERAL_EXPR)
        }
        SyntaxKind::NULL_KW => literal_keyword_atom(p, SyntaxKind::LITERAL_EXPR),
        SyntaxKind::PARAM => {
            let m = p.start();
            p.bump(SyntaxKind::PARAM);
            m.complete(p, SyntaxKind::PARAM_EXPR)
        }
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
            // Variable reference. A following `(` is handled as a postfix
            // function-call in the infix/postfix loop.
            let m = p.start();
            p.bump_any();
            m.complete(p, SyntaxKind::VAR_EXPR)
        }
        // `EXISTS` has three surface forms (cy-lve, spec §6.1 / §19):
        //
        //   1. `EXISTS ( <pattern> )` — pattern predicate. Accepted; lowered
        //      to `PATTERN_PREDICATE` so HIR / sema see the same shape as
        //      a bare `(a)-->(b)` in WHERE position.
        //   2. `EXISTS ( <expr> )` — `exists(expr)` function call (e.g.
        //      `exists(n.prop)`). Falls through to the `VAR_EXPR` + postfix
        //      function-call path.
        //   3. `EXISTS { … }` — block-subquery form. Deferred per spec §19 /
        //      §20 D1; emit E4017 and swallow the braced body for recovery.
        //
        // Disambiguation between (1) and (2): openCypher patterns always
        // begin with an `(` (the first node pattern). A `(` immediately
        // after `EXISTS (` therefore indicates form (1); anything else
        // falls through to form (2). This is the same shape heuristic
        // tree-sitter-cypher uses for `exists_expression` vs.
        // `exists_function_invocation` (spec §19 "Pattern predicates in
        // expressions" — the ambiguity is resolved by the nested `(`).
        SyntaxKind::EXISTS_KW => {
            if p.nth(1) == SyntaxKind::L_BRACE {
                // Form (3): block subquery. Not in v1. Emit E4017 and
                // recover by consuming the balanced braces.
                exists_block_deferred(p)
            } else if p.nth(1) == SyntaxKind::L_PAREN && p.nth(2) == SyntaxKind::L_PAREN {
                // Form (1): pattern predicate.
                exists_pattern_predicate(p)
            } else {
                // Form (2): fall through to function-call shape.
                let m = p.start();
                p.bump_any();
                m.complete(p, SyntaxKind::VAR_EXPR)
            }
        }
        // `COUNT` lexes as a dedicated keyword token (lexer §4.1) but in
        // expression position it stands in for the aggregate function
        // identifier — `count(n)` / `count(*)`. Accept the keyword as a
        // VAR_EXPR so the postfix `(` loop can wrap it in a FUNCTION_CALL.
        SyntaxKind::COUNT_KW => {
            let m = p.start();
            p.bump_any();
            m.complete(p, SyntaxKind::VAR_EXPR)
        }
        // List predicates (cy-8x5). `ANY / ALL / NONE / SINGLE (x IN xs
        // [WHERE p])`. The discriminant keyword token stays as the first
        // child of the emitted `LIST_PREDICATE_EXPR` so downstream passes
        // (HIR lowering, pretty-printers) can classify without a per-kind
        // SyntaxKind. `ALL_KW` also appears in `UNION ALL`; there the
        // `(` lookahead is absent so the expression path does not fire.
        SyntaxKind::ANY_KW | SyntaxKind::ALL_KW | SyntaxKind::NONE_KW | SyntaxKind::SINGLE_KW
            if p.nth(1) == SyntaxKind::L_PAREN =>
        {
            list_predicate(p, depth)
        }
        // `(` in expression position is ambiguous between a parenthesised
        // expression (`(1 + 2)`, `(a.name)`, …) and a bare pattern predicate
        // (`(a)-->(b)`, `(:Label)-[:R]->()`, …) — spec §6.1 desugaring row
        // "Pattern predicates in expressions" / §19. Dispatch on a two-token
        // lookahead past the opening paren per cy-7lf; see
        // [`at_bare_pattern_predicate`] for the token table.
        SyntaxKind::L_PAREN => {
            if at_bare_pattern_predicate(p) {
                bare_pattern_predicate(p)
            } else {
                paren_expr(p, depth)
            }
        }
        SyntaxKind::L_BRACK => list_literal(p, depth),
        SyntaxKind::L_BRACE => map_literal(p, depth),
        // `CASE` expression — generic + simple-when forms (cy-41u,
        // spec §19 row "CASE").
        SyntaxKind::CASE_KW => case_expr(p, depth),
        // cy-nom: v1 scope — pattern predicates, EXISTS(...) land in
        // follow-up beads.
        _ => return None,
    })
}

/// `ListLiteral = '[' (Expr (',' Expr)*)? ']'` — spec `cypher.ungrammar`
/// `ListLiteral`. List comprehensions (`[x IN xs WHERE p | f(x)]`) share
/// the opening `[` with literals and are disambiguated here: if the first
/// element is an IDENT followed by `IN`, this parses as a
/// [`SyntaxKind::LIST_COMPREHENSION`]; otherwise it is a
/// [`SyntaxKind::LIST_LITERAL`].
fn list_literal(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_BRACK));

    // Comprehension lookahead: `[ IDENT IN ...`.
    // The first significant token past `[` is at nth(1); the IN check sits
    // at nth(2). We only dispatch to the comprehension parse path on an
    // exact match to avoid regressing the list-literal grammar (cy-7s6.1).
    if matches!(p.nth(1), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
        && p.nth(2) == SyntaxKind::IN_KW
    {
        return list_comprehension(p, depth);
    }

    let m = p.start();
    p.bump(SyntaxKind::L_BRACK);
    if !p.at(SyntaxKind::R_BRACK) {
        if expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_LIST_ELEM,
                "expected expression in list literal",
            );
        }
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            if expr_bp_depth(p, 0, depth + 1).is_none() {
                p.error_code(
                    sc::EXPECTED_LIST_ELEM,
                    "expected expression in list literal",
                );
                break;
            }
        }
    }
    if !p.eat(SyntaxKind::R_BRACK) {
        p.error_code(
            sc::EXPECTED_RBRACK_LIST,
            "expected ']' to close list literal",
        );
    }
    m.complete(p, SyntaxKind::LIST_LITERAL)
}

/// Parse a list comprehension (spec §19 row "List comprehensions",
/// ungrammar `ListComprehension`):
///
/// ```text
/// ListComprehension = '[' NameDef 'IN' Expr ( 'WHERE' Expr )? ( '|' Expr )? ']'
/// ```
///
/// Every production is optional except the iteration variable binder and
/// the source expression (`NameDef 'IN' Expr`). The four legal shapes:
///
/// - `[x IN xs]`                              — no predicate, no map
/// - `[x IN xs WHERE p(x)]`                   — filter only (implicit identity map)
/// - `[x IN xs | f(x)]`                       — map only
/// - `[x IN xs WHERE p(x) | f(x)]`            — filter and map
///
/// Enters on the opening `[`; `nth(1)` / `nth(2)` lookahead already
/// confirmed the `IDENT IN` prefix at the [`list_literal`] dispatch point.
fn list_comprehension(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_BRACK));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACK);

    // NameDef — wrap the identifier in a NAME node so lowering can find it.
    {
        let name = p.start();
        if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
            // The dispatch lookahead guarantees we are at an IDENT here.
            // Defensive fallback: emit and close an empty NAME so the tree
            // still round-trips.
            p.error_code(
                sc::EXPECTED_IDENT,
                "expected iteration variable in list comprehension",
            );
        }
        name.complete(p, SyntaxKind::NAME);
    }

    // 'IN' — required.
    if !p.eat(SyntaxKind::IN_KW) {
        p.error_code(
            sc::EXPECTED_IN_LIST_COMP,
            "expected `IN` in list comprehension",
        );
    }

    // Source expression (iterable) — required.
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(
            sc::EXPECTED_LIST_ELEM,
            "expected expression for list-comprehension source",
        );
    }

    // Optional WHERE predicate.
    if p.at(SyntaxKind::WHERE_KW) {
        p.bump(SyntaxKind::WHERE_KW);
        if expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_WHERE_EXPR,
                "expected predicate expression after `WHERE` in list comprehension",
            );
        }
    }

    // Optional `|` projection. Matches openCypher §3.3 (list comprehension
    // production). After the optional WHERE predicate the grammar accepts
    // either `|` followed by the projection expression, or directly `]`.
    if p.at(SyntaxKind::PIPE) {
        p.bump(SyntaxKind::PIPE);
        if expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_BINOP_RHS,
                "expected projection expression after `|` in list comprehension",
            );
        }
    }

    if !p.eat(SyntaxKind::R_BRACK) {
        p.error_code(
            sc::EXPECTED_PIPE_OR_RBRACK_LIST_COMP,
            "expected `|` or `]` in list comprehension",
        );
    }

    m.complete(p, SyntaxKind::LIST_COMPREHENSION)
}

/// `MapLiteral = '{' (PropertyKV (',' PropertyKV)*)? '}'` — spec
/// `cypher.ungrammar` `MapLiteral`. Same shape as the property-map
/// shorthand inside patterns; when the `{` appears in *expression*
/// position the parser binds this production, when it appears inside a
/// `NodePattern` / `RelDetail` the caller's existing property-map
/// handling owns it (see `grammar::pattern::property_map`).
fn map_literal(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACE);
    if !p.at(SyntaxKind::R_BRACE) {
        map_entry(p, depth);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            if p.at(SyntaxKind::R_BRACE) {
                break;
            }
            map_entry(p, depth);
        }
    }
    if !p.eat(SyntaxKind::R_BRACE) {
        p.error_code(sc::EXPECTED_RBRACE_MAP, "expected '}' to close map literal");
    }
    m.complete(p, SyntaxKind::MAP_LITERAL)
}

fn map_entry(p: &mut Parser<'_>, depth: u32) {
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_MAP_KEY, "expected key in map literal");
    }
    if !p.eat(SyntaxKind::COLON) {
        p.error_code(sc::EXPECTED_COLON_MAP, "expected ':' in map entry");
    }
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(sc::EXPECTED_MAP_VALUE, "expected expression for map value");
    }
}

fn literal_atom(p: &mut Parser<'_>, node: SyntaxKind) -> CompletedMarker {
    let m = p.start();
    p.bump_any();
    m.complete(p, node)
}

/// TRUE/FALSE/NULL keywords are wrapped into a literal expression so the
/// AST sees them uniformly with the numeric/string literals above.
fn literal_keyword_atom(p: &mut Parser<'_>, node: SyntaxKind) -> CompletedMarker {
    let m = p.start();
    p.bump_any();
    m.complete(p, node)
}

/// Parse a list-predicate expression: `ANY|ALL|NONE|SINGLE(x IN xs [WHERE p])`.
///
/// Spec §19 row "List predicates". The returned `LIST_PREDICATE_EXPR`
/// preserves the discriminant keyword as its first child token so HIR
/// lowering can classify without a dedicated `SyntaxKind` per keyword.
/// Grammar identical shape for all four:
///
/// ```text
/// LIST_PREDICATE_EXPR
///   (ANY_KW | ALL_KW | NONE_KW | SINGLE_KW)
///   '('
///   NAME
///   IN_KW
///   <iterable Expr>
///   (WHERE_KW <predicate Expr>)?
///   ')'
/// ```
///
/// Recovery per AGENTS.md §10:
///   E0065 — missing `(` after the predicate keyword
///   E0066 — missing `IN`
///   E0067 — missing `)` to close the predicate
fn list_predicate(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(matches!(
        p.current(),
        SyntaxKind::ANY_KW | SyntaxKind::ALL_KW | SyntaxKind::NONE_KW | SyntaxKind::SINGLE_KW
    ));
    let m = p.start();
    // Discriminant keyword. Consumed as-is so it survives as the first
    // token child of the emitted node.
    p.bump_any();

    if !p.eat(SyntaxKind::L_PAREN) {
        p.error_code(
            sc::EXPECTED_LPAREN_LIST_PREDICATE,
            "expected `(` after ANY/ALL/NONE/SINGLE",
        );
    }

    // The binder name. Wrap it in a NAME node so AST / HIR can address it
    // uniformly (mirrors how `UNWIND ... AS v` and LIST_COMPREHENSION
    // emit a NAME child).
    let name_marker = p.start();
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(
            sc::EXPECTED_IDENT,
            "expected binder identifier in list predicate",
        );
    }
    name_marker.complete(p, SyntaxKind::NAME);

    if !p.eat(SyntaxKind::IN_KW) {
        p.error_code(sc::EXPECTED_IN_LIST_PREDICATE, "expected `IN`");
    }

    // Iterable expression.
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(
            sc::EXPECTED_BINOP_RHS,
            "expected iterable expression in list predicate",
        );
    }

    // Optional `WHERE <expr>` predicate. Per openCypher semantics the
    // WHERE clause is optional: bare `ANY(x IN xs)` is true iff xs is
    // non-empty, etc. — we accept the form and leave the filter absent.
    if p.eat(SyntaxKind::WHERE_KW) && expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(
            sc::EXPECTED_WHERE_EXPR,
            "expected predicate expression after WHERE in list predicate",
        );
    }

    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_LIST_PREDICATE,
            "expected `)` to close list predicate",
        );
    }

    m.complete(p, SyntaxKind::LIST_PREDICATE_EXPR)
}

/// Parse a `CASE` expression — generic or simple-when form (spec §19 row
/// "CASE"; cy-41u).
///
/// ```text
/// GenericCase = 'CASE' (WHEN Expr THEN Expr)+ ('ELSE' Expr)? 'END'
/// SimpleCase  = 'CASE' Expr (WHEN Expr THEN Expr)+ ('ELSE' Expr)? 'END'
/// ```
///
/// The two forms share an emitted shape: a `CASE_EXPR` node with an
/// optional scrutinee expression child (present iff a token other than
/// `WHEN` follows the leading `CASE`), one or more `CASE_WHEN_ARM`
/// children, and an optional trailing `CASE_ELSE_ARM`.
///
/// Recovery:
///   E0070 — missing `THEN` after `WHEN <value>`
///   E0071 — missing `END` at the close of the expression
fn case_expr(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::CASE_KW));
    let m = p.start();
    p.bump(SyntaxKind::CASE_KW);

    // Optional scrutinee — present iff the token following `CASE` is not
    // a `WHEN` / `ELSE` / `END`. `ELSE` / `END` here mean an empty CASE
    // (no arms), which is ill-formed but we accept for recovery — the
    // `WHEN` loop below will emit `E0007`-style missing-arm diagnostics
    // via the standard expr recovery path.
    if !matches!(
        p.current(),
        SyntaxKind::WHEN_KW | SyntaxKind::ELSE_KW | SyntaxKind::END_KW
    ) && expr_bp_depth(p, 0, depth + 1).is_none()
    {
        p.error_code(
            sc::EXPECTED_BINOP_RHS,
            "expected expression after `CASE` (simple-when scrutinee)",
        );
    }

    // One or more WHEN arms.
    while p.at(SyntaxKind::WHEN_KW) {
        let arm = p.start();
        p.bump(SyntaxKind::WHEN_KW);
        // `WHEN <value / predicate>` — required expression.
        if expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_BINOP_RHS,
                "expected expression after `WHEN` in CASE arm",
            );
        }
        if !p.eat(SyntaxKind::THEN_KW) {
            p.error_code(sc::EXPECTED_THEN_CASE, "expected `THEN` in CASE arm");
        }
        // `THEN <result>` — required expression.
        if expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_BINOP_RHS,
                "expected expression after `THEN` in CASE arm",
            );
        }
        arm.complete(p, SyntaxKind::CASE_WHEN_ARM);
    }

    // Optional ELSE arm.
    if p.at(SyntaxKind::ELSE_KW) {
        let else_arm = p.start();
        p.bump(SyntaxKind::ELSE_KW);
        if expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(
                sc::EXPECTED_BINOP_RHS,
                "expected expression after `ELSE` in CASE",
            );
        }
        else_arm.complete(p, SyntaxKind::CASE_ELSE_ARM);
    }

    // Closing `END` — required. Virtual-token insertion on miss so the
    // CST round-trips and downstream passes see a well-formed node.
    if !p.eat(SyntaxKind::END_KW) {
        p.error_code(
            sc::EXPECTED_END_CASE,
            "expected `END` to close CASE expression",
        );
    }

    m.complete(p, SyntaxKind::CASE_EXPR)
}

/// Parse a pattern-predicate `EXISTS ( <pattern> )` — spec §6.1 (sugar
/// desugared in HIR) / §19 row "Pattern predicates in expressions".
///
/// Enters on the `EXISTS` keyword with the two-token lookahead
/// `EXISTS ( (` already confirmed by [`atom`]. Consumes the keyword, the
/// outer `(`, a path pattern, and the closing `)`. Returns a
/// [`SyntaxKind::PATTERN_PREDICATE`] node so HIR lowering reuses the same
/// `Expr::PatternPredicate` path as a bare `(a)-->(b)` would if it were
/// reachable from expression position.
///
/// Recovery: E0072 on a missing `)` after the pattern. The outer opening
/// `(` is guaranteed by the dispatch lookahead, so no miss diagnostic is
/// needed there.
///
/// # Ambiguity note (spec §19 "Pattern predicates in expressions")
///
/// `EXISTS ( <expr> )` remains a function-call form and is handled by the
/// fallthrough branch in [`atom`]. Disambiguation is the two-token
/// lookahead `EXISTS ( (`: a pattern always begins with a parenthesised
/// node pattern, and `exists(expr)` never starts with `(`. This matches
/// the tree-sitter grammar's `exists_expression` vs.
/// `exists_function_invocation` split.
fn exists_pattern_predicate(p: &mut Parser<'_>) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::EXISTS_KW));
    let m = p.start();
    p.bump(SyntaxKind::EXISTS_KW);
    // Outer `(` — guaranteed by the atom dispatch lookahead.
    p.bump(SyntaxKind::L_PAREN);
    // Reuse the canonical pattern parser so we pick up every pattern
    // shape the grammar accepts in MATCH position (labels, rels with
    // types / directions, chained path elements).
    pattern::pattern(p);
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_EXISTS,
            "expected ')' to close EXISTS pattern predicate",
        );
    }
    m.complete(p, SyntaxKind::PATTERN_PREDICATE)
}

/// Reject the block-subquery form `EXISTS { ... }` with the
/// `exists_subquery` dialect-gate code E4017 (spec §9.3 / §19 row
/// "`EXISTS { <subquery> }`" / §20 D1).
///
/// The form is not part of either v1 dialect and will not be in v1. We
/// still parse it permissively enough to recover: consume the keyword,
/// skip the balanced braces, and emit an ERROR-marker so the rest of the
/// statement parses.
fn exists_block_deferred(p: &mut Parser<'_>) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::EXISTS_KW));
    debug_assert!(p.nth(1) == SyntaxKind::L_BRACE);
    let m = p.start();
    p.error_code(
        sc::EXISTS_BLOCK_DEFERRED,
        "EXISTS { ... } block subqueries are deferred per spec §19 / §20 D1",
    );
    // Consume the keyword and the brace-delimited body without
    // interpreting its contents. Track brace depth so nested maps
    // inside the body don't prematurely end the skip.
    p.bump(SyntaxKind::EXISTS_KW);
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
    // Tag as ERROR so downstream passes (sema/plan) do not try to
    // interpret this as a valid expression. The primary diagnostic is
    // already on the CST.
    m.complete(p, SyntaxKind::ERROR)
}

/// Two-token lookahead disambiguator for an `L_PAREN` in expression
/// position: decides whether the `(` starts a bare pattern predicate
/// (`(a)-->(b)`, `(:Label)`, `(a {k: 1})`, …) or a parenthesised
/// expression (`(1 + 2)`, `(a.name)`, …). Spec §6.1 / §19 row
/// "Pattern predicates in expressions" (cy-7lf).
///
/// The parser is positioned *at* the opening paren; `nth(1)` is the
/// first token inside, `nth(2)` the second. Matches the classification
/// table below — every token combination maps to exactly one branch so
/// the caller never needs backtracking:
///
/// | `nth(1)` | `nth(2)` | Interpretation               |
/// | -------- | -------- | ---------------------------- |
/// | `)`      | —        | bare pattern (empty node)    |
/// | `:`      | —        | bare pattern (`(:Label)`)    |
/// | `{`      | —        | bare pattern (`({k: v})`)    |
/// | ident    | `:`      | bare pattern (`(a:Label)`)   |
/// | ident    | `,`      | bare pattern (comma in path) |
/// | ident    | `)`      | **ambiguous → pattern**      |
/// | ident    | `-`      | bare pattern (rel follows)   |
/// | ident    | `<-`     | bare pattern (rel follows)   |
/// | ident    | `{`      | bare pattern (inline props)  |
/// | anything else        | parenthesised expression     |
///
/// The `ident` + `)` ambiguity is resolved in favour of the pattern
/// form per the bead's spec; `(a)` read as a pattern predicate lowers to an
/// existential check on node `a`, while `(a)` read as an expression is
/// just `a` — they are not equivalent in type, but openCypher's bare
/// pattern form is the high-value reading (see TCK `expressions/pattern`
/// scenario [13] `MATCH (n) WHERE (n) RETURN n`). Users who meant the
/// expression form can disambiguate with `.prop`, an operator, or by
/// dropping the parens entirely.
fn at_bare_pattern_predicate(p: &mut Parser<'_>) -> bool {
    debug_assert!(p.at(SyntaxKind::L_PAREN));
    match p.nth(1) {
        // `()` — empty node pattern. Also `(:Label)`, `({k: v})` —
        // node pattern without a binder.
        SyntaxKind::R_PAREN | SyntaxKind::COLON | SyntaxKind::L_BRACE => true,
        // `(IDENT …)` — inspect the next token to decide.
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => matches!(
            p.nth(2),
            // Label decoration → pattern.
            SyntaxKind::COLON
            // Inline property map → pattern.
            | SyntaxKind::L_BRACE
            // Ambiguous `(a)` → pattern per cy-7lf disambiguation rule.
            | SyntaxKind::R_PAREN
            // A trailing relationship always means a pattern: `(a)-[]->(b)`
            // opens with `MINUS` or `ARROW_L` after the first node pattern
            // closes, so seeing one inside means we're still mid-binder.
            // The `MINUS` / `ARROW_L` tokens here apply to a following rel
            // pattern after the closing `)` — but if they appear in the
            // *next* slot they never belong to an expression `(a - b)`:
            // expressions need whitespace-tolerant `a - b` which is `IDENT
            // MINUS IDENT`; so an `IDENT MINUS IDENT` shape stays an expr.
            // We only dispatch to pattern when we see a `,` which only
            // appears in comma-separated pattern lists.
            | SyntaxKind::COMMA
        ),
        // Everything else (literal, param, keyword, operator…) is an
        // expression.
        _ => false,
    }
}

/// Parse a bare pattern predicate in expression position: `(a)-->(b)`,
/// `(:Label)`, … — spec §6.1 / §19 row "Pattern predicates in
/// expressions" (cy-7lf).
///
/// Enters at the opening `(` of the first node pattern. Delegates the
/// whole path to [`pattern::pattern`], which walks `NodePattern
/// (RelPattern NodePattern)*` and leaves the cursor past the final
/// closing paren. The result is wrapped in a [`SyntaxKind::PATTERN_PREDICATE`]
/// node so HIR lowering reuses the same `Expr::PatternPredicate` path
/// as the `EXISTS(<pattern>)` form (cy-lve).
fn bare_pattern_predicate(p: &mut Parser<'_>) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_PAREN));
    let m = p.start();
    pattern::pattern(p);
    m.complete(p, SyntaxKind::PATTERN_PREDICATE)
}

fn paren_expr(p: &mut Parser<'_>, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_PAREN));
    let m = p.start();
    p.bump(SyntaxKind::L_PAREN);
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(
            sc::EXPECTED_EXPR_IN_PARENS,
            "expected expression inside parentheses",
        );
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        // Virtual-token insertion per spec §4.3.
        p.error_code(sc::EXPECTED_RPAREN_EXPR, "expected ')' to close expression");
    }
    m.complete(p, SyntaxKind::PAREN_EXPR)
}

// --------------------------------------------------------------------------
// Prefix / unary binding
// --------------------------------------------------------------------------

fn prefix_bp(kind: SyntaxKind) -> Option<u8> {
    Some(match kind {
        // `NOT` at priority 4 — lower than comparison, higher than AND/XOR/OR.
        SyntaxKind::NOT_KW => 8,
        // Unary `-` / `+` at priority 9 — higher than multiplicative ops.
        SyntaxKind::MINUS | SyntaxKind::PLUS => 18,
        _ => return None,
    })
}

// --------------------------------------------------------------------------
// Infix binary operators
// --------------------------------------------------------------------------

/// Binding powers use doubled priorities (2 per table row) so left-assoc
/// can set `right_bp` = `left_bp` + 1 and right-assoc can set them equal —
/// the standard Pratt encoding.
#[derive(Copy, Clone, Debug)]
struct InfixOp {
    kind: InfixKind,
    left_bp: u8,
    right_bp: u8,
    /// `SyntaxKind` used for the resulting node.
    node: SyntaxKind,
}

#[derive(Copy, Clone, Debug)]
enum InfixKind {
    /// Single-token operator — just bump `tok`.
    Single(SyntaxKind),
    /// `STARTS WITH` / `ENDS WITH` — two keyword tokens.
    StartsWith,
    EndsWith,
}

fn infix_op(p: &mut Parser<'_>) -> Option<InfixOp> {
    let c = p.current();
    Some(match c {
        // Priority 1: OR
        SyntaxKind::OR_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 2,
            right_bp: 3,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 2: XOR
        SyntaxKind::XOR_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 4,
            right_bp: 5,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 3: AND
        SyntaxKind::AND_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 6,
            right_bp: 7,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 5: comparison family (non-assoc — but implemented as
        // left-assoc with a spec-aligned diagnostic later; harmless for
        // well-formed input).
        SyntaxKind::EQ
        | SyntaxKind::NEQ
        | SyntaxKind::BANG_EQ
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::BINARY_EXPR,
        },
        SyntaxKind::REGEX_EQ => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::REGEX_MATCH_EXPR,
        },
        SyntaxKind::IN_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::IN_EXPR,
        },
        SyntaxKind::STARTS_KW => InfixOp {
            kind: InfixKind::StartsWith,
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::STRING_OP_EXPR,
        },
        SyntaxKind::ENDS_KW => InfixOp {
            kind: InfixKind::EndsWith,
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::STRING_OP_EXPR,
        },
        SyntaxKind::CONTAINS_KW => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 10,
            right_bp: 11,
            node: SyntaxKind::STRING_OP_EXPR,
        },
        // Priority 6: additive
        SyntaxKind::PLUS | SyntaxKind::MINUS => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 12,
            right_bp: 13,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 7: multiplicative
        SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 14,
            right_bp: 15,
            node: SyntaxKind::BINARY_EXPR,
        },
        // Priority 8: power (right-assoc → right_bp == left_bp).
        SyntaxKind::CARET => InfixOp {
            kind: InfixKind::Single(c),
            left_bp: 16,
            right_bp: 16,
            node: SyntaxKind::BINARY_EXPR,
        },
        _ => return None,
    })
}

fn consume_infix_op(p: &mut Parser<'_>, kind: InfixKind) {
    match kind {
        InfixKind::Single(tok) => p.bump(tok),
        InfixKind::StartsWith => {
            p.bump(SyntaxKind::STARTS_KW);
            if !p.eat(SyntaxKind::WITH_KW) {
                p.error_code(sc::EXPECTED_WITH_AFTER_STARTS, "expected WITH after STARTS");
            }
        }
        InfixKind::EndsWith => {
            p.bump(SyntaxKind::ENDS_KW);
            if !p.eat(SyntaxKind::WITH_KW) {
                p.error_code(sc::EXPECTED_WITH_AFTER_ENDS, "expected WITH after ENDS");
            }
        }
    }
}

// --------------------------------------------------------------------------
// Postfix
// --------------------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
struct PostfixOp {
    bp: u8,
    kind: PostfixKind,
}

#[derive(Copy, Clone, Debug)]
enum PostfixKind {
    /// `.ident` — property access.
    Dot,
    /// `[expr]` or `[i..j]` — list indexing / slicing. The helper
    /// [`index_or_slice_postfix`] disambiguates after the opening `[`
    /// (cy-7s6.1).
    Index,
    /// `(arg, arg, ...)` — function call. Only allowed when the lhs is
    /// an IDENT — the Pratt loop checks this via `postfix_op`.
    Call,
    /// `IS NULL` / `IS NOT NULL`. `IS` is also handled as a binary op
    /// above because openCypher uses `IS NULL` with the lhs as operand —
    /// the infix path handles all well-formed cases.
    /// (Kept as a placeholder variant for future null-check recovery.)
    #[allow(dead_code)]
    IsNull,
}

fn postfix_op(p: &mut Parser<'_>) -> Option<PostfixOp> {
    let bp = 20; // higher than any infix left_bp.
    Some(match p.current() {
        SyntaxKind::DOT => PostfixOp {
            bp,
            kind: PostfixKind::Dot,
        },
        SyntaxKind::L_BRACK => PostfixOp {
            bp,
            kind: PostfixKind::Index,
        },
        SyntaxKind::L_PAREN => PostfixOp {
            bp,
            kind: PostfixKind::Call,
        },
        _ => return None,
    })
}

fn apply_postfix(
    p: &mut Parser<'_>,
    lhs: CompletedMarker,
    op: PostfixOp,
    depth: u32,
) -> CompletedMarker {
    match op.kind {
        PostfixKind::Dot => {
            let m = lhs.precede(p);
            p.bump(SyntaxKind::DOT);
            if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
                p.error_code(
                    sc::EXPECTED_PROP_KEY_AFTER_DOT,
                    "expected property key after '.'",
                );
            }
            m.complete(p, SyntaxKind::PROP_ACCESS_EXPR)
        }
        PostfixKind::Index => index_or_slice_postfix(p, lhs, depth),
        PostfixKind::Call => call_postfix(p, lhs, depth),
        PostfixKind::IsNull => {
            let m = lhs.precede(p);
            // Unused currently; reserved for non-infix IS paths.
            m.complete(p, SyntaxKind::IS_NULL_EXPR)
        }
    }
}

fn call_postfix(p: &mut Parser<'_>, lhs: CompletedMarker, depth: u32) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(SyntaxKind::L_PAREN);
    // Optional inline `DISTINCT` (aggregation form).
    p.eat(SyntaxKind::DISTINCT_KW);
    // Arg list.
    if !p.at(SyntaxKind::R_PAREN) {
        let args = p.start();
        call_arg(p, depth);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            call_arg(p, depth);
        }
        args.complete(p, SyntaxKind::ARG_LIST);
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_CALL,
            "expected ')' to close function call",
        );
    }
    m.complete(p, SyntaxKind::FUNCTION_CALL)
}

fn call_arg(p: &mut Parser<'_>, depth: u32) {
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(sc::EXPECTED_CALL_ARG, "expected function argument");
    }
}

/// Parse the `[...]` postfix form and classify it as either
/// [`SyntaxKind::INDEX_EXPR`] (`xs[0]`) or [`SyntaxKind::SLICE_EXPR`]
/// (`xs[i..j]`, `xs[..j]`, `xs[i..]`). Both forms can elide inner
/// expressions: a slice with both bounds elided is `xs[..]`.
///
/// Recovery: an unclosed bracket yields diagnostic
/// [`sc::UNCLOSED_INDEX_BRACKET`] (E0064) — distinct from the legacy
/// `SUBSCRIPT_EXPR` path's E0033 so tooling can tell the two apart.
///
/// Grammar:
/// ```text
/// IndexExpr = Expr '[' Expr ']'
/// SliceExpr = Expr '[' Expr? '..' Expr? ']'
/// ```
///
/// cy-7s6.1 (spec §19 row "List indexing / slicing").
fn index_or_slice_postfix(p: &mut Parser<'_>, lhs: CompletedMarker, depth: u32) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(SyntaxKind::L_BRACK);

    // Start marker: we don't yet know if this is an INDEX_EXPR or SLICE_EXPR.
    // Decide based on whether:
    //   - the first token is `..` (slice with elided start), or
    //   - after parsing an expression we see `..` (slice form), or
    //   - after parsing an expression we see `]` (index form).

    // Elided-start form: `[..j]` or `[..]`.
    if p.at(SyntaxKind::DOT_DOT) {
        p.bump(SyntaxKind::DOT_DOT);
        // Optional end expression.
        if !p.at(SyntaxKind::R_BRACK) && expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(sc::EXPECTED_INDEX_EXPR, "expected slice end expression");
        }
        if !p.eat(SyntaxKind::R_BRACK) {
            p.error_code(
                sc::UNCLOSED_INDEX_BRACKET,
                "expected ']' to close indexing bracket",
            );
        }
        return m.complete(p, SyntaxKind::SLICE_EXPR);
    }

    // Non-elided: parse the first expression.
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(sc::EXPECTED_INDEX_EXPR, "expected index expression");
    }

    if p.at(SyntaxKind::DOT_DOT) {
        // Slice form with start expression: `[i..]` or `[i..j]`.
        p.bump(SyntaxKind::DOT_DOT);
        if !p.at(SyntaxKind::R_BRACK) && expr_bp_depth(p, 0, depth + 1).is_none() {
            p.error_code(sc::EXPECTED_INDEX_EXPR, "expected slice end expression");
        }
        if !p.eat(SyntaxKind::R_BRACK) {
            p.error_code(
                sc::UNCLOSED_INDEX_BRACKET,
                "expected ']' to close indexing bracket",
            );
        }
        m.complete(p, SyntaxKind::SLICE_EXPR)
    } else {
        // Plain index form: `[i]`.
        if !p.eat(SyntaxKind::R_BRACK) {
            p.error_code(
                sc::UNCLOSED_INDEX_BRACKET,
                "expected ']' to close indexing bracket",
            );
        }
        m.complete(p, SyntaxKind::INDEX_EXPR)
    }
}

// --------------------------------------------------------------------------
// Unused helpers (kept for grammar extensibility)
// --------------------------------------------------------------------------

#[allow(dead_code)]
fn _recovery_anchor_placeholder(_: &mut Parser<'_>, _: TokenSet, _: Marker) {
    // Present so future per-production recovery tables (cy-2vh) can be
    // threaded without re-plumbing every call site.
}
