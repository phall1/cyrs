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
use crate::parser::Parser;

use super::expression;

/// `Pattern (',' Pattern)*`
pub(crate) fn pattern_list(p: &mut Parser<'_>) {
    pattern(p);
    while p.at(SyntaxKind::COMMA) {
        p.bump(SyntaxKind::COMMA);
        pattern(p);
    }
}

/// cy-nom scope: `Pattern = PathPattern`. Path binders (`p = ...`),
/// shortestPath / allShortestPaths land in a follow-up bead.
pub(crate) fn pattern(p: &mut Parser<'_>) {
    let m = p.start();
    path_pattern(p);
    m.complete(p, SyntaxKind::PATTERN);
}

/// `PathPattern = NodePattern (RelPattern NodePattern)*`
fn path_pattern(p: &mut Parser<'_>) {
    let m = p.start();
    if !p.at(SyntaxKind::L_PAREN) {
        p.error("expected '(' to start a node pattern");
        m.complete(p, SyntaxKind::PATTERN_PART);
        return;
    }
    node_pattern(p);
    while at_rel_start(p) {
        rel_pattern(p);
        if p.at(SyntaxKind::L_PAREN) {
            node_pattern(p);
        } else {
            p.error("expected node pattern after relationship");
            break;
        }
    }
    m.complete(p, SyntaxKind::PATTERN_PART);
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
        p.error("expected ')' to close node pattern");
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
            p.error("expected '-' at relationship start");
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
            p.error("expected '-' to close relationship");
        }
    } else if !(p.eat(SyntaxKind::ARROW_R) || p.eat(SyntaxKind::MINUS)) {
        p.error("expected '-' or '->' to close relationship");
    }

    m.complete(p, SyntaxKind::REL_PATTERN);
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
    // cy-nom: v1 scope — variable-length hops (`*m..n`) land in a follow-up bead.
    // Optional property map.
    if p.at(SyntaxKind::L_BRACE) {
        property_map(p);
    }

    if !p.eat(SyntaxKind::R_BRACK) {
        p.error("expected ']' to close relationship detail");
    }
    m.complete(p, SyntaxKind::REL_DETAIL);
}

/// `(':' IDENT)+`
fn label_expr(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::COLON));
    let m = p.start();
    while p.at(SyntaxKind::COLON) {
        p.bump(SyntaxKind::COLON);
        if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
            p.error("expected label after ':'");
            break;
        }
    }
    m.complete(p, SyntaxKind::LABEL_EXPR);
}

/// `':' IDENT` — rel-type expression. Pipe disjunction (`A|B`) is
/// deferred per cy-nom scope.
fn rel_type_expr(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::COLON));
    let m = p.start();
    p.bump(SyntaxKind::COLON);
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error("expected relationship type after ':'");
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
        p.error("expected '}' to close property map");
    }
    m.complete(p, SyntaxKind::PROPERTY_MAP);
}

fn property_kv(p: &mut Parser<'_>) {
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error("expected property key");
    }
    if !p.eat(SyntaxKind::COLON) {
        p.error("expected ':' in property entry");
    }
    if expression::expr(p).is_none() {
        p.error("expected expression for property value");
    }
}

/// A plain `IDENT` / `QUOTED_IDENT` wrapped in a `NAME` node, for binders that
/// live inside patterns.
fn name_binder(p: &mut Parser<'_>) {
    let m = p.start();
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error("expected identifier");
    }
    m.complete(p, SyntaxKind::NAME);
}
