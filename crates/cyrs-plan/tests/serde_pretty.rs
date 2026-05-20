//! Integration tests for plan serde (JSON round-trip) and pretty-printer.
//! Spec 0001 §12 — plan serialisation and pretty-print.

#![cfg(test)]

use cyrs_plan::lower::PlanStatement;
use cyrs_plan::pretty::pretty;
use cyrs_plan::{
    AggExpr, BinOp, Direction, Expr, LabelSet, NodeSpec, OpId, OrderKey, Projection, ReadOp,
    RelLength, RelSpec, SortDir, UnionKind, VarId, WriteOp,
};
use indexmap::IndexMap;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Round-trip a `PlanStatement` through JSON serialisation + deserialisation.
/// Returns the deserialized plan; panics if they differ.
fn json_roundtrip(plan: &PlanStatement) -> PlanStatement {
    let json = serde_json::to_string(plan).expect("serialise");
    serde_json::from_str::<PlanStatement>(&json).expect("deserialise")
}

fn assert_roundtrip_eq(plan: &PlanStatement) {
    let rt = json_roundtrip(plan);
    // Compare JSON repr for equality (PlanStatement doesn't derive PartialEq
    // because of IndexMap). Compare via re-serialisation for determinism.
    let json1 = serde_json::to_string(plan).unwrap();
    let json2 = serde_json::to_string(&rt).unwrap();
    assert_eq!(json1, json2, "JSON round-trip produced different output");
}

/// Build a minimal read-only [`PlanStatement`] from a single root op.
fn read_plan(ops: Vec<ReadOp>) -> PlanStatement {
    PlanStatement {
        ops,
        write_ops: vec![],
        var_map: IndexMap::new(),
        params: IndexMap::new(),
    }
}

/// Build a write-only plan (no read ops).
fn write_plan(write_ops: Vec<WriteOp>) -> PlanStatement {
    PlanStatement {
        ops: vec![],
        write_ops,
        var_map: IndexMap::new(),
        params: IndexMap::new(),
    }
}

// ── JSON snapshot tests (11 cases) ───────────────────────────────────────────

#[test]
fn json_snap_source_all_nodes() {
    let plan = read_plan(vec![ReadOp::Source {
        label: None,
        bind: VarId(0),
    }]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_source_all", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_source_with_label() {
    let plan = read_plan(vec![ReadOp::Source {
        label: Some(LabelSet(vec!["Person".into()])),
        bind: VarId(0),
    }]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_source_label", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_filter() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "age".into(),
                }),
                rhs: Box::new(Expr::Int(18)),
            },
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_filter", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_project() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![
                Projection {
                    expr: Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "name".into(),
                    },
                    alias: "name".into(),
                },
                Projection {
                    expr: Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "age".into(),
                    },
                    alias: "age".into(),
                },
            ],
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_project", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_aggregate() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![Expr::Var(VarId(0))],
            aggs: vec![AggExpr {
                func: "count".into(),
                args: vec![Expr::Var(VarId(0))],
                distinct: false,
            }],
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_aggregate", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_order_skip_limit() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::OrderBy {
            input: OpId(0),
            keys: vec![OrderKey {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                dir: SortDir::Asc,
            }],
        },
        ReadOp::Skip {
            input: OpId(1),
            count: Expr::Int(10),
        },
        ReadOp::Limit {
            input: OpId(2),
            count: Expr::Int(5),
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!(
        "json_order_skip_limit",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn json_snap_union_all() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["Animal".into()])),
            bind: VarId(1),
        },
        ReadOp::Union {
            left: OpId(0),
            right: OpId(1),
            kind: UnionKind::All,
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_union_all", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_union_distinct() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["Robot".into()])),
            bind: VarId(1),
        },
        ReadOp::Union {
            left: OpId(0),
            right: OpId(1),
            kind: UnionKind::Distinct,
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_union_distinct", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_expand() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["KNOWS".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec!["Person".into()]),
                properties: None,
            },
            bind_rel: VarId(1),
            bind_to: VarId(2),
        },
    ]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_expand", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_write_create_node() {
    let plan = write_plan(vec![WriteOp::CreateNode {
        labels: vec!["Person".into()],
        props: Expr::Map(vec![("name".into(), Expr::String("Alice".into()))]),
        bind: Some(VarId(0)),
    }]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!(
        "json_write_create_node",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn json_snap_write_merge_with_on_create() {
    let plan = write_plan(vec![WriteOp::MergeNode {
        labels: vec!["Person".into()],
        props: Expr::Map(vec![("name".into(), Expr::String("Bob".into()))]),
        on_create: vec![WriteOp::SetProperty {
            target: VarId(0),
            prop: "created".into(),
            value: Expr::Bool(true),
        }],
        on_match: vec![WriteOp::SetProperty {
            target: VarId(0),
            prop: "updated".into(),
            value: Expr::Bool(true),
        }],
        bind: Some(VarId(0)),
    }]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!(
        "json_write_merge_node",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn json_snap_expr_all_literals() {
    // Cover all Expr literal kinds in a single plan.
    let plan = read_plan(vec![ReadOp::Filter {
        input: OpId(0),
        predicate: Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Bool(true)),
            rhs: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Int(42)),
                rhs: Box::new(Expr::Float(3.0)),
            }),
        },
    }]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_expr_literals", serde_json::to_value(&plan).unwrap());
}

#[test]
fn json_snap_expr_case() {
    let plan = read_plan(vec![ReadOp::Project {
        input: OpId(0),
        items: vec![Projection {
            expr: Expr::Case {
                scrutinee: None,
                arms: vec![(Expr::Bool(true), Expr::String("yes".into()))],
                otherwise: Some(Box::new(Expr::String("no".into()))),
            },
            alias: "result".into(),
        }],
    }]);
    assert_roundtrip_eq(&plan);
    insta::assert_json_snapshot!("json_expr_case", serde_json::to_value(&plan).unwrap());
}

// ── JSON round-trip unit tests (representative plans) ────────────────────────

#[test]
fn roundtrip_empty_plan() {
    let plan = PlanStatement {
        ops: vec![],
        write_ops: vec![],
        var_map: IndexMap::new(),
        params: IndexMap::new(),
    };
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_nested_filter_project() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "age".into(),
                }),
                rhs: Box::new(Expr::Int(18)),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
    ]);
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_write_delete() {
    let plan = write_plan(vec![WriteOp::Delete {
        targets: vec![Expr::Var(VarId(0))],
        detach: true,
    }]);
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_unwind() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Unwind {
            input: OpId(0),
            list: Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
            bind: VarId(1),
        },
    ]);
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_param_expr() {
    let plan = read_plan(vec![ReadOp::Filter {
        input: OpId(0),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(Expr::Prop {
                target: Box::new(Expr::Var(VarId(0))),
                prop: "id".into(),
            }),
            rhs: Box::new(Expr::Param {
                name: "userId".into(),
            }),
        },
    }]);
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_optional_join() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::OptionalJoin {
            input: OpId(0),
            pattern: Box::new(ReadOp::Source {
                label: Some(LabelSet(vec!["Post".into()])),
                bind: VarId(1),
            }),
        },
    ]);
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_distinct() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Distinct { input: OpId(0) },
    ]);
    assert_roundtrip_eq(&plan);
}

#[test]
fn roundtrip_with_filter() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::With {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
            filter: Some(Expr::Bool(true)),
        },
    ]);
    assert_roundtrip_eq(&plan);
}

// ── Pretty-print snapshot tests (11 cases) ───────────────────────────────────

#[test]
fn pretty_snap_source_all() {
    let plan = read_plan(vec![ReadOp::Source {
        label: None,
        bind: VarId(0),
    }]);
    insta::assert_snapshot!("pretty_source_all", pretty(&plan));
}

#[test]
fn pretty_snap_source_label() {
    let plan = read_plan(vec![ReadOp::Source {
        label: Some(LabelSet(vec!["Person".into()])),
        bind: VarId(0),
    }]);
    insta::assert_snapshot!("pretty_source_label", pretty(&plan));
}

#[test]
fn pretty_snap_filter_project() {
    // Project [n.name AS name]
    //   Filter (n.age > 18)
    //     Source (:Person AS $0)
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "age".into(),
                }),
                rhs: Box::new(Expr::Int(18)),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("pretty_filter_project", pretty(&plan));
}

#[test]
fn pretty_snap_aggregate() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![Expr::Var(VarId(0))],
            aggs: vec![AggExpr {
                func: "count".into(),
                args: vec![Expr::Var(VarId(0))],
                distinct: true,
            }],
        },
    ]);
    insta::assert_snapshot!("pretty_aggregate", pretty(&plan));
}

#[test]
fn pretty_snap_order_skip_limit() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::OrderBy {
            input: OpId(0),
            keys: vec![OrderKey {
                expr: Expr::Var(VarId(0)),
                dir: SortDir::Desc,
            }],
        },
        ReadOp::Skip {
            input: OpId(1),
            count: Expr::Int(10),
        },
        ReadOp::Limit {
            input: OpId(2),
            count: Expr::Int(5),
        },
    ]);
    insta::assert_snapshot!("pretty_order_skip_limit", pretty(&plan));
}

#[test]
fn pretty_snap_union_all() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["Animal".into()])),
            bind: VarId(1),
        },
        ReadOp::Union {
            left: OpId(0),
            right: OpId(1),
            kind: UnionKind::All,
        },
    ]);
    insta::assert_snapshot!("pretty_union_all", pretty(&plan));
}

#[test]
fn pretty_snap_union_distinct() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["Robot".into()])),
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![Projection {
                expr: Expr::Var(VarId(1)),
                alias: "n".into(),
            }],
        },
        ReadOp::Union {
            left: OpId(1),
            right: OpId(3),
            kind: UnionKind::Distinct,
        },
    ]);
    insta::assert_snapshot!("pretty_union_distinct", pretty(&plan));
}

#[test]
fn pretty_snap_write_ops() {
    let plan = PlanStatement {
        ops: vec![ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        }],
        write_ops: vec![
            WriteOp::SetProperty {
                target: VarId(0),
                prop: "age".into(),
                value: Expr::Int(30),
            },
            WriteOp::SetLabels {
                target: VarId(0),
                labels: vec!["Admin".into()],
            },
        ],
        var_map: IndexMap::new(),
        params: IndexMap::new(),
    };
    insta::assert_snapshot!("pretty_write_ops", pretty(&plan));
}

#[test]
fn pretty_snap_delete_detach() {
    let plan = write_plan(vec![WriteOp::Delete {
        targets: vec![Expr::Var(VarId(0))],
        detach: true,
    }]);
    insta::assert_snapshot!("pretty_delete_detach", pretty(&plan));
}

#[test]
fn pretty_snap_optional_join() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::OptionalJoin {
            input: OpId(0),
            pattern: Box::new(ReadOp::Source {
                label: Some(LabelSet(vec!["Post".into()])),
                bind: VarId(1),
            }),
        },
    ]);
    insta::assert_snapshot!("pretty_optional_join", pretty(&plan));
}

#[test]
fn pretty_snap_expand() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["KNOWS".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec!["Person".into()]),
                properties: None,
            },
            bind_rel: VarId(1),
            bind_to: VarId(2),
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "a".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "b".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("pretty_expand", pretty(&plan));
}

#[test]
fn pretty_snap_merge_node() {
    let plan = write_plan(vec![WriteOp::MergeNode {
        labels: vec!["Person".into()],
        props: Expr::Map(vec![("name".into(), Expr::String("Carol".into()))]),
        on_create: vec![WriteOp::SetProperty {
            target: VarId(0),
            prop: "created_at".into(),
            value: Expr::Int(0),
        }],
        on_match: vec![WriteOp::SetProperty {
            target: VarId(0),
            prop: "updated_at".into(),
            value: Expr::Int(1),
        }],
        bind: Some(VarId(0)),
    }]);
    insta::assert_snapshot!("pretty_merge_node", pretty(&plan));
}

// ── Pretty determinism ────────────────────────────────────────────────────────

#[test]
fn pretty_is_deterministic() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "active".into(),
                }),
                rhs: Box::new(Expr::Bool(true)),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "n".into(),
            }],
        },
    ]);
    let s1 = pretty(&plan);
    let s2 = pretty(&plan);
    assert_eq!(s1, s2, "pretty-print must be deterministic");
}

// ── JSON determinism ──────────────────────────────────────────────────────────

#[test]
fn json_serialisation_is_deterministic() {
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "a".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "b".into(),
                },
            ],
        },
    ]);
    let j1 = serde_json::to_string(&plan).unwrap();
    let j2 = serde_json::to_string(&plan).unwrap();
    assert_eq!(j1, j2, "JSON serialisation must be deterministic");
}
