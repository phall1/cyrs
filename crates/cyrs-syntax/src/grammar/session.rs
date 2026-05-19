//! GQL `SESSION SET` statements (ISO/IEC 39075:2024 §14.15; cy-9kzx).
//!
//! `SESSION SET` is a top-level statement category disjoint from the
//! query `STATEMENT` (no `Clause+` body, no `UNION` participation).
//! Four sub-forms collapse into one outer `SESSION_SET_STMT` node whose
//! discriminant is the inner variant child:
//!
//! - `SESSION SET GRAPH <graph-ref>` →
//!   [`SyntaxKind::SESSION_SET_GRAPH_VARIANT`].
//! - `SESSION SET PROPERTY GRAPH <graph-ref>` →
//!   [`SyntaxKind::SESSION_SET_GRAPH_VARIANT`] with a leading `PROPERTY`
//!   IDENT child. The `PROPERTY` qualifier is documentation only — both
//!   shapes assign a graph reference to the current session.
//! - `SESSION SET TIME ZONE <string-literal>` →
//!   [`SyntaxKind::SESSION_SET_TIME_ZONE_VARIANT`].
//! - `SESSION SET VALUE [IF NOT EXISTS] $param = <expr>` →
//!   [`SyntaxKind::SESSION_SET_VALUE_VARIANT`].
//!
//! A `<graph-ref>` is `CURRENT_GRAPH`, `CURRENT_PROPERTY_GRAPH`, or any
//! identifier (graph name). All three lex as `IDENT`; recognition is
//! contextual so promoting the names to keywords would not buy
//! anything.
//!
//! Sema is intentionally minimal at this point (well-formedness only);
//! the parser accepts the statement and the typed AST wrapper exposes
//! it for downstream tooling. HIR lowering (`lower_parse` ignores
//! non-`STATEMENT` children of `SOURCE_FILE` today) and Plan IR work
//! are deferred — TODO(cy-9kzx-follow-up): wire a `Hir::SessionSet`
//! variant once the Plan layer wants to observe session-state
//! mutations.

use crate::SyntaxKind;
use crate::parser::{Parser, syntax_codes as sc};

use super::expression;

/// Entry point: parse a `SESSION SET ...` statement. Caller guarantees
/// `p.at(SESSION_KW)`.
pub(crate) fn session_set_stmt(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::SESSION_KW));
    let m = p.start();
    p.bump(SyntaxKind::SESSION_KW);
    if !p.eat(SyntaxKind::SET_KW) {
        p.error_code(sc::EXPECTED_SET_AFTER_SESSION, "expected SET after SESSION");
        m.complete(p, SyntaxKind::SESSION_SET_STMT);
        return;
    }
    dispatch_variant(p);
    m.complete(p, SyntaxKind::SESSION_SET_STMT);
}

/// Dispatch on the first token after `SESSION SET`. Four arms per
/// ISO §14.15: `GRAPH`, `PROPERTY GRAPH`, `TIME ZONE`, `VALUE`.
fn dispatch_variant(p: &mut Parser<'_>) {
    // `GRAPH` is a reserved keyword (cy-rgqg), so check `GRAPH_KW`
    // directly instead of going through `at_contextual`. `PROPERTY` /
    // `TIME` / `VALUE` remain ordinary identifiers — recognised
    // contextually so they don't collide with property keys in
    // openCypher TCK queries.
    if p.at(SyntaxKind::GRAPH_KW) {
        graph_variant(p, /*has_property=*/ false);
    } else if p.at_contextual("PROPERTY") {
        graph_variant(p, /*has_property=*/ true);
    } else if p.at_contextual("TIME") {
        time_zone_variant(p);
    } else if p.at_contextual("VALUE") {
        value_variant(p);
    } else {
        p.error_code(
            sc::EXPECTED_SESSION_SET_TARGET,
            "expected GRAPH, PROPERTY GRAPH, TIME ZONE, or VALUE after SESSION SET",
        );
    }
}

/// `'GRAPH' GraphRef`
///       — or —
/// `'PROPERTY' 'GRAPH' GraphRef`
///
/// The `PROPERTY` qualifier is consumed by the caller's branch; both
/// surface forms emit the same variant kind. The discriminant for
/// downstream consumers is the presence of an IDENT child with
/// case-insensitive text `PROPERTY` immediately after the outer
/// `SET_KW`.
fn graph_variant(p: &mut Parser<'_>, has_property: bool) {
    let m = p.start();
    if has_property {
        debug_assert!(p.at_contextual("PROPERTY"));
        // Consume the `PROPERTY` IDENT (contextual keyword).
        p.bump_any();
        if p.at(SyntaxKind::GRAPH_KW) {
            p.bump(SyntaxKind::GRAPH_KW);
        } else {
            p.error_code(
                sc::EXPECTED_SESSION_SET_TARGET,
                "expected GRAPH after PROPERTY in SESSION SET PROPERTY GRAPH",
            );
        }
    } else {
        debug_assert!(p.at(SyntaxKind::GRAPH_KW));
        p.bump(SyntaxKind::GRAPH_KW);
    }
    graph_ref(p);
    m.complete(p, SyntaxKind::SESSION_SET_GRAPH_VARIANT);
}

/// `GraphRef = 'CURRENT_GRAPH' | 'CURRENT_PROPERTY_GRAPH' | IDENT
///           | QUOTED_IDENT`.
///
/// All three lex as identifiers; we accept any `IDENT` / `QUOTED_IDENT` so
/// the parser is dialect-name-agnostic. Sema can later validate that
/// the name resolves to a known graph reference.
fn graph_ref(p: &mut Parser<'_>) {
    if !(p.eat(SyntaxKind::IDENT) || p.eat(SyntaxKind::QUOTED_IDENT)) {
        p.error_code(
            sc::EXPECTED_GRAPH_REF,
            "expected graph reference (CURRENT_GRAPH, CURRENT_PROPERTY_GRAPH, or name)",
        );
    }
}

/// `'TIME' 'ZONE' STRING_LITERAL`
///
/// `TIME` is consumed by the caller's contextual branch; `ZONE` is a
/// reserved keyword (see lexer); the trailing time-zone is a bare
/// string literal per ISO §14.15.
fn time_zone_variant(p: &mut Parser<'_>) {
    debug_assert!(p.at_contextual("TIME"));
    let m = p.start();
    p.bump_any(); // TIME
    if !p.eat(SyntaxKind::ZONE_KW) {
        p.error_code(
            sc::EXPECTED_ZONE_AFTER_TIME,
            "expected ZONE after TIME in SESSION SET TIME ZONE",
        );
    }
    if !p.eat(SyntaxKind::STRING_LITERAL) {
        p.error_code(
            sc::EXPECTED_TIME_ZONE_LITERAL,
            "expected string literal time zone after TIME ZONE",
        );
    }
    m.complete(p, SyntaxKind::SESSION_SET_TIME_ZONE_VARIANT);
}

/// `'VALUE' ('IF' 'NOT' 'EXISTS')? '$' param '=' Expr`
///
/// The optional `IF NOT EXISTS` guard is recognised contextually (`IF`
/// is not a reserved keyword in v1; `NOT` and `EXISTS` already are).
/// `$param` lexes as a single `PARAM` token. The trailing expression
/// uses the existing GQL expression grammar — when typed temporal
/// literals (cy-51we) land, expressions like `DATE '2022-10-10'` will
/// be accepted for free.
fn value_variant(p: &mut Parser<'_>) {
    debug_assert!(p.at_contextual("VALUE"));
    let m = p.start();
    p.bump_any(); // VALUE
    // Optional `IF NOT EXISTS` guard.
    if p.at_contextual("IF") {
        p.bump_any(); // IF
        if !p.eat(SyntaxKind::NOT_KW) {
            // Tolerate a malformed guard; the value form is still
            // well-defined without IF NOT EXISTS.
            p.error_code(
                sc::EXPECTED_SESSION_SET_TARGET,
                "expected NOT after IF in SESSION SET VALUE IF NOT EXISTS",
            );
        } else if !p.eat(SyntaxKind::EXISTS_KW) {
            p.error_code(
                sc::EXPECTED_SESSION_SET_TARGET,
                "expected EXISTS after NOT in SESSION SET VALUE IF NOT EXISTS",
            );
        }
    }
    if !p.eat(SyntaxKind::PARAM) {
        p.error_code(
            sc::EXPECTED_PARAM_IN_SESSION_VALUE,
            "expected `$param` in SESSION SET VALUE",
        );
    }
    if !p.eat(SyntaxKind::EQ) {
        p.error_code(
            sc::EXPECTED_EQ_IN_SESSION_VALUE,
            "expected `=` in SESSION SET VALUE",
        );
    }
    if expression::expr(p).is_none() {
        p.error_code(
            sc::EXPECTED_SESSION_VALUE_EXPR,
            "expected expression after `=` in SESSION SET VALUE",
        );
    }
    m.complete(p, SyntaxKind::SESSION_SET_VALUE_VARIANT);
}

#[cfg(test)]
mod tests {
    use crate::SyntaxKind;
    use crate::parser::parse;

    fn first_kind_of(src: &str) -> Option<SyntaxKind> {
        let p = parse(src);
        let root = p.syntax();
        root.children().next().map(|c| c.kind())
    }

    #[test]
    fn session_set_graph_parses_cleanly() {
        let p = parse("SESSION SET GRAPH CURRENT_GRAPH");
        assert!(
            p.errors().is_empty(),
            "expected clean parse, got {:?}",
            p.errors()
        );
        assert_eq!(
            first_kind_of("SESSION SET GRAPH g"),
            Some(SyntaxKind::SESSION_SET_STMT)
        );
    }

    #[test]
    fn session_set_property_graph_parses_cleanly() {
        let p = parse("SESSION SET PROPERTY GRAPH CURRENT_PROPERTY_GRAPH");
        assert!(p.errors().is_empty(), "errs: {:?}", p.errors());
    }

    #[test]
    fn session_set_time_zone_parses_cleanly() {
        let p = parse("SESSION SET TIME ZONE \"utc\"");
        assert!(p.errors().is_empty(), "errs: {:?}", p.errors());
    }

    #[test]
    fn session_set_value_parses_cleanly() {
        let p = parse("SESSION SET VALUE IF NOT EXISTS $foo = 42");
        assert!(p.errors().is_empty(), "errs: {:?}", p.errors());
        let p2 = parse("SESSION SET VALUE $bar = 'hi'");
        assert!(p2.errors().is_empty(), "errs: {:?}", p2.errors());
    }

    #[test]
    fn session_set_recovers_from_missing_set() {
        let p = parse("SESSION GRAPH CURRENT_GRAPH");
        // SET missing → one error, but the statement node is still emitted.
        assert!(!p.errors().is_empty());
        assert_eq!(p.errors()[0].code, super::sc::EXPECTED_SET_AFTER_SESSION);
    }

    // ---------------------------------------------------------------
    // Error-recovery coverage (cy-m2hz). Exercises the diagnostic
    // branches in dispatch_variant / graph_variant / time_zone_variant
    // / value_variant so a future refactor can't silently drop the
    // E-codes.
    // ---------------------------------------------------------------

    fn err_codes(src: &str) -> Vec<u16> {
        let p = parse(src);
        // Lossless round-trip even on error paths.
        assert_eq!(p.syntax().to_string(), src);
        p.errors().iter().map(|e| e.code).collect()
    }

    #[test]
    fn session_set_quoted_ident_graph() {
        // Drives the QUOTED_IDENT branch of graph_ref.
        let p = parse("SESSION SET GRAPH `my graph`");
        assert!(p.errors().is_empty(), "errs: {:?}", p.errors());
    }

    #[test]
    fn session_set_property_missing_graph() {
        // `SESSION SET PROPERTY` not followed by `GRAPH`. The
        // EXPECTED_SESSION_SET_TARGET branch fires inside graph_variant.
        let codes = err_codes("SESSION SET PROPERTY foo");
        assert!(codes.contains(&super::sc::EXPECTED_SESSION_SET_TARGET));
    }

    #[test]
    fn session_set_unknown_target() {
        // None of GRAPH / PROPERTY / TIME / VALUE — the catch-all in
        // dispatch_variant.
        let codes = err_codes("SESSION SET FOO bar");
        assert!(codes.contains(&super::sc::EXPECTED_SESSION_SET_TARGET));
    }

    #[test]
    fn session_set_time_missing_zone() {
        // `TIME` not followed by `ZONE`.
        let codes = err_codes("SESSION SET TIME \"utc\"");
        assert!(codes.contains(&super::sc::EXPECTED_ZONE_AFTER_TIME));
    }

    #[test]
    fn session_set_time_zone_missing_literal() {
        // `TIME ZONE` not followed by a string literal.
        let codes = err_codes("SESSION SET TIME ZONE foo");
        assert!(codes.contains(&super::sc::EXPECTED_TIME_ZONE_LITERAL));
    }

    #[test]
    fn session_set_value_missing_not() {
        // `IF` not followed by `NOT`.
        let codes = err_codes("SESSION SET VALUE IF EXISTS $foo = 42");
        assert!(codes.contains(&super::sc::EXPECTED_SESSION_SET_TARGET));
    }

    #[test]
    fn session_set_value_missing_exists() {
        // `IF NOT` not followed by `EXISTS`.
        let codes = err_codes("SESSION SET VALUE IF NOT $foo = 42");
        assert!(codes.contains(&super::sc::EXPECTED_SESSION_SET_TARGET));
    }

    #[test]
    fn session_set_value_missing_param() {
        // Missing `$param` token.
        let codes = err_codes("SESSION SET VALUE = 42");
        assert!(codes.contains(&super::sc::EXPECTED_PARAM_IN_SESSION_VALUE));
    }

    #[test]
    fn session_set_value_missing_eq() {
        // Missing `=` between param and value.
        let codes = err_codes("SESSION SET VALUE $foo 42");
        assert!(codes.contains(&super::sc::EXPECTED_EQ_IN_SESSION_VALUE));
    }

    #[test]
    fn session_set_value_missing_expr() {
        // Missing expression after `=`.
        let codes = err_codes("SESSION SET VALUE $foo =");
        assert!(codes.contains(&super::sc::EXPECTED_SESSION_VALUE_EXPR));
    }

    #[test]
    fn session_set_graph_missing_ref() {
        // Missing the graph reference.
        let codes = err_codes("SESSION SET GRAPH 42");
        assert!(codes.contains(&super::sc::EXPECTED_GRAPH_REF));
    }
}
