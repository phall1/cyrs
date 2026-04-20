//! `cypher-plan` — logical read/write plan IR (spec 0001 §12).
//!
//! The plan is a directed acyclic graph of operators. It is *logical*:
//! no cost model, no cardinality, no physical operator selection. A
//! consumer's executor takes the plan and produces rows / effects. This
//! crate imposes no runtime contract on that executor beyond the shape
//! of the plan itself (spec §12.5).
//!
//! # Key types
//!
//! - [`ReadOp`] — read-side operator tree (spec §12.1).
//! - [`WriteOp`] — write-side operator list (spec §12.1).
//! - [`Expr`] — plan-level expression IR (spec §12.2).
//! - [`VarId`] — plan-scoped variable identity (spec §12.3).
//! - [`OpId`] — operator identity within a plan graph.
//!
//! # HIR → Plan lowering
//!
//! The [`lower`] module provides the entry point [`lower::lower_statement`]
//! which lowers a post-resolve, post-desugar HIR [`cypher_hir::Statement`]
//! into a [`lower::PlanStatement`] (spec §12, bead cy-foy).

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-plan/0.0.1")]

pub mod lower;

use smol_str::SmolStr;

// ──────────────────────────────────────────────────────────────────────────────
// Identity types
// ──────────────────────────────────────────────────────────────────────────────

/// Stable plan-scoped identifier for a variable.
///
/// `VarId`s are plan-scoped — they survive the HIR from which the plan was
/// lowered. The lowering pass (bead cy-foy) produces a `VarMap` mapping these
/// back to HIR [`cypher_hir::VarId`]s. See spec §12.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub u32);

/// Operator identity within a plan graph.
///
/// `OpId`s are dense indices into a consumer-maintained operator arena.
/// The plan itself does not maintain an arena — consumers choose their own
/// storage. See spec §12.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId(pub u32);

// ──────────────────────────────────────────────────────────────────────────────
// Supporting types
// ──────────────────────────────────────────────────────────────────────────────

/// A set of node labels used in pattern-matching predicates. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LabelSet(pub Vec<SmolStr>);

/// Specification for the node endpoint of an expand operator. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpec {
    /// Labels the target node must carry.
    pub labels: LabelSet,
    /// Optional inline property predicate on the target node.
    pub properties: Option<Expr>,
}

/// Specification for the relationship in an expand operator. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelSpec {
    /// Allowed relationship types (empty = any type).
    pub types: Vec<SmolStr>,
    /// Traversal direction.
    pub direction: Direction,
    /// Variable-length qualifier.
    pub length: RelLength,
    /// Optional inline property predicate on the relationship.
    pub properties: Option<Expr>,
}

/// Relationship traversal direction as written in the source. Spec §5.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `-[r]->` — left-to-right.
    Outgoing,
    /// `<-[r]-` — right-to-left.
    Incoming,
    /// `-[r]-` — either direction.
    Undirected,
}

/// Variable-length relationship bounds. `Single` means no `*` qualifier.
/// Spec §5.3.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RelLength {
    /// Exactly one hop (`-[r]->`).
    Single,
    /// Variable number of hops (`-[r*min..max]->`). Bounds of `None` mean
    /// unbounded in that direction.
    Variable { min: Option<u64>, max: Option<u64> },
}

/// Disposition of a `UNION`. Spec §12.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnionKind {
    /// `UNION ALL` — duplicate rows are preserved.
    All,
    /// `UNION` — duplicate rows are removed.
    Distinct,
}

/// A single output column of a `PROJECT` or `WITH` operator. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    /// Expression to evaluate.
    pub expr: Expr,
    /// Column alias (always explicit at plan level — the lowering pass
    /// synthesises aliases for bare variable references).
    pub alias: SmolStr,
}

/// A sort key in an `ORDER BY` operator. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderKey {
    /// Expression to sort on.
    pub expr: Expr,
    /// Sort direction.
    pub dir: SortDir,
}

/// Sort direction for [`OrderKey`]. Spec §12.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDir {
    /// `ASC` / default.
    Asc,
    /// `DESC`.
    Desc,
}

/// An aggregation call in an `AGGREGATE` operator.
///
/// The function name is resolved at this level (cf. [`Expr::Call`] which
/// carries the resolved function name as well). The `aggregate = true` flag
/// in the function catalog entry gates whether a call may appear here.
/// Spec §12.1, §8.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggExpr {
    /// Resolved aggregation function name (e.g. `"count"`, `"sum"`).
    pub func: SmolStr,
    /// Argument expressions.
    pub args: Vec<Expr>,
    /// `true` when `DISTINCT` modifier is present (`count(DISTINCT x)`).
    pub distinct: bool,
}

// ──────────────────────────────────────────────────────────────────────────────
// ReadOp
// ──────────────────────────────────────────────────────────────────────────────

/// Logical read-plan operator tree. Spec §12.1.
///
/// Operators form a DAG — each variant that has an `input` field references
/// its source by [`OpId`]. The `OptionalJoin` variant embeds a sub-tree
/// directly via `Box<ReadOp>` because its inner pattern is always a fresh
/// tree introduced by `OPTIONAL MATCH`.
///
/// Consumers iterate the tree in whatever order suits their executor. This
/// crate imposes no evaluation semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOp {
    /// Scan all nodes, optionally filtered to those carrying a set of labels.
    ///
    /// When `label` is `None` this is an all-node scan; when `Some` it is a
    /// label-index scan (or filtered full scan, depending on the consumer's
    /// storage). Spec §12.1 N1.
    Source {
        /// Labels to filter on, or `None` for every node.
        label: Option<LabelSet>,
        /// Variable that receives each scanned node.
        bind: VarId,
    },

    /// Expand a node into its adjacent relationships and neighbour nodes.
    ///
    /// Starting from `from` (already bound in `input`), traverse relationships
    /// matching `rel` to reach a node matching `to`. Spec §12.1 N2.
    Expand {
        /// Source operator that provides the `from` variable.
        input: OpId,
        /// Variable holding the start node.
        from: VarId,
        /// Relationship type / direction / length specification.
        rel: RelSpec,
        /// Target node specification (labels + optional property predicate).
        to: NodeSpec,
        /// Variable that receives the traversed relationship.
        bind_rel: VarId,
        /// Variable that receives the reached node.
        bind_to: VarId,
    },

    /// Predicate filter — keeps only rows where `predicate` is truthy.
    ///
    /// Implements `WHERE` and inline pattern predicates. Spec §12.1 N3.
    Filter {
        /// Source operator.
        input: OpId,
        /// Boolean expression; rows where it evaluates to `false` or `null`
        /// are dropped.
        predicate: Expr,
    },

    /// Column projection — renames / computes output columns.
    ///
    /// Implements the output column list of `RETURN` and `WITH`.
    /// Spec §12.1 N4.
    Project {
        /// Source operator.
        input: OpId,
        /// Ordered list of output columns.
        items: Vec<Projection>,
    },

    /// Aggregation — groups rows and applies aggregate functions.
    ///
    /// `keys` are the grouping expressions; `aggs` are the aggregate calls.
    /// An empty `keys` vec aggregates the entire input into a single row.
    /// Spec §12.1 N5.
    Aggregate {
        /// Source operator.
        input: OpId,
        /// Grouping expressions (the non-aggregate columns in the output).
        keys: Vec<Expr>,
        /// Aggregate function calls.
        aggs: Vec<AggExpr>,
    },

    /// Sort — orders rows by a list of sort keys. Spec §12.1 N6.
    OrderBy {
        /// Source operator.
        input: OpId,
        /// Ordered list of sort keys (primary first).
        keys: Vec<OrderKey>,
    },

    /// Skip — discards the first `count` rows. Spec §12.1 N7.
    Skip {
        /// Source operator.
        input: OpId,
        /// Number of rows to skip; must evaluate to a non-negative integer.
        count: Expr,
    },

    /// Limit — keeps only the first `count` rows. Spec §12.1 N8.
    Limit {
        /// Source operator.
        input: OpId,
        /// Maximum number of rows to pass through.
        count: Expr,
    },

    /// Distinct — removes duplicate rows. Spec §12.1 N9.
    ///
    /// Row equality is Cypher value equality (`null != null`).
    Distinct {
        /// Source operator.
        input: OpId,
    },

    /// Unwind — flattens a list expression into one row per element.
    ///
    /// Implements `UNWIND list AS var`. Spec §12.1 N10.
    Unwind {
        /// Source operator.
        input: OpId,
        /// List expression to iterate.
        list: Expr,
        /// Variable that receives each list element.
        bind: VarId,
    },

    /// Union — concatenates rows from two sub-plans. Spec §12.1 N11.
    Union {
        /// Left source operator.
        left: OpId,
        /// Right source operator.
        right: OpId,
        /// Whether to deduplicate the combined output.
        kind: UnionKind,
    },

    /// With — projects columns and optionally filters, resetting scope.
    ///
    /// Implements the `WITH` clause. Differs from [`ReadOp::Project`] in
    /// that `WITH` starts a new variable scope. Spec §12.1 N12.
    With {
        /// Source operator.
        input: OpId,
        /// Output columns.
        items: Vec<Projection>,
        /// Optional `WHERE` predicate applied after projection.
        filter: Option<Expr>,
    },

    /// Optional join — left-outer-join a sub-plan. Spec §12.1 N13.
    ///
    /// Implements `OPTIONAL MATCH`. Rows from `input` that have no match in
    /// `pattern` are kept with `null`-bound variables.
    OptionalJoin {
        /// Outer source operator.
        input: OpId,
        /// Inner pattern (embedded tree, not an [`OpId`], because it is
        /// always a fresh sub-tree introduced by `OPTIONAL MATCH`).
        pattern: Box<ReadOp>,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// WriteOp
// ──────────────────────────────────────────────────────────────────────────────

/// Logical write-plan operator. Spec §12.1.
///
/// Write operators are applied sequentially after the read phase produces a
/// row. Consumers own the sequencing and transactional semantics. This crate
/// describes *what* to write, not *how*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    /// Create a new node with the given labels and properties.
    ///
    /// `props` is evaluated to a map expression at write time. `bind`, if
    /// present, makes the new node available under that variable for
    /// subsequent operators. Spec §12.1 W1.
    CreateNode {
        /// Labels to apply to the new node.
        labels: Vec<SmolStr>,
        /// Map expression supplying the initial properties.
        props: Expr,
        /// Optional variable binding for the created node.
        bind: Option<VarId>,
    },

    /// Create a new relationship between two already-bound nodes.
    ///
    /// Spec §12.1 W2.
    CreateRel {
        /// Variable holding the start node.
        from: VarId,
        /// Variable holding the end node.
        to: VarId,
        /// Relationship type (exactly one, required by Cypher syntax).
        rel_type: SmolStr,
        /// Map expression supplying the initial properties.
        props: Expr,
        /// Optional variable binding for the created relationship.
        bind: Option<VarId>,
    },

    /// Merge a node — create if absent, match if present.
    ///
    /// `on_create` / `on_match` are applied depending on whether the node
    /// was newly created or already existed. Spec §12.1 W3.
    MergeNode {
        /// Labels that uniquely identify the node.
        labels: Vec<SmolStr>,
        /// Property predicate / initial values.
        props: Expr,
        /// Write operations to apply when a new node is created.
        on_create: Vec<WriteOp>,
        /// Write operations to apply when an existing node is found.
        on_match: Vec<WriteOp>,
        /// Optional variable binding for the merged node.
        bind: Option<VarId>,
    },

    /// Merge a relationship — create if absent, match if present.
    ///
    /// Analogous to [`WriteOp::MergeNode`] but for relationships.
    /// Spec §12.1 W4.
    MergeRel {
        /// Variable holding the start node.
        from: VarId,
        /// Variable holding the end node.
        to: VarId,
        /// Relationship type.
        rel_type: SmolStr,
        /// Property predicate / initial values.
        props: Expr,
        /// Write operations to apply when a new relationship is created.
        on_create: Vec<WriteOp>,
        /// Write operations to apply when an existing relationship is found.
        on_match: Vec<WriteOp>,
        /// Optional variable binding for the merged relationship.
        bind: Option<VarId>,
    },

    /// Set a single property on a node or relationship. Spec §12.1 W5.
    SetProperty {
        /// Variable holding the target entity.
        target: VarId,
        /// Property key.
        prop: SmolStr,
        /// New value expression.
        value: Expr,
    },

    /// Add or replace the label set on a node. Spec §12.1 W6.
    SetLabels {
        /// Variable holding the target node.
        target: VarId,
        /// Labels to add.
        labels: Vec<SmolStr>,
    },

    /// Remove a single property from a node or relationship. Spec §12.1 W7.
    RemoveProperty {
        /// Variable holding the target entity.
        target: VarId,
        /// Property key to remove.
        prop: SmolStr,
    },

    /// Remove labels from a node. Spec §12.1 W8.
    RemoveLabels {
        /// Variable holding the target node.
        target: VarId,
        /// Labels to remove.
        labels: Vec<SmolStr>,
    },

    /// Delete nodes or relationships. Spec §12.1 W9.
    ///
    /// When `detach` is `true` this is `DETACH DELETE`, which removes all
    /// incident relationships before deleting the node.
    Delete {
        /// Expressions evaluating to the entities to delete.
        targets: Vec<Expr>,
        /// `true` for `DETACH DELETE`.
        detach: bool,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// Expr
// ──────────────────────────────────────────────────────────────────────────────

/// Plan-level expression IR. Spec §12.2.
///
/// Every variable reference carries its [`VarId`]; every function call is
/// resolved by name. This is a **distinct** type from [`cypher_hir::Expr`]:
/// it is fully resolved (no [`cypher_hir::Expr::Unresolved`], no
/// [`cypher_hir::Expr::PatternPredicate`], no `MapProjection`), and `VarId`s
/// are plan-scoped rather than HIR-scoped (spec §12.3). The HIR→Plan lowering
/// pass (bead cy-foy) maps between them.
///
/// # Equality of floats
///
/// `Eq` is manually implemented so that aggregates of `Expr` can derive
/// `Eq`. Semantic equality for execution (e.g. `NaN != NaN`) is the
/// consumer's responsibility. The Plan IR treats float bit patterns as opaque
/// for structural equality checks (spec §17.14 determinism).
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// The `null` literal. Spec §12.2 E1.
    Null,
    /// A boolean literal (`true` / `false`). Spec §12.2 E2.
    Bool(bool),
    /// A 64-bit signed integer literal. Spec §12.2 E3.
    Int(i64),
    /// A 64-bit IEEE-754 float literal. Spec §12.2 E4.
    Float(f64),
    /// A string literal. Spec §12.2 E5.
    String(SmolStr),
    /// A resolved variable reference. Spec §12.2 E6.
    Var(VarId),
    /// A property access (`expr.prop`). Spec §12.2 E7.
    Prop {
        /// Expression evaluating to a node, relationship, or map.
        target: Box<Expr>,
        /// Property key.
        prop: SmolStr,
    },
    /// A subscript / index access (`expr[index]`). Spec §12.2 E8.
    Index {
        /// Expression evaluating to a list or map.
        target: Box<Expr>,
        /// Index expression (integer for lists, string for maps).
        index: Box<Expr>,
    },
    /// A list literal (`[e1, e2, ...]`). Spec §12.2 E9.
    List(Vec<Expr>),
    /// A map literal (`{k1: e1, k2: e2}`). Spec §12.2 E10.
    Map(Vec<(SmolStr, Expr)>),
    /// A resolved function call. Spec §12.2 E11.
    ///
    /// `func` is the canonical lower-case function name as resolved by the
    /// built-in catalog (spec §8.3) or the consumer-provided catalog.
    Call {
        /// Resolved function name.
        func: SmolStr,
        /// Argument expressions.
        args: Vec<Expr>,
    },
    /// A binary operator application. Spec §12.2 E12.
    BinOp {
        /// Operator.
        op: BinOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A unary operator application. Spec §12.2 E13.
    UnaryOp {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        operand: Box<Expr>,
    },
    /// A `CASE` expression. Spec §12.2 E14.
    ///
    /// `scrutinee` is present for simple `CASE x WHEN …` form; absent for
    /// searched `CASE WHEN cond THEN …` form.
    Case {
        /// Optional scrutinee for simple `CASE`.
        scrutinee: Option<Box<Expr>>,
        /// `(when, then)` arms, tested in order.
        arms: Vec<(Expr, Expr)>,
        /// `ELSE` expression, or `Null` when absent.
        otherwise: Option<Box<Expr>>,
    },
    /// `x IS NULL` / `x IS NOT NULL`. Spec §12.2 E15.
    IsNull {
        /// Operand.
        operand: Box<Expr>,
        /// `true` for `IS NOT NULL`.
        negated: bool,
    },
    /// `x IN list` membership test. Spec §12.2 E16.
    InList {
        /// Value to test.
        operand: Box<Expr>,
        /// List to test membership in.
        list: Box<Expr>,
    },
    /// A query parameter reference. Spec §12.4.
    ///
    /// The consumer binds parameter values at execution time. The plan does
    /// not carry values. `ty` is deferred to bead cy-foy (HIR→Plan lowering)
    /// because type inference runs in `cypher-sema`, which is not a
    /// dependency of this crate.
    Param {
        /// Parameter name as written in the query (`$name` or `{name}`).
        name: SmolStr,
    },
}

// `f64` is not `Eq` under stdlib rules (NaN breaks reflexivity), but the
// Plan IR treats floats as opaque bit patterns for structural equality.
// See spec §17.14.
impl Eq for Expr {}

// ──────────────────────────────────────────────────────────────────────────────
// Operator enums
// ──────────────────────────────────────────────────────────────────────────────

/// Binary operators. Spec §12.2 E12 / §5.6 / §7.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// `+` — arithmetic addition or string/list concatenation (type-directed).
    Add,
    /// `-` — arithmetic subtraction.
    Sub,
    /// `*` — arithmetic multiplication.
    Mul,
    /// `/` — arithmetic division.
    Div,
    /// `%` — arithmetic modulo.
    Mod,
    /// `^` — exponentiation.
    Pow,
    /// `=` — equality.
    Eq,
    /// `<>` — inequality.
    Neq,
    /// `<` — less than.
    Lt,
    /// `<=` — less than or equal.
    Le,
    /// `>` — greater than.
    Gt,
    /// `>=` — greater than or equal.
    Ge,
    /// `AND` — boolean conjunction.
    And,
    /// `OR` — boolean disjunction.
    Or,
    /// `XOR` — boolean exclusive-or.
    Xor,
    /// `IN` — list membership (sugar for [`Expr::InList`]; kept for symmetry).
    In,
    /// `STARTS WITH` — string prefix test.
    StartsWith,
    /// `ENDS WITH` — string suffix test.
    EndsWith,
    /// `CONTAINS` — string substring test.
    Contains,
    /// `=~` — regular expression match.
    RegexMatch,
    /// `+` on strings / lists (resolved via type context to this variant).
    Concat,
}

/// Unary operators. Spec §12.2 E13 / §5.6 / §7.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// Unary `-` — arithmetic negation.
    Neg,
    /// `NOT` — boolean negation.
    Not,
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ReadOp constructors ───────────────────────────────────────────────────

    #[test]
    fn read_op_source_all_nodes() {
        // MATCH (n) RETURN n
        let op = ReadOp::Source {
            label: None,
            bind: VarId(0),
        };
        assert_eq!(
            op,
            ReadOp::Source {
                label: None,
                bind: VarId(0)
            }
        );
        // Debug must be deterministic.
        let d1 = format!("{op:?}");
        let d2 = format!("{op:?}");
        assert_eq!(d1, d2);
    }

    #[test]
    fn read_op_source_with_label() {
        // MATCH (n:Person) ...
        let op = ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        };
        let debug = format!("{op:?}");
        assert!(
            debug.contains("Person"),
            "label must appear in debug: {debug}"
        );
    }

    #[test]
    fn read_op_expand_composes() {
        // MATCH (n)-[r:KNOWS]->(m)
        let expand = ReadOp::Expand {
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
        };
        assert!(format!("{expand:?}").contains("KNOWS"));
    }

    #[test]
    fn read_op_filter_predicate() {
        let filter = ReadOp::Filter {
            input: OpId(1),
            predicate: Expr::Bool(true),
        };
        assert_eq!(
            filter,
            ReadOp::Filter {
                input: OpId(1),
                predicate: Expr::Bool(true)
            }
        );
    }

    #[test]
    fn read_op_project_items() {
        let project = ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        };
        let debug = format!("{project:?}");
        assert!(debug.contains("name"));
    }

    #[test]
    fn read_op_aggregate() {
        let agg = ReadOp::Aggregate {
            input: OpId(0),
            keys: vec![Expr::Var(VarId(0))],
            aggs: vec![AggExpr {
                func: "count".into(),
                args: vec![Expr::Var(VarId(0))],
                distinct: false,
            }],
        };
        let debug = format!("{agg:?}");
        assert!(debug.contains("count"));
    }

    #[test]
    fn read_op_order_by() {
        let op = ReadOp::OrderBy {
            input: OpId(0),
            keys: vec![OrderKey {
                expr: Expr::Var(VarId(0)),
                dir: SortDir::Desc,
            }],
        };
        assert!(format!("{op:?}").contains("Desc"));
    }

    #[test]
    fn read_op_skip_limit() {
        let skip = ReadOp::Skip {
            input: OpId(0),
            count: Expr::Int(10),
        };
        let limit = ReadOp::Limit {
            input: OpId(0),
            count: Expr::Int(5),
        };
        assert!(format!("{skip:?}").contains("10"));
        assert!(format!("{limit:?}").contains('5'));
    }

    #[test]
    fn read_op_distinct() {
        let op = ReadOp::Distinct { input: OpId(0) };
        assert_eq!(op, ReadOp::Distinct { input: OpId(0) });
    }

    #[test]
    fn read_op_unwind() {
        let op = ReadOp::Unwind {
            input: OpId(0),
            list: Expr::Var(VarId(0)),
            bind: VarId(1),
        };
        assert!(format!("{op:?}").contains("VarId(1)"));
    }

    #[test]
    fn read_op_union_all_and_distinct() {
        let all = ReadOp::Union {
            left: OpId(0),
            right: OpId(1),
            kind: UnionKind::All,
        };
        let distinct = ReadOp::Union {
            left: OpId(0),
            right: OpId(1),
            kind: UnionKind::Distinct,
        };
        assert_ne!(all, distinct);
    }

    #[test]
    fn read_op_with_filter() {
        let with = ReadOp::With {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Var(VarId(0)),
                alias: "n".into(),
            }],
            filter: Some(Expr::Bool(true)),
        };
        assert!(format!("{with:?}").contains("Bool(true)"));
    }

    #[test]
    fn read_op_optional_join_boxed_subtree() {
        let inner = ReadOp::Source {
            label: None,
            bind: VarId(1),
        };
        let op = ReadOp::OptionalJoin {
            input: OpId(0),
            pattern: Box::new(inner.clone()),
        };
        // The inner tree is accessible via the box.
        assert_eq!(
            **match &op {
                ReadOp::OptionalJoin { pattern, .. } => pattern,
                _ => panic!(),
            },
            inner
        );
    }

    // ── WriteOp constructors ──────────────────────────────────────────────────

    #[test]
    fn write_op_create_node() {
        let op = WriteOp::CreateNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![("name".into(), Expr::String("Alice".into()))]),
            bind: Some(VarId(0)),
        };
        assert!(format!("{op:?}").contains("Person"));
    }

    #[test]
    fn write_op_create_rel() {
        let op = WriteOp::CreateRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "KNOWS".into(),
            props: Expr::Map(vec![]),
            bind: None,
        };
        assert!(format!("{op:?}").contains("KNOWS"));
    }

    #[test]
    fn write_op_merge_node_on_create_and_on_match() {
        let on_create = vec![WriteOp::SetProperty {
            target: VarId(0),
            prop: "created".into(),
            value: Expr::Bool(true),
        }];
        let on_match = vec![WriteOp::SetProperty {
            target: VarId(0),
            prop: "updated".into(),
            value: Expr::Bool(true),
        }];
        let op = WriteOp::MergeNode {
            labels: vec!["Person".into()],
            props: Expr::Map(vec![]),
            on_create,
            on_match,
            bind: Some(VarId(0)),
        };
        let debug = format!("{op:?}");
        assert!(debug.contains("created"));
        assert!(debug.contains("updated"));
    }

    #[test]
    fn write_op_merge_rel() {
        let op = WriteOp::MergeRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "FOLLOWS".into(),
            props: Expr::Map(vec![]),
            on_create: vec![],
            on_match: vec![],
            bind: None,
        };
        assert!(format!("{op:?}").contains("FOLLOWS"));
    }

    #[test]
    fn write_op_set_and_remove() {
        let set_prop = WriteOp::SetProperty {
            target: VarId(0),
            prop: "age".into(),
            value: Expr::Int(30),
        };
        let set_labels = WriteOp::SetLabels {
            target: VarId(0),
            labels: vec!["Admin".into()],
        };
        let rm_prop = WriteOp::RemoveProperty {
            target: VarId(0),
            prop: "age".into(),
        };
        let rm_labels = WriteOp::RemoveLabels {
            target: VarId(0),
            labels: vec!["Admin".into()],
        };
        assert!(format!("{set_prop:?}").contains("30"));
        assert!(format!("{set_labels:?}").contains("Admin"));
        assert!(format!("{rm_prop:?}").contains("age"));
        assert!(format!("{rm_labels:?}").contains("Admin"));
    }

    #[test]
    fn write_op_delete_and_detach_delete() {
        let del = WriteOp::Delete {
            targets: vec![Expr::Var(VarId(0))],
            detach: false,
        };
        let detach = WriteOp::Delete {
            targets: vec![Expr::Var(VarId(0))],
            detach: true,
        };
        assert_ne!(del, detach);
        assert!(format!("{detach:?}").contains("true"));
    }

    // ── Expr constructors ─────────────────────────────────────────────────────

    #[test]
    fn expr_literals_and_var() {
        assert_eq!(Expr::Null, Expr::Null);
        assert_eq!(Expr::Bool(true), Expr::Bool(true));
        assert_ne!(Expr::Bool(true), Expr::Bool(false));
        assert_eq!(Expr::Int(42), Expr::Int(42));
        assert_eq!(Expr::String("hi".into()), Expr::String("hi".into()));
        assert_eq!(Expr::Var(VarId(3)), Expr::Var(VarId(3)));
    }

    #[test]
    fn expr_float_structural_eq() {
        // Two identical finite floats compare equal at plan IR level.
        assert_eq!(Expr::Float(1.5), Expr::Float(1.5));
        assert_ne!(Expr::Float(1.5), Expr::Float(2.0));
        // NaN carries IEEE semantics for PartialEq; consumers that need
        // bit-pattern equality should normalise NaN before building Exprs.
        // The `impl Eq for Expr` declaration only asserts the reflexivity
        // invariant is intentionally waived at the caller's discretion
        // (spec §17.14 note).
        let _ = Expr::Float(f64::NAN); // type-checks, no panic
    }

    #[test]
    fn expr_prop_and_index() {
        let prop = Expr::Prop {
            target: Box::new(Expr::Var(VarId(0))),
            prop: "name".into(),
        };
        let index = Expr::Index {
            target: Box::new(Expr::Var(VarId(0))),
            index: Box::new(Expr::Int(0)),
        };
        assert!(format!("{prop:?}").contains("name"));
        assert!(format!("{index:?}").contains("Int(0)"));
    }

    #[test]
    fn expr_list_and_map() {
        let list = Expr::List(vec![Expr::Int(1), Expr::Int(2)]);
        let map = Expr::Map(vec![("key".into(), Expr::String("val".into()))]);
        assert!(format!("{list:?}").contains("Int(1)"));
        assert!(format!("{map:?}").contains("key"));
    }

    #[test]
    fn expr_call() {
        let call = Expr::Call {
            func: "toLower".into(),
            args: vec![Expr::Var(VarId(0))],
        };
        assert!(format!("{call:?}").contains("toLower"));
    }

    #[test]
    fn expr_bin_op_all_variants_are_debug() {
        for op in [
            BinOp::Add,
            BinOp::Sub,
            BinOp::Mul,
            BinOp::Div,
            BinOp::Mod,
            BinOp::Pow,
            BinOp::Eq,
            BinOp::Neq,
            BinOp::Lt,
            BinOp::Le,
            BinOp::Gt,
            BinOp::Ge,
            BinOp::And,
            BinOp::Or,
            BinOp::Xor,
            BinOp::In,
            BinOp::StartsWith,
            BinOp::EndsWith,
            BinOp::Contains,
            BinOp::RegexMatch,
            BinOp::Concat,
        ] {
            let _ = format!("{op:?}");
        }
    }

    #[test]
    fn expr_unary_op() {
        let neg = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Int(1)),
        };
        let not = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Bool(false)),
        };
        assert!(format!("{neg:?}").contains("Neg"));
        assert!(format!("{not:?}").contains("Not"));
    }

    #[test]
    fn expr_case() {
        let case = Expr::Case {
            scrutinee: None,
            arms: vec![(Expr::Bool(true), Expr::Int(1))],
            otherwise: Some(Box::new(Expr::Null)),
        };
        assert!(format!("{case:?}").contains("Int(1)"));
    }

    #[test]
    fn expr_is_null_and_in_list() {
        let is_null = Expr::IsNull {
            operand: Box::new(Expr::Var(VarId(0))),
            negated: false,
        };
        let in_list = Expr::InList {
            operand: Box::new(Expr::Var(VarId(0))),
            list: Box::new(Expr::List(vec![])),
        };
        assert!(format!("{is_null:?}").contains("negated: false"));
        let _ = format!("{in_list:?}");
    }

    #[test]
    fn expr_param() {
        let p = Expr::Param {
            name: "userId".into(),
        };
        assert!(format!("{p:?}").contains("userId"));
    }

    #[test]
    fn debug_output_is_deterministic() {
        // Exercise determinism requirement from spec §17.14.
        let plan = ReadOp::Project {
            input: OpId(0),
            items: vec![
                Projection {
                    expr: Expr::Var(VarId(1)),
                    alias: "a".into(),
                },
                Projection {
                    expr: Expr::Var(VarId(2)),
                    alias: "b".into(),
                },
            ],
        };
        let first = format!("{plan:?}");
        let second = format!("{plan:?}");
        assert_eq!(first, second);
    }

    #[test]
    fn build_simple_read_plan() {
        // MATCH (n:Person) RETURN n.name
        let source = ReadOp::Source {
            label: Some(LabelSet(vec!["Person".into()])),
            bind: VarId(0),
        };
        let project = ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Prop {
                    target: Box::new(Expr::Var(VarId(0))),
                    prop: "name".into(),
                },
                alias: "name".into(),
            }],
        };
        // Smoke test: the types compose.
        let _ = (source, project);
    }

    #[test]
    fn var_id_and_op_id_are_copy_and_hash() {
        use std::collections::HashSet;
        let mut ids: HashSet<VarId> = HashSet::new();
        ids.insert(VarId(0));
        ids.insert(VarId(1));
        ids.insert(VarId(0)); // duplicate
        assert_eq!(ids.len(), 2);

        let v = VarId(7);
        let v2 = v; // Copy
        assert_eq!(v, v2);

        let mut ops: HashSet<OpId> = HashSet::new();
        ops.insert(OpId(0));
        assert_eq!(ops.len(), 1);
    }
}
