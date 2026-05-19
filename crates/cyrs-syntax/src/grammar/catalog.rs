//! GQL catalog-DDL productions — `CREATE GRAPH` / `CREATE SCHEMA`
//! (ISO/IEC 39075:2024 §14.14 / §14.15; cy-rgqg).
//!
//! Catalog-DDL statements sit at top-level statement scope alongside
//! the query `Statement` production: they are dispatched from
//! [`statement::statement`] when the leading tokens are `CREATE GRAPH`
//! or `CREATE SCHEMA`. The five surface forms exercised by the
//! `OpenGQL` upstream samples are all covered here:
//!
//! ```text
//!   CREATE GRAPH name ::graphTypeName              -- double-colon ref
//!   CREATE GRAPH name TYPED graphTypeName          -- lexical ref
//!   CREATE GRAPH name ::{ ( … ) }                  -- inline ref
//!   CREATE GRAPH name ANY                          -- open graph
//!   CREATE GRAPH name { ( … ) }                    -- inline literal
//!   CREATE GRAPH name graphTypeName                -- bare-ident ref
//!   CREATE GRAPH /name LIKE /src                   -- path + LIKE source
//!   CREATE GRAPH name ANY AS COPY OF src           -- ANY + AS COPY OF
//!   CREATE GRAPH name { … } AS COPY OF src         -- inline + AS COPY OF
//!   CREATE SCHEMA /path                            -- catalog schema
//! ```
//!
//! `NEXT` acts as a statement separator (§14.14, peer to `;`); it is
//! consumed by [`super::source_file`] in the top-level loop.
//!
//! Dialect-gate, HIR-lowering, and sema-side catalog-op recording are
//! deferred to follow-up beads — the parser only commits to producing
//! a well-shaped CST so downstream layers can light up incrementally.
//! (Per the cy-rgqg dispatch prompt the §0 amendment authorises HIR /
//! sema work, but the v0 acceptance criterion is parser-acceptance, so
//! it is intentionally scoped here.)

use crate::SyntaxKind;
use crate::parser::{Parser, syntax_codes as sc};

/// True iff the parser is positioned at a `CREATE GRAPH` or `CREATE
/// SCHEMA` opener — used by [`super::statement::statement`] to decide
/// whether to dispatch into a catalog-DDL statement vs the normal
/// `SingleQuery` path.
pub(crate) fn at_catalog_create(p: &Parser<'_>) -> bool {
    if !p.at(SyntaxKind::CREATE_KW) {
        return false;
    }
    matches!(p.nth(1), SyntaxKind::GRAPH_KW | SyntaxKind::SCHEMA_KW)
}

/// Dispatch a top-level catalog-DDL statement. Caller guarantees
/// [`at_catalog_create`].
pub(crate) fn catalog_statement(p: &mut Parser<'_>) {
    debug_assert!(at_catalog_create(p));
    match p.nth(1) {
        SyntaxKind::GRAPH_KW => create_graph_stmt(p),
        SyntaxKind::SCHEMA_KW => create_schema_stmt(p),
        _ => unreachable!("at_catalog_create guarantees GRAPH/SCHEMA at nth(1)"),
    }
}

/// `CreateGraphStmt = 'CREATE' 'GRAPH' GraphName GraphTypeRef? GraphSource?`
fn create_graph_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(SyntaxKind::CREATE_KW);
    p.bump(SyntaxKind::GRAPH_KW);
    graph_name(p);
    if at_graph_type_ref(p) {
        graph_type_ref(p);
    }
    if at_graph_source(p) {
        graph_source(p);
    }
    m.complete(p, SyntaxKind::CATALOG_CREATE_GRAPH_STMT);
}

/// `CreateSchemaStmt = 'CREATE' 'SCHEMA' PathIdent`
fn create_schema_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(SyntaxKind::CREATE_KW);
    p.bump(SyntaxKind::SCHEMA_KW);
    // Schemas are always introduced by a path ident per the `OpenGQL`
    // sample shapes (`CREATE SCHEMA /foo/myschema`). Recovery: if we
    // see a bare IDENT instead, accept it anyway under `GRAPH_NAME` so
    // the statement still completes.
    if p.at(SyntaxKind::SLASH) {
        path_ident(p);
    } else if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
        let m_name = p.start();
        p.bump_any();
        m_name.complete(p, SyntaxKind::GRAPH_NAME);
    } else {
        p.error_code(sc::EXPECTED_IDENT, "expected schema path or identifier");
    }
    m.complete(p, SyntaxKind::CATALOG_CREATE_SCHEMA_STMT);
}

/// `GraphName = PathIdent | IDENT | QUOTED_IDENT`
fn graph_name(p: &mut Parser<'_>) {
    let m = p.start();
    if p.at(SyntaxKind::SLASH) {
        path_ident(p);
    } else if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
        p.bump_any();
    } else {
        p.error_code(
            sc::EXPECTED_IDENT,
            "expected graph name (identifier or `/path`)",
        );
    }
    m.complete(p, SyntaxKind::GRAPH_NAME);
}

/// `PathIdent = ('/' IDENT)+` — used by `CREATE SCHEMA /path`.
fn path_ident(p: &mut Parser<'_>) {
    let m = p.start();
    path_ident_inner(p);
    m.complete(p, SyntaxKind::PATH_IDENT);
}

/// Helper: bump `/ IDENT (/ IDENT)*` into the current node without
/// wrapping. Caller decides which node kind owns the segments.
fn path_ident_inner(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::SLASH));
    while p.at(SyntaxKind::SLASH) {
        p.bump(SyntaxKind::SLASH);
        if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
            p.bump_any();
        } else {
            p.error_code(sc::EXPECTED_IDENT, "expected identifier after `/`");
            break;
        }
    }
}

/// True iff the parser is positioned at the start of a
/// `GraphTypeRef`. Used to gate the optional graph-type slot in
/// `CREATE GRAPH name [GraphTypeRef] [GraphSource]`.
///
/// The bare-ident form (`CREATE GRAPH name mygraphtype`) is ambiguous
/// with the contextual `LIKE` graph-source opener at the lexical
/// level, so we explicitly exclude `LIKE` here — the caller's
/// [`at_graph_source`] guard takes over for that IDENT instead.
fn at_graph_type_ref(p: &Parser<'_>) -> bool {
    if matches!(
        p.current(),
        SyntaxKind::DOUBLE_COLON | SyntaxKind::TYPED_KW | SyntaxKind::ANY_KW | SyntaxKind::L_BRACE
    ) {
        return true;
    }
    if (p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT)) && !p.at_contextual("LIKE") {
        return true;
    }
    false
}

/// ```text
/// GraphTypeRef =
///     '::' (InlineGraphType | IDENT)
///   | 'TYPED' IDENT
///   | 'ANY'
///   | InlineGraphType
///   | IDENT
/// ```
fn graph_type_ref(p: &mut Parser<'_>) {
    let m = p.start();
    match p.current() {
        SyntaxKind::DOUBLE_COLON => {
            p.bump(SyntaxKind::DOUBLE_COLON);
            if p.at(SyntaxKind::L_BRACE) {
                inline_graph_type(p);
            } else if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
                p.bump_any();
            } else {
                p.error_code(
                    sc::EXPECTED_IDENT,
                    "expected graph-type name or inline literal after `::`",
                );
            }
        }
        SyntaxKind::TYPED_KW => {
            p.bump(SyntaxKind::TYPED_KW);
            if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
                p.bump_any();
            } else {
                p.error_code(sc::EXPECTED_IDENT, "expected graph-type name after `TYPED`");
            }
        }
        SyntaxKind::ANY_KW => {
            p.bump(SyntaxKind::ANY_KW);
        }
        SyntaxKind::L_BRACE => {
            inline_graph_type(p);
        }
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
            p.bump_any();
        }
        _ => {
            p.error_code(
                sc::EXPECTED_IDENT,
                "expected graph type (`ANY`, `::name`, `TYPED name`, inline `{…}`, or bare name)",
            );
        }
    }
    m.complete(p, SyntaxKind::GRAPH_TYPE_REF);
}

/// ```text
/// InlineGraphType =
///     '{' GraphTypeElement (',' GraphTypeElement)* '}'
/// ```
///
/// Each element is a paren-wrapped node-shape declaration
/// `( IDENT? LabelExpr? PropertyTypeMap? )`. Edge-shape declarations
/// (`-[:KNOWS { since DATE }]->`) are accepted via permissive
/// brace-balanced recovery so that more elaborate samples still
/// parse — extending the strict node-only form is a follow-up.
fn inline_graph_type(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACE);
    if !p.at(SyntaxKind::R_BRACE) {
        graph_type_element(p);
        while p.eat(SyntaxKind::COMMA) {
            if p.at(SyntaxKind::R_BRACE) {
                break;
            }
            graph_type_element(p);
        }
    }
    if !p.eat(SyntaxKind::R_BRACE) {
        p.error_code(
            sc::EXPECTED_RBRACE_PROP,
            "expected `}` to close inline graph type",
        );
    }
    m.complete(p, SyntaxKind::INLINE_GRAPH_TYPE);
}

/// ```text
/// GraphTypeElement =
///     '(' Binder? LabelList? PropertyTypeMap? ')'
///
///   Binder         = IDENT | QUOTED_IDENT       -- at most one
///   LabelList      = (':' IDENT)+               -- when present
///   PropertyTypeMap = '{' (PropertyTypeDecl (',' PropertyTypeDecl)*)? '}'
/// ```
///
/// --- cy-dxu9 graph-type tightening ---
///
/// Tightened from the original cy-rgqg permissive `(IDENT? (:IDENT)* {…}?)`
/// shape to enforce ISO/IEC 39075:2024 §14.14 / §18.2 ordering
/// (`nodeTypePattern → LEFT_PAREN localNodeTypeAlias? nodeTypeFiller?
/// RIGHT_PAREN`). The strict-order constraints are:
///
///   1. **At most one binder.** A bare IDENT may appear at most once and
///      only before any `:Label`. A second bare IDENT (e.g.
///      `(City Foo :Bar)`) is rejected as binder-after-label.
///   2. **Labels must follow `:`.** Naked label tokens (e.g.
///      `(City Person)` parsed as binder + would-be-label) are caught by
///      rule 1; a `:` with no IDENT after it is reported as
///      `EXPECTED_LABEL`.
///   3. **Binder before labels.** Once any `:Label` has been consumed, a
///      subsequent IDENT (not preceded by `:`) is binder-after-label and
///      is rejected.
///   4. **Property-type map is braced and well-formed.** Only `{…}` opens
///      a property-type map — a bare `prop TYPE` outside braces is
///      rejected.
///
/// Anything that does not start with `(` is consumed up to the next `,`
/// or `}` as an ERROR run so recovery stays bounded.
///
/// Edge-shape elements (`-[:KNOWS]->` / endpoint-pair phrases per
/// ISO §18.3) are **not** accepted here — the five `OpenGQL` upstream
/// samples that use the inline graph-type literal only exercise node
/// shapes (`(Binder :Label { … })`). Edge-element support is a
/// follow-up (tracked under cy-rgqg's "more elaborate samples" TODO).
fn graph_type_element(p: &mut Parser<'_>) {
    if !p.at(SyntaxKind::L_PAREN) {
        // Recovery: skip to the next comma or closing brace.
        let m = p.start();
        p.error_code(
            sc::EXPECTED_LPAREN_NODE,
            "expected `(` to start a graph-type element",
        );
        while !matches!(
            p.current(),
            SyntaxKind::COMMA | SyntaxKind::R_BRACE | SyntaxKind::EOF
        ) {
            p.bump_any();
        }
        m.complete(p, SyntaxKind::ERROR);
        return;
    }
    let m = p.start();
    p.bump(SyntaxKind::L_PAREN);

    // ISO §14.14: ( binder? label* propertyTypeMap? ) — strict order.
    let mut have_binder = false;
    let mut have_label = false;
    loop {
        match p.current() {
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                if have_label {
                    // Binder after labels — reject without consuming so the
                    // outer recovery skips to `)` / `,` / `}`.
                    p.error_code(
                        sc::EXPECTED_RPAREN_NODE,
                        "binder identifier must precede any `:Label` in a graph-type element",
                    );
                    break;
                }
                if have_binder {
                    // Second bare IDENT — only one binder is allowed.
                    p.error_code(
                        sc::EXPECTED_RPAREN_NODE,
                        "graph-type element may have at most one binder identifier",
                    );
                    break;
                }
                p.bump_any();
                have_binder = true;
            }
            SyntaxKind::COLON => {
                p.bump(SyntaxKind::COLON);
                if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
                    p.bump_any();
                    have_label = true;
                } else {
                    p.error_code(sc::EXPECTED_LABEL, "expected label after `:`");
                    break;
                }
            }
            _ => break,
        }
    }

    if p.at(SyntaxKind::L_BRACE) {
        property_type_map(p);
    }

    // Bounded recovery: consume any stray tokens up to `)` / `,` / `}` /
    // EOF so a malformed element doesn't desync the outer brace loop.
    while !matches!(
        p.current(),
        SyntaxKind::R_PAREN | SyntaxKind::COMMA | SyntaxKind::R_BRACE | SyntaxKind::EOF
    ) {
        p.bump_any();
    }

    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_NODE,
            "expected `)` to close graph-type element",
        );
    }
    m.complete(p, SyntaxKind::GRAPH_TYPE_ELEMENT);
}
// --- end cy-dxu9 ---

/// `PropertyTypeMap = '{' PropertyTypeDecl (',' PropertyTypeDecl)* '}'`
fn property_type_map(p: &mut Parser<'_>) {
    debug_assert!(p.at(SyntaxKind::L_BRACE));
    let m = p.start();
    p.bump(SyntaxKind::L_BRACE);
    if !p.at(SyntaxKind::R_BRACE) {
        property_type_decl(p);
        while p.eat(SyntaxKind::COMMA) {
            if p.at(SyntaxKind::R_BRACE) {
                break;
            }
            property_type_decl(p);
        }
    }
    if !p.eat(SyntaxKind::R_BRACE) {
        p.error_code(
            sc::EXPECTED_RBRACE_PROP,
            "expected `}` to close property-type map",
        );
    }
    m.complete(p, SyntaxKind::PROPERTY_TYPE_MAP);
}

/// `PropertyTypeDecl = PropertyKey TypeName`
///
/// The type name is one or two IDENT tokens (`STRING`, `DATE`,
/// `INTEGER`, `ZONED DATETIME`, …). Two-word type names are common
/// in ISO/IEC 39075:2024 §6.2 (`ZONED DATETIME`, `LOCAL TIMESTAMP`,
/// `LIST OF INT`), so we tolerantly consume one or two leading IDENTs
/// before the next `,` or `}`.
fn property_type_decl(p: &mut Parser<'_>) {
    let m = p.start();
    if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
        p.bump_any();
    } else {
        p.error_code(sc::EXPECTED_PROP_KEY, "expected property key");
        m.complete(p, SyntaxKind::PROPERTY_TYPE_DECL);
        return;
    }
    // One or more type-name IDENTs (e.g. `STRING`, `ZONED DATETIME`,
    // `LIST OF INT`). Stop at the entry separator / map closer.
    let mut consumed_type_word = false;
    while matches!(
        p.current(),
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT | SyntaxKind::OF_KW
    ) {
        p.bump_any();
        consumed_type_word = true;
    }
    if !consumed_type_word {
        p.error_code(sc::EXPECTED_IDENT, "expected type name after property key");
    }
    m.complete(p, SyntaxKind::PROPERTY_TYPE_DECL);
}

/// True iff the parser is positioned at a `GraphSource` opener.
///
/// `LIKE` is contextual: it stays an IDENT in the lexer because
/// openCypher TCK queries spell `-[:LIKE …]->` as a relationship
/// type. We only treat it as the graph-source opener at this
/// catalog-DDL position via [`Parser::at_contextual`].
fn at_graph_source(p: &Parser<'_>) -> bool {
    p.at(SyntaxKind::AS_KW) || p.at_contextual("LIKE")
}

/// `GraphSource = 'LIKE' GraphName | 'AS' 'COPY' 'OF' IDENT`
fn graph_source(p: &mut Parser<'_>) {
    let m = p.start();
    if p.at(SyntaxKind::AS_KW) {
        p.bump(SyntaxKind::AS_KW);
        if !p.eat(SyntaxKind::COPY_KW) {
            p.error_code(sc::EXPECTED_IDENT, "expected `COPY` after `AS`");
        }
        if !p.eat(SyntaxKind::OF_KW) {
            p.error_code(sc::EXPECTED_IDENT, "expected `OF` after `AS COPY`");
        }
        if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
            let m_name = p.start();
            p.bump_any();
            m_name.complete(p, SyntaxKind::GRAPH_NAME);
        } else if p.at(SyntaxKind::SLASH) {
            graph_name(p);
        } else {
            p.error_code(
                sc::EXPECTED_IDENT,
                "expected source graph name after `AS COPY OF`",
            );
        }
    } else {
        // `LIKE …` (contextual IDENT). Consume the IDENT explicitly so
        // we don't accidentally re-trigger the keyword path.
        debug_assert!(p.current() == SyntaxKind::IDENT);
        p.bump_any();
        graph_name(p);
    }
    m.complete(p, SyntaxKind::GRAPH_SOURCE);
}

#[cfg(test)]
mod tests {
    //! Catalog-DDL parser smoke tests. The acceptance criterion for
    //! cy-rgqg is the `OpenGQL`-samples baseline (a TCK harness assertion
    //! in `crates/cyrs-tck/tests/opengql_samples.rs`); these unit tests
    //! lock in the per-shape behaviour so a future regression surfaces
    //! at the cyrs-syntax layer instead of only the tck baseline.
    use crate::{SyntaxKind, SyntaxNode, parse};

    fn assert_ok(src: &str) -> SyntaxNode {
        let p = parse(src);
        assert_eq!(
            p.syntax().to_string(),
            src,
            "lossless round-trip failed for {src:?}",
        );
        assert!(
            p.errors().is_empty(),
            "unexpected errors parsing {src:?}: {:?}",
            p.errors(),
        );
        p.syntax()
    }

    fn has_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
        node.descendants().any(|n| n.kind() == kind)
    }

    #[test]
    fn create_graph_any() {
        let n = assert_ok("CREATE GRAPH mygraph ANY");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_GRAPH_STMT));
        assert!(has_kind(&n, SyntaxKind::GRAPH_TYPE_REF));
        assert!(has_kind(&n, SyntaxKind::GRAPH_NAME));
    }

    #[test]
    fn create_graph_double_colon_named_type() {
        let n = assert_ok("CREATE GRAPH mySocialNetwork ::socialNetworkGraphType");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_GRAPH_STMT));
        assert!(has_kind(&n, SyntaxKind::GRAPH_TYPE_REF));
    }

    #[test]
    fn create_graph_typed_named_type() {
        let n = assert_ok("CREATE GRAPH mySocialNetwork TYPED socialNetworkGraphType");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_GRAPH_STMT));
        assert!(has_kind(&n, SyntaxKind::GRAPH_TYPE_REF));
    }

    #[test]
    fn create_graph_inline_double_colon() {
        let src = "CREATE GRAPH mySocialNetwork ::{(City :City {name STRING, state STRING, country STRING})}";
        let n = assert_ok(src);
        assert!(has_kind(&n, SyntaxKind::INLINE_GRAPH_TYPE));
        assert!(has_kind(&n, SyntaxKind::GRAPH_TYPE_ELEMENT));
        assert!(has_kind(&n, SyntaxKind::PROPERTY_TYPE_MAP));
        assert!(has_kind(&n, SyntaxKind::PROPERTY_TYPE_DECL));
    }

    #[test]
    fn create_graph_inline_literal() {
        let src = "CREATE GRAPH mygraph {\n  (Person :Person {lastname STRING, firstname STRING,joined DATE})\n}";
        let n = assert_ok(src);
        assert!(has_kind(&n, SyntaxKind::INLINE_GRAPH_TYPE));
    }

    #[test]
    fn create_graph_bare_typename() {
        assert_ok("CREATE GRAPH mygraph mygraphtype");
    }

    #[test]
    fn create_graph_path_like_source() {
        let n = assert_ok("CREATE GRAPH /mygraph LIKE /mysrcgraph");
        assert!(has_kind(&n, SyntaxKind::PATH_IDENT));
        assert!(has_kind(&n, SyntaxKind::GRAPH_SOURCE));
    }

    #[test]
    fn create_graph_as_copy_of() {
        let n = assert_ok("CREATE GRAPH mygraph ANY AS COPY OF mysrcgraph");
        assert!(has_kind(&n, SyntaxKind::GRAPH_SOURCE));
    }

    #[test]
    fn create_graph_inline_as_copy_of() {
        let src =
            "CREATE GRAPH mygraph {\n  (Person :Person {lastname STRING})\n} AS COPY OF mysrcgraph";
        let n = assert_ok(src);
        assert!(has_kind(&n, SyntaxKind::INLINE_GRAPH_TYPE));
        assert!(has_kind(&n, SyntaxKind::GRAPH_SOURCE));
    }

    #[test]
    fn create_schema_path() {
        let n = assert_ok("CREATE SCHEMA /myschema");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_SCHEMA_STMT));
        assert!(has_kind(&n, SyntaxKind::PATH_IDENT));
    }

    #[test]
    fn create_schema_multi_level_path() {
        let n = assert_ok("CREATE SCHEMA /foo/myschema");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_SCHEMA_STMT));
    }

    #[test]
    fn create_schema_next_separator() {
        // `NEXT` is a top-level statement separator (ISO/IEC 39075:2024
        // §14.14) — peer to `;`.
        let n = assert_ok("CREATE SCHEMA /foo\nNEXT CREATE SCHEMA /fee");
        let count = n
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::CATALOG_CREATE_SCHEMA_STMT)
            .count();
        assert_eq!(count, 2, "expected two CREATE SCHEMA stmts joined by NEXT");
    }

    // --- cy-dxu9 graph-type tightening ---
    fn assert_err_contains(src: &str, needle: &str) {
        let p = parse(src);
        assert!(
            p.syntax().to_string() == src,
            "lossless round-trip failed for {src:?}",
        );
        let errs = p.errors();
        assert!(
            !errs.is_empty(),
            "expected at least one parse error for {src:?}",
        );
        assert!(
            errs.iter().any(|e| e.message.contains(needle)),
            "expected error containing {needle:?} for {src:?}, got {errs:?}",
        );
    }

    #[test]
    fn graph_type_element_rejects_binder_after_label() {
        // `(Foo :Bar Baz)` — second IDENT is binder-after-label.
        assert_err_contains(
            "CREATE GRAPH g { (Foo :Bar Baz) }",
            "binder identifier must precede any `:Label`",
        );
    }

    #[test]
    fn graph_type_element_rejects_two_binders() {
        // `(Foo Bar)` — two bare IDENTs.
        assert_err_contains(
            "CREATE GRAPH g { (Foo Bar) }",
            "may have at most one binder identifier",
        );
    }

    #[test]
    fn graph_type_element_rejects_label_without_ident() {
        // `(Foo :)` — trailing `:` with no label IDENT.
        assert_err_contains("CREATE GRAPH g { (Foo :) }", "expected label after `:`");
    }

    #[test]
    fn graph_type_element_accepts_labels_only() {
        // `(:City)` — no binder, single label. ISO permits this.
        assert_ok("CREATE GRAPH g { (:City) }");
    }

    #[test]
    fn graph_type_element_accepts_multiple_labels() {
        // `(p :Person :Employee)` — multi-label is permitted by ISO.
        assert_ok("CREATE GRAPH g { (p :Person :Employee) }");
    }

    #[test]
    fn graph_type_element_accepts_empty_parens() {
        // `()` — anonymous, untyped element. ISO permits via the
        // all-optional `localNodeTypeAlias? nodeTypeFiller?` rule.
        assert_ok("CREATE GRAPH g { () }");
    }
    // --- end cy-dxu9 ---

    #[test]
    fn match_returns_with_like_reltype_still_parses() {
        // Regression: openCypher TCK queries spell `LIKE` as a
        // relationship type. The catalog grammar's contextual `LIKE`
        // recognition must not steal IDENT spellings in pattern
        // context. Spec §0 amendment 2026-05-19 (cy-5e3f) calls this
        // out explicitly.
        assert_ok("CREATE (a)-[:LIKE]->(b)");
    }

    // ---------------------------------------------------------------
    // Error-recovery / additional shape coverage (cy-m2hz).
    //
    // The smoke tests above lock the happy paths; the cases below
    // exercise the diagnostic branches in graph_name / graph_type_ref /
    // graph_source / inline_graph_type / property_type_decl /
    // path_ident_inner so a future refactor doesn't silently drop
    // error reporting.
    // ---------------------------------------------------------------

    fn parse_with_errors(src: &str) -> (SyntaxNode, Vec<u16>) {
        let p = parse(src);
        let codes: Vec<u16> = p.errors().iter().map(|e| e.code).collect();
        // Lossless round-trip is mandatory even on error paths.
        assert_eq!(p.syntax().to_string(), src);
        (p.syntax(), codes)
    }

    #[test]
    fn create_graph_quoted_ident_name() {
        // QUOTED_IDENT graph name + QUOTED_IDENT type ref via TYPED.
        let n = assert_ok("CREATE GRAPH `my graph` TYPED `my type`");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_GRAPH_STMT));
    }

    #[test]
    fn create_graph_double_colon_quoted_type() {
        // `::QUOTED_IDENT` branch of graph_type_ref.
        assert_ok("CREATE GRAPH g ::`graph type`");
    }

    #[test]
    fn create_graph_double_colon_inline() {
        // `::{ ... }` branch — DOUBLE_COLON + L_BRACE.
        assert_ok("CREATE GRAPH g ::{(Person {name STRING})}");
    }

    #[test]
    fn create_graph_inline_multi_element() {
        // Comma-separated elements inside InlineGraphType — exercises
        // the loop after the first element.
        assert_ok("CREATE GRAPH g {(A {x STRING}),(B {y INTEGER})}");
    }

    #[test]
    fn create_graph_inline_trailing_comma() {
        // Trailing comma before `}` — break-on-R_BRACE inside the loop.
        assert_ok("CREATE GRAPH g {(A {x STRING}),}");
    }

    #[test]
    fn create_graph_property_type_two_word() {
        // Multi-word type name (`ZONED DATETIME`) — drives the
        // type-name continuation loop in property_type_decl.
        assert_ok("CREATE GRAPH g {(A {ts ZONED DATETIME})}");
    }

    #[test]
    fn create_graph_property_type_list_of_int() {
        // Three-token type name via `OF_KW`.
        assert_ok("CREATE GRAPH g {(A {xs LIST OF INT})}");
    }

    #[test]
    fn create_schema_missing_name_errors() {
        // Schema name absent — the IDENT/SLASH branch in
        // create_schema_stmt should fire EXPECTED_IDENT.
        let (n, codes) = parse_with_errors("CREATE SCHEMA");
        assert!(has_kind(&n, SyntaxKind::CATALOG_CREATE_SCHEMA_STMT));
        assert!(!codes.is_empty(), "expected at least one error code");
    }

    #[test]
    fn create_graph_missing_name_errors() {
        // `CREATE GRAPH` without a following name — graph_name's
        // error_code branch is exercised.
        let (_, codes) = parse_with_errors("CREATE GRAPH");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_bad_type_ref_after_double_colon() {
        // `::` followed by something other than IDENT / L_BRACE.
        let (_, codes) = parse_with_errors("CREATE GRAPH g ::42");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_typed_missing_name() {
        // `TYPED` not followed by IDENT.
        let (_, codes) = parse_with_errors("CREATE GRAPH g TYPED 42");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_as_missing_copy() {
        // `AS` not followed by `COPY`.
        let (_, codes) = parse_with_errors("CREATE GRAPH g ANY AS X Y src");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_as_copy_missing_of() {
        // `AS COPY` not followed by `OF`.
        let (_, codes) = parse_with_errors("CREATE GRAPH g ANY AS COPY src");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_as_copy_of_path_source() {
        // `AS COPY OF /path` — SLASH branch of the source-name dispatch.
        let n = assert_ok("CREATE GRAPH g ANY AS COPY OF /src");
        assert!(has_kind(&n, SyntaxKind::GRAPH_SOURCE));
    }

    #[test]
    fn create_graph_as_copy_of_missing_name() {
        // `AS COPY OF` not followed by IDENT or SLASH.
        let (_, codes) = parse_with_errors("CREATE GRAPH g ANY AS COPY OF 42");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_path_name_multi_segment() {
        // Multi-segment path name (e.g. `/foo/bar/baz`) — drives the
        // PATH_IDENT continuation loop.
        let n = assert_ok("CREATE GRAPH /foo/bar/baz ANY");
        assert!(has_kind(&n, SyntaxKind::PATH_IDENT));
    }

    #[test]
    fn create_graph_path_dangling_slash() {
        // `/` not followed by IDENT — error branch of path_ident_inner.
        let (_, codes) = parse_with_errors("CREATE GRAPH /foo/");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_inline_unclosed() {
        // Missing closing `}` on inline literal — error branch of
        // inline_graph_type.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A {x STRING})");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_element_missing_open_paren() {
        // Comma-separated element NOT starting with `(` — drives the
        // ERROR-recovery run inside graph_type_element.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A {x STRING}),BAD}");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_element_missing_close_paren() {
        // Missing `)` on a graph-type element.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A {x STRING}");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_label_missing_after_colon() {
        // `(A:` with nothing after — drives the EXPECTED_LABEL branch.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A: {x STRING})}");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_property_missing_type() {
        // Property key without a following type name — drives
        // EXPECTED_IDENT in property_type_decl.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A {x})}");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_property_missing_key() {
        // Property map opens with something other than IDENT —
        // EXPECTED_PROP_KEY in property_type_decl.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A {: STRING})}");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_property_map_unclosed() {
        // Property-type map missing its closing `}`.
        let (_, codes) = parse_with_errors("CREATE GRAPH g {(A {x STRING)}");
        assert!(!codes.is_empty());
    }

    #[test]
    fn create_graph_inline_empty_body() {
        // `{ }` — empty inline graph type. Exercises the
        // `!p.at(R_BRACE)` short-circuit.
        assert_ok("CREATE GRAPH g {}");
    }

    #[test]
    fn create_graph_property_map_empty() {
        // `(A {})` — empty property-type map.
        assert_ok("CREATE GRAPH g {(A {})}");
    }
}
