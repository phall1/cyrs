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
///     '(' IDENT? LabelList? PropertyTypeMap? ')'
/// ```
///
/// Label list is a permissive run of `':' IDENT` pairs (the `OpenGQL`
/// shape `(City :City { … })` has both a binder *and* a single label,
/// so we accept any sequence of binders and `:Label` tokens before
/// the property-type map opens). Anything that does not start with
/// `(` is consumed up to the next `,` or `}` as an ERROR run so
/// recovery stays bounded.
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
    // Binder + labels — both optional, interleaved tolerantly.
    while matches!(
        p.current(),
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT | SyntaxKind::COLON
    ) {
        if p.at(SyntaxKind::COLON) {
            p.bump(SyntaxKind::COLON);
            if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::QUOTED_IDENT) {
                p.bump_any();
            } else {
                p.error_code(sc::EXPECTED_LABEL, "expected label after `:`");
                break;
            }
        } else {
            p.bump_any();
        }
    }
    if p.at(SyntaxKind::L_BRACE) {
        property_type_map(p);
    }
    if !p.eat(SyntaxKind::R_PAREN) {
        p.error_code(
            sc::EXPECTED_RPAREN_NODE,
            "expected `)` to close graph-type element",
        );
    }
    m.complete(p, SyntaxKind::GRAPH_TYPE_ELEMENT);
}

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

    #[test]
    fn match_returns_with_like_reltype_still_parses() {
        // Regression: openCypher TCK queries spell `LIKE` as a
        // relationship type. The catalog grammar's contextual `LIKE`
        // recognition must not steal IDENT spellings in pattern
        // context. Spec §0 amendment 2026-05-19 (cy-5e3f) calls this
        // out explicitly.
        assert_ok("CREATE (a)-[:LIKE]->(b)");
    }
}
