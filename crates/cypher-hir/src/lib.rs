//! `cypher-hir` — High-level IR with name resolution (spec 0001 §6).
//!
//! HIR is owned and resolved. Every variable reference carries its
//! definition; syntactic sugar (list/pattern/map comprehensions, short-
//! hand property matching, map projection) is desugared. The AST ↔ HIR
//! map is preserved for span-accurate diagnostics.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-hir/0.0.1")]

use cypher_syntax::TextRange;
use indexmap::IndexMap;
use smol_str::SmolStr;

/// Statement-scoped index identifying an HIR node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HirId(pub u32);

/// Interned variable identity within a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub u32);

/// Variable kind recorded at binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Node,
    Relationship,
    Path,
    Value,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub id: VarId,
    pub name: SmolStr,
    pub kind: VarKind,
    pub defined_at: TextRange,
}

/// Owned HIR statement. The tree of clauses under the statement is the
/// analysis target.
#[derive(Debug, Clone)]
pub struct Statement {
    pub clauses: Vec<Clause>,
    pub bindings: IndexMap<VarId, Binding>,
    pub span: TextRange,
}

#[derive(Debug, Clone)]
pub enum Clause {
    Match {
        optional: bool,
        pattern: Pattern,
        span: TextRange,
    },
    Where {
        predicate: Expr,
        span: TextRange,
    },
    With {
        projections: Vec<Projection>,
        filter: Option<Expr>,
        span: TextRange,
    },
    Return {
        projections: Vec<Projection>,
        distinct: bool,
        span: TextRange,
    },
    Unwind {
        list: Expr,
        bind: VarId,
        span: TextRange,
    },
    Create {
        pattern: Pattern,
        span: TextRange,
    },
    Merge {
        pattern: Pattern,
        on_create: Vec<SetItem>,
        on_match: Vec<SetItem>,
        span: TextRange,
    },
    Set {
        items: Vec<SetItem>,
        span: TextRange,
    },
    Remove {
        items: Vec<RemoveItem>,
        span: TextRange,
    },
    Delete {
        targets: Vec<Expr>,
        detach: bool,
        span: TextRange,
    },
    Call {
        procedure: SmolStr,
        args: Vec<Expr>,
        yields: Vec<YieldItem>,
        span: TextRange,
    },
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub parts: Vec<PatternPart>,
}

#[derive(Debug, Clone)]
pub struct PatternPart {
    pub named_as: Option<VarId>,
    pub elements: Vec<PatternElement>,
}

#[derive(Debug, Clone)]
pub enum PatternElement {
    Node {
        bind: Option<VarId>,
        labels: Vec<SmolStr>,
        props: Option<Expr>,
        span: TextRange,
    },
    Rel {
        bind: Option<VarId>,
        types: Vec<SmolStr>,
        direction: Direction,
        length: RelLength,
        props: Option<Expr>,
        span: TextRange,
    },
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

#[derive(Debug, Clone)]
pub struct Projection {
    pub expr: Expr,
    pub alias: Option<SmolStr>,
    pub span: TextRange,
}

#[derive(Debug, Clone)]
pub enum SetItem {
    Property {
        target: Expr,
        prop: SmolStr,
        value: Expr,
    },
    Labels {
        target: VarId,
        labels: Vec<SmolStr>,
    },
    AssignMap {
        target: VarId,
        map: Expr,
        replace: bool,
    },
}

#[derive(Debug, Clone)]
pub enum RemoveItem {
    Property { target: Expr, prop: SmolStr },
    Labels { target: VarId, labels: Vec<SmolStr> },
}

#[derive(Debug, Clone)]
pub struct YieldItem {
    pub name: SmolStr,
    pub alias: Option<SmolStr>,
}

/// HIR expression. Fully resolved: every `Var` carries its `VarId`.
#[derive(Debug, Clone)]
pub enum Expr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(SmolStr),
    Var(VarId),
    Param(SmolStr),
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
        name: SmolStr,
        args: Vec<Expr>,
        distinct: bool,
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
    PatternPredicate(Pattern),
    /// Unresolved variable reference surviving name resolution; carries
    /// the original name for diagnostic messages.
    Unresolved(SmolStr),
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
