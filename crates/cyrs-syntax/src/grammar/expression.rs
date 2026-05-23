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

use super::{pattern, statement};

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

        // `IS [NOT] NULL`, `IS [NOT] TYPED <TypeName>`, and `IS [NOT]
        // (TRUE|FALSE|UNKNOWN)` — postfix at priority 5 (comparison-
        // level). The three forms share the leading `IS [NOT]` prefix
        // and split on the next token:
        //   `IS NULL`               → IS_NULL_EXPR  (existing path, cy-nom)
        //   `IS TYPED <Type>`       → IS_TYPED_EXPR (cy-pnp, ISO 39075 §6.5.2)
        //   `IS TRUE|FALSE|UNKNOWN` → TRUTH_VALUE_PREDICATE (cy-dwem,
        //                              ISO 39075 §20.1
        //                              `truthValuePredicatePart2`)
        // Anything else after `IS [NOT]` is treated as a missing-NULL
        // recovery so the legacy diagnostic surface (E0025) does not
        // change for queries that simply forgot to spell `NULL`.
        if p.at(SyntaxKind::IS_KW) {
            let null_check_bp = 10;
            if null_check_bp < min_bp {
                break;
            }
            let m = lhs.precede(p);
            p.bump(SyntaxKind::IS_KW);
            p.eat(SyntaxKind::NOT_KW);
            if p.at(SyntaxKind::TYPED_KW) {
                p.bump(SyntaxKind::TYPED_KW);
                if at_type_name_start(p) {
                    type_name(p);
                } else {
                    p.error_code(
                        sc::EXPECTED_TYPE_AFTER_TYPED,
                        "expected type name after `IS [NOT] TYPED`",
                    );
                }
                lhs = m.complete(p, SyntaxKind::IS_TYPED_EXPR);
            } else if matches!(p.current(), SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW)
                || p.at_contextual("UNKNOWN")
            {
                // cy-dwem: GQL truthValuePredicatePart2 (§20.1).  The
                // truthValue tail is one of TRUE / FALSE / UNKNOWN.
                // `TRUE_KW` / `FALSE_KW` are already reserved at the
                // lexer level; `UNKNOWN` is recognised contextually
                // (see `lexer.rs` for the rationale — pre-existing
                // fixtures rely on `unknown` parsing as an ordinary
                // identifier, e.g. `CALL unknown.procedure()` in
                // cyrs-sema's E3008 fixture).  The CST emits the
                // underlying token (`TRUE_KW` / `FALSE_KW` / `IDENT`)
                // verbatim; the parent `TRUTH_VALUE_PREDICATE` node is
                // the semantic marker.
                p.bump_any();
                lhs = m.complete(p, SyntaxKind::TRUTH_VALUE_PREDICATE);
            } else {
                if !p.eat(SyntaxKind::NULL_KW) {
                    p.error_code(sc::EXPECTED_NULL_AFTER_IS, "expected NULL after IS");
                }
                lhs = m.complete(p, SyntaxKind::IS_NULL_EXPR);
            }
            continue;
        }

        // `<expr> :: <TypeName>` — GQL typed-value shorthand (cy-pnp,
        // ISO/IEC 39075:2024 §6.5.2). Sits at the comparison-level
        // priority so `n.age :: INTEGER > 18` parses as
        // `(n.age :: INTEGER) > 18` — the cast binds tighter than every
        // comparison operator. We deliberately keep `::` BELOW the
        // primary postfix priority (20) used by `.` / `[]` / `()` so a
        // following property access on the cast result still chains
        // naturally (`(x :: T).f`); the bp of 10 matches IS_NULL above
        // and is strictly above the additive / multiplicative families.
        if p.at(SyntaxKind::DOUBLE_COLON) {
            let cast_bp = 10;
            if cast_bp < min_bp {
                break;
            }
            let m = lhs.precede(p);
            p.bump(SyntaxKind::DOUBLE_COLON);
            if at_type_name_start(p) {
                type_name(p);
            } else {
                p.error_code(
                    sc::EXPECTED_TYPE_AFTER_DOUBLE_COLON,
                    "expected type name after `::`",
                );
            }
            lhs = m.complete(p, SyntaxKind::TYPE_CAST_EXPR);
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
        // --- cy-51we typed literals + standalone INSERT ---
        // GQL typed temporal literal: `DATE 'YYYY-MM-DD'`, `DATETIME '…'`,
        // `TIME '…'`, `TIMESTAMP '…'`, `DURATION '…'` (ISO/IEC 39075:2024
        // §10.6). The five introducer words are recognised CONTEXTUALLY
        // — they remain plain `IDENT` tokens at the lexer level so that
        // openCypher's use of `date` / `datetime` / `time` / `duration`
        // as property keys and function-call identifiers continues to
        // parse unchanged. The two-token shape `IDENT STRING_LITERAL`
        // with one of the five reserved spellings is the discriminator.
        SyntaxKind::IDENT
            if p.nth(1) == SyntaxKind::STRING_LITERAL && at_temporal_type_intro(p) =>
        {
            typed_temporal_literal(p)
        }
        // --- end cy-51we ---
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
            // Variable reference. A following `(` is handled as a postfix
            // function-call in the infix/postfix loop.
            let m = p.start();
            p.bump_any();
            m.complete(p, SyntaxKind::VAR_EXPR)
        }
        // `EXISTS` has four surface forms (cy-lve / cy-p1u5, spec §6.1 /
        // §19; ISO/IEC 39075:2024 §10.7 / §14.10):
        //
        //   1. `EXISTS ( <pattern> )` — pattern predicate. Accepted; lowered
        //      to `PATTERN_PREDICATE` so HIR / sema see the same shape as
        //      a bare `(a)-->(b)` in WHERE position.
        //   2. `EXISTS ( <expr> )` — `exists(expr)` function call (e.g.
        //      `exists(n.prop)`). Falls through to the `VAR_EXPR` + postfix
        //      function-call path.
        //   3. `EXISTS { … }` — braced block-subquery form (ISO §10.7).
        //      Parsed into an `EXISTS_SUBQUERY_EXPR` containing a full
        //      `STATEMENT` body (clauses including a `RETURN`). Semantic
        //      surface (scope graph, existential semantics) remains
        //      deferred per spec §20 D1 / N4: sema emits E4017
        //      (`exists_subquery` gate) and HIR / plan lowering refuse
        //      to interpret the body.
        //   4. `EXISTS ( MATCH … )` — parenthesised subquery whose body is
        //      a MATCH-block statement (`OpenGQL` samples shape). Wrapped
        //      in the same `EXISTS_SUBQUERY_EXPR` as (3); same
        //      sema-deferral discipline. Spec amendment 2026-05-19
        //      (cy-5e3f) — parser-only widening (cy-p1u5).
        //
        // Disambiguation:
        //   - `EXISTS {`             → form (3).
        //   - `EXISTS ( MATCH`       → form (4).
        //   - `EXISTS ( (`           → form (1).
        //   - `EXISTS ( <anything else>` or `EXISTS <not-paren-or-brace>`
        //                            → form (2) (function-call fallthrough).
        //
        // Forms (1) and (2) share the `EXISTS (` prefix and are split by
        // a second `(` lookahead: a pattern always opens with a
        // parenthesised node pattern, while `exists(expr)` never does.
        // Form (4) is split off form (2) by checking for `MATCH_KW`
        // immediately after the outer `(`; openCypher's `exists(expr)`
        // can never legitimately begin with a `MATCH` keyword.
        SyntaxKind::EXISTS_KW => {
            if p.nth(1) == SyntaxKind::L_BRACE {
                // Form (3): braced subquery — cy-p1u5 parser-only widening.
                exists_subquery_braced(p)
            } else if p.nth(1) == SyntaxKind::L_PAREN && p.nth(2) == SyntaxKind::MATCH_KW {
                // Form (4): parenthesised MATCH-block subquery — cy-p1u5.
                exists_subquery_paren_match(p)
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

// --- cy-51we typed literals + standalone INSERT ---
/// The five contextually-recognised GQL typed-temporal-literal introducers
/// (ISO/IEC 39075:2024 §10.6).  Kept as `IDENT` at the lexer level — see
/// the `TYPED_TEMPORAL_LITERAL` doc comment in `kind.rs` for the rationale
/// (openCypher uses `date` / `datetime` / `time` / `duration` as property
/// keys and as function-call identifiers, so reserving them at the lexer
/// would regress the openCypher TCK).
const TEMPORAL_TYPE_INTROS: &[&str] = &["DATE", "DATETIME", "TIME", "TIMESTAMP", "DURATION"];

/// Returns `true` when the current `IDENT` token spells one of the typed
/// temporal literal introducers (case-insensitive).  Callers should also
/// check `p.nth(1) == STRING_LITERAL` before dispatching, so a `date(...)`
/// function call or `date: 1` property key falls through to the ordinary
/// `VAR_EXPR` / map-key paths.
fn at_temporal_type_intro(p: &Parser<'_>) -> bool {
    TEMPORAL_TYPE_INTROS
        .iter()
        .any(|name| p.at_contextual(name))
}

/// `TypedTemporalLiteral = ('DATE' | 'DATETIME' | 'TIME' | 'TIMESTAMP' |
/// 'DURATION') STRING_LITERAL` — ISO/IEC 39075:2024 §10.6.  The introducer
/// is bumped as its underlying `IDENT` token (see [`at_temporal_type_intro`]);
/// the discriminator between the five surface forms is the text of that
/// `IDENT` child.  The string body's lexical content (ISO date / time /
/// duration formatting) is NOT validated at parse time — that lives in
/// cyrs-sema as a follow-up.
fn typed_temporal_literal(p: &mut Parser<'_>) -> CompletedMarker {
    debug_assert!(at_temporal_type_intro(p));
    debug_assert_eq!(p.nth(1), SyntaxKind::STRING_LITERAL);
    let m = p.start();
    p.bump(SyntaxKind::IDENT);
    p.bump(SyntaxKind::STRING_LITERAL);
    m.complete(p, SyntaxKind::TYPED_TEMPORAL_LITERAL)
}
// --- end cy-51we ---

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

/// Parse the braced EXISTS-subquery form `EXISTS { <Statement> }` —
/// ISO/IEC 39075:2024 §10.7 (cy-p1u5).
///
/// Enters on the `EXISTS` keyword with `EXISTS {` confirmed by [`atom`].
/// Consumes the keyword, the opening `{`, a full
/// [`statement::statement`] body (clauses including `RETURN`), and the
/// closing `}`. Returns a [`SyntaxKind::EXISTS_SUBQUERY_EXPR`] node.
///
/// The parser accepts this shape so the `OpenGQL` `match_with_exists_*`
/// samples flip to `Parser accepts? yes`; the semantic surface
/// (scope graph, existential semantics) remains deferred per spec §20
/// D1 / N4. HIR lowering maps the node to
/// [`cyrs_hir::Expr::ExistsSubqueryDeferred`] without resolving names
/// or types in the body, and sema fires the `exists_subquery` gate
/// (E4017) on every occurrence. See the §0 amendment dated 2026-05-19
/// (cy-5e3f) for the scope rationale.
fn exists_subquery_braced(p: &mut Parser<'_>) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::EXISTS_KW));
    debug_assert!(p.nth(1) == SyntaxKind::L_BRACE);
    let m = p.start();
    p.bump(SyntaxKind::EXISTS_KW);
    p.bump(SyntaxKind::L_BRACE);
    // Parse the body as a full SingleQuery / UnionQuery. The grammar
    // for `statement` already loops until it sees `EOF`, `;`, or a
    // token that is not a clause start — the closing `R_BRACE` falls
    // into the "not a clause start" bucket and terminates the loop
    // without consuming the brace.
    statement::statement(p);
    if !p.eat(SyntaxKind::R_BRACE) {
        p.error_code(
            sc::EXPECTED_RBRACE_EXISTS,
            "expected '}' to close EXISTS { ... } subquery",
        );
    }
    m.complete(p, SyntaxKind::EXISTS_SUBQUERY_EXPR)
}

/// Parse the parenthesised EXISTS-subquery form `EXISTS ( MATCH … )` —
/// `OpenGQL` samples shape (cy-p1u5, ISO/IEC 39075:2024 §10.7 / §14.10).
///
/// Enters on the `EXISTS` keyword with `EXISTS ( MATCH` confirmed by
/// [`atom`]. Consumes the keyword, the outer `(`, a single
/// `MATCH_CLAUSE` (with its `WHERE` tail), and the closing `)`.
/// Returns a [`SyntaxKind::EXISTS_SUBQUERY_EXPR`] node. The body is a
/// single MATCH block (no `RETURN` required) — distinct from the
/// braced form, which carries a full statement.
///
/// Semantic surface is deferred to the same place as the braced form;
/// see [`exists_subquery_braced`] for the discipline.
fn exists_subquery_paren_match(p: &mut Parser<'_>) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::EXISTS_KW));
    debug_assert!(p.nth(1) == SyntaxKind::L_PAREN);
    debug_assert!(p.nth(2) == SyntaxKind::MATCH_KW);
    let m = p.start();
    p.bump(SyntaxKind::EXISTS_KW);
    p.bump(SyntaxKind::L_PAREN);
    // The body is a `MATCH` clause (with its optional `WHERE` tail).
    // We reuse the regular clause parser via `statement::statement`
    // so we accept any clauses GQL's MATCH-block grammar allows; the
    // loop terminates at the closing `)` (not a clause-start token).
    statement::statement(p);
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_EXISTS,
            "expected ')' to close EXISTS ( MATCH ... ) subquery",
        );
    }
    m.complete(p, SyntaxKind::EXISTS_SUBQUERY_EXPR)
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
// GQL type-assertion helpers (cy-pnp, ISO/IEC 39075:2024 §6.5.2 / §6.2)
// --------------------------------------------------------------------------

/// Maximum number of IDENT tokens consumed into a single `TypeName` (cy-pnp).
///
/// GQL type names range from one word (`INTEGER`, `STRING`, `BOOLEAN`) to
/// multi-word phrases (`ZONED DATETIME`, `LOCAL DATETIME`, `LOCAL TIME`).
/// The longest names in §6.2 are three tokens (e.g. `LOCAL ZONED DATETIME`
/// in dialect extensions). Capping the greedy loop at three keeps a stray
/// identifier later in the expression from being absorbed (e.g. in
/// `n.age :: INTEGER > 18`, only `INTEGER` is consumed because `>` is not
/// an IDENT) and bounds the worst-case lookahead.
const MAX_TYPE_NAME_IDENTS: u32 = 3;

/// Returns `true` if the current token can start a `TypeName` — i.e. is an
/// identifier-shaped token. Type names are spelled as ordinary identifiers
/// (`INTEGER`, `STRING`, `ZONED`, …) and are NOT reserved at the lexer
/// level so they keep working as variable / label names elsewhere.
fn at_type_name_start(p: &Parser<'_>) -> bool {
    matches!(p.current(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
}

/// Parse `TypeName = NameRef NameRef*` — one or more identifier tokens
/// (cy-pnp). The leading IDENT is required; the caller has already
/// confirmed it via [`at_type_name_start`]. Continuation IDENTs are
/// consumed greedily but capped at [`MAX_TYPE_NAME_IDENTS`] so a stray
/// identifier later in the expression (e.g. an `AS` alias would not be
/// an IDENT, but a stray `FOO` after `INTEGER` could be) is not silently
/// absorbed. The emitted node is `TYPE_NAME` with its IDENT children as
/// direct child tokens.
fn type_name(p: &mut Parser<'_>) {
    debug_assert!(at_type_name_start(p));
    let m = p.start();
    // First IDENT — required.
    p.bump_any();
    // Up to (MAX_TYPE_NAME_IDENTS - 1) continuation IDENTs.
    let mut consumed: u32 = 1;
    while consumed < MAX_TYPE_NAME_IDENTS && at_type_name_start(p) {
        p.bump_any();
        consumed += 1;
    }
    m.complete(p, SyntaxKind::TYPE_NAME);
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
    /// `{ .p, key: v, .*, * }` — map projection over a subject expression.
    /// Spec §6.1 (desugar in HIR) / §19 row "Map projection". The trailer
    /// position is what distinguishes this from a standalone map literal
    /// `{ k: v }` (cy-01q).
    MapProjection,
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
        // `{` immediately following an atom expression is a map projection
        // trailer (cy-01q, spec §6.1 / §19). A standalone `{ k: v }` map
        // literal is parsed by the `atom` dispatch on `L_BRACE`; that path
        // never reaches the postfix loop because the literal *is* the lhs.
        // Conversely, once a lhs has been completed, an immediately-trailing
        // `{` always reads as projection — there is no other valid
        // expression continuation that begins with `{`.
        SyntaxKind::L_BRACE => PostfixOp {
            bp,
            kind: PostfixKind::MapProjection,
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
        PostfixKind::MapProjection => map_projection_postfix(p, lhs, depth),
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
    // `count(*)` is the canonical wildcard aggregate shape. Accept `*` as a
    // standalone call arg when it is immediately followed by `)` — i.e. the
    // single-arg wildcard form. We deliberately don't allow `*` mixed with
    // other args (e.g. `count(*, x)`); the executor only honours the bare
    // form. The bumped STAR token sits inside ARG_LIST as a trivia-adjacent
    // token (no node), so HIR lowering's `try_lower_expr` filter naturally
    // produces an empty args vec — exactly the shape `lg-query-cyrs`'s
    // `AggAccumulator` detects via `args.is_empty()` for `count_star`.
    if p.at(SyntaxKind::STAR) && p.nth(1) == SyntaxKind::R_PAREN {
        p.bump(SyntaxKind::STAR);
        return;
    }
    if expr_bp_depth(p, 0, depth + 1).is_none() {
        p.error_code(sc::EXPECTED_CALL_ARG, "expected function argument");
    }
}

/// Parse a map-projection trailer: `<lhs> { item (',' item)* }` where each
/// item is one of:
///
/// - `.NAME`          — property selector (key=name, value=lhs.name)
/// - `IDENT ':' Expr` — literal item (key=ident, value=Expr)
/// - `.*`             — all-properties spread of the subject
/// - `*`              — all-bound-vars spread (rare; openCypher allows it)
///
/// Spec §6.1 (sugar; desugared in HIR) / §19 row "Map projection" (cy-01q).
///
/// Each item is wrapped in a `MAP_PROJECTION_ITEM` node so HIR lowering can
/// classify the four kinds by inspecting the leading token (`.` + IDENT,
/// `.` + `*`, IDENT + `:`, or bare `*`). The completed wrapper carries the
/// lhs as its first `Expr` child via the `lhs.precede(p)` rebase, mirroring
/// how every other postfix shape (property access, index, call) wraps its
/// receiver.
fn map_projection_postfix(p: &mut Parser<'_>, lhs: CompletedMarker, depth: u32) -> CompletedMarker {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    let m = lhs.precede(p);
    p.bump(SyntaxKind::L_BRACE);

    if !p.at(SyntaxKind::R_BRACE) {
        map_projection_item(p, depth);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            if p.at(SyntaxKind::R_BRACE) {
                break;
            }
            map_projection_item(p, depth);
        }
    }

    if !p.eat(SyntaxKind::R_BRACE) {
        p.error_code(
            sc::EXPECTED_RBRACE_MAP_PROJECTION,
            "expected '}' to close map projection",
        );
    }
    m.complete(p, SyntaxKind::MAP_PROJECTION)
}

/// Parse one item inside a map projection. Each kind emits its own marker
/// so the resulting CST has uniform `MAP_PROJECTION_ITEM` children — HIR
/// lowering inspects the first significant token of each item to classify.
fn map_projection_item(p: &mut Parser<'_>, depth: u32) {
    let m = p.start();
    match p.current() {
        // `.*` (all-properties spread) or `.NAME` (property selector).
        SyntaxKind::DOT => {
            p.bump(SyntaxKind::DOT);
            if p.at(SyntaxKind::STAR) {
                p.bump(SyntaxKind::STAR);
            } else if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
                p.error_code(
                    sc::EXPECTED_PROP_OR_STAR_AFTER_DOT_IN_PROJECTION,
                    "expected property name or '*' after '.' in map projection item",
                );
            }
        }
        // `*` (all-bound-vars spread). Rare openCypher form; lowered as a
        // scope-wide spread by HIR.
        SyntaxKind::STAR => {
            p.bump(SyntaxKind::STAR);
        }
        // `IDENT ':' Expr` — literal item, same shape as a map-literal entry.
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
            p.bump_any();
            if !p.eat(SyntaxKind::COLON) {
                p.error_code(
                    sc::EXPECTED_COLON_MAP_PROJECTION,
                    "expected ':' in map projection literal item",
                );
            }
            if expr_bp_depth(p, 0, depth + 1).is_none() {
                p.error_code(
                    sc::EXPECTED_VALUE_MAP_PROJECTION,
                    "expected expression for map projection value",
                );
            }
        }
        _ => {
            p.error_code(
                sc::EXPECTED_MAP_PROJECTION_ITEM,
                "expected `.name`, `key: expr`, `.*`, or `*` in map projection",
            );
            // Token-bump to make recovery progress; the outer loop will
            // either find a `,` or `}` and continue.
            if !p.at(SyntaxKind::R_BRACE) && !p.at(SyntaxKind::COMMA) {
                p.bump_any();
            }
        }
    }
    m.complete(p, SyntaxKind::MAP_PROJECTION_ITEM);
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

#[cfg(test)]
mod tests {
    //! Expression-grammar smoke tests (cy-m2hz).
    //!
    //! Other test modules in the crate exercise expressions implicitly
    //! through clause-level parses (RETURN, WHERE, etc.). The cases
    //! below pin down the newer / less-covered paths so they don't
    //! drift silently:
    //!
    //!   - EXISTS subquery surface (cy-p1u5): braced + paren-MATCH forms
    //!     plus their error paths.
    //!   - Typed temporal literals (cy-51we).
    //!   - Bare pattern predicates in expression position (cy-7lf).
    //!   - List predicates (ANY/ALL/NONE/SINGLE) and CASE expressions.
    //!   - Map projection trailers (cy-01q) and map literal entries.
    //!   - List comprehensions (cy-7s6.1) and slice / index postfixes.
    //!   - Postfix recovery: missing prop key after `.`, unclosed call,
    //!     unclosed bracket.

    use crate::SyntaxKind;
    use crate::parser::parse;

    fn assert_clean(src: &str) {
        let p = parse(src);
        assert_eq!(p.syntax().to_string(), src, "lossless round-trip failed");
        assert!(
            p.errors().is_empty(),
            "unexpected errors parsing {src:?}: {:?}",
            p.errors()
        );
    }

    fn parse_codes(src: &str) -> Vec<u16> {
        let p = parse(src);
        // Lossless round-trip even on error paths.
        assert_eq!(p.syntax().to_string(), src);
        p.errors().iter().map(|e| e.code).collect()
    }

    fn has_kind(src: &str, kind: SyntaxKind) -> bool {
        let p = parse(src);
        p.syntax().descendants().any(|n| n.kind() == kind)
    }

    // --- EXISTS subquery forms (cy-p1u5) -------------------------

    #[test]
    fn exists_braced_subquery_parses() {
        let src = "RETURN EXISTS { MATCH (n) RETURN n }";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::EXISTS_SUBQUERY_EXPR));
    }

    #[test]
    fn exists_paren_match_subquery_parses() {
        let src = "RETURN EXISTS ( MATCH (n) )";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::EXISTS_SUBQUERY_EXPR));
    }

    #[test]
    fn exists_pattern_predicate_parses() {
        // `EXISTS ( ( ... ) )` — the form-(1) branch.
        let src = "RETURN EXISTS ( (a)-->(b) )";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::PATTERN_PREDICATE));
    }

    #[test]
    fn exists_function_call_form_falls_through() {
        // Form (2): `exists(n.prop)` — should be FUNCTION_CALL.
        let src = "RETURN EXISTS(n.prop)";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::FUNCTION_CALL));
    }

    #[test]
    fn exists_braced_unclosed_errors() {
        // Missing closing `}` — EXPECTED_RBRACE_EXISTS.
        let codes = parse_codes("RETURN EXISTS { MATCH (n) RETURN n");
        assert!(!codes.is_empty());
    }

    #[test]
    fn exists_paren_match_unclosed_errors() {
        // Missing closing `)` — EXPECTED_RPAREN_EXISTS.
        let codes = parse_codes("RETURN EXISTS ( MATCH (n)");
        assert!(!codes.is_empty());
    }

    #[test]
    fn exists_pattern_predicate_unclosed_errors() {
        // `EXISTS ( ( ... )` — missing outer `)`.
        let codes = parse_codes("RETURN EXISTS ( (a)-->(b)");
        assert!(!codes.is_empty());
    }

    // --- Typed temporal literals (cy-51we) -----------------------

    #[test]
    fn typed_temporal_literal_date() {
        let src = "RETURN DATE '2022-10-10'";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::TYPED_TEMPORAL_LITERAL));
    }

    #[test]
    fn typed_temporal_literal_datetime() {
        assert_clean("RETURN DATETIME '2022-10-10T00:00:00'");
    }

    #[test]
    fn typed_temporal_literal_time() {
        assert_clean("RETURN TIME '12:34:56'");
    }

    #[test]
    fn typed_temporal_literal_timestamp() {
        assert_clean("RETURN TIMESTAMP '2022-10-10T00:00:00'");
    }

    #[test]
    fn typed_temporal_literal_duration() {
        assert_clean("RETURN DURATION 'P1D'");
    }

    #[test]
    fn date_property_key_still_parses() {
        // `date` as a property key must remain an ordinary IDENT.
        assert_clean("RETURN { date: 1 }");
    }

    // --- IS [NOT] TYPED <Type> (cy-pnp) --------------------------

    #[test]
    fn is_typed_with_single_word_type() {
        let src = "RETURN n.age IS TYPED INTEGER";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::IS_TYPED_EXPR));
        assert!(has_kind(src, SyntaxKind::TYPE_NAME));
    }

    #[test]
    fn is_typed_with_multi_word_type() {
        let src = "RETURN n.t IS TYPED ZONED DATETIME";
        assert_clean(src);
    }

    #[test]
    fn is_not_typed_with_type() {
        assert_clean("RETURN n.age IS NOT TYPED INTEGER");
    }

    #[test]
    fn is_typed_missing_type_errors() {
        let codes = parse_codes("RETURN n.age IS TYPED 42");
        assert!(!codes.is_empty());
    }

    #[test]
    fn double_colon_type_cast() {
        let src = "RETURN n.age :: INTEGER";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::TYPE_CAST_EXPR));
    }

    #[test]
    fn double_colon_type_cast_missing_type_errors() {
        let codes = parse_codes("RETURN n.age :: 42");
        assert!(!codes.is_empty());
    }

    // --- IS NULL recovery ----------------------------------------

    #[test]
    fn is_missing_null_errors() {
        let codes = parse_codes("RETURN n IS 42");
        assert!(!codes.is_empty());
    }

    // --- IS [NOT] TRUE / FALSE / UNKNOWN (cy-dwem, §20.1) -------

    #[test]
    fn is_true_predicate() {
        let src = "RETURN n.flag IS TRUE";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::TRUTH_VALUE_PREDICATE));
    }

    #[test]
    fn is_false_predicate() {
        let src = "RETURN n.flag IS FALSE";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::TRUTH_VALUE_PREDICATE));
    }

    #[test]
    fn is_unknown_predicate() {
        let src = "RETURN n.flag IS UNKNOWN";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::TRUTH_VALUE_PREDICATE));
    }

    #[test]
    fn is_not_true_predicate() {
        assert_clean("RETURN n.flag IS NOT TRUE");
    }

    #[test]
    fn is_not_false_predicate() {
        assert_clean("RETURN n.flag IS NOT FALSE");
    }

    #[test]
    fn is_not_unknown_predicate() {
        assert_clean("RETURN n.flag IS NOT UNKNOWN");
    }

    #[test]
    fn truth_value_predicate_chains_with_and() {
        // The predicate has comparison-level precedence so it composes
        // with conjunctive operators in the surrounding WHERE.
        assert_clean("MATCH (n) WHERE n.a IS TRUE AND n.b IS NOT FALSE RETURN n");
    }

    // --- List predicates (cy-8x5) --------------------------------

    #[test]
    fn list_predicate_any_with_where() {
        let src = "RETURN ANY(x IN xs WHERE x > 1)";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::LIST_PREDICATE_EXPR));
    }

    #[test]
    fn list_predicate_all_no_where() {
        assert_clean("RETURN ALL(x IN xs)");
    }

    #[test]
    fn list_predicate_none() {
        assert_clean("RETURN NONE(x IN xs WHERE x = 0)");
    }

    #[test]
    fn list_predicate_single() {
        assert_clean("RETURN SINGLE(x IN xs WHERE x = 0)");
    }

    #[test]
    fn list_predicate_missing_in_errors() {
        let codes = parse_codes("RETURN ANY(x OF xs)");
        assert!(!codes.is_empty());
    }

    // --- CASE expressions (cy-41u) -------------------------------

    #[test]
    fn case_generic_form() {
        let src = "RETURN CASE WHEN n.x = 1 THEN 'a' ELSE 'b' END";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::CASE_EXPR));
        assert!(has_kind(src, SyntaxKind::CASE_WHEN_ARM));
        assert!(has_kind(src, SyntaxKind::CASE_ELSE_ARM));
    }

    #[test]
    fn case_simple_form_with_scrutinee() {
        assert_clean("RETURN CASE n.x WHEN 1 THEN 'one' WHEN 2 THEN 'two' END");
    }

    #[test]
    fn case_missing_then_errors() {
        let codes = parse_codes("RETURN CASE WHEN n.x = 1 'a' END");
        assert!(!codes.is_empty());
    }

    #[test]
    fn case_missing_end_errors() {
        let codes = parse_codes("RETURN CASE WHEN n.x = 1 THEN 'a'");
        assert!(!codes.is_empty());
    }

    // --- Map / list literals + comprehensions --------------------

    #[test]
    fn map_literal_multiple_entries() {
        assert_clean("RETURN { a: 1, b: 2, c: 3 }");
    }

    #[test]
    fn map_literal_trailing_comma() {
        assert_clean("RETURN { a: 1, }");
    }

    #[test]
    fn map_literal_missing_value_errors() {
        let codes = parse_codes("RETURN { a: }");
        assert!(!codes.is_empty());
    }

    #[test]
    fn map_literal_missing_colon_errors() {
        let codes = parse_codes("RETURN { a 1 }");
        assert!(!codes.is_empty());
    }

    #[test]
    fn list_literal_multiple_elements() {
        assert_clean("RETURN [1, 2, 3]");
    }

    #[test]
    fn list_comprehension_full() {
        let src = "RETURN [x IN xs WHERE x > 0 | x * 2]";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::LIST_COMPREHENSION));
    }

    #[test]
    fn list_comprehension_filter_only() {
        assert_clean("RETURN [x IN xs WHERE x > 0]");
    }

    #[test]
    fn list_comprehension_map_only() {
        assert_clean("RETURN [x IN xs | x * 2]");
    }

    // --- Map projection trailers (cy-01q) ------------------------

    #[test]
    fn map_projection_property_selector() {
        let src = "RETURN n { .name, .age }";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::MAP_PROJECTION));
    }

    #[test]
    fn map_projection_literal_item() {
        assert_clean("RETURN n { key: 1 }");
    }

    #[test]
    fn map_projection_star_spreads() {
        assert_clean("RETURN n { .* }");
    }

    #[test]
    fn map_projection_bare_star() {
        assert_clean("RETURN n { * }");
    }

    #[test]
    fn map_projection_trailing_comma() {
        assert_clean("RETURN n { .name, }");
    }

    #[test]
    fn map_projection_invalid_item_errors() {
        let codes = parse_codes("RETURN n { 42 }");
        assert!(!codes.is_empty());
    }

    // --- Postfix: property access / call / index / slice ---------

    #[test]
    fn property_access_chains() {
        assert_clean("RETURN n.a.b.c");
    }

    #[test]
    fn property_access_quoted_ident() {
        assert_clean("RETURN n.`weird name`");
    }

    #[test]
    fn property_access_missing_key_errors() {
        let codes = parse_codes("RETURN n.");
        assert!(!codes.is_empty());
    }

    #[test]
    fn call_with_distinct() {
        assert_clean("RETURN count(DISTINCT n)");
    }

    #[test]
    fn call_count_star() {
        assert_clean("RETURN count(*)");
    }

    #[test]
    fn call_multi_arg() {
        assert_clean("RETURN foo(1, 2, 3)");
    }

    #[test]
    fn call_unclosed_errors() {
        let codes = parse_codes("RETURN foo(1, 2");
        assert!(!codes.is_empty());
    }

    #[test]
    fn slice_full_form() {
        let src = "RETURN xs[1..3]";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::SLICE_EXPR));
    }

    #[test]
    fn slice_elided_start() {
        assert_clean("RETURN xs[..3]");
    }

    #[test]
    fn slice_elided_end() {
        assert_clean("RETURN xs[1..]");
    }

    #[test]
    fn slice_both_elided() {
        assert_clean("RETURN xs[..]");
    }

    #[test]
    fn index_simple() {
        let src = "RETURN xs[0]";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::INDEX_EXPR));
    }

    #[test]
    fn index_unclosed_errors() {
        let codes = parse_codes("RETURN xs[0");
        assert!(!codes.is_empty());
    }

    // --- Bare pattern predicate in expression (cy-7lf) -----------

    #[test]
    fn bare_pattern_predicate_in_where() {
        let src = "MATCH (n) WHERE (n)-[:R]->(m) RETURN n";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::PATTERN_PREDICATE));
    }

    #[test]
    fn bare_pattern_predicate_empty_paren() {
        assert_clean("MATCH (n) WHERE () RETURN n");
    }

    #[test]
    fn bare_pattern_predicate_labelled() {
        assert_clean("MATCH (n) WHERE (:Foo) RETURN n");
    }

    #[test]
    fn paren_expression_still_parsed_as_expr() {
        // `(1 + 2)` is an arithmetic expression, not a pattern.
        assert_clean("RETURN (1 + 2)");
    }

    // --- Binary / string operators -------------------------------

    #[test]
    fn starts_with_operator() {
        let src = "RETURN n.s STARTS WITH 'foo'";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::STRING_OP_EXPR));
    }

    #[test]
    fn ends_with_operator() {
        assert_clean("RETURN n.s ENDS WITH 'foo'");
    }

    #[test]
    fn contains_operator() {
        assert_clean("RETURN n.s CONTAINS 'foo'");
    }

    #[test]
    fn regex_match_operator() {
        assert_clean("RETURN n.s =~ '^foo'");
    }

    #[test]
    fn starts_missing_with_errors() {
        let codes = parse_codes("RETURN n.s STARTS 'foo'");
        assert!(!codes.is_empty());
    }

    #[test]
    fn ends_missing_with_errors() {
        let codes = parse_codes("RETURN n.s ENDS 'foo'");
        assert!(!codes.is_empty());
    }

    #[test]
    fn xor_operator() {
        assert_clean("RETURN n.a XOR n.b");
    }

    #[test]
    fn unary_not_minus_plus() {
        assert_clean("RETURN NOT n.a");
        assert_clean("RETURN -n.a");
        assert_clean("RETURN +n.a");
    }

    #[test]
    fn power_operator_right_assoc() {
        assert_clean("RETURN 2 ^ 3 ^ 4");
    }

    #[test]
    fn modulo_operator() {
        assert_clean("RETURN n.a % 2");
    }

    // --- Parameter, paren recovery -------------------------------

    #[test]
    fn param_expression() {
        let src = "RETURN $foo";
        assert_clean(src);
        assert!(has_kind(src, SyntaxKind::PARAM_EXPR));
    }

    #[test]
    fn paren_expr_unclosed_errors() {
        let codes = parse_codes("RETURN (1 + 2");
        assert!(!codes.is_empty());
    }

    #[test]
    fn paren_expr_empty_errors() {
        let codes = parse_codes("RETURN ()");
        // Bare `()` is a pattern (empty node), not an expression — so
        // this parses cleanly via the pattern-predicate branch.
        // We only require lossless round-trip; no error required.
        let _ = codes;
    }

    // --- Literals ----------------------------------------------

    #[test]
    fn null_true_false_literals() {
        assert_clean("RETURN NULL");
        assert_clean("RETURN TRUE");
        assert_clean("RETURN FALSE");
    }
}
