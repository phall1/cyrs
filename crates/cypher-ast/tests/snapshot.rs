//! AST snapshot corpus (spec §17.2).
//!
//! Each test renders the typed AST view of a representative Cypher query
//! using [`ast_dump`] and asserts it against a committed `insta` snapshot.
//!
//! Format produced by `ast_dump`:
//! - Every CST node that matches an AST wrapper is printed as
//!   `AstType(SYNTAX_KIND@start..end)` at its indentation depth.
//! - CST nodes with no AST wrapper print as `SYNTAX_KIND@start..end`.
//! - Tokens print as `TOKEN_KIND@start..end "text"`.
//!
//! This gives reviewers both the structural AST annotation and the raw
//! token text in a single deterministic artefact.

use std::fmt::Write as _;

use cypher_ast::{
    ArgList, BinaryExpr, CallClause, CaseExpr, CreateClause, DeleteClause, FunctionCall, LabelExpr,
    ListComprehension, ListLiteral, ListPredicateExpr, MapLiteral, MatchClause, MergeAction,
    MergeClause, NodePattern, OrderBy, ParenExpr, Pattern, PatternComprehension, PatternPredicate,
    PropertyMap, RelDetail, RemoveClause, ReturnClause, ReturnItem, SetClause, SourceFile,
    UnaryExpr, UnwindClause, WhereClause, WithClause, YieldItem,
};
use cypher_syntax::{SyntaxNode, parse};
use rowan::{NodeOrToken, WalkEvent};

/// Return the AST wrapper name for a syntax node, if one exists.
fn ast_name(node: &SyntaxNode) -> Option<&'static str> {
    // Struct wrappers — order follows generated.rs
    if SourceFile::cast(node.clone()).is_some() {
        return Some("SourceFile");
    }
    if MatchClause::cast(node.clone()).is_some() {
        return Some("MatchClause");
    }
    if WithClause::cast(node.clone()).is_some() {
        return Some("WithClause");
    }
    if ReturnClause::cast(node.clone()).is_some() {
        return Some("ReturnClause");
    }
    if UnwindClause::cast(node.clone()).is_some() {
        return Some("UnwindClause");
    }
    if CallClause::cast(node.clone()).is_some() {
        return Some("CallClause");
    }
    if CreateClause::cast(node.clone()).is_some() {
        return Some("CreateClause");
    }
    if MergeClause::cast(node.clone()).is_some() {
        return Some("MergeClause");
    }
    if SetClause::cast(node.clone()).is_some() {
        return Some("SetClause");
    }
    if RemoveClause::cast(node.clone()).is_some() {
        return Some("RemoveClause");
    }
    if DeleteClause::cast(node.clone()).is_some() {
        return Some("DeleteClause");
    }
    if Pattern::cast(node.clone()).is_some() {
        return Some("Pattern");
    }
    if WhereClause::cast(node.clone()).is_some() {
        return Some("WhereClause");
    }
    if OrderBy::cast(node.clone()).is_some() {
        return Some("OrderBy");
    }
    if ArgList::cast(node.clone()).is_some() {
        return Some("ArgList");
    }
    if YieldItem::cast(node.clone()).is_some() {
        return Some("YieldItem");
    }
    if MergeAction::cast(node.clone()).is_some() {
        return Some("MergeAction");
    }
    if ReturnItem::cast(node.clone()).is_some() {
        return Some("ReturnItem");
    }
    if LabelExpr::cast(node.clone()).is_some() {
        return Some("LabelExpr");
    }
    if NodePattern::cast(node.clone()).is_some() {
        return Some("NodePattern");
    }
    if PropertyMap::cast(node.clone()).is_some() {
        return Some("PropertyMap");
    }
    if RelDetail::cast(node.clone()).is_some() {
        return Some("RelDetail");
    }
    // Expr variants
    if FunctionCall::cast(node.clone()).is_some() {
        return Some("Expr::FunctionCall");
    }
    if ParenExpr::cast(node.clone()).is_some() {
        return Some("Expr::ParenExpr");
    }
    if ListLiteral::cast(node.clone()).is_some() {
        return Some("Expr::ListLiteral");
    }
    if MapLiteral::cast(node.clone()).is_some() {
        return Some("Expr::MapLiteral");
    }
    if ListComprehension::cast(node.clone()).is_some() {
        return Some("Expr::ListComprehension");
    }
    if ListPredicateExpr::cast(node.clone()).is_some() {
        return Some("Expr::ListPredicateExpr");
    }
    if PatternComprehension::cast(node.clone()).is_some() {
        return Some("Expr::PatternComprehension");
    }
    if CaseExpr::cast(node.clone()).is_some() {
        return Some("Expr::CaseExpr");
    }
    if BinaryExpr::cast(node.clone()).is_some() {
        return Some("Expr::BinaryExpr");
    }
    if UnaryExpr::cast(node.clone()).is_some() {
        return Some("Expr::UnaryExpr");
    }
    if PatternPredicate::cast(node.clone()).is_some() {
        return Some("Expr::PatternPredicate");
    }
    None
}

/// Walk the CST rooted at `node` and produce an annotated textual tree.
///
/// Each CST node is printed with its AST wrapper name (if any), its
/// `SyntaxKind`, and its byte range.  Tokens print their text.
/// Indentation is two spaces per depth level.
///
/// Traversal uses a deterministic preorder walk (no `HashMap` iteration);
/// output is stable across runs (§8, §17.14).
fn ast_dump(node: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut depth: usize = 0;

    for ev in node.preorder_with_tokens() {
        match ev {
            WalkEvent::Enter(NodeOrToken::Node(n)) => {
                let indent = "  ".repeat(depth);
                let r = n.text_range();
                let start: u32 = r.start().into();
                let end: u32 = r.end().into();
                if let Some(ast) = ast_name(&n) {
                    writeln!(out, "{indent}{ast}({:?}@{start}..{end})", n.kind()).unwrap();
                } else {
                    writeln!(out, "{indent}{:?}@{start}..{end}", n.kind()).unwrap();
                }
                depth += 1;
            }
            WalkEvent::Leave(NodeOrToken::Node(_)) => {
                depth -= 1;
            }
            WalkEvent::Enter(NodeOrToken::Token(t)) => {
                let indent = "  ".repeat(depth);
                let r = t.text_range();
                let start: u32 = r.start().into();
                let end: u32 = r.end().into();
                writeln!(out, "{indent}{:?}@{start}..{end} {:?}", t.kind(), t.text()).unwrap();
            }
            WalkEvent::Leave(NodeOrToken::Token(_)) => {}
        }
    }
    out
}

/// Parse `src`, run `ast_dump`, and append any parse errors.
fn dump(src: &str) -> String {
    let parse = parse(src);
    let mut out = ast_dump(&parse.syntax());
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
    insta::assert_snapshot!(dump("MATCH (n) RETURN n"));
}

#[test]
fn clause_match_labeled_node() {
    insta::assert_snapshot!(dump("MATCH (p:Person) RETURN p"));
}

#[test]
fn clause_match_multiple_patterns() {
    insta::assert_snapshot!(dump("MATCH (a), (b), (c) RETURN a, b, c"));
}

#[test]
fn clause_optional_match() {
    insta::assert_snapshot!(dump("OPTIONAL MATCH (a:Person {name: 'Alice'}) RETURN a"));
}

#[test]
fn clause_match_where() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.age > 18 RETURN n"));
}

#[test]
fn clause_return_basic() {
    insta::assert_snapshot!(dump("RETURN 1"));
}

#[test]
fn clause_return_distinct() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN DISTINCT n.name"));
}

#[test]
fn clause_return_alias() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN n.name AS alias"));
}

#[test]
fn clause_return_multiple_items() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN n.name, n.age, n.id"));
}

#[test]
fn clause_return_star() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN *"));
}

#[test]
fn clause_with_projection() {
    insta::assert_snapshot!(dump("MATCH (n) WITH n RETURN n"));
}

#[test]
fn clause_with_where() {
    insta::assert_snapshot!(dump("MATCH (n) WITH n WHERE n.age > 30 RETURN n"));
}

#[test]
fn clause_with_return_projection() {
    insta::assert_snapshot!(dump(
        "MATCH (n:Person) WITH n.name AS name, n.age AS age RETURN name, age"
    ));
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[test]
fn pattern_anonymous_node() {
    insta::assert_snapshot!(dump("MATCH () RETURN 1"));
}

#[test]
fn pattern_bound_node() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN n"));
}

#[test]
fn pattern_labeled_node() {
    insta::assert_snapshot!(dump("MATCH (n:Person) RETURN n"));
}

#[test]
fn pattern_multi_label_node() {
    insta::assert_snapshot!(dump("MATCH (n:Person:Employee) RETURN n"));
}

#[test]
fn pattern_node_with_properties() {
    insta::assert_snapshot!(dump("MATCH (n:Person {name: 'Alice', age: 30}) RETURN n"));
}

#[test]
fn pattern_rel_directed_right() {
    insta::assert_snapshot!(dump("MATCH (a)-[:KNOWS]->(b) RETURN a, b"));
}

#[test]
fn pattern_rel_directed_left() {
    insta::assert_snapshot!(dump("MATCH (a)<-[:KNOWS]-(b) RETURN a, b"));
}

#[test]
fn pattern_rel_undirected() {
    insta::assert_snapshot!(dump("MATCH (a)-[r]-(b) RETURN a, b"));
}

#[test]
fn pattern_rel_with_props() {
    insta::assert_snapshot!(dump("MATCH (a)-[r:KNOWS {since: 2020}]->(b) RETURN r"));
}

#[test]
fn pattern_chain_three_nodes() {
    insta::assert_snapshot!(dump("MATCH (a)-[:R1]->(b)-[:R2]->(c) RETURN a, b, c"));
}

// ---------------------------------------------------------------------------
// Expressions — literals & atoms
// ---------------------------------------------------------------------------

#[test]
fn expr_literal_integer() {
    insta::assert_snapshot!(dump("RETURN 42"));
}

#[test]
fn expr_literal_float() {
    insta::assert_snapshot!(dump("RETURN 3.14"));
}

#[test]
fn expr_literal_string() {
    insta::assert_snapshot!(dump("RETURN 'hello'"));
}

#[test]
fn expr_literal_bool_true() {
    insta::assert_snapshot!(dump("RETURN true"));
}

#[test]
fn expr_literal_null() {
    insta::assert_snapshot!(dump("RETURN null"));
}

#[test]
fn expr_parameter() {
    insta::assert_snapshot!(dump("RETURN $param"));
}

#[test]
fn expr_variable() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN n"));
}

#[test]
fn expr_property_access() {
    insta::assert_snapshot!(dump("MATCH (n) RETURN n.name"));
}

// ---------------------------------------------------------------------------
// Expressions — compound
// ---------------------------------------------------------------------------

#[test]
fn expr_binary_arithmetic() {
    insta::assert_snapshot!(dump("RETURN 1 + 2 * 3"));
}

#[test]
fn expr_binary_comparison() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.age > 18 RETURN n"));
}

#[test]
fn expr_binary_boolean_and() {
    insta::assert_snapshot!(dump(
        "MATCH (n) WHERE n.age > 18 AND n.active = true RETURN n"
    ));
}

#[test]
fn expr_binary_boolean_or() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.age < 18 OR n.age > 65 RETURN n"));
}

#[test]
fn expr_unary_minus() {
    insta::assert_snapshot!(dump("RETURN -1"));
}

#[test]
fn expr_unary_not() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE NOT n.active RETURN n"));
}

#[test]
fn expr_is_null() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.name IS NULL RETURN n"));
}

#[test]
fn expr_is_not_null() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.name IS NOT NULL RETURN n"));
}

#[test]
fn expr_function_call() {
    insta::assert_snapshot!(dump("RETURN length('hello')"));
}

#[test]
fn expr_paren() {
    insta::assert_snapshot!(dump("RETURN (1 + 2) * 3"));
}

// ---------------------------------------------------------------------------
// WHERE with complex expressions
// ---------------------------------------------------------------------------

#[test]
fn where_expr_starts_with() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.name STARTS WITH 'Al' RETURN n"));
}

#[test]
fn where_expr_in_list() {
    insta::assert_snapshot!(dump("MATCH (n) WHERE n.id IN [1, 2, 3] RETURN n"));
}
