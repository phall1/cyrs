//! Pattern productions — nodes, fixed-length relationships, labels,
//! property maps. Spec `cypher.ungrammar` `Pattern` / `NodePattern` /
//! `RelPattern` / `LabelExpr` / `PropertyMap`.
//!
//! # cy-nom scope
//! - `NodePattern = '(' IDENT? LabelList? PropertyMap? ')'`
//! - `RelPattern` supports the three arrow shapes `-[]-`, `-[]->`,
//!   `<-[]-`, chained to extend a path.
//! - Label lists: `(':' IDENT)+`.
//! - Property-map values are full `Expr`s.
//!
//! Deferred (each tagged with `cy-nom: v1 scope — ...` at its stub):
//! variable-length rels (`*m..n`), path binders (`p = <path>`), pipe-
//! disjunction rel-type expressions, shortestPath / allShortestPaths
//! patterns, pattern predicates in expression position.

use crate::SyntaxKind;
use crate::parser::{Parser, syntax_codes as sc};

use super::expression;

/// `Pattern (',' Pattern)*`
pub(crate) fn pattern_list(p: &mut Parser<'_>) {
    pattern(p);
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        pattern(p);
    }
}

/// `Pattern = (bind:NameDef '=')? PathPattern` — spec ungrammar `Pattern`.
///
/// A leading `IDENT =` binds the path expression to the named variable
/// (`p = (a)-[]->(b)`). The `NAMED_PATTERN_PART` wrapper surrounds the
/// inner `PATTERN_PART`; without the binder we emit only the
/// `PATTERN_PART`, matching the previous cy-nom shape so existing AST
/// consumers don't see gratuitous wrappers.
pub(crate) fn pattern(p: &mut Parser<'_>) {
    let m = p.start();
    if is_path_binder(p) {
        let bind = p.start();
        name_binder(p);
        p.bump(SyntaxKind::EQ);
        // GQL path selector (cy-3mq, ISO/IEC 39075:2024 §10.4.2).
        // After the binder's `=`, optionally consume one of:
        //   `ANY SHORTEST` | `ALL SHORTEST` | `SHORTEST <int>`
        // before the path pattern itself.
        if at_path_selector(p) {
            path_selector(p);
        }
        path_pattern(p);
        bind.complete(p, SyntaxKind::NAMED_PATTERN_PART);
    } else {
        path_pattern(p);
    }
    m.complete(p, SyntaxKind::PATTERN);
}

/// Path-selector lookahead — true at `ANY SHORTEST`, `ALL SHORTEST`, or
/// `SHORTEST k`.  Used by `pattern` after binder `=`.
fn at_path_selector(p: &mut Parser<'_>) -> bool {
    if p.at(SyntaxKind::SHORTEST_KW) {
        return true;
    }
    if (p.at(SyntaxKind::ANY_KW) || p.at(SyntaxKind::ALL_KW))
        && p.nth(1) == SyntaxKind::SHORTEST_KW
    {
        return true;
    }
    false
}

/// Parse a GQL path selector (cy-3mq, ISO/IEC 39075:2024 §10.4.2):
///   `ANY SHORTEST` | `ALL SHORTEST` | `SHORTEST INT_LITERAL`.
/// Wraps in a `PATH_SELECTOR` node whose first keyword child is the
/// discriminant. No dedicated diag code: a malformed selector (e.g.
/// `SHORTEST <non-int>`) leaves the trailing tokens to be picked up by
/// the path-pattern parser.
fn path_selector(p: &mut Parser<'_>) {
    let m = p.start();
    if p.at(SyntaxKind::ANY_KW) || p.at(SyntaxKind::ALL_KW) {
        p.bump_any();
        // SHORTEST_KW guaranteed by at_path_selector.
        let _ = p.eat(SyntaxKind::SHORTEST_KW);
    } else {
        // SHORTEST_KW; optional INT_LITERAL for `SHORTEST k`.
        p.bump(SyntaxKind::SHORTEST_KW);
        let _ = p.eat(SyntaxKind::INT_LITERAL);
    }
    m.complete(p, SyntaxKind::PATH_SELECTOR);
}

/// A path binder is `IDENT '='` before an `L_PAREN` — the `=` disambiguates
/// from a bare node pattern or a following clause. We require the `=`
/// token to sit between two valid positions, so one-token lookahead
/// suffices.
fn is_path_binder(p: &mut Parser<'_>) -> bool {
    if !(p.current() == SyntaxKind::IDENT || p.current() == SyntaxKind::QUOTED_IDENT) {
        return false;
    }
    p.nth(1) == SyntaxKind::EQ
}

/// `PathPattern = ShortestPathPattern | NodePattern (RelPattern NodePattern)*`
///
/// Spec ungrammar `PathPattern` / `ShortestPathPattern`. The leading
/// `shortestPath` / `allShortestPaths` keyword (lexed as
/// `SHORTESTPATH_KW` / `ALLSHORTESTPATHS_KW` per spec §6.4 / §19 row
/// "shortest-path") switches the parser into the shortest-path arm,
/// which wraps an inner path pattern in a single dedicated
/// `SHORTEST_PATH_PATTERN` node. The discriminant between the two
/// surface forms is the first keyword child token, mirroring the
/// `LIST_PREDICATE_EXPR` discriminant convention.
fn path_pattern(p: &mut Parser<'_>) {
    if p.at(SyntaxKind::SHORTESTPATH_KW) || p.at(SyntaxKind::ALLSHORTESTPATHS_KW) {
        shortest_path_pattern(p);
        return;
    }
    let m = p.start();
    if !p.at(SyntaxKind::L_PAREN) {
        p.error_code(
            sc::EXPECTED_LPAREN_NODE,
            "expected '(' to start a node pattern",
        );
        m.complete(p, SyntaxKind::PATTERN_PART);
        return;
    }
    node_pattern(p);
    while at_rel_start(p) {
        rel_pattern(p);
        if p.at(SyntaxKind::L_PAREN) {
            node_pattern(p);
        } else {
            p.error_code(
                sc::EXPECTED_NODE_AFTER_REL,
                "expected node pattern after relationship",
            );
            break;
        }
    }
    m.complete(p, SyntaxKind::PATTERN_PART);
}

/// `ShortestPathPattern = ('shortestPath' | 'allShortestPaths') '(' PathElement+ ')'`
/// — spec ungrammar `ShortestPathPattern`, spec §6.4 / §19 row
/// "shortest-path".
///
/// Enters on the keyword token. Consumes the keyword, the opening `(`,
/// the inner path pattern (a single relationship pattern, normally
/// with a variable-length quantifier `*`), and the closing `)`. The
/// inner pattern is wrapped in a `PATTERN_PART` so HIR / sema lower
/// it through the same code path as a bare path. Recovery: E0077 on
/// a missing `)`.
fn shortest_path_pattern(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::SHORTESTPATH_KW) || p.at(SyntaxKind::ALLSHORTESTPATHS_KW));
    let m = p.start();
    p.bump_any();
    if !p.eat(SyntaxKind::L_PAREN) {
        p.error_code(
            sc::EXPECTED_LPAREN_NODE,
            "expected '(' after `shortestPath` / `allShortestPaths`",
        );
    }
    // Reuse `path_pattern` for the canonical inner shape. This keeps
    // the grammar accepted here exactly aligned with what MATCH accepts
    // in a bare position — labels, types, variable-length hops (`*`,
    // `*1..n`).
    if p.at(SyntaxKind::L_PAREN) {
        path_pattern(p);
    } else {
        p.error_code(
            sc::EXPECTED_LPAREN_NODE,
            "expected '(' to start a node pattern",
        );
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_SHORTEST_PATH,
            "expected ')' to close `shortestPath` / `allShortestPaths`",
        );
    }
    m.complete(p, SyntaxKind::SHORTEST_PATH_PATTERN);
}

fn at_rel_start(p: &mut Parser<'_>) -> bool {
    matches!(p.current(), SyntaxKind::MINUS | SyntaxKind::ARROW_L)
}

/// `NodePattern = '(' IDENT? LabelList? PropertyMap? ')'`
fn node_pattern(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_PAREN));
    let m = p.start();
    p.bump(SyntaxKind::L_PAREN);

    // Optional name binder.
    if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
        name_binder(p);
    }
    // Optional label list.
    if p.at(SyntaxKind::COLON) {
        label_expr(p);
    }
    // Optional property map.
    if p.at(SyntaxKind::L_BRACE) {
        property_map(p);
    }

    if !p.eat(SyntaxKind::R_PAREN) {
        // Virtual-token insertion per spec §4.3: emit diagnostic at the
        // expected position and continue. No bytes are consumed.
        p.error_code(
            sc::EXPECTED_RPAREN_NODE,
            "expected ')' to close node pattern",
        );
    }
    m.complete(p, SyntaxKind::NODE_PATTERN);
}

/// Relationship pattern: `-[...]-`, `-[...]->`, or `<-[...]-`. Only
/// fixed-length; variable-length (`*m..n`) is deferred per cy-nom scope.
fn rel_pattern(p: &mut Parser<'_>) {
    debug_assert!(at_rel_start(p));
    let m = p.start();

    let left_arrow = if p.at(SyntaxKind::ARROW_L) {
        p.bump(SyntaxKind::ARROW_L);
        true
    } else {
        // `-` or `-[...]-*`
        if !p.eat(SyntaxKind::MINUS) {
            p.error_code(
                sc::EXPECTED_DASH_REL_START,
                "expected '-' at relationship start",
            );
        }
        false
    };

    // Optional detail in square brackets.
    if p.at(SyntaxKind::L_BRACK) {
        rel_detail(p);
    }

    // Closing arrow. The left_arrow case requires a plain `-`; otherwise
    // we accept either `-` or `->`.
    if left_arrow {
        if !p.eat(SyntaxKind::MINUS) {
            p.error_code(
                sc::EXPECTED_DASH_REL_CLOSE,
                "expected '-' to close relationship",
            );
        }
    } else if !(p.eat(SyntaxKind::ARROW_R) || p.eat(SyntaxKind::MINUS)) {
        p.error_code(
            sc::EXPECTED_DASH_OR_ARROW,
            "expected '-' or '->' to close relationship",
        );
    }

    // GQL postfix one-or-more quantifier `->+` (cy-3mq, ISO/IEC
    // 39075:2024 §10.5). A bare `+` immediately after the closing arrow
    // means "one or more hops"; we wrap it in the existing REL_LENGTH
    // node so HIR sees a uniform quantifier shape regardless of which
    // surface spelling was used.
    if p.at(SyntaxKind::PLUS) {
        let q = p.start();
        p.bump(SyntaxKind::PLUS);
        q.complete(p, SyntaxKind::REL_LENGTH);
    }

    // GQL-distinct post-arrow variable-length quantifier `->{m,n}`
    // (ISO/IEC 39075:2024 §10.6; cy-q2g). The classical openCypher form
    // is `-[*m..n]->` (quantifier inside the bracketed detail, parsed
    // via `rel_length`). The GQL brace-form puts the quantifier after
    // the closing arrow as `->{m,n}` or `-{m,n}-` style; we accept the
    // brace-block in this trailing position and wrap it as a
    // `REL_LENGTH` child of the same `REL_PATTERN`, so HIR /
    // downstream sees a single uniform quantifier shape regardless of
    // which surface spelling was used. The brace pair `{m,n}` is
    // unambiguous here — no other production places `{` directly
    // against the end of a relationship pattern (the property-map `{`
    // sits inside `REL_DETAIL`, which is already closed by `]`).
    if p.at(SyntaxKind::L_BRACE) {
        rel_brace_length(p);
    }

    m.complete(p, SyntaxKind::REL_PATTERN);
}

/// `RelBraceLength = '{' IntLiteral (',' IntLiteral)? '}'` — GQL
/// postfix variable-length quantifier on a relationship pattern
/// (cy-q2g; ISO/IEC 39075:2024 §10.6).
///
/// Emits a `REL_LENGTH` node identical in shape to the classical
/// `*m..n` quantifier produced by `rel_length`, so HIR lowering does
/// not branch on the surface spelling. The parser is permissive (no
/// dedicated diagnostics) — a malformed brace-block leaves downstream
/// recovery to flag the trailing tokens, mirroring how `rel_length`
/// itself only consumes what matches. The recovery-budget gate stays
/// honest because we emit no codes here.
fn rel_brace_length(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACE);
    let _ = p.eat(SyntaxKind::INT_LITERAL);
    if p.eat(SyntaxKind::COMMA) {
        let _ = p.eat(SyntaxKind::INT_LITERAL);
    }
    let _ = p.eat(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::REL_LENGTH);
}

/// `'[' RelDetail? ']'` — contents mirror `NodePattern`'s inner trio but
/// without the outer parens. Variable-length hops (`*m..n`) land later.
fn rel_detail(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_BRACK));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACK);

    // Optional name binder.
    if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
        name_binder(p);
    }
    // Optional type expression: `:Type` (pipe disjunction deferred).
    if p.at(SyntaxKind::COLON) {
        rel_type_expr(p);
    }
    // Optional variable-length quantifier: `*`, `*n`, `*n..`, `*n..m`, `*..m`.
    if p.at(SyntaxKind::STAR) {
        rel_length(p);
    }
    // Optional property map.
    if p.at(SyntaxKind::L_BRACE) {
        property_map(p);
    }

    if !p.eat(SyntaxKind::R_BRACK) {
        p.error_code(
            sc::EXPECTED_RBRACK_REL,
            "expected ']' to close relationship detail",
        );
    }
    m.complete(p, SyntaxKind::REL_DETAIL);
}

/// Label / rel-type names are (per Cypher) unrestricted identifiers — any
/// keyword spelling is a valid label. Accept `IDENT` / `QUOTED_IDENT` and any
/// token whose kind is in the keyword zone; the concrete token kind still
/// round-trips in the CST so diagnostic-producing passes keep the span.
fn eat_label_or_type_name(p: &mut Parser<'_>) -> bool {
    if p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT) {
        return true;
    }
    if p.current().is_keyword() {
        p.bump_any();
        return true;
    }
    false
}

/// `(':' IDENT)+`
fn label_expr(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::COLON));
    let m = p.start();
    while p.at(SyntaxKind::COLON) {
        p.bump(SyntaxKind::COLON);
        if !eat_label_or_type_name(p) {
            p.error_code(sc::EXPECTED_LABEL, "expected label after ':'");
            break;
        }
    }
    m.complete(p, SyntaxKind::LABEL_EXPR);
}

/// `RangeHops = '*' (IntLiteral ('..' IntLiteral?)?)? | '*' '..' IntLiteral`
/// — the variable-length hop quantifier inside `REL_DETAIL`. Spec
/// `cypher.ungrammar` `RangeHops`; emitted as `REL_LENGTH`.
fn rel_length(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::STAR));
    let m = p.start();
    p.bump(SyntaxKind::STAR);
    // Three shapes: `*`, `*n ...`, `*.. m`.
    if p.at(SyntaxKind::INT_LITERAL) {
        p.bump(SyntaxKind::INT_LITERAL);
        if p.at(SyntaxKind::DOT_DOT) {
            p.bump(SyntaxKind::DOT_DOT);
            // Upper bound is optional.
            p.eat(SyntaxKind::INT_LITERAL);
        }
    } else if p.at(SyntaxKind::DOT_DOT) {
        p.bump(SyntaxKind::DOT_DOT);
        p.eat(SyntaxKind::INT_LITERAL);
    }
    m.complete(p, SyntaxKind::REL_LENGTH);
}

/// `':' IDENT` — rel-type expression. Pipe disjunction (`A|B`) is
/// deferred per cy-nom scope.
fn rel_type_expr(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::COLON));
    let m = p.start();
    p.bump(SyntaxKind::COLON);
    if !eat_label_or_type_name(p) {
        p.error_code(
            sc::EXPECTED_REL_TYPE,
            "expected relationship type after ':'",
        );
    }
    // cy-nom: v1 scope — `A|B` rel-type disjunction lands in a follow-up bead.
    m.complete(p, SyntaxKind::REL_TYPE_EXPR);
}

/// `'{' (key ':' Expr (',' key ':' Expr)*)? '}'`
fn property_map(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACE);

    if !p.at(SyntaxKind::R_BRACE) {
        property_kv(p);
        while p.at(SyntaxKind::COMMA) {
            p.bump(SyntaxKind::COMMA);
            property_kv(p);
        }
    }

    if !p.eat(SyntaxKind::R_BRACE) {
        p.error_code(
            sc::EXPECTED_RBRACE_PROP,
            "expected '}' to close property map",
        );
    }
    m.complete(p, SyntaxKind::PROPERTY_MAP);
}

fn property_kv(p: &mut Parser<'_>) {
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_PROP_KEY, "expected property key");
    }
    if !p.eat(SyntaxKind::COLON) {
        p.error_code(sc::EXPECTED_COLON_PROP, "expected ':' in property entry");
    }
    if expression::expr(p).is_none() {
        p.error_code(
            sc::EXPECTED_PROP_VALUE,
            "expected expression for property value",
        );
    }
}

/// A plain `IDENT` / `QUOTED_IDENT` wrapped in a `NAME` node, for binders that
/// live inside patterns.
fn name_binder(p: &mut Parser<'_>) {
    let m = p.start();
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(sc::EXPECTED_IDENT, "expected identifier");
    }
    m.complete(p, SyntaxKind::NAME);
}
