//! `cypher-hir` — High-level IR with name resolution (spec 0001 §6).
//!
//! HIR is an owned, resolved representation of a single Cypher statement.
//! Every variable reference carries its definition ([`VarId`]); syntactic
//! sugar (list/pattern/map comprehensions, shorthand property matching,
//! map projection) is desugared during lowering. The AST ↔ HIR map is
//! preserved on [`Statement`] for span-accurate diagnostics: each lowered
//! node carries a [`HirId`] that keys back into the originating
//! [`cypher_syntax::SyntaxNode`].
//!
//! The [`lower`] module provides the entry-point [`lower::lower_statement`]
//! that performs the AST → HIR lowering pass (spec §6.1).

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-hir/0.0.1")]

pub mod desugar;
pub mod lower;
pub mod scope;

pub use scope::{
    BindingKind, Resolution, ResolvedBinding, ResolvedNames, ScopeGraph, ScopeId, ScopeKind,
    ScopeNode,
};

use cypher_syntax::{SyntaxNode, TextRange};

// Re-export span types so downstream crates (cypher-sema) can use them
// without adding a direct cypher-syntax dependency.
pub use cypher_syntax::{TextRange as HirSpan, TextSize as HirOffset};
use indexmap::IndexMap;
use smol_str::SmolStr;

/// Statement-scoped index identifying an HIR node.
///
/// `HirId`s are dense and monotonic within a single [`Statement`]: the
/// lowering pass allocates them via [`Statement::alloc_id`] as it walks
/// the AST. `HirId(0)` is reserved for [`HirId::DUMMY`] so that
/// uninitialised fields are detectable in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HirId(pub u32);

impl HirId {
    /// Sentinel value used before an id has been assigned. Never points
    /// at a real node; the [`Statement::node_map`] entry is absent.
    pub const DUMMY: HirId = HirId(0);
}

/// Interned variable identity within a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub u32);

/// Variable kind recorded at binding (spec §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// Bound by a node pattern (e.g. `MATCH (a)`).
    Node,
    /// Bound by a relationship pattern (e.g. `-[r:KNOWS]->`).
    Relationship,
    /// Bound by a path assignment (e.g. `p = (a)-[]->(b)`).
    Path,
    /// Bound by `UNWIND ... AS v`, `WITH expr AS v`, or `CALL ... YIELD v`.
    Value,
}

/// A variable declaration within a statement scope.
#[derive(Debug, Clone)]
pub struct Binding {
    pub id: VarId,
    pub name: SmolStr,
    pub kind: VarKind,
    pub defined_at: TextRange,
}

/// Owned HIR statement.
///
/// The tree of [`Clause`]s under the statement is the analysis target.
/// Bindings are interned in [`Statement::bindings`]; the AST ↔ HIR map
/// is kept in [`Statement::node_map`] and queried via
/// [`Statement::syntax_for`].
#[derive(Debug, Clone)]
pub struct Statement {
    pub clauses: Vec<Clause>,
    pub bindings: IndexMap<VarId, Binding>,
    pub span: TextRange,
    /// AST ↔ HIR map. Keyed by each node's [`HirId`]; values are the
    /// originating concrete syntax node. Determinism is preserved by
    /// [`IndexMap`]'s insertion order.
    pub node_map: IndexMap<HirId, SyntaxNode>,
    /// Monotonic counter for [`Self::alloc_id`]. Starts at 1 so that
    /// [`HirId::DUMMY`] remains a distinguishable sentinel.
    next_id: u32,
}

impl Statement {
    /// Create an empty statement spanning `span`.
    pub fn new(span: TextRange) -> Self {
        Self {
            clauses: Vec::new(),
            bindings: IndexMap::new(),
            span,
            node_map: IndexMap::new(),
            next_id: 1,
        }
    }

    /// Allocate a fresh [`HirId`] and record its originating syntax
    /// node. The returned id is unique within this statement.
    pub fn alloc_id(&mut self, syntax: SyntaxNode) -> HirId {
        let id = HirId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("HirId counter overflowed u32 within a single statement");
        self.node_map.insert(id, syntax);
        id
    }

    /// Look up the concrete syntax node a given [`HirId`] was lowered
    /// from. Returns `None` for unknown or dummy ids.
    pub fn syntax_for(&self, id: HirId) -> Option<&SyntaxNode> {
        self.node_map.get(&id)
    }

    /// Total number of lowered HIR nodes recorded in the map.
    pub fn node_count(&self) -> usize {
        self.node_map.len()
    }
}

/// A single clause in a statement. Each variant carries its own
/// [`HirId`] so diagnostics can point back at the originating AST.
#[derive(Debug, Clone)]
pub enum Clause {
    Match {
        id: HirId,
        optional: bool,
        pattern: Pattern,
        span: TextRange,
    },
    Where {
        id: HirId,
        predicate: Expr,
        span: TextRange,
    },
    With {
        id: HirId,
        projections: Vec<Projection>,
        filter: Option<Expr>,
        span: TextRange,
    },
    Return {
        id: HirId,
        projections: Vec<Projection>,
        distinct: bool,
        span: TextRange,
    },
    Unwind {
        id: HirId,
        list: Expr,
        bind: VarId,
        span: TextRange,
    },
    Create {
        id: HirId,
        pattern: Pattern,
        span: TextRange,
    },
    Merge {
        id: HirId,
        pattern: Pattern,
        on_create: Vec<SetItem>,
        on_match: Vec<SetItem>,
        span: TextRange,
    },
    Set {
        id: HirId,
        items: Vec<SetItem>,
        span: TextRange,
    },
    Remove {
        id: HirId,
        items: Vec<RemoveItem>,
        span: TextRange,
    },
    Delete {
        id: HirId,
        targets: Vec<Expr>,
        detach: bool,
        span: TextRange,
    },
    Call {
        id: HirId,
        procedure: SmolStr,
        args: Vec<Expr>,
        yields: Vec<YieldItem>,
        span: TextRange,
    },
}

impl Clause {
    /// Return the [`HirId`] identifying this clause.
    pub fn id(&self) -> HirId {
        match self {
            Clause::Match { id, .. }
            | Clause::Where { id, .. }
            | Clause::With { id, .. }
            | Clause::Return { id, .. }
            | Clause::Unwind { id, .. }
            | Clause::Create { id, .. }
            | Clause::Merge { id, .. }
            | Clause::Set { id, .. }
            | Clause::Remove { id, .. }
            | Clause::Delete { id, .. }
            | Clause::Call { id, .. } => *id,
        }
    }

    /// Source span of the clause.
    pub fn span(&self) -> TextRange {
        match self {
            Clause::Match { span, .. }
            | Clause::Where { span, .. }
            | Clause::With { span, .. }
            | Clause::Return { span, .. }
            | Clause::Unwind { span, .. }
            | Clause::Create { span, .. }
            | Clause::Merge { span, .. }
            | Clause::Set { span, .. }
            | Clause::Remove { span, .. }
            | Clause::Delete { span, .. }
            | Clause::Call { span, .. } => *span,
        }
    }
}

/// A graph pattern, broken into connected components.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub parts: Vec<PatternPart>,
}

/// One connected component of a [`Pattern`], optionally bound to a
/// path variable (`p = (a)-[]->(b)`).
#[derive(Debug, Clone)]
pub struct PatternPart {
    pub named_as: Option<VarId>,
    pub elements: Vec<PatternElement>,
}

/// An individual node or relationship within a [`PatternPart`].
#[derive(Debug, Clone)]
pub enum PatternElement {
    Node {
        id: HirId,
        bind: Option<VarId>,
        labels: Vec<SmolStr>,
        props: Option<Expr>,
        span: TextRange,
    },
    Rel {
        id: HirId,
        bind: Option<VarId>,
        types: Vec<SmolStr>,
        direction: Direction,
        length: RelLength,
        props: Option<Expr>,
        span: TextRange,
    },
}

impl PatternElement {
    /// Return the [`HirId`] identifying this element.
    pub fn id(&self) -> HirId {
        match self {
            PatternElement::Node { id, .. } | PatternElement::Rel { id, .. } => *id,
        }
    }

    /// Source span of the element.
    pub fn span(&self) -> TextRange {
        match self {
            PatternElement::Node { span, .. } | PatternElement::Rel { span, .. } => *span,
        }
    }
}

/// Relationship direction as written in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

/// Variable-length relationship bounds. `Single` means no `*` suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelLength {
    Single,
    Variable { min: Option<u64>, max: Option<u64> },
}

/// A single output of `RETURN` or `WITH`.
#[derive(Debug, Clone)]
pub struct Projection {
    pub expr: Expr,
    pub alias: Option<SmolStr>,
    pub span: TextRange,
}

/// Right-hand side of a `SET` clause item.
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

/// An item within a `REMOVE` clause.
#[derive(Debug, Clone)]
pub enum RemoveItem {
    Property { target: Expr, prop: SmolStr },
    Labels { target: VarId, labels: Vec<SmolStr> },
}

/// A single `YIELD` binding on a `CALL` clause.
#[derive(Debug, Clone)]
pub struct YieldItem {
    pub name: SmolStr,
    pub alias: Option<SmolStr>,
}

/// HIR expression. Fully resolved: every [`Expr::Var`] carries its
/// [`VarId`].
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
    /// Desugared list comprehension (spec §6.1).
    ///
    /// `[x IN xs WHERE p(x) | e(x)]` is lowered to this canonical form.
    /// `filter_var` is the iteration variable, `iterable` is the source
    /// list, `filter` is the optional predicate (present when `WHERE`
    /// appears), and `map_expr` is the projection expression (the part
    /// after `|`; equals `Expr::Var(filter_var)` when omitted).
    ListComprehension {
        filter_var: VarId,
        iterable: Box<Expr>,
        filter: Option<Box<Expr>>,
        map_expr: Box<Expr>,
    },
    /// Desugared map projection (spec §6.1).
    ///
    /// `a { .name, .age, computed: f(a) }` is lowered to an explicit map
    /// construction carrying the base expression and a list of named
    /// fields. Each [`MapProjectionItem`] is either a property copy or a
    /// computed key-value pair.
    MapProjection {
        base: Box<Expr>,
        items: Vec<MapProjectionItem>,
    },
    /// Unresolved variable reference surviving name resolution; carries
    /// the original name for diagnostic messages.
    Unresolved(SmolStr),
}

/// One item in a [`Expr::MapProjection`] (spec §6.1).
#[derive(Debug, Clone)]
pub enum MapProjectionItem {
    /// `.prop` — copy property `prop` from the base expression.
    PropCopy { prop: SmolStr },
    /// `key: expr` — computed key-value pair.
    Computed { key: SmolStr, value: Expr },
    /// `varName` — include a variable as `{varName: varName}`.
    VarShorthand { var: VarId, name: SmolStr },
    /// `varName: expr` — aliased expression inside projection.
    Aliased { key: SmolStr, value: Expr },
}

/// Binary operators (spec §5.6 / §7.2).
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

/// Unary operators (spec §5.6 / §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cypher_syntax::parse;

    fn sample_syntax() -> SyntaxNode {
        parse("MATCH (a) RETURN a").syntax()
    }

    #[test]
    fn hir_id_dummy_is_zero() {
        assert_eq!(HirId::DUMMY, HirId(0));
    }

    #[test]
    fn alloc_id_is_monotonic_and_records_node_map() {
        let root = sample_syntax();
        let mut stmt = Statement::new(root.text_range());

        let id1 = stmt.alloc_id(root.clone());
        let id2 = stmt.alloc_id(root.clone());

        assert_ne!(id1, HirId::DUMMY);
        assert_eq!(id1, HirId(1));
        assert_eq!(id2, HirId(2));
        assert_eq!(stmt.node_count(), 2);
        assert!(stmt.syntax_for(id1).is_some());
        assert!(stmt.syntax_for(id2).is_some());
        assert!(stmt.syntax_for(HirId::DUMMY).is_none());
        assert!(stmt.syntax_for(HirId(999)).is_none());
    }

    #[test]
    fn statement_with_bindings_and_clauses_round_trips() {
        let root = sample_syntax();
        let mut stmt = Statement::new(root.text_range());

        let var = VarId(0);
        stmt.bindings.insert(
            var,
            Binding {
                id: var,
                name: SmolStr::new("a"),
                kind: VarKind::Node,
                defined_at: root.text_range(),
            },
        );

        let match_id = stmt.alloc_id(root.clone());
        let node_id = stmt.alloc_id(root.clone());
        stmt.clauses.push(Clause::Match {
            id: match_id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: node_id,
                        bind: Some(var),
                        labels: Vec::new(),
                        props: None,
                        span: root.text_range(),
                    }],
                }],
            },
            span: root.text_range(),
        });

        let return_id = stmt.alloc_id(root.clone());
        stmt.clauses.push(Clause::Return {
            id: return_id,
            projections: vec![Projection {
                expr: Expr::Var(var),
                alias: None,
                span: root.text_range(),
            }],
            distinct: false,
            span: root.text_range(),
        });

        assert_eq!(stmt.clauses.len(), 2);
        assert_eq!(stmt.clauses[0].id(), match_id);
        assert_eq!(stmt.clauses[1].id(), return_id);
        assert_eq!(stmt.bindings.len(), 1);
        assert!(stmt.syntax_for(match_id).is_some());
        assert!(stmt.syntax_for(return_id).is_some());

        // Clone + Debug bounds on Statement.
        let cloned: Statement = stmt.clone();
        let _ = format!("{cloned:?}");
        assert_eq!(cloned.node_count(), stmt.node_count());
    }

    #[test]
    fn pattern_element_accessors() {
        let root = sample_syntax();
        let mut stmt = Statement::new(root.text_range());
        let node_id = stmt.alloc_id(root.clone());
        let elem = PatternElement::Node {
            id: node_id,
            bind: None,
            labels: Vec::new(),
            props: None,
            span: root.text_range(),
        };
        assert_eq!(elem.id(), node_id);
        assert_eq!(elem.span(), root.text_range());
    }
}
