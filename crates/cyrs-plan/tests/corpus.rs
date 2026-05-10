//! Plan snapshot corpus — wide coverage of the HIR→Plan lowering +
//! pretty-print chain (spec 0001 §17.2).
//!
//! Snapshot names are prefixed `corpus_pretty_*` (pretty-print) or
//! `corpus_json_*` (JSON serde). This file extends — never replaces —
//! the snapshots in `serde_pretty.rs`.

#![cfg(test)]

use cyrs_plan::lower::PlanStatement;
use cyrs_plan::pretty::pretty;
use cyrs_plan::{
    AggExpr, BinOp, Direction, Expr, LabelSet, ListPredKind, NodeSpec, OpId, OrderKey, Projection,
    ReadOp, RelLength, RelSpec, SortDir, UnaryOp, UnionKind, VarId, WriteOp,
};
use indexmap::IndexMap;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_plan(ops: Vec<ReadOp>) -> PlanStatement {
    PlanStatement {
        ops,
        write_ops: vec![],
        var_map: IndexMap::new(),
    }
}

fn write_plan(write_ops: Vec<WriteOp>) -> PlanStatement {
    PlanStatement {
        ops: vec![],
        write_ops,
        var_map: IndexMap::new(),
    }
}

fn rw_plan(ops: Vec<ReadOp>, write_ops: Vec<WriteOp>) -> PlanStatement {
    PlanStatement {
        ops,
        write_ops,
        var_map: IndexMap::new(),
    }
}

fn assert_json_roundtrip(plan: &PlanStatement) {
    let json = serde_json::to_string(plan).expect("serialise");
    let rt = serde_json::from_str::<PlanStatement>(&json).expect("deserialise");
    let json2 = serde_json::to_string(&rt).expect("re-serialise");
    assert_eq!(json, json2, "JSON round-trip produced different output");
}

// ── §1 Multi-clause patterns ──────────────────────────────────────────────────

#[test]
fn corpus_pretty_match_where_return() {
    // MATCH (n:Person) WHERE n.age > 18 RETURN n.name, n.age
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
    insta::assert_snapshot!("corpus_pretty_match_where_return", pretty(&plan));
}

#[test]
fn corpus_pretty_match_with_where_return() {
    // MATCH (n:Person) WITH n WHERE n.active = true RETURN n
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::With {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
            filter: Some(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "active".into(),
                }),
                rhs: Box::new(Expr::Bool(true)),
            }),
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_match_with_where_return", pretty(&plan));
}

#[test]
fn corpus_pretty_match_with_match_return() {
    // MATCH (n:Person) WITH n MATCH (n)-[:KNOWS]->(m) RETURN n, m
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::With {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
            filter: None,
        },
        ReadOp::Expand {
            input: OpId(1),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["KNOWS".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
                properties: None,
            },
            bind_rel: VarId(1),
            bind_to: VarId(2),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_match_with_match_return", pretty(&plan));
}

#[test]
fn corpus_pretty_optional_match_chained() {
    // MATCH (n:Person) OPTIONAL MATCH (n)-[:LIKES]->(p:Post) RETURN n, p
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::OptionalJoin {
            input: OpId(0),
            pattern: Box::new(ReadOp::Expand {
                input: OpId(0),
                from: VarId(0),
                rel: RelSpec {
                    types: vec!["LIKES".into()],
                    direction: Direction::Outgoing,
                    length: RelLength::Single,
                    properties: None,
                },
                to: NodeSpec {
                    labels: LabelSet(vec!["Post".into()]),
                    properties: None,
                },
                bind_rel: VarId(1),
                bind_to: VarId(2),
            }),
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "p".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_optional_match_chained", pretty(&plan));
}

// ── §2 Pattern shapes ─────────────────────────────────────────────────────────

#[test]
fn corpus_pretty_single_node_no_label() {
    // MATCH (n) RETURN n
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_single_node_no_label", pretty(&plan));
}

#[test]
fn corpus_pretty_node_multi_label() {
    // MATCH (n:Person:Employee) RETURN n
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into(), "Employee".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_node_multi_label", pretty(&plan));
}

#[test]
fn corpus_pretty_node_with_props() {
    // MATCH (n:Person {name: "Alice"}) RETURN n
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::Map(vec![("name".into(), Expr::String("Alice".into()))]),
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_node_with_props", pretty(&plan));
}

#[test]
fn corpus_pretty_expand_incoming() {
    // MATCH (n)<-[:FOLLOWS]-(m) RETURN n, m
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["FOLLOWS".into()],
                direction: Direction::Incoming,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
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
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expand_incoming", pretty(&plan));
}

#[test]
fn corpus_pretty_expand_undirected() {
    // MATCH (n)-[:RELATED]-(m) RETURN n, m
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["RELATED".into()],
                direction: Direction::Undirected,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
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
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expand_undirected", pretty(&plan));
}

#[test]
fn corpus_pretty_expand_varlen() {
    // MATCH (n)-[:KNOWS*1..3]->(m) RETURN n, m
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
                length: RelLength::Variable {
                    min: Some(1),
                    max: Some(3),
                },
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
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
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expand_varlen", pretty(&plan));
}

#[test]
fn corpus_pretty_expand_varlen_unbounded() {
    // MATCH (n)-[:KNOWS*]->(m) RETURN n, m
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["KNOWS".into()],
                direction: Direction::Outgoing,
                length: RelLength::Variable {
                    min: None,
                    max: None,
                },
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
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
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expand_varlen_unbounded", pretty(&plan));
}

#[test]
fn corpus_pretty_expand_multi_rel_type() {
    // MATCH (n)-[:KNOWS|:FOLLOWS]->(m) RETURN n, m
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["KNOWS".into(), "FOLLOWS".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
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
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expand_multi_rel_type", pretty(&plan));
}

#[test]
fn corpus_pretty_rel_with_props() {
    // MATCH (n)-[r:KNOWS {since: 2020}]->(m) RETURN n, r, m
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["KNOWS".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: Some(Expr::Map(vec![("since".into(), Expr::Int(2020))])),
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
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
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(1)),
                    alias: "r".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "m".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_rel_with_props", pretty(&plan));
}

#[test]
fn corpus_pretty_chained_expands() {
    // MATCH (a)-[:KNOWS]->(b)-[:LIKES]->(c) RETURN a, b, c
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
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
                labels: LabelSet(vec![]),
                properties: None,
            },
            bind_rel: VarId(1),
            bind_to: VarId(2),
        },
        ReadOp::Expand {
            input: OpId(1),
            from: VarId(2),
            rel: RelSpec {
                types: vec!["LIKES".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec![]),
                properties: None,
            },
            bind_rel: VarId(3),
            bind_to: VarId(4),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "a".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "b".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(4)),
                    alias: "c".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_chained_expands", pretty(&plan));
}

// ── §3 Aggregation ────────────────────────────────────────────────────────────

#[test]
fn corpus_pretty_agg_count_star() {
    // MATCH (n) RETURN count(n) AS cnt
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![],
            aggs: vec![AggExpr {
                func: "count".into(),
                args: vec![Expr::Var(VarId(0))],
                distinct: false,
            }],
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "cnt".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_agg_count_star", pretty(&plan));
}

#[test]
fn corpus_pretty_agg_sum_avg() {
    // MATCH (n:Person) RETURN sum(n.salary) AS total, avg(n.salary) AS mean
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![],
            aggs: vec![
                AggExpr {
                    func: "sum".into(),
                    args: vec![Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "salary".into(),
                    }],
                    distinct: false,
                },
                AggExpr {
                    func: "avg".into(),
                    args: vec![Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "salary".into(),
                    }],
                    distinct: false,
                },
            ],
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "total".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(1)),
                    alias: "mean".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_agg_sum_avg", pretty(&plan));
}

#[test]
fn corpus_pretty_agg_min_max_collect() {
    // MATCH (n:Person) RETURN min(n.age), max(n.age), collect(n.name)
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![],
            aggs: vec![
                AggExpr {
                    func: "min".into(),
                    args: vec![Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "age".into(),
                    }],
                    distinct: false,
                },
                AggExpr {
                    func: "max".into(),
                    args: vec![Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "age".into(),
                    }],
                    distinct: false,
                },
                AggExpr {
                    func: "collect".into(),
                    args: vec![Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "name".into(),
                    }],
                    distinct: false,
                },
            ],
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "min_age".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(1)),
                    alias: "max_age".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "names".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_agg_min_max_collect", pretty(&plan));
}

#[test]
fn corpus_pretty_agg_count_distinct() {
    // MATCH (n) RETURN count(DISTINCT n.city) AS cities
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![],
            aggs: vec![AggExpr {
                func: "count".into(),
                args: vec![Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "city".into(),
                }],
                distinct: true,
            }],
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "cities".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_agg_count_distinct", pretty(&plan));
}

#[test]
fn corpus_pretty_agg_groupby_with() {
    // MATCH (n:Person)-[:WORKS_AT]->(c:Company)
    // WITH c.name AS company, count(n) AS headcount
    // RETURN company, headcount
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Expand {
            input: OpId(0),
            from: VarId(0),
            rel: RelSpec {
                types: vec!["WORKS_AT".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec!["Company".into()]),
                properties: None,
            },
            bind_rel: VarId(1),
            bind_to: VarId(2),
        },
        ReadOp::Aggregate {
            input: OpId(1),
            keys: vec![Expr::Prop {
                target: Box::new(Expr::Var(VarId(2))),
                prop: "name".into(),
            }],
            aggs: vec![AggExpr {
                func: "count".into(),
                args: vec![Expr::Var(VarId(0))],
                distinct: false,
            }],
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![
                Projection {
                    expr: Expr::Prop {
                        target: Box::new(Expr::Var(VarId(2))),
                        prop: "name".into(),
                    },
                    alias: "company".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(3)),
                    alias: "headcount".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_agg_groupby_with", pretty(&plan));
}

// ── §4 DISTINCT + ORDER BY + SKIP + LIMIT ────────────────────────────────────

#[test]
fn corpus_pretty_distinct_return() {
    // MATCH (n:Person) RETURN DISTINCT n.city
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "city".into(),
                },
                alias: "city".into(),
            }],
        },
        ReadOp::Distinct { input: OpId(1) },
    ]);
    insta::assert_snapshot!("corpus_pretty_distinct_return", pretty(&plan));
}

#[test]
fn corpus_pretty_order_by_multi_col() {
    // MATCH (n:Person) RETURN n.name, n.age ORDER BY n.age DESC, n.name ASC
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
        ReadOp::OrderBy {
            input: OpId(1),
            keys: vec![
                OrderKey {
                    expr: Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "age".into(),
                    },
                    dir: SortDir::Desc,
                },
                OrderKey {
                    expr: Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "name".into(),
                    },
                    dir: SortDir::Asc,
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_order_by_multi_col", pretty(&plan));
}

#[test]
fn corpus_pretty_skip_limit_combo() {
    // MATCH (n) RETURN n SKIP 20 LIMIT 10
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
        ReadOp::Skip {
            input: OpId(1),
            count: Expr::Int(20),
        },
        ReadOp::Limit {
            input: OpId(2),
            count: Expr::Int(10),
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_skip_limit_combo", pretty(&plan));
}

#[test]
fn corpus_pretty_order_skip_limit_full() {
    // MATCH (n:Person) RETURN n.name ORDER BY n.name ASC SKIP 5 LIMIT 25
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
        ReadOp::OrderBy {
            input: OpId(1),
            keys: vec![OrderKey {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                dir: SortDir::Asc,
            }],
        },
        ReadOp::Skip {
            input: OpId(2),
            count: Expr::Int(5),
        },
        ReadOp::Limit {
            input: OpId(3),
            count: Expr::Int(25),
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_order_skip_limit_full", pretty(&plan));
}

#[test]
fn corpus_pretty_skip_with_param() {
    // MATCH (n) RETURN n SKIP $offset LIMIT $count
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
        ReadOp::Skip {
            input: OpId(1),
            count: Expr::Param {
                name: "offset".into(),
            },
        },
        ReadOp::Limit {
            input: OpId(2),
            count: Expr::Param {
                name: "count".into(),
            },
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_skip_with_param", pretty(&plan));
}

// ── §5 UNION variants ─────────────────────────────────────────────────────────

#[test]
fn corpus_pretty_union_all_two_branches() {
    // MATCH (n:Person) RETURN n.name
    // UNION ALL
    // MATCH (n:Robot) RETURN n.name
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["Robot".into()])),
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(1))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
        ReadOp::Union {
            left: OpId(1),
            right: OpId(3),
            kind: UnionKind::All,
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_union_all_two_branches", pretty(&plan));
}

#[test]
fn corpus_pretty_union_distinct_two_branches() {
    // MATCH (n:Person) RETURN n.name
    // UNION
    // MATCH (n:Animal) RETURN n.name
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["Animal".into()])),
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(1))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        },
        ReadOp::Union {
            left: OpId(1),
            right: OpId(3),
            kind: UnionKind::Distinct,
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_union_distinct_two_branches", pretty(&plan));
}

#[test]
fn corpus_pretty_union_nested() {
    // Three-way union: (A UNION ALL B) UNION C
    let plan = read_plan(vec![
        // Branch A
        ReadOp::Source {
            label: Some(LabelSet(vec!["A".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "x".into(),
            }],
        },
        // Branch B
        ReadOp::Source {
            label: Some(LabelSet(vec!["B".into()])),
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![Projection {
                expr: Expr::Var(VarId(1)),
                alias: "x".into(),
            }],
        },
        // Union A+B
        ReadOp::Union {
            left: OpId(1),
            right: OpId(3),
            kind: UnionKind::All,
        },
        // Branch C
        ReadOp::Source {
            label: Some(LabelSet(vec!["C".into()])),
            bind: VarId(2),
        },
        ReadOp::Project {
            input: OpId(5),
            items: vec![Projection {
                expr: Expr::Var(VarId(2)),
                alias: "x".into(),
            }],
        },
        // Union (A+B)+C
        ReadOp::Union {
            left: OpId(4),
            right: OpId(6),
            kind: UnionKind::Distinct,
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_union_nested", pretty(&plan));
}

// ── §6 Write operations ───────────────────────────────────────────────────────

#[test]
fn corpus_pretty_create_node_chain() {
    // CREATE (a:Person {name: "Alice"}), (b:Person {name: "Bob"})
    let plan = write_plan(vec![
        WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![("name".into(), Expr::String("Alice".into()))]),
            bind: Some(VarId(0)),
        },
        WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![("name".into(), Expr::String("Bob".into()))]),
            bind: Some(VarId(1)),
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_create_node_chain", pretty(&plan));
}

#[test]
fn corpus_pretty_create_rel_chain() {
    // MATCH (a:Person), (b:Person) CREATE (a)-[:KNOWS]->(b)
    let plan = rw_plan(
        vec![
            ReadOp::Source {
                label: Some(LabelSet(vec!["Person".into()])),
                bind: VarId(0),
            },
            ReadOp::Source {
                label: Some(LabelSet(vec!["Person".into()])),
                bind: VarId(1),
            },
        ],
        vec![WriteOp::CreateRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "KNOWS".into(),
            props: Expr::Map(vec![]),
            bind: None,
        }],
    );
    insta::assert_snapshot!("corpus_pretty_create_rel_chain", pretty(&plan));
}

#[test]
fn corpus_pretty_create_with_rel_props() {
    // CREATE (a:Person)-[r:KNOWS {since: 2020}]->(b:Person)
    let plan = write_plan(vec![
        WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![]),
            bind: Some(VarId(0)),
        },
        WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![]),
            bind: Some(VarId(1)),
        },
        WriteOp::CreateRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "KNOWS".into(),
            props: Expr::Map(vec![("since".into(), Expr::Int(2020))]),
            bind: Some(VarId(2)),
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_create_with_rel_props", pretty(&plan));
}

#[test]
fn corpus_pretty_merge_node_on_create_match() {
    // MERGE (n:Person {email: $email})
    // ON CREATE SET n.created = true
    // ON MATCH SET n.updated = true
    let plan = write_plan(vec![WriteOp::MergeNode {
        labels: vec!["Person".into()],
        props: Expr::Map(vec![(
            "email".into(),
            Expr::Param {
                name: "email".into(),
            },
        )]),
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
    insta::assert_snapshot!("corpus_pretty_merge_node_on_create_match", pretty(&plan));
}

#[test]
fn corpus_pretty_merge_rel() {
    // MATCH (a:Person), (b:Person)
    // MERGE (a)-[r:FOLLOWS]->(b)
    // ON CREATE SET r.since = 2024
    let plan = rw_plan(
        vec![
            ReadOp::Source {
                label: Some(LabelSet(vec!["Person".into()])),
                bind: VarId(0),
            },
            ReadOp::Source {
                label: Some(LabelSet(vec!["Person".into()])),
                bind: VarId(1),
            },
        ],
        vec![WriteOp::MergeRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "FOLLOWS".into(),
            props: Expr::Map(vec![]),
            on_create: vec![WriteOp::SetProperty {
                target: VarId(2),
                prop: "since".into(),
                value: Expr::Int(2024),
            }],
            on_match: vec![],
            bind: Some(VarId(2)),
        }],
    );
    insta::assert_snapshot!("corpus_pretty_merge_rel", pretty(&plan));
}

#[test]
fn corpus_pretty_set_multiple_props() {
    // MATCH (n:Person) SET n.name = "Alice", n.age = 30, n.active = true
    let plan = rw_plan(
        vec![ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        }],
        vec![
            WriteOp::SetProperty {
                target: VarId(0),
                prop: "name".into(),
                value: Expr::String("Alice".into()),
            },
            WriteOp::SetProperty {
                target: VarId(0),
                prop: "age".into(),
                value: Expr::Int(30),
            },
            WriteOp::SetProperty {
                target: VarId(0),
                prop: "active".into(),
                value: Expr::Bool(true),
            },
        ],
    );
    insta::assert_snapshot!("corpus_pretty_set_multiple_props", pretty(&plan));
}

#[test]
fn corpus_pretty_set_labels() {
    // MATCH (n:Person) SET n:Admin:Moderator
    let plan = rw_plan(
        vec![ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        }],
        vec![WriteOp::SetLabels {
            target: VarId(0),
            labels: vec!["Admin".into(), "Moderator".into()],
        }],
    );
    insta::assert_snapshot!("corpus_pretty_set_labels", pretty(&plan));
}

#[test]
fn corpus_pretty_remove_prop_and_label() {
    // MATCH (n:Person) REMOVE n.age, n:Inactive
    let plan = rw_plan(
        vec![ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        }],
        vec![
            WriteOp::RemoveProperty {
                target: VarId(0),
                prop: "age".into(),
            },
            WriteOp::RemoveLabels {
                target: VarId(0),
                labels: vec!["Inactive".into()],
            },
        ],
    );
    insta::assert_snapshot!("corpus_pretty_remove_prop_and_label", pretty(&plan));
}

#[test]
fn corpus_pretty_delete_multiple() {
    // MATCH (n)-[r]->(m) DELETE r
    let plan = rw_plan(
        vec![
            ReadOp::Source {
                label: None,
                bind: VarId(0),
            },
            ReadOp::Expand {
                input: OpId(0),
                from: VarId(0),
                rel: RelSpec {
                    types: vec![],
                    direction: Direction::Outgoing,
                    length: RelLength::Single,
                    properties: None,
                },
                to: NodeSpec {
                    labels: LabelSet(vec![]),
                    properties: None,
                },
                bind_rel: VarId(1),
                bind_to: VarId(2),
            },
        ],
        vec![WriteOp::Delete {
            targets: vec![Expr::Var(VarId(1))],
            detach: false,
        }],
    );
    insta::assert_snapshot!("corpus_pretty_delete_multiple", pretty(&plan));
}

#[test]
fn corpus_pretty_detach_delete_node() {
    // MATCH (n:Person {name: "Alice"}) DETACH DELETE n
    let plan = rw_plan(
        vec![
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
                        prop: "name".into(),
                    }),
                    rhs: Box::new(Expr::String("Alice".into())),
                },
            },
        ],
        vec![WriteOp::Delete {
            targets: vec![Expr::Var(VarId(0))],
            detach: true,
        }],
    );
    insta::assert_snapshot!("corpus_pretty_detach_delete_node", pretty(&plan));
}

// ── §7 UNWIND ─────────────────────────────────────────────────────────────────

#[test]
fn corpus_pretty_unwind_list_literal() {
    // UNWIND [1, 2, 3] AS x RETURN x
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(99), // synthetic source
        },
        ReadOp::Unwind {
            input: OpId(0),
            list: Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "x".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_unwind_list_literal", pretty(&plan));
}

#[test]
fn corpus_pretty_unwind_param() {
    // UNWIND $items AS item RETURN item.name
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(99),
        },
        ReadOp::Unwind {
            input: OpId(0),
            list: Expr::Param {
                name: "items".into(),
            },
            bind: VarId(0),
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
    insta::assert_snapshot!("corpus_pretty_unwind_param", pretty(&plan));
}

#[test]
fn corpus_pretty_unwind_then_match() {
    // UNWIND $ids AS id
    // MATCH (n) WHERE n.id = id
    // RETURN n
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(99),
        },
        ReadOp::Unwind {
            input: OpId(0),
            list: Expr::Param { name: "ids".into() },
            bind: VarId(0),
        },
        ReadOp::Source {
            label: None,
            bind: VarId(1),
        },
        ReadOp::Filter {
            input: OpId(2),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(1))),
                    prop: "id".into(),
                }),
                rhs: Box::new(Expr::Var(VarId(0))),
            },
        },
        ReadOp::Project {
            input: OpId(3),
            items: vec![Projection {
                expr: Expr::Var(VarId(1)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_unwind_then_match", pretty(&plan));
}

// ── §8 Expression trees ───────────────────────────────────────────────────────

#[test]
fn corpus_pretty_expr_nested_binops() {
    // WHERE (n.age > 18 AND n.active = true) OR n.role = "admin"
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::Or,
                lhs: Box::new(Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "age".into(),
                        }),
                        rhs: Box::new(Expr::Int(18)),
                    }),
                    rhs: Box::new(Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: Box::new(Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "active".into(),
                        }),
                        rhs: Box::new(Expr::Bool(true)),
                    }),
                }),
                rhs: Box::new(Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "role".into(),
                    }),
                    rhs: Box::new(Expr::String("admin".into())),
                }),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_nested_binops", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_function_calls() {
    // RETURN toLower(n.name), size(n.friends), toString(n.age)
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![
                Projection {
                    expr: Expr::Call {
                        func: "toLower".into(),
                        args: vec![Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "name".into(),
                        }],
                    },
                    alias: "lower_name".into(),
                },
                Projection {
                    expr: Expr::Call {
                        func: "size".into(),
                        args: vec![Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "friends".into(),
                        }],
                    },
                    alias: "friend_count".into(),
                },
                Projection {
                    expr: Expr::Call {
                        func: "toString".into(),
                        args: vec![Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "age".into(),
                        }],
                    },
                    alias: "age_str".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_function_calls", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_case_simple() {
    // CASE n.status WHEN 'active' THEN 1 WHEN 'inactive' THEN 0 ELSE -1 END
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Case {
                    scrutinee: Some(Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "status".into(),
                    })),
                    arms: vec![
                        (Expr::String("active".into()), Expr::Int(1)),
                        (Expr::String("inactive".into()), Expr::Int(0)),
                    ],
                    otherwise: Some(Box::new(Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        operand: Box::new(Expr::Int(1)),
                    })),
                },
                alias: "score".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_case_simple", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_case_searched() {
    // CASE WHEN n.age < 18 THEN 'minor' WHEN n.age < 65 THEN 'adult' ELSE 'senior' END
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Case {
                    scrutinee: None,
                    arms: vec![
                        (
                            Expr::BinOp {
                                op: BinOp::Lt,
                                lhs: Box::new(Expr::Prop {
                                    target: Box::new(Expr::Var(VarId(0))),
                                    prop: "age".into(),
                                }),
                                rhs: Box::new(Expr::Int(18)),
                            },
                            Expr::String("minor".into()),
                        ),
                        (
                            Expr::BinOp {
                                op: BinOp::Lt,
                                lhs: Box::new(Expr::Prop {
                                    target: Box::new(Expr::Var(VarId(0))),
                                    prop: "age".into(),
                                }),
                                rhs: Box::new(Expr::Int(65)),
                            },
                            Expr::String("adult".into()),
                        ),
                    ],
                    otherwise: Some(Box::new(Expr::String("senior".into()))),
                },
                alias: "category".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_case_searched", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_in_list() {
    // WHERE n.status IN ['active', 'pending', 'review']
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::InList {
                operand: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "status".into(),
                }),
                list: Box::new(Expr::List(vec![
                    Expr::String("active".into()),
                    Expr::String("pending".into()),
                    Expr::String("review".into()),
                ])),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_in_list", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_is_null_not_null() {
    // WHERE n.email IS NOT NULL AND n.phone IS NULL
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(Expr::IsNull {
                    operand: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "email".into(),
                    }),
                    negated: true,
                }),
                rhs: Box::new(Expr::IsNull {
                    operand: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "phone".into(),
                    }),
                    negated: false,
                }),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_is_null_not_null", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_prop_chain() {
    // RETURN n.address.city — represented as Prop(Prop(Var, "address"), "city")
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "address".into(),
                    }),
                    prop: "city".into(),
                },
                alias: "city".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_prop_chain", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_index_access() {
    // RETURN n.tags[0]
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Index {
                    target: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "tags".into(),
                    }),
                    index: Box::new(Expr::Int(0)),
                },
                alias: "first_tag".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_index_access", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_string_ops() {
    // WHERE n.name STARTS WITH "Al" AND n.bio CONTAINS "developer" AND n.email ENDS WITH ".com"
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::StartsWith,
                        lhs: Box::new(Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "name".into(),
                        }),
                        rhs: Box::new(Expr::String("Al".into())),
                    }),
                    rhs: Box::new(Expr::BinOp {
                        op: BinOp::Contains,
                        lhs: Box::new(Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "bio".into(),
                        }),
                        rhs: Box::new(Expr::String("developer".into())),
                    }),
                }),
                rhs: Box::new(Expr::BinOp {
                    op: BinOp::EndsWith,
                    lhs: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "email".into(),
                    }),
                    rhs: Box::new(Expr::String(".com".into())),
                }),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_string_ops", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_not_and_unary_neg() {
    // WHERE NOT(n.deleted) AND -(n.score) > -100
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "deleted".into(),
                    }),
                }),
                rhs: Box::new(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        operand: Box::new(Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "score".into(),
                        }),
                    }),
                    rhs: Box::new(Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        operand: Box::new(Expr::Int(100)),
                    }),
                }),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_not_and_unary_neg", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_arithmetic() {
    // RETURN (n.price * n.qty) + n.shipping - n.discount AS total
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::BinOp {
                    op: BinOp::Sub,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(Expr::BinOp {
                            op: BinOp::Mul,
                            lhs: Box::new(Expr::Prop {
                                target: Box::new(Expr::Var(VarId(0))),
                                prop: "price".into(),
                            }),
                            rhs: Box::new(Expr::Prop {
                                target: Box::new(Expr::Var(VarId(0))),
                                prop: "qty".into(),
                            }),
                        }),
                        rhs: Box::new(Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: "shipping".into(),
                        }),
                    }),
                    rhs: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "discount".into(),
                    }),
                },
                alias: "total".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_arithmetic", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_regex_match() {
    // WHERE n.name =~ "^Alice.*"
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Filter {
            input: OpId(0),
            predicate: Expr::BinOp {
                op: BinOp::RegexMatch,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                }),
                rhs: Box::new(Expr::String("^Alice.*".into())),
            },
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_regex_match", pretty(&plan));
}

#[test]
fn corpus_pretty_expr_all_literals() {
    // Cover null, bool, int, float, string, list, map in one snapshot.
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![
                Projection {
                    expr: Expr::Null,
                    alias: "nul".into(),
                },
                Projection {
                    expr: Expr::Bool(false),
                    alias: "flag".into(),
                },
                Projection {
                    expr: Expr::Int(42),
                    alias: "num".into(),
                },
                Projection {
                    expr: Expr::Float(2.71),
                    alias: "flt".into(),
                },
                Projection {
                    expr: Expr::String("hello".into()),
                    alias: "str".into(),
                },
                Projection {
                    expr: Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
                    alias: "lst".into(),
                },
                Projection {
                    expr: Expr::Map(vec![
                        ("key".into(), Expr::String("value".into())),
                        ("n".into(), Expr::Int(99)),
                    ]),
                    alias: "mp".into(),
                },
            ],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_expr_all_literals", pretty(&plan));
}

// ── §9 JSON serde snapshots (representative handful) ─────────────────────────

#[test]
fn corpus_json_chained_expands() {
    // JSON serde for a chained expand plan.
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
                labels: LabelSet(vec![]),
                properties: None,
            },
            bind_rel: VarId(1),
            bind_to: VarId(2),
        },
        ReadOp::Expand {
            input: OpId(1),
            from: VarId(2),
            rel: RelSpec {
                types: vec!["LIKES".into()],
                direction: Direction::Outgoing,
                length: RelLength::Single,
                properties: None,
            },
            to: NodeSpec {
                labels: LabelSet(vec!["Post".into()]),
                properties: None,
            },
            bind_rel: VarId(3),
            bind_to: VarId(4),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(0)),
                    alias: "n".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(4)),
                    alias: "p".into(),
                },
            ],
        },
    ]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_chained_expands",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_aggregation_groupby() {
    // JSON serde for a grouping aggregation.
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        },
        ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![Expr::Prop {
                target: Box::new(Expr::Var(VarId(0))),
                prop: "city".into(),
            }],
            aggs: vec![
                AggExpr {
                    func: "count".into(),
                    args: vec![Expr::Var(VarId(0))],
                    distinct: false,
                },
                AggExpr {
                    func: "avg".into(),
                    args: vec![Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "age".into(),
                    }],
                    distinct: false,
                },
            ],
        },
        ReadOp::Project {
            input: OpId(1),
            items: vec![
                Projection {
                    expr: Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "city".into(),
                    },
                    alias: "city".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(1)),
                    alias: "cnt".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "avg_age".into(),
                },
            ],
        },
    ]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_aggregation_groupby",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_write_create_rel_chain() {
    // JSON serde for CREATE with rel chain.
    let plan = write_plan(vec![
        WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![("name".into(), Expr::String("Alice".into()))]),
            bind: Some(VarId(0)),
        },
        WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![("name".into(), Expr::String("Bob".into()))]),
            bind: Some(VarId(1)),
        },
        WriteOp::CreateRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "KNOWS".into(),
            props: Expr::Map(vec![("since".into(), Expr::Int(2020))]),
            bind: Some(VarId(2)),
        },
    ]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_write_create_rel_chain",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_merge_rel_with_on_create() {
    // JSON serde for MERGE rel with ON CREATE.
    let plan = rw_plan(
        vec![
            ReadOp::Source {
                label: Some(LabelSet(vec!["Person".into()])),
                bind: VarId(0),
            },
            ReadOp::Source {
                label: Some(LabelSet(vec!["Person".into()])),
                bind: VarId(1),
            },
        ],
        vec![WriteOp::MergeRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "FOLLOWS".into(),
            props: Expr::Map(vec![]),
            on_create: vec![WriteOp::SetProperty {
                target: VarId(2),
                prop: "since".into(),
                value: Expr::Int(2024),
            }],
            on_match: vec![WriteOp::SetProperty {
                target: VarId(2),
                prop: "strength".into(),
                value: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(2))),
                        prop: "strength".into(),
                    }),
                    rhs: Box::new(Expr::Int(1)),
                },
            }],
            bind: Some(VarId(2)),
        }],
    );
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_merge_rel_with_on_create",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_union_nested() {
    // JSON serde for nested union.
    let plan = read_plan(vec![
        ReadOp::Source {
            label: Some(LabelSet(vec!["A".into()])),
            bind: VarId(0),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "x".into(),
            }],
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["B".into()])),
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(2),
            items: vec![Projection {
                expr: Expr::Var(VarId(1)),
                alias: "x".into(),
            }],
        },
        ReadOp::Union {
            left: OpId(1),
            right: OpId(3),
            kind: UnionKind::All,
        },
        ReadOp::Source {
            label: Some(LabelSet(vec!["C".into()])),
            bind: VarId(2),
        },
        ReadOp::Project {
            input: OpId(5),
            items: vec![Projection {
                expr: Expr::Var(VarId(2)),
                alias: "x".into(),
            }],
        },
        ReadOp::Union {
            left: OpId(4),
            right: OpId(6),
            kind: UnionKind::Distinct,
        },
    ]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_union_nested",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_expr_case_and_isnull() {
    // JSON serde for CASE + IS NULL expressions.
    let plan = read_plan(vec![ReadOp::Project {
        input: OpId(0),
        items: vec![
            Projection {
                expr: Expr::Case {
                    scrutinee: Some(Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "tier".into(),
                    })),
                    arms: vec![
                        (Expr::String("gold".into()), Expr::Int(3)),
                        (Expr::String("silver".into()), Expr::Int(2)),
                    ],
                    otherwise: Some(Box::new(Expr::Int(1))),
                },
                alias: "tier_level".into(),
            },
            Projection {
                expr: Expr::IsNull {
                    operand: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(VarId(0))),
                        prop: "email".into(),
                    }),
                    negated: false,
                },
                alias: "no_email".into(),
            },
        ],
    }]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_expr_case_and_isnull",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_varlen_expand() {
    // JSON serde for variable-length expand.
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
                direction: Direction::Undirected,
                length: RelLength::Variable {
                    min: Some(1),
                    max: Some(5),
                },
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
                    alias: "start".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "end".into(),
                },
            ],
        },
    ]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_varlen_expand",
        serde_json::to_value(&plan).unwrap()
    );
}

#[test]
fn corpus_json_full_delete_detach() {
    // JSON serde for DETACH DELETE.
    let plan = rw_plan(
        vec![
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
                        prop: "id".into(),
                    }),
                    rhs: Box::new(Expr::Param {
                        name: "targetId".into(),
                    }),
                },
            },
        ],
        vec![WriteOp::Delete {
            targets: vec![Expr::Var(VarId(0))],
            detach: true,
        }],
    );
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_full_delete_detach",
        serde_json::to_value(&plan).unwrap()
    );
}

// ── cy-8x5 — list predicates (§19 row "List predicates") ────────────────────

/// RETURN ANY(x IN [1, 2, 3] WHERE x > 0) — the plan `Expr::ListPredicate`
/// survives through the HIR→Plan boundary so consumers can evaluate it
/// directly (no desugaring required).
#[test]
fn corpus_pretty_list_pred_any_return() {
    let any_expr = Expr::ListPredicate {
        kind: ListPredKind::Any,
        var: VarId(0),
        iterable: Box::new(Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)])),
        predicate: Some(Box::new(Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Var(VarId(0))),
            rhs: Box::new(Expr::Int(0)),
        })),
    };
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: any_expr,
                alias: "pred".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_list_pred_any_return", pretty(&plan));
}

/// RETURN ALL(x IN xs) — bare (predicate-free) form. Plan `predicate`
/// field is `None`; pretty output omits the `WHERE` clause.
#[test]
fn corpus_pretty_list_pred_all_bare() {
    let all_expr = Expr::ListPredicate {
        kind: ListPredKind::All,
        var: VarId(0),
        iterable: Box::new(Expr::Var(VarId(1))),
        predicate: None,
    };
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: all_expr,
                alias: "pred".into(),
            }],
        },
    ]);
    insta::assert_snapshot!("corpus_pretty_list_pred_all_bare", pretty(&plan));
}

/// JSON round-trip: all four kinds survive serialise + deserialise
/// identically (deterministic snapshot). Exercises the `ExprSer`
/// `list_predicate` proxy with the `pred_kind` tag rename.
#[test]
fn corpus_json_list_pred_roundtrip_all_kinds() {
    let mk = |kind: ListPredKind| Expr::ListPredicate {
        kind,
        var: VarId(0),
        iterable: Box::new(Expr::Var(VarId(1))),
        predicate: Some(Box::new(Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Var(VarId(0))),
            rhs: Box::new(Expr::Int(0)),
        })),
    };
    let plan = read_plan(vec![
        ReadOp::Source {
            label: None,
            bind: VarId(1),
        },
        ReadOp::Project {
            input: OpId(0),
            items: vec![
                Projection {
                    expr: mk(ListPredKind::Any),
                    alias: "a".into(),
                },
                Projection {
                    expr: mk(ListPredKind::All),
                    alias: "b".into(),
                },
                Projection {
                    expr: mk(ListPredKind::None),
                    alias: "c".into(),
                },
                Projection {
                    expr: mk(ListPredKind::Single),
                    alias: "d".into(),
                },
            ],
        },
    ]);
    assert_json_roundtrip(&plan);
    insta::assert_json_snapshot!(
        "corpus_json_list_pred_roundtrip_all_kinds",
        serde_json::to_value(&plan).unwrap()
    );
}
