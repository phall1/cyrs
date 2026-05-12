//! HIR snapshot corpus with resolved-name overlay (spec 0001 §17.2 / §6.2).
//!
//! Each test runs the full pipeline:
//!   1. Parse → AST (via [`cyrs_hir::lower::lower_statement`])
//!   2. Lower → HIR
//!   3. Desugar ([`cyrs_hir::desugar::desugar_statement`])
//!   4. Resolve names ([`cyrs_sema::resolve`])
//!   5. Snapshot the HIR with the resolved-name overlay
//!      ([`cyrs_hir::pretty::print_overlay`])
//!
//! Overlay format example:
//!   `Var(0)[n←Node@0..9]`  — resolved: `VarId(0)`, name "n", kind Node,
//!                             defined at bytes 0..9 in the source.
//!   `Var(N)[name?Unresolved]` — unresolved reference.
//!   `Unresolved("x")`         — survived lowering with no `VarId`.
//!
//! Coverage map:
//!  01  simple MATCH–RETURN, one Node binding
//!  02  labeled MATCH–RETURN
//!  03  multi-pattern: Node + Relationship bindings
//!  04  three-hop chain: multiple nodes + rels
//!  05  optional MATCH
//!  06  WHERE predicate on bound var
//!  07  RETURN property access on bound var
//!  08  RETURN DISTINCT
//!  09  multi-binding RETURN (all `VarKind`s)
//!  10  unresolved variable → E1001 diagnostic in overlay
//!  11  WITH barrier: projected name survives
//!  12  WITH barrier: dropped name → E1001 after barrier
//!  13  WITH alias: `WITH a AS b` — b visible, a gone
//!  14  UNWIND → Value binding
//!  15  shadowing inside nested scopes (warn=true)
//!  16  double WITH barrier — a survives both, b dropped
//!  17  kind-mismatch: Node var in arithmetic → E2007
//!  18  kind-mismatch: Path var in prop access → E2007
//!  19  list comprehension (desugared, `filter_var` overlay)
//!  20  pattern predicate in WHERE
//!  21  map projection with `VarShorthand`
//!  22  CREATE binds Node var
//!  23  CALL … YIELD … binds Value vars

#![allow(clippy::too_many_lines)]

use cyrs_diag::DiagnosticsSink;
use cyrs_hir::{
    Binding, Clause, Direction, Expr, HirId, Pattern, PatternElement, PatternPart, Projection,
    RelLength, Statement, VarId, VarKind, YieldItem, desugar::desugar_statement,
    lower::lower_statement, pretty::print_overlay,
};
use cyrs_sema::resolve;
use cyrs_syntax::{TextRange, TextSize};
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn zero_range() -> TextRange {
    TextRange::new(TextSize::new(0), TextSize::new(0))
}

fn dummy_syntax() -> cyrs_syntax::SyntaxNode {
    cyrs_syntax::parse("x").syntax()
}

fn alloc(stmt: &mut Statement) -> HirId {
    stmt.alloc_id(dummy_syntax())
}

fn intern_var(stmt: &mut Statement, name: &str, kind: VarKind) -> VarId {
    for (id, b) in &stmt.bindings {
        if b.name.as_str() == name {
            return *id;
        }
    }
    let id = VarId(u32::try_from(stmt.bindings.len()).expect("overflow"));
    stmt.bindings.insert(
        id,
        Binding {
            id,
            name: SmolStr::new(name),
            kind,
            defined_at: zero_range(),
        },
    );
    id
}

/// Full pipeline: parse, lower, desugar, resolve, overlay.
fn pipeline(src: &str) -> String {
    pipeline_warn(src, false)
}

fn fmt_overlay_with_diags(overlay: String, diags: &[cyrs_diag::Diagnostic]) -> String {
    use std::fmt::Write as _;
    if diags.is_empty() {
        return overlay;
    }
    let mut diag_block = String::new();
    for d in diags {
        let _ = writeln!(diag_block, "  {}: {}", d.code, d.message);
    }
    format!("{overlay}\nDiagnostics:\n{diag_block}")
}

fn pipeline_warn(src: &str, warn_shadowing: bool) -> String {
    let stmt = lower_statement(src);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let result = resolve(&stmt, warn_shadowing, &mut sink);
    let diags = sink.into_sorted();
    let overlay = print_overlay(&stmt, &result.resolved_names);
    fmt_overlay_with_diags(overlay, &diags)
}

/// Pipeline for a hand-constructed HIR statement (bypasses the parser).
fn pipeline_stmt(stmt: Statement, warn_shadowing: bool) -> String {
    let mut sink = DiagnosticsSink::new();
    let result = resolve(&stmt, warn_shadowing, &mut sink);
    let diags = sink.into_sorted();
    let overlay = print_overlay(&stmt, &result.resolved_names);
    fmt_overlay_with_diags(overlay, &diags)
}

// ---------------------------------------------------------------------------
// Also run check_kinds so kind-mismatch E2007/E2008 appear in the snapshots.
// ---------------------------------------------------------------------------
fn pipeline_stmt_with_kinds(stmt: Statement, warn_shadowing: bool) -> String {
    let mut sink = DiagnosticsSink::new();
    let result = resolve(&stmt, warn_shadowing, &mut sink);
    cyrs_sema::kinds::check_kinds(&stmt, &mut sink);
    let diags = sink.into_sorted();
    let overlay = print_overlay(&stmt, &result.resolved_names);
    fmt_overlay_with_diags(overlay, &diags)
}

// ---------------------------------------------------------------------------
// 01: Simple MATCH–RETURN with one Node binding
// ---------------------------------------------------------------------------
#[test]
fn snap_01_simple_match_return() {
    insta::assert_snapshot!("01_simple_match_return", pipeline("MATCH (n) RETURN n"));
}

// ---------------------------------------------------------------------------
// 02: Labeled MATCH–RETURN
// ---------------------------------------------------------------------------
#[test]
fn snap_02_labeled_match_return() {
    insta::assert_snapshot!(
        "02_labeled_match_return",
        pipeline("MATCH (p:Person) RETURN p")
    );
}

// ---------------------------------------------------------------------------
// 03: Multi-pattern: Node + Relationship bindings
// ---------------------------------------------------------------------------
#[test]
fn snap_03_multi_pattern_node_rel() {
    insta::assert_snapshot!(
        "03_multi_pattern_node_rel",
        pipeline("MATCH (a)-[r:KNOWS]->(b) RETURN a, r, b")
    );
}

// ---------------------------------------------------------------------------
// 04: Three-hop chain
// ---------------------------------------------------------------------------
#[test]
fn snap_04_three_hop_chain() {
    insta::assert_snapshot!(
        "04_three_hop_chain",
        pipeline("MATCH (a)-[:R1]->(b)-[:R2]->(c) RETURN a, b, c")
    );
}

// ---------------------------------------------------------------------------
// 05: OPTIONAL MATCH
// ---------------------------------------------------------------------------
#[test]
fn snap_05_optional_match() {
    insta::assert_snapshot!(
        "05_optional_match",
        pipeline("OPTIONAL MATCH (a:Person) RETURN a")
    );
}

// ---------------------------------------------------------------------------
// 06: WHERE predicate on bound var
// ---------------------------------------------------------------------------
#[test]
fn snap_06_where_predicate() {
    insta::assert_snapshot!(
        "06_where_predicate",
        pipeline("MATCH (n) WHERE n.age > 18 RETURN n")
    );
}

// ---------------------------------------------------------------------------
// 07: RETURN property access
// ---------------------------------------------------------------------------
#[test]
fn snap_07_return_prop_access() {
    insta::assert_snapshot!(
        "07_return_prop_access",
        pipeline("MATCH (n:Person) RETURN n.name")
    );
}

// ---------------------------------------------------------------------------
// 08: RETURN DISTINCT
// ---------------------------------------------------------------------------
#[test]
fn snap_08_return_distinct() {
    insta::assert_snapshot!(
        "08_return_distinct",
        pipeline("MATCH (n) RETURN DISTINCT n.name")
    );
}

// ---------------------------------------------------------------------------
// 09: Multi-binding RETURN, all kinds via hand-constructed HIR
//     (Node n, Relationship r, Value x)
// ---------------------------------------------------------------------------
#[test]
fn snap_09_multi_binding_all_kinds() {
    let mut stmt = Statement::new(zero_range());
    let n = intern_var(&mut stmt, "n", VarKind::Node);
    let r = intern_var(&mut stmt, "r", VarKind::Relationship);
    let x = intern_var(&mut stmt, "x", VarKind::Value);

    // MATCH (n)-[r]->()
    let mid = alloc(&mut stmt);
    let nid_a = alloc(&mut stmt);
    let rid = alloc(&mut stmt);
    let nid_b = alloc(&mut stmt);
    stmt.clauses.push(Clause::Match {
        id: mid,
        optional: false,
        pattern: Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![
                    PatternElement::Node {
                        id: nid_a,
                        bind: Some(n),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    },
                    PatternElement::Rel {
                        id: rid,
                        bind: Some(r),
                        types: vec![SmolStr::new("KNOWS")],
                        direction: Direction::Outgoing,
                        length: RelLength::Single,
                        props: None,
                        span: zero_range(),
                    },
                    PatternElement::Node {
                        id: nid_b,
                        bind: None,
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    },
                ],
            }],
        },
        span: zero_range(),
    });
    // UNWIND [1,2,3] AS x
    let uid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Unwind {
        id: uid,
        list: Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
        bind: x,
        span: zero_range(),
    });
    // RETURN n, r, x
    let ret_id = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: ret_id,
        projections: vec![
            Projection {
                expr: Expr::Var(n),
                alias: None,
                span: zero_range(),
            },
            Projection {
                expr: Expr::Var(r),
                alias: None,
                span: zero_range(),
            },
            Projection {
                expr: Expr::Var(x),
                alias: None,
                span: zero_range(),
            },
        ],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("09_multi_binding_all_kinds", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 10: Unresolved variable → E1001 in overlay
// ---------------------------------------------------------------------------
#[test]
fn snap_10_unresolved_variable() {
    insta::assert_snapshot!("10_unresolved_variable", pipeline("MATCH (n) RETURN z"));
}

// ---------------------------------------------------------------------------
// 11: WITH barrier — projected name visible after
// ---------------------------------------------------------------------------
#[test]
fn snap_11_with_barrier_projected() {
    let mut stmt = Statement::new(zero_range());
    let a = intern_var(&mut stmt, "a", VarKind::Node);
    let b = intern_var(&mut stmt, "b", VarKind::Node);

    for v in [a, b] {
        let id = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(v),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
    }
    // WITH a  (b not projected)
    let wid = alloc(&mut stmt);
    stmt.clauses.push(Clause::With {
        id: wid,
        projections: vec![Projection {
            expr: Expr::Var(a),
            alias: None,
            span: zero_range(),
        }],
        filter: None,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });
    // RETURN a
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Var(a),
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("11_with_barrier_projected", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 12: WITH barrier — dropped var → E1001 after barrier
// ---------------------------------------------------------------------------
#[test]
fn snap_12_with_barrier_dropped() {
    let mut stmt = Statement::new(zero_range());
    let a = intern_var(&mut stmt, "a", VarKind::Node);
    let b = intern_var(&mut stmt, "b", VarKind::Node);

    for v in [a, b] {
        let id = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(v),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
    }
    // WITH a — drops b
    let wid = alloc(&mut stmt);
    stmt.clauses.push(Clause::With {
        id: wid,
        projections: vec![Projection {
            expr: Expr::Var(a),
            alias: None,
            span: zero_range(),
        }],
        filter: None,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });
    // RETURN b — unresolved → E1001
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Unresolved(SmolStr::new("b")),
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("12_with_barrier_dropped", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 13: WITH alias — `WITH a AS b`
// ---------------------------------------------------------------------------
#[test]
fn snap_13_with_alias() {
    let mut stmt = Statement::new(zero_range());
    let a = intern_var(&mut stmt, "a", VarKind::Node);
    let b = intern_var(&mut stmt, "b", VarKind::Value);

    // MATCH (a)
    let mid = alloc(&mut stmt);
    let nid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Match {
        id: mid,
        optional: false,
        pattern: Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![PatternElement::Node {
                    id: nid,
                    bind: Some(a),
                    labels: vec![],
                    props: None,
                    span: zero_range(),
                }],
            }],
        },
        span: zero_range(),
    });
    // WITH a AS b
    let wid = alloc(&mut stmt);
    stmt.clauses.push(Clause::With {
        id: wid,
        projections: vec![Projection {
            expr: Expr::Var(a),
            alias: Some(SmolStr::new("b")),
            span: zero_range(),
        }],
        filter: None,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });
    // RETURN b
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Var(b),
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("13_with_alias", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 14: UNWIND → Value binding
// ---------------------------------------------------------------------------
#[test]
fn snap_14_unwind_value() {
    let mut stmt = Statement::new(zero_range());
    let x = intern_var(&mut stmt, "x", VarKind::Value);

    let uid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Unwind {
        id: uid,
        list: Expr::List(vec![Expr::Int(1), Expr::Int(2)]),
        bind: x,
        span: zero_range(),
    });
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Var(x),
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("14_unwind_value", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 15: Shadowing inside nested scopes (warn=true)
// ---------------------------------------------------------------------------
#[test]
fn snap_15_shadowing_warn() {
    let mut stmt = Statement::new(zero_range());
    let a = intern_var(&mut stmt, "a", VarKind::Node);

    // MATCH (a)
    {
        let id = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(a),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
    }
    // WITH a
    {
        let id = alloc(&mut stmt);
        stmt.clauses.push(Clause::With {
            id,
            projections: vec![Projection {
                expr: Expr::Var(a),
                alias: None,
                span: zero_range(),
            }],
            filter: None,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
    }
    // MATCH (a) again — rebind in post-WITH scope → shadow warning E1002
    {
        let id = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(a),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
    }
    // RETURN a
    {
        let id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id,
            projections: vec![Projection {
                expr: Expr::Var(a),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
    }

    insta::assert_snapshot!("15_shadowing_warn", pipeline_stmt(stmt, true));
}

// ---------------------------------------------------------------------------
// 16: Double WITH barrier — a survives both, b dropped
// ---------------------------------------------------------------------------
#[test]
fn snap_16_double_with_barrier() {
    let mut stmt = Statement::new(zero_range());
    let a = intern_var(&mut stmt, "a", VarKind::Node);
    let b = intern_var(&mut stmt, "b", VarKind::Node);
    let c = intern_var(&mut stmt, "c", VarKind::Node);

    for v in [a, b] {
        let id = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(v),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
    }
    // WITH a (drops b)
    {
        let wid = alloc(&mut stmt);
        stmt.clauses.push(Clause::With {
            id: wid,
            projections: vec![Projection {
                expr: Expr::Var(a),
                alias: None,
                span: zero_range(),
            }],
            filter: None,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
    }
    // MATCH (c)
    {
        let id = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(c),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
    }
    // WITH a, c
    {
        let wid = alloc(&mut stmt);
        stmt.clauses.push(Clause::With {
            id: wid,
            projections: vec![
                Projection {
                    expr: Expr::Var(a),
                    alias: None,
                    span: zero_range(),
                },
                Projection {
                    expr: Expr::Var(c),
                    alias: None,
                    span: zero_range(),
                },
            ],
            filter: None,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
    }
    // RETURN a, c
    {
        let rid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: rid,
            projections: vec![
                Projection {
                    expr: Expr::Var(a),
                    alias: None,
                    span: zero_range(),
                },
                Projection {
                    expr: Expr::Var(c),
                    alias: None,
                    span: zero_range(),
                },
            ],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
    }

    insta::assert_snapshot!("16_double_with_barrier", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 17: Kind-mismatch: Node var in arithmetic → E2007
// ---------------------------------------------------------------------------
#[test]
fn snap_17_kind_mismatch_node_in_arithmetic() {
    let mut stmt = Statement::new(zero_range());
    let n = intern_var(&mut stmt, "n", VarKind::Node);

    let mid = alloc(&mut stmt);
    let nid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Match {
        id: mid,
        optional: false,
        pattern: Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![PatternElement::Node {
                    id: nid,
                    bind: Some(n),
                    labels: vec![],
                    props: None,
                    span: zero_range(),
                }],
            }],
        },
        span: zero_range(),
    });
    // RETURN n + 1  — Node var in arithmetic context
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::BinOp {
                op: cyrs_hir::BinOp::Add,
                lhs: Box::new(Expr::Var(n)),
                rhs: Box::new(Expr::Int(1)),
            },
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!(
        "17_kind_mismatch_node_arithmetic",
        pipeline_stmt_with_kinds(stmt, false)
    );
}

// ---------------------------------------------------------------------------
// 18: Kind-mismatch: Path var in prop access → E2007
// ---------------------------------------------------------------------------
#[test]
fn snap_18_kind_mismatch_path_prop_access() {
    let mut stmt = Statement::new(zero_range());
    let p = intern_var(&mut stmt, "p", VarKind::Path);

    // RETURN p.length  — Path var in property-access context
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Prop {
                target: Box::new(Expr::Var(p)),
                prop: SmolStr::new("length"),
            },
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!(
        "18_kind_mismatch_path_prop_access",
        pipeline_stmt_with_kinds(stmt, false)
    );
}

// ---------------------------------------------------------------------------
// 19: List comprehension (desugared) — filter_var overlay
// ---------------------------------------------------------------------------
#[test]
fn snap_19_list_comprehension() {
    // Parse and desugar: MATCH (n) RETURN [x IN n.friends WHERE x.active | x.name]
    // The lowerer handles LIST_COMPREHENSION and binds x as VarKind::Value.
    insta::assert_snapshot!(
        "19_list_comprehension",
        pipeline("MATCH (n) RETURN [x IN n.friends WHERE x.active | x.name]")
    );
}

// ---------------------------------------------------------------------------
// 20: Pattern predicate in WHERE
// ---------------------------------------------------------------------------
#[test]
fn snap_20_pattern_predicate() {
    insta::assert_snapshot!(
        "20_pattern_predicate",
        pipeline("MATCH (a) WHERE (a)-[:KNOWS]->(:Person) RETURN a")
    );
}

// ---------------------------------------------------------------------------
// 21: Map projection with VarShorthand
// ---------------------------------------------------------------------------
#[test]
fn snap_21_map_projection() {
    // MATCH (a) RETURN a { .name, .age }
    insta::assert_snapshot!(
        "21_map_projection",
        pipeline("MATCH (a) RETURN a { .name, .age }")
    );
}

// ---------------------------------------------------------------------------
// 22: CREATE binds Node var
// ---------------------------------------------------------------------------
#[test]
fn snap_22_create_binds() {
    let mut stmt = Statement::new(zero_range());
    let n = intern_var(&mut stmt, "n", VarKind::Node);

    let cid = alloc(&mut stmt);
    let nid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Create {
        id: cid,
        pattern: Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![PatternElement::Node {
                    id: nid,
                    bind: Some(n),
                    labels: vec![SmolStr::new("Person")],
                    props: None,
                    span: zero_range(),
                }],
            }],
        },
        span: zero_range(),
    });
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Var(n),
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("22_create_binds", pipeline_stmt(stmt, false));
}

// ---------------------------------------------------------------------------
// 23: CALL … YIELD … binds Value vars
// ---------------------------------------------------------------------------
#[test]
fn snap_23_call_yield() {
    let mut stmt = Statement::new(zero_range());
    // Pre-register the yield column so the resolver can find it by name.
    let col = intern_var(&mut stmt, "name", VarKind::Value);

    let cid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Call {
        id: cid,
        procedure: SmolStr::new("db.labels"),
        args: vec![],
        yields: vec![YieldItem {
            name: SmolStr::new("name"),
            alias: None,
        }],
        span: zero_range(),
    });
    let rid = alloc(&mut stmt);
    stmt.clauses.push(Clause::Return {
        id: rid,
        projections: vec![Projection {
            expr: Expr::Var(col),
            alias: None,
            span: zero_range(),
        }],
        distinct: false,
        span: zero_range(),
        order_by: Vec::new(),
        skip: None,
        limit: None,
    });

    insta::assert_snapshot!("23_call_yield", pipeline_stmt(stmt, false));
}
