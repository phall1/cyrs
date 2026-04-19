//! CST snapshot corpus (spec §17.2).
//!
//! Each test renders the rowan tree for a representative query using
//! [`format_cst`] and asserts it against a committed `insta` snapshot.
//! Snapshots are the review artefact; the textual form is chosen for
//! structural clarity:
//!
//! - nodes print as `NODE_KIND@start..end`
//! - tokens print as `TOKEN_KIND@start..end "text"` with the text
//!   debug-quoted so whitespace / newlines are visible
//! - depth is rendered as two-space indent
//!
//! The corpus is organised by grammar area (clauses → patterns →
//! expressions → errors). Constructs that are deferred in cy-nom scope
//! (see `grammar/clause.rs`, `grammar/pattern.rs`, `grammar/expression.rs`)
//! are marked with `TODO(cy-nom): <bead>` comments so a later bead can
//! flip them on without re-deriving the corpus layout.

use std::fmt::Write as _;

use cypher_syntax::{SyntaxNode, parse};
use rowan::{NodeOrToken, WalkEvent};

/// Render a rowan CST as an indented textual tree.
///
/// Format:
/// ```text
/// SOURCE_FILE@0..17
///   MATCH_CLAUSE@0..9
///     MATCH_KW@0..5 "MATCH"
///     ...
/// ```
///
/// Trivia and ERROR tokens are included — the tree is lossless, and the
/// snapshot reflects that.
fn format_cst(node: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut depth: usize = 0;
    for ev in node.preorder_with_tokens() {
        match ev {
            WalkEvent::Enter(NodeOrToken::Node(n)) => {
                for _ in 0..depth {
                    out.push_str("  ");
                }
                let r = n.text_range();
                let start: u32 = r.start().into();
                let end: u32 = r.end().into();
                writeln!(out, "{:?}@{start}..{end}", n.kind()).unwrap();
                depth += 1;
            }
            WalkEvent::Leave(NodeOrToken::Node(_)) => {
                depth -= 1;
            }
            WalkEvent::Enter(NodeOrToken::Token(t)) => {
                for _ in 0..depth {
                    out.push_str("  ");
                }
                let r = t.text_range();
                let start: u32 = r.start().into();
                let end: u32 = r.end().into();
                writeln!(out, "{:?}@{start}..{end} {:?}", t.kind(), t.text()).unwrap();
            }
            WalkEvent::Leave(NodeOrToken::Token(_)) => {}
        }
    }
    out
}

/// Append any parse errors to the snapshot after a `---errors---` divider so
/// diagnostic emission is reviewed together with tree shape.
fn format_with_errors(src: &str) -> String {
    let parse = parse(src);
    let mut out = format_cst(&parse.syntax());
    let errs = parse.errors();
    if !errs.is_empty() {
        out.push_str("---errors---\n");
        for e in errs {
            let off: u32 = e.offset.into();
            writeln!(out, "@{off}: {}", e.message).unwrap();
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Clauses
// ---------------------------------------------------------------------------

#[test]
fn clause_match_basic() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN n"));
}

#[test]
fn clause_match_labeled_node() {
    insta::assert_snapshot!(format_with_errors("MATCH (p:Person) RETURN p"));
}

#[test]
fn clause_match_multiple_patterns() {
    insta::assert_snapshot!(format_with_errors("MATCH (a), (b), (c) RETURN a, b, c"));
}

#[test]
fn clause_optional_match() {
    insta::assert_snapshot!(format_with_errors(
        "OPTIONAL MATCH (a:Person {name: 'Alice'}) RETURN a"
    ));
}

#[test]
fn clause_match_where() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) WHERE n.age > 18 RETURN n"));
}

#[test]
fn clause_return_basic() {
    insta::assert_snapshot!(format_with_errors("RETURN 1"));
}

#[test]
fn clause_return_distinct() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN DISTINCT n.name"));
}

#[test]
fn clause_return_star() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN *"));
}

#[test]
fn clause_return_order_by_skip_limit() {
    insta::assert_snapshot!(format_with_errors(
        "MATCH (n) RETURN n.name ORDER BY n.age DESC SKIP 1 LIMIT 5"
    ));
}

#[test]
fn clause_return_alias() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN n.name AS name"));
}

// TODO(cy-nom): WITH clause (currently parsed as a deferred_clause_stub;
// re-enable with a dedicated snapshot once WITH / DISTINCT / WHERE-filter
// land).
// TODO(cy-nom): UNWIND clause (deferred_clause_stub).
// TODO(cy-nom): CREATE clause (deferred_clause_stub).
// TODO(cy-nom): MERGE with ON CREATE / ON MATCH (deferred_clause_stub).
// TODO(cy-nom): SET (property / labels / map) (deferred_clause_stub).
// TODO(cy-nom): REMOVE (property / labels) (deferred_clause_stub).
// TODO(cy-nom): DELETE / DETACH DELETE (deferred_clause_stub).
// TODO(cy-nom): CALL / CALL YIELD (deferred_clause_stub).
//
// Each of the deferred clauses *does* produce a tree today — a stub
// ERROR node wrapping the keyword — but the shape will change when the
// production lands; snapshotting now would just be churn.

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[test]
fn pattern_anonymous_node() {
    insta::assert_snapshot!(format_with_errors("MATCH () RETURN 1"));
}

#[test]
fn pattern_bound_node() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN n"));
}

#[test]
fn pattern_multi_label_node() {
    insta::assert_snapshot!(format_with_errors("MATCH (n:Person:Employee) RETURN n"));
}

#[test]
fn pattern_node_with_properties() {
    insta::assert_snapshot!(format_with_errors(
        "MATCH (n:Person {name: 'Alice', age: 30}) RETURN n"
    ));
}

#[test]
fn pattern_rel_undirected() {
    insta::assert_snapshot!(format_with_errors("MATCH (a)-[r]-(b) RETURN a, b"));
}

#[test]
fn pattern_rel_directed_right() {
    insta::assert_snapshot!(format_with_errors("MATCH (a)-[:KNOWS]->(b) RETURN a, b"));
}

#[test]
fn pattern_rel_directed_left() {
    insta::assert_snapshot!(format_with_errors("MATCH (a)<-[:KNOWS]-(b) RETURN a, b"));
}

#[test]
fn pattern_rel_with_props() {
    insta::assert_snapshot!(format_with_errors(
        "MATCH (a)-[r:KNOWS {since: 2020}]->(b) RETURN r"
    ));
}

#[test]
fn pattern_chain_three_nodes() {
    insta::assert_snapshot!(format_with_errors(
        "MATCH (a)-[:R1]->(b)-[:R2]->(c) RETURN a, b, c"
    ));
}

// TODO(cy-nom): variable-length relationships (`*`, `*1..3`, `*1..`) —
// not yet parsed; flagged in grammar/pattern.rs.

// ---------------------------------------------------------------------------
// Expressions — atoms
// ---------------------------------------------------------------------------

#[test]
fn expr_literal_int() {
    insta::assert_snapshot!(format_with_errors("RETURN 42"));
}

#[test]
fn expr_literal_float() {
    insta::assert_snapshot!(format_with_errors("RETURN 3.14"));
}

#[test]
fn expr_literal_string() {
    insta::assert_snapshot!(format_with_errors("RETURN 'hello'"));
}

#[test]
fn expr_literal_bool() {
    insta::assert_snapshot!(format_with_errors("RETURN TRUE, FALSE"));
}

#[test]
fn expr_literal_null() {
    insta::assert_snapshot!(format_with_errors("RETURN NULL"));
}

#[test]
fn expr_parameter() {
    insta::assert_snapshot!(format_with_errors("RETURN $name"));
}

#[test]
fn expr_variable() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN n"));
}

#[test]
fn expr_property_access() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN n.name"));
}

#[test]
fn expr_subscript() {
    insta::assert_snapshot!(format_with_errors("RETURN a[0]"));
}

#[test]
fn expr_function_call() {
    insta::assert_snapshot!(format_with_errors("RETURN count(n)"));
}

#[test]
fn expr_function_call_distinct() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN count(DISTINCT n)"));
}

#[test]
fn expr_paren() {
    insta::assert_snapshot!(format_with_errors("RETURN (1 + 2) * 3"));
}

// TODO(cy-nom): list literals `[a, b]`, map literals `{k: v}`, list /
// pattern comprehensions, CASE (simple + searched), pattern predicates,
// EXISTS(...), standalone COUNT(*) — deferred per grammar/expression.rs.

// ---------------------------------------------------------------------------
// Expressions — operators
// ---------------------------------------------------------------------------

#[test]
fn expr_binary_arithmetic() {
    insta::assert_snapshot!(format_with_errors("RETURN 1 + 2 * 3"));
}

#[test]
fn expr_binary_power_right_assoc() {
    insta::assert_snapshot!(format_with_errors("RETURN 2 ^ 3 ^ 2"));
}

#[test]
fn expr_binary_comparison() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) WHERE n.age >= 18 RETURN n"));
}

#[test]
fn expr_binary_boolean() {
    insta::assert_snapshot!(format_with_errors("RETURN NOT a OR b AND c"));
}

#[test]
fn expr_starts_with() {
    insta::assert_snapshot!(format_with_errors("RETURN a STARTS WITH 'foo'"));
}

#[test]
fn expr_ends_with() {
    insta::assert_snapshot!(format_with_errors("RETURN a ENDS WITH 'foo'"));
}

#[test]
fn expr_contains() {
    insta::assert_snapshot!(format_with_errors("RETURN a CONTAINS 'foo'"));
}

#[test]
fn expr_regex_match() {
    insta::assert_snapshot!(format_with_errors("RETURN a =~ 'r.*'"));
}

#[test]
fn expr_unary_minus() {
    insta::assert_snapshot!(format_with_errors("RETURN -1"));
}

#[test]
fn expr_unary_not() {
    insta::assert_snapshot!(format_with_errors("RETURN NOT a"));
}

#[test]
fn expr_is_null() {
    insta::assert_snapshot!(format_with_errors("RETURN a IS NULL"));
}

#[test]
fn expr_is_not_null() {
    insta::assert_snapshot!(format_with_errors("RETURN a IS NOT NULL"));
}

#[test]
fn expr_in_operator() {
    insta::assert_snapshot!(format_with_errors(
        "MATCH (n) WHERE n.age IN n.ages RETURN n"
    ));
}

// TODO(cy-nom): IN with list literal RHS (`x IN [1, 2, 3]`) — requires
// list-literal support in the expression grammar.

// ---------------------------------------------------------------------------
// Error cases — tree + diagnostics both snapshotted
// ---------------------------------------------------------------------------

#[test]
fn err_unclosed_paren_in_pattern() {
    insta::assert_snapshot!(format_with_errors("MATCH (n RETURN n"));
}

#[test]
fn err_missing_return_target() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) RETURN"));
}

#[test]
fn err_stray_keyword_after_where() {
    insta::assert_snapshot!(format_with_errors("MATCH (n) WHERE RETURN n"));
}

#[test]
fn err_leading_garbage() {
    insta::assert_snapshot!(format_with_errors("garbage MATCH (n) RETURN n"));
}

#[test]
fn err_unclosed_property_map() {
    insta::assert_snapshot!(format_with_errors("MATCH (n {x: 1 RETURN n"));
}

#[test]
fn err_missing_label_after_colon() {
    insta::assert_snapshot!(format_with_errors("MATCH (n:) RETURN n"));
}

#[test]
fn err_unterminated_string() {
    insta::assert_snapshot!(format_with_errors("RETURN 'oops"));
}

#[test]
fn err_missing_closing_bracket_in_rel() {
    insta::assert_snapshot!(format_with_errors("MATCH (a)-[r (b) RETURN a"));
}
