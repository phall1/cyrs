//! `cyrs-hir` — High-level IR with name resolution (spec 0001 §6).
//!
//! HIR is an owned, resolved representation of a single Cypher statement.
//! Every variable reference carries its definition ([`VarId`]); syntactic
//! sugar (list/pattern/map comprehensions, shorthand property matching,
//! map projection) is desugared during lowering. The AST ↔ HIR map is
//! preserved on [`Statement`] for span-accurate diagnostics: each lowered
//! node carries a [`HirId`] that keys back into the originating
//! [`cyrs_syntax::SyntaxNode`].
//!
//! The [`lower`] module provides the entry-point [`lower::lower_statement`]
//! that performs the AST → HIR lowering pass (spec §6.1).

// Embedders: see ../../docs/integration-depth.md before depending on this surface.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cyrs-hir/0.0.1")]
// The HIR surface is a grammar catalog — `Clause`, `Expr`,
// `Pattern`, `SetItem`, `RemoveItem`, `Projection`, etc. each have
// one variant per grammar production, and the struct fields on
// those variants (id, span, arguments, …) follow the same
// enumerated shape.  Per-variant / per-field docstrings would
// duplicate §6 of `docs/specs/0001-cypher-frontend.md` and rot
// with the grammar.  Semantic meaning lives at the module level
// (see the top-of-file doc comment + the spec); exempt the whole
// file from `missing_docs` rather than paper them all over.
// See also the parallel allowlist in `cyrs-syntax/src/kind.rs`
// and `cyrs-ast/src/generated.rs`.
#![allow(missing_docs)]

// --- cy-v5u6 catalog HIR ---
pub mod catalog;
// --- end cy-v5u6 ---
pub mod desugar;
// --- cy-cfi typed HIR-lowering failure channel ---
pub mod error;
// --- end cy-cfi ---
pub mod lower;
pub mod pretty;
pub mod scope;
// --- cy-lp3y SESSION SET HIR ---
pub mod session;
// --- end cy-lp3y ---
pub mod visit;

// --- cy-v5u6 catalog HIR ---
pub use catalog::{
    CatalogHir, CatalogStatement, GraphSourceHir, GraphTypeHir, lower_catalog_from_parse,
};
// --- end cy-v5u6 ---
// --- cy-cfi typed HIR-lowering failure channel ---
pub use error::HirLowerError;
// --- end cy-cfi ---
pub use lower::{lower_parse, lower_statement};
pub use scope::{
    BindingKind, Resolution, ResolvedBinding, ResolvedNames, ScopeGraph, ScopeId, ScopeKind,
    ScopeNode,
};
// --- cy-lp3y SESSION SET HIR ---
pub use session::{SessionSetHir, SessionSetVariantHir};
// --- end cy-lp3y ---
pub use visit::{Visitor, walk_clause, walk_expr, walk_statement};

use std::ops::Range;

use cyrs_syntax::{Parse, SyntaxError, SyntaxNode, TextRange};

/// Aggregated output of [`parse_to_hir`].
///
/// Bundles the [`Parse`] (so callers can walk the concrete syntax tree),
/// the lowered HIR [`Statement`], and the parser's accumulated
/// [`SyntaxError`]s in a single struct so embedders that want all three
/// don't have to run the parser twice.
#[derive(Debug, Clone)]
pub struct ParseToHir {
    /// The [`Parse`] result: green tree + accumulated errors.
    pub parse: Parse,
    /// Best-effort HIR lowering. Even when [`Self::syntax_errors`] is
    /// non-empty, the HIR is produced from whatever the parser
    /// recovered (parsers in this workspace always emit a tree).
    pub hir: Statement,
    /// The parser's accumulated [`SyntaxError`]s in source order. A
    /// clone of `self.parse.errors()`, lifted to an owned `Vec` for
    /// caller convenience.
    pub syntax_errors: Vec<SyntaxError>,
}

/// Parse `src` and lower it to HIR in a single call (cy-emb1).
///
/// This is the embedder convenience wrapper: it runs the lexer + parser
/// exactly once, returning the [`Parse`], the lowered [`Statement`], and
/// the parser's [`SyntaxError`]s together. Use this on hot paths instead
/// of separately calling [`cyrs_syntax::parse`] and
/// [`lower::lower_statement`] (which would parse twice).
///
/// # Example
///
/// ```
/// use cyrs_hir::parse_to_hir;
///
/// let result = parse_to_hir("MATCH (n) RETURN n");
/// assert!(result.syntax_errors.is_empty());
/// assert!(!result.hir.clauses.is_empty());
/// // The original `Parse` is still available for AST-level inspection:
/// let _root = result.parse.syntax();
/// ```
#[must_use]
pub fn parse_to_hir(src: &str) -> ParseToHir {
    let parse = cyrs_syntax::parse(src);
    // `lower_parse` is infallible today (it never returns `Err`); the
    // typed channel exists for forward-compatibility (cy-cfi). This
    // convenience wrapper deliberately keeps its best-effort contract:
    // it surfaces syntax errors via `syntax_errors` and always yields a
    // (possibly partial) `Statement`, so an `Err` here would be a
    // genuine lowering-invariant bug — `expect` rather than swallow it.
    let hir = lower::lower_parse(&parse)
        .expect("lower_parse is infallible; an Err signals a lowering-invariant bug");
    let syntax_errors = parse.errors().to_vec();
    ParseToHir {
        parse,
        hir,
        syntax_errors,
    }
}

// Re-export span types so downstream crates (cyrs-sema) can use them
// without adding a direct cyrs-syntax dependency.
pub use cyrs_syntax::{TextRange as HirSpan, TextSize as HirOffset};
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
//
// NOTE (cy-2i9.1): this enum is heavily matched across `cyrs-sema`,
// `cyrs-plan`, `cyrs-db`, and `cyrs-lang-services`.  Marking it
// `#[non_exhaustive]` here would force wildcard arms at every cross-crate
// match site.  Deferred to a follow-up bead; see `docs/stability.md`
// "Deferred: intra-workspace non_exhaustive" section.
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
    // --- cy-lp3y SESSION SET HIR ---
    /// GQL `SESSION SET …` top-level statement (ISO §14.15), when the
    /// source statement was of that form. Mutually exclusive with
    /// `clauses`: when `session_set.is_some()`, `clauses` is empty.
    /// See [`session`] for the variant shape.
    pub session_set: Option<SessionSetHir>,
    // --- end cy-lp3y ---
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
            // --- cy-lp3y SESSION SET HIR ---
            session_set: None,
            // --- end cy-lp3y ---
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

    /// Byte range of the HIR node identified by `id` in the original
    /// source text.
    ///
    /// This is the **supported, SemVer-stable** path for embedders that
    /// project cyrs diagnostics onto a host's caret/underline API — for
    /// example mapping a HIR node back to a byte offset for a Postgres
    /// `errposition()` call. The returned [`Range<usize>`] is a
    /// half-open `start..end` of UTF-8 byte offsets into the source the
    /// statement was lowered from.
    ///
    /// Returns `None` for [`HirId::DUMMY`] and for any id with no entry
    /// in [`Statement::node_map`] (the same cases as
    /// [`Statement::syntax_for`]).
    ///
    /// Equivalent to `self.syntax_for(id).map(|n| n.text_range())`
    /// followed by a [`TextRange`] → byte-offset conversion; prefer this
    /// accessor over open-coding that conversion.
    pub fn span_of(&self, id: HirId) -> Option<Range<usize>> {
        self.syntax_for(id).map(|node| {
            let range = node.text_range();
            u32::from(range.start()) as usize..u32::from(range.end()) as usize
        })
    }

    /// Total number of lowered HIR nodes recorded in the map.
    pub fn node_count(&self) -> usize {
        self.node_map.len()
    }
}

/// A single clause in a statement. Each variant carries its own
/// [`HirId`] so diagnostics can point back at the originating AST.
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
        /// `ORDER BY` trailer items, in source order. Empty when absent.
        order_by: Vec<OrderItem>,
        /// `SKIP` expression, or `None` when the trailer is absent.
        skip: Option<Expr>,
        /// `LIMIT` expression, or `None` when the trailer is absent.
        limit: Option<Expr>,
        span: TextRange,
    },
    Return {
        id: HirId,
        projections: Vec<Projection>,
        distinct: bool,
        /// `ORDER BY` trailer items, in source order. Empty when absent.
        order_by: Vec<OrderItem>,
        /// `SKIP` expression, or `None` when the trailer is absent.
        skip: Option<Expr>,
        /// `LIMIT` expression, or `None` when the trailer is absent.
        limit: Option<Expr>,
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
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
#[non_exhaustive]
pub enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

/// Variable-length relationship bounds. `Single` means no `*` suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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

/// A single `ORDER BY` item — sort key plus direction.
///
/// Lowered from `ORDER_ITEM` syntax nodes attached to `RETURN_BODY` /
/// `WITH_CLAUSE` (spec §6.1 trailer). `descending` is `true` for `DESC`
/// or `DESCENDING`; ascending is the default and applies when no
/// direction keyword is present (or `ASC` / `ASCENDING` is given).
#[derive(Debug, Clone)]
pub struct OrderItem {
    pub expr: Expr,
    pub descending: bool,
    pub span: TextRange,
}

/// Right-hand side of a `SET` clause item.
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
    /// List slicing — `xs[i..j]`, `xs[..j]`, `xs[i..]` (cy-7s6.1).
    ///
    /// `start` and `end` are optional: elision maps to `None` (openCypher
    /// semantics — absent `start` defaults to 0, absent `end` defaults to
    /// `len`). Negative indices are permitted and carry their from-end
    /// meaning at evaluation time; the HIR does not normalise them.
    ///
    /// v1 scope is lists only. Passing a non-list to `Expr::Slice` is a
    /// type error reported by the sema `kinds` pass (E5010) — see
    /// `cyrs-sema::kinds` for the emission site.
    Slice {
        target: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
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
    /// A list predicate — `ANY|ALL|NONE|SINGLE(v IN xs [WHERE p(v)])`
    /// (cy-8x5, spec §19 row "List predicates").
    ///
    /// The `var` is scoped to the `predicate` expression only; callers
    /// that traverse the HIR must introduce a child scope before
    /// descending into the predicate (this parallels the
    /// `ListComprehension` `filter_var` discipline). The `predicate` is
    /// optional per openCypher spec: bare `ANY(x IN xs)` is true iff
    /// `xs` is non-empty, `ALL(x IN xs)` is vacuously true, etc.
    ///
    /// Result type is always `Bool`.
    ListPredicate {
        kind: ListPredKind,
        var: VarId,
        iterable: Box<Expr>,
        predicate: Option<Box<Expr>>,
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
    // --- cy-p1u5 EXISTS parser-only ---
    /// `EXISTS { <Statement> }` / `EXISTS ( MATCH … )` subquery
    /// expression (ISO/IEC 39075:2024 §10.7 / §14.10, spec amendment
    /// 2026-05-19 cy-5e3f, bead cy-p1u5).
    ///
    /// **Inert marker** — the body is *not* lowered into HIR clauses;
    /// no `VarId`s are minted for the inner pattern, and name
    /// resolution does not descend into it. The variant exists so the
    /// HIR is reachable from the CST shape that bead cy-p1u5 added,
    /// and so the sema dialect-gate pass can emit
    /// `DiagCode::E4017` (`exists_subquery` gate) on every
    /// occurrence — keeping spec §20 D1 / N4 in force at the
    /// semantic surface while the parser accepts the syntax.
    ///
    /// `span` is the [`TextRange`] covering the entire `EXISTS …`
    /// expression on the originating CST node, so the diagnostic
    /// points at the offending construct rather than a zero-width
    /// span. Type, in any consumer that infers one, is treated as
    /// `Type::Unknown` (cascading errors are suppressed in the same
    /// way as [`Expr::Unresolved`]).
    ExistsSubqueryDeferred {
        span: TextRange,
    },
    // --- end cy-p1u5 ---
}

/// One item in a [`Expr::MapProjection`] (spec §6.1).
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
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
//
// NOTE (cy-2i9.1): heavily matched across the workspace — see
// `VarKind` note above and `docs/stability.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// Discriminant for [`Expr::ListPredicate`] — one of the four openCypher
/// list predicates (cy-8x5, spec §19 row "List predicates").
///
/// Marked `#[non_exhaustive]` at the public boundary so future list
/// predicates (e.g. `EVERY`) can be added without breaking downstream
/// matches (see `docs/stability.md`, same policy as cy-2i9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListPredKind {
    /// `ANY(v IN xs WHERE p)` — at least one element matches.
    Any,
    /// `ALL(v IN xs WHERE p)` — every element matches.
    All,
    /// `NONE(v IN xs WHERE p)` — no element matches.
    None,
    /// `SINGLE(v IN xs WHERE p)` — exactly one element matches.
    Single,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrs_syntax::parse;

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
    fn span_of_returns_byte_range_of_recorded_node() {
        // Lower a real statement so the node map carries distinct,
        // sub-statement syntax nodes (the MATCH and RETURN clauses).
        let src = "MATCH (n) RETURN n";
        let stmt = lower::lower_statement(src);

        let match_id = stmt.clauses[0].id();
        let return_id = stmt.clauses[1].id();

        // Each clause id resolves to the byte range its syntax node
        // covers, and that range agrees with `syntax_for`.
        for id in [match_id, return_id] {
            let range = stmt.span_of(id).expect("clause id has a node");
            let syntax_range = stmt.syntax_for(id).unwrap().text_range();
            assert_eq!(range.start, u32::from(syntax_range.start()) as usize);
            assert_eq!(range.end, u32::from(syntax_range.end()) as usize);
            // The range is a valid, in-bounds slice of the source.
            assert!(range.start <= range.end);
            assert!(range.end <= src.len());
            let _ = &src[range];
        }

        // The MATCH clause starts at the head of the source.
        assert_eq!(stmt.span_of(match_id).unwrap().start, 0);

        // Unknown and dummy ids yield `None`, matching `syntax_for`.
        assert_eq!(stmt.span_of(HirId::DUMMY), None);
        assert_eq!(stmt.span_of(HirId(99_999)), None);
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
            order_by: Vec::new(),
            skip: None,
            limit: None,
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
    fn parse_to_hir_returns_parse_hir_and_empty_errors() {
        let result = parse_to_hir("MATCH (n) RETURN n");
        // SyntaxError list is empty for well-formed input.
        assert!(
            result.syntax_errors.is_empty(),
            "expected no syntax errors, got: {:?}",
            result.syntax_errors
        );
        // The Parse is preserved and walks back to the original source.
        assert_eq!(result.parse.syntax().to_string(), "MATCH (n) RETURN n");
        // HIR was lowered: at least MATCH + RETURN clauses exist.
        assert_eq!(result.hir.clauses.len(), 2);
        assert!(matches!(result.hir.clauses[0], Clause::Match { .. }));
        assert!(matches!(result.hir.clauses[1], Clause::Return { .. }));
    }

    #[test]
    fn lower_statement_matches_lower_parse() {
        // The sugar wrapper must agree with the primitive.
        let src = "MATCH (a) RETURN a";
        let parse = cyrs_syntax::parse(src);
        let via_parse = lower::lower_parse(&parse).expect("clean input lowers");
        let via_str = lower::lower_statement(src).expect("clean input lowers");
        assert_eq!(via_parse.clauses.len(), via_str.clauses.len());
        assert_eq!(via_parse.bindings.len(), via_str.bindings.len());
        assert_eq!(via_parse.node_count(), via_str.node_count());
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
