//! Integration tests for the cy-nom parser surface.
//!
//! Three axes:
//!
//! 1. **Grammar happy paths** — each well-formed query parses without
//!    errors and round-trips losslessly.
//! 2. **Statement boundaries** (spec §4.6) — empty input, trailing `;`,
//!    multi-statement `;`-separated input.
//! 3. **Error recovery** (spec §4.3) — unclosed `(`, missing expression,
//!    leading garbage. Each asserts that an error is recorded AND the
//!    tree still round-trips losslessly.
//! 4. **Pratt precedence sanity** — a structural walk of the rowan tree
//!    for `a + b * c` verifying that `+` is the root binop and its RHS
//!    is a `*` binop.

use cypher_syntax::{SyntaxKind, SyntaxNode, parse};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn assert_ok(src: &str) -> SyntaxNode {
    let p = parse(src);
    assert_eq!(
        p.syntax().to_string(),
        src,
        "lossless round-trip failed for {src:?}"
    );
    assert!(
        p.errors().is_empty(),
        "unexpected errors parsing {src:?}: {:?}",
        p.errors()
    );
    p.syntax()
}

fn assert_err_recovery(src: &str) -> SyntaxNode {
    let p = parse(src);
    assert_eq!(
        p.syntax().to_string(),
        src,
        "lossless round-trip failed for {src:?}"
    );
    assert!(
        !p.errors().is_empty(),
        "expected at least one error for {src:?}"
    );
    p.syntax()
}

/// Recursively find the first descendant (preorder) with the given kind.
fn find_first(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    if node.kind() == kind {
        return Some(node.clone());
    }
    node.descendants().find(|d| d.kind() == kind)
}

// ---------------------------------------------------------------------------
// 1. Grammar happy paths
// ---------------------------------------------------------------------------

#[test]
fn parse_match_return() {
    assert_ok("MATCH (n) RETURN n");
}

#[test]
fn parse_optional_match_with_label_and_property_map() {
    assert_ok("OPTIONAL MATCH (a:Person {name: 'Alice'}) RETURN a.name AS name");
}

#[test]
fn parse_directed_rel_with_type_and_predicate() {
    assert_ok("MATCH (a)-[:KNOWS]->(b) WHERE a.age > 18 RETURN a, b");
}

#[test]
fn parse_arithmetic_pratt_precedence() {
    // `2 * 3` binds tighter than `1 + ...`.
    assert_ok("RETURN 1 + 2 * 3");
}

#[test]
fn parse_logical_precedence() {
    // NOT > AND > OR; parses as (NOT a) OR (b AND c).
    assert_ok("RETURN NOT a OR b AND c");
}

#[test]
fn parse_chained_postfix() {
    // Property access, subscript, property access — all left-assoc
    // postfix at the tightest binding.
    assert_ok("RETURN a.b[0].c");
}

#[test]
fn parse_function_call() {
    assert_ok("RETURN f(x, y)");
}

#[test]
fn parse_function_call_sum() {
    // The dedicated function-call idioms that TCK @AGGREGATIONS exercises.
    // `sum` is a bare identifier; `count` is a keyword that also stands in
    // for a function name — both must parse cleanly.
    assert_ok("MATCH (n:Order) RETURN sum(n.amount)");
    assert_ok("MATCH (n:Person) RETURN count(n)");
}

#[test]
fn parse_string_concat() {
    assert_ok("RETURN \"hello\" + ' world'");
}

#[test]
fn parse_parameter() {
    assert_ok("RETURN $param");
}

#[test]
fn parse_is_null_postfix_forms() {
    assert_ok("RETURN a IS NULL, b IS NOT NULL");
}

#[test]
fn parse_starts_with() {
    assert_ok("RETURN a STARTS WITH 'foo'");
}

#[test]
fn parse_ends_with_contains_regex() {
    assert_ok("RETURN a ENDS WITH 'x', b CONTAINS 'y', c =~ 'r.*'");
}

#[test]
fn parse_unary_minus() {
    assert_ok("RETURN -1 + -2");
}

#[test]
fn parse_power_is_right_assoc() {
    // 2 ^ 3 ^ 2 should parse as 2 ^ (3 ^ 2).
    assert_ok("RETURN 2 ^ 3 ^ 2");
}

#[test]
fn parse_comparison_chain() {
    assert_ok("RETURN a < b AND b < c");
}

#[test]
fn parse_paren_expr() {
    assert_ok("RETURN (a + b) * c");
}

#[test]
fn parse_multiple_patterns() {
    assert_ok("MATCH (a), (b) RETURN a, b");
}

#[test]
fn parse_multi_label() {
    assert_ok("MATCH (a:Person:Employee) RETURN a");
}

#[test]
fn parse_return_distinct_order_limit() {
    assert_ok("MATCH (n) RETURN DISTINCT n.name ORDER BY n.age DESC SKIP 1 LIMIT 5");
}

// ---------------------------------------------------------------------------
// 2. Statement boundaries (spec §4.6)
// ---------------------------------------------------------------------------

/// Helper: count direct `STATEMENT` node children of the root.
fn count_statements(tree: &SyntaxNode) -> usize {
    tree.children()
        .filter(|c| c.kind() == SyntaxKind::STATEMENT)
        .count()
}

#[test]
fn empty_input() {
    let p = parse("");
    assert!(p.errors().is_empty(), "empty input must have no errors");
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert_eq!(p.syntax().to_string(), "");
    // spec §4.6: empty file → 0 Statement children, not an error.
    assert_eq!(
        count_statements(&p.syntax()),
        0,
        "empty file must have 0 Statement children"
    );
}

#[test]
fn whitespace_only_input() {
    let p = parse("   \n  ");
    assert!(p.errors().is_empty());
    assert_eq!(p.syntax().to_string(), "   \n  ");
    // Whitespace-only is still an empty tree (no statements).
    assert_eq!(count_statements(&p.syntax()), 0);
}

#[test]
fn single_statement_no_semicolon() {
    // spec §4.6: trailing `;` is optional — a lone statement with no `;` is fine.
    let tree = assert_ok("MATCH (n) RETURN n");
    assert_eq!(count_statements(&tree), 1, "one Statement expected");
}

#[test]
fn single_statement_trailing_semicolon() {
    // spec §4.6: trailing `;` is optional but permitted.
    let tree = assert_ok("MATCH (n) RETURN n;");
    assert_eq!(count_statements(&tree), 1, "one Statement expected");
}

#[test]
fn trailing_semicolon_ok() {
    assert_ok("MATCH (n) RETURN n;");
}

#[test]
fn two_statements_semi_separated() {
    // spec §4.6: `;` separates two statements.
    let tree = assert_ok("MATCH (n) RETURN n; MATCH (m) RETURN m");
    assert_eq!(count_statements(&tree), 2, "two Statements expected");
}

#[test]
fn two_statements_with_trailing_semi() {
    let tree = assert_ok("MATCH (n) RETURN n; MATCH (m) RETURN m;");
    assert_eq!(count_statements(&tree), 2, "two Statements expected");
}

#[test]
fn multi_clause_single_statement() {
    // A MATCH … RETURN is one statement with multiple clauses (not two statements).
    // This verifies the clause-loop in single_query correctly accumulates clauses.
    let tree = assert_ok("MATCH (n) RETURN n");
    assert_eq!(count_statements(&tree), 1, "MATCH+RETURN is 1 Statement");
}

#[test]
fn two_statements_missing_separator_error_recovery() {
    // spec §4.6 + §4.2 error-tolerance: when an unexpected non-clause
    // token appears between two statements, the parser emits a diagnostic
    // but still recovers to parse the second statement.
    let tree = assert_err_recovery("RETURN 1; junk RETURN 2");
    // Both RETURN clauses should survive after recovery.
    assert!(
        find_first(&tree, SyntaxKind::RETURN_CLAUSE).is_some(),
        "expected at least one RETURN_CLAUSE after recovery"
    );
    let stmts = count_statements(&tree);
    assert!(
        stmts >= 1,
        "expected at least 1 Statement after recovery, got {stmts}"
    );
}

// ---------------------------------------------------------------------------
// 2b. UNION / UNION ALL — spec §19 (cy-2xm)
// ---------------------------------------------------------------------------

#[test]
fn union_simple() {
    // `RETURN 1 UNION RETURN 2` must parse in bounded time and round-trip
    // losslessly. Regression test for cy-2xm parser hang.
    let tree = assert_ok("RETURN 1 UNION RETURN 2");
    assert!(
        find_first(&tree, SyntaxKind::UNION_TAIL).is_some(),
        "expected a UNION_TAIL node"
    );
}

#[test]
fn union_all() {
    let tree = assert_ok("RETURN 1 UNION ALL RETURN 2");
    assert!(
        find_first(&tree, SyntaxKind::UNION_TAIL).is_some(),
        "expected a UNION_TAIL node for UNION ALL"
    );
}

#[test]
fn union_three_way() {
    let tree = assert_ok("RETURN 1 UNION RETURN 2 UNION ALL RETURN 3");
    // At least two UNION_TAIL nodes.
    let tails = tree
        .descendants()
        .filter(|d| d.kind() == SyntaxKind::UNION_TAIL)
        .count();
    assert_eq!(tails, 2, "expected 2 UNION_TAIL nodes, got {tails}");
}

#[test]
fn union_semicolon_separated_statements() {
    // Two separate UNION queries divided by `;`.
    let tree = assert_ok("RETURN 1 UNION RETURN 2; RETURN 3 UNION RETURN 4");
    assert_eq!(count_statements(&tree), 2, "expected 2 statements");
}

// ---------------------------------------------------------------------------
// 3. Error recovery
// ---------------------------------------------------------------------------

#[test]
fn recover_unclosed_paren_in_node_pattern() {
    // Unclosed `(n` — the parser should emit a diagnostic at the missing
    // `)` (virtual-token insertion, spec §4.3) and continue parsing the
    // RETURN clause. Tree round-trips losslessly including the missing
    // closer (recorded as zero-width error).
    let tree = assert_err_recovery("MATCH (n RETURN n");
    // RETURN clause should still be present.
    assert!(
        find_first(&tree, SyntaxKind::RETURN_CLAUSE).is_some(),
        "RETURN clause missing after recovery"
    );
}

#[test]
fn recover_missing_expression_after_where() {
    // `WHERE` immediately followed by `RETURN` — expression parser sees
    // a non-atom token and reports. Sync-set skip keeps the rest of the
    // statement parseable.
    let tree = assert_err_recovery("MATCH (n) WHERE RETURN n");
    assert!(find_first(&tree, SyntaxKind::RETURN_CLAUSE).is_some());
}

#[test]
fn recover_leading_junk_before_match() {
    // Junk at the start of a file. Recovery skips to the first clause
    // keyword and parses from there.
    let tree = assert_err_recovery("garbage MATCH (n) RETURN n");
    assert!(find_first(&tree, SyntaxKind::MATCH_CLAUSE).is_some());
    assert!(find_first(&tree, SyntaxKind::RETURN_CLAUSE).is_some());
}

#[test]
fn recover_unclosed_property_map() {
    let tree = assert_err_recovery("MATCH (n {x: 1 RETURN n");
    // Tree round-trips losslessly despite the missing `}` and `)`.
    assert!(find_first(&tree, SyntaxKind::RETURN_CLAUSE).is_some());
}

// ---------------------------------------------------------------------------
// 4. Pratt precedence structural check
// ---------------------------------------------------------------------------

/// Direct-child predicate: is there a token of this kind attached to
/// `node` (not counting tokens nested inside child nodes)?
fn has_direct_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .any(|t| t.kind() == kind)
}

#[test]
fn pratt_additive_times_multiplicative_structure() {
    // `a + b * c` must group as `a + (b * c)`. Concretely:
    //   ROOT
    //     RETURN_CLAUSE
    //       RETURN_BODY → RETURN_ITEMS → RETURN_ITEM
    //         BINARY_EXPR  <-- `+`
    //           VAR_EXPR   <-- `a`
    //           BINARY_EXPR  <-- `*`
    //             VAR_EXPR <-- `b`
    //             VAR_EXPR <-- `c`
    let tree = assert_ok("RETURN a + b * c");
    let top_bin =
        find_first(&tree, SyntaxKind::BINARY_EXPR).expect("top-level BINARY_EXPR missing");
    assert!(
        has_direct_token(&top_bin, SyntaxKind::PLUS),
        "top-level binop is not `+`"
    );

    // The RHS of the `+` binop should itself be a `BINARY_EXPR` with `*`.
    let rhs = top_bin
        .children()
        .find(|c| c.kind() == SyntaxKind::BINARY_EXPR)
        .expect("no nested BINARY_EXPR for RHS of `+`");
    assert!(
        has_direct_token(&rhs, SyntaxKind::STAR),
        "RHS binop is not `*` (precedence wrong)"
    );
}

#[test]
fn pratt_unary_not_binds_tighter_than_and() {
    // `NOT a AND b` → `(NOT a) AND b`. The top is a BINARY_EXPR whose
    // LHS is a UNARY_EXPR.
    let tree = assert_ok("RETURN NOT a AND b");
    let top_bin =
        find_first(&tree, SyntaxKind::BINARY_EXPR).expect("top-level BINARY_EXPR missing");
    assert!(
        has_direct_token(&top_bin, SyntaxKind::AND_KW),
        "top-level binop is not AND"
    );
    // Its LHS child is a UNARY_EXPR.
    assert!(
        top_bin
            .children()
            .any(|c| c.kind() == SyntaxKind::UNARY_EXPR),
        "LHS of AND is not a UNARY_EXPR (NOT binding wrong)"
    );
}

#[test]
fn pratt_power_is_right_associative() {
    // `2 ^ 3 ^ 2` → `2 ^ (3 ^ 2)` — so the RHS of the outer `^` is
    // another BINARY_EXPR whose operator is also `^`.
    let tree = assert_ok("RETURN 2 ^ 3 ^ 2");
    let top_bin =
        find_first(&tree, SyntaxKind::BINARY_EXPR).expect("top-level BINARY_EXPR missing");
    assert!(
        has_direct_token(&top_bin, SyntaxKind::CARET),
        "top-level binop is not `^`"
    );
    let rhs_bin = top_bin
        .children()
        .find(|c| c.kind() == SyntaxKind::BINARY_EXPR)
        .expect("RHS of outer ^ is not a binop");
    assert!(
        has_direct_token(&rhs_bin, SyntaxKind::CARET),
        "RHS binop is not `^` — associativity wrong"
    );
}
