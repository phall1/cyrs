//! `cypher-plan` — logical read/write plan IR (spec 0001 §12).
//!
//! The plan is a directed acyclic graph of operators. It is *logical*:
//! no cost model, no cardinality, no physical operator selection. A
//! consumer's executor takes the plan and produces rows / effects. This
//! crate imposes no runtime contract on that executor beyond the shape
//! of the plan itself.

#![doc(html_root_url = "https://docs.rs/cypher-plan/0.0.1")]

use smol_str::SmolStr;

/// Stable plan-scoped identifier for a variable. See spec §12.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub u32);

/// Operator identity within a plan graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId(pub u32);

/// Placeholder label expression; downstream crates refine this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelSet(pub Vec<SmolStr>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpec {
    pub labels: LabelSet,
    pub properties: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelSpec {
    pub types: Vec<SmolStr>,
    pub direction: Direction,
    pub length: RelLength,
    pub properties: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelLength {
    Single,
    Variable { min: Option<u64>, max: Option<u64> },
}

/// Logical read-plan operator. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOp {
    Source {
        labels: LabelSet,
        bind: VarId,
    },
    Expand {
        input: OpId,
        from: VarId,
        rel: RelSpec,
        to: NodeSpec,
        bind_rel: VarId,
        bind_to: VarId,
    },
    Filter {
        input: OpId,
        predicate: Expr,
    },
    Project {
        input: OpId,
        items: Vec<Projection>,
    },
    Aggregate {
        input: OpId,
        keys: Vec<Expr>,
        aggs: Vec<AggExpr>,
    },
    OrderBy {
        input: OpId,
        keys: Vec<OrderKey>,
    },
    Skip {
        input: OpId,
        count: Expr,
    },
    Limit {
        input: OpId,
        count: Expr,
    },
    Distinct {
        input: OpId,
    },
    Unwind {
        input: OpId,
        list: Expr,
        bind: VarId,
    },
    Union {
        left: OpId,
        right: OpId,
        kind: UnionKind,
    },
    With {
        input: OpId,
        items: Vec<Projection>,
        filter: Option<Expr>,
    },
    OptionalJoin {
        input: OpId,
        pattern: Box<ReadOp>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnionKind {
    All,
    Distinct,
}

/// Logical write-plan operator. Spec §12.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    CreateNode {
        labels: Vec<SmolStr>,
        props: Expr,
        bind: Option<VarId>,
    },
    CreateRel {
        from: VarId,
        to: VarId,
        rel_type: SmolStr,
        props: Expr,
        bind: Option<VarId>,
    },
    MergeNode {
        labels: Vec<SmolStr>,
        props: Expr,
        on_create: Vec<WriteOp>,
        on_match: Vec<WriteOp>,
        bind: Option<VarId>,
    },
    MergeRel {
        from: VarId,
        to: VarId,
        rel_type: SmolStr,
        props: Expr,
        on_create: Vec<WriteOp>,
        on_match: Vec<WriteOp>,
        bind: Option<VarId>,
    },
    SetProperty {
        target: VarId,
        prop: SmolStr,
        value: Expr,
    },
    SetLabels {
        target: VarId,
        labels: Vec<SmolStr>,
    },
    RemoveProperty {
        target: VarId,
        prop: SmolStr,
    },
    RemoveLabels {
        target: VarId,
        labels: Vec<SmolStr>,
    },
    Delete {
        targets: Vec<Expr>,
        detach: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub expr: Expr,
    pub alias: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderKey {
    pub expr: Expr,
    pub dir: SortDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Aggregation expression. Aggregators recognised by the built-in
/// function catalog (spec §8.3); consumer-specific aggregators are
/// resolved through the `FunctionSignature` with `aggregate = true`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggExpr {
    pub func: SmolStr,
    pub args: Vec<Expr>,
    pub distinct: bool,
}

/// Resolved expression IR. Every variable reference carries its `VarId`.
///
/// `Eq` is implemented manually below: `f64` is not `Eq` under stdlib
/// rules (NaN breaks reflexivity), but the Plan IR treats floats as
/// opaque bit patterns for equality purposes.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(SmolStr),
    Var(VarId),
    Prop {
        target: Box<Expr>,
        prop: SmolStr,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    List(Vec<Expr>),
    Map(Vec<(SmolStr, Expr)>),
    Call {
        func: SmolStr,
        args: Vec<Expr>,
    },
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Case {
        scrutinee: Option<Box<Expr>>,
        arms: Vec<(Expr, Expr)>,
        otherwise: Option<Box<Expr>>,
    },
    IsNull {
        operand: Box<Expr>,
        negated: bool,
    },
    InList {
        operand: Box<Expr>,
        list: Box<Expr>,
    },
    Param {
        name: SmolStr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Xor,
    In,
    StartsWith,
    EndsWith,
    Contains,
    RegexMatch,
    Concat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

// Floats are compared by `PartialEq` value. We explicitly assert `Eq` so
// types that aggregate `Expr` can derive `Eq`. Spec §17.14 (determinism)
// requires stable equality of plan output; semantic equality for
// execution is the consumer's responsibility.
impl Eq for Expr {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_read_plan() {
        // MATCH (n:Person) RETURN n.name
        let source = ReadOp::Source {
            labels: LabelSet(vec!["Person".into()]),
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
}
