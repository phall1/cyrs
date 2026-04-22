//! Kind-consistency pass (spec 0001 §6.3).
//!
//! Walks the HIR after name resolution and flags variable uses that are
//! structurally incompatible with their binding kind:
//!
//! - [`E2007`] — variable of kind `Node`, `Relationship`, or `Path` appears
//!   in an arithmetic / numeric expression context (e.g., `n + 1`).
//! - [`E2008`] — variable of wrong kind in a pattern-position context:
//!   a `Value` variable used where a node/rel binder is expected, or a
//!   non-`Path` variable used where a path binder is expected.
//!
//! These are schema-free structural checks. Schema-aware checks (unknown
//! labels, property types, …) live in the `E3000` range and are out of
//! scope here (§6.3 fence).
//!
//! [`E2007`]: cypher_diag::DiagCode::E2007
//! [`E2008`]: cypher_diag::DiagCode::E2008

use cypher_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cypher_hir::{
    BinOp, Binding, Clause, Expr, HirOffset, HirSpan, Pattern, PatternElement, PatternPart,
    SetItem, Statement, VarId, VarKind,
};
use indexmap::IndexMap;

/// Run the kind-consistency pass on a resolved HIR statement.
///
/// `bindings` is the [`Statement::bindings`] map; `sink` receives all
/// diagnostics emitted by the pass.
pub fn check_kinds(stmt: &Statement, sink: &mut DiagnosticsSink) {
    let ctx = KindCtx {
        bindings: &stmt.bindings,
    };
    for clause in &stmt.clauses {
        ctx.check_clause(clause, sink);
    }
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

struct KindCtx<'a> {
    bindings: &'a IndexMap<VarId, Binding>,
}

impl KindCtx<'_> {
    fn kind_of(&self, var: VarId) -> Option<VarKind> {
        self.bindings.get(&var).map(|b| b.kind)
    }

    fn name_of(&self, var: VarId) -> String {
        self.bindings
            .get(&var)
            .map_or_else(|| format!("__var{}", var.0), |b| b.name.to_string())
    }

    fn check_clause(&self, clause: &Clause, sink: &mut DiagnosticsSink) {
        match clause {
            Clause::Match { pattern, .. }
            | Clause::Create { pattern, .. }
            | Clause::Merge { pattern, .. } => {
                self.check_pattern(pattern, sink);
            }
            Clause::Where { predicate, .. } => {
                self.check_expr(predicate, ExprCtx::General, sink);
            }
            Clause::With {
                projections,
                filter,
                ..
            } => {
                for proj in projections {
                    self.check_expr(&proj.expr, ExprCtx::General, sink);
                }
                if let Some(f) = filter {
                    self.check_expr(f, ExprCtx::General, sink);
                }
            }
            Clause::Return { projections, .. } => {
                for proj in projections {
                    self.check_expr(&proj.expr, ExprCtx::General, sink);
                }
            }
            Clause::Unwind { list, .. } => {
                self.check_expr(list, ExprCtx::General, sink);
            }
            Clause::Set { items, .. } => {
                for item in items {
                    match item {
                        SetItem::Property { target, value, .. } => {
                            self.check_expr(target, ExprCtx::General, sink);
                            self.check_expr(value, ExprCtx::General, sink);
                        }
                        SetItem::AssignMap { map, .. } => {
                            self.check_expr(map, ExprCtx::General, sink);
                        }
                        SetItem::Labels { .. } => {}
                    }
                }
            }
            Clause::Remove { items, .. } => {
                use cypher_hir::RemoveItem;
                for item in items {
                    match item {
                        RemoveItem::Property { target, .. } => {
                            self.check_expr(target, ExprCtx::General, sink);
                        }
                        RemoveItem::Labels { .. } => {}
                    }
                }
            }
            Clause::Delete { targets, .. } => {
                for expr in targets {
                    self.check_expr(expr, ExprCtx::General, sink);
                }
            }
            Clause::Call { args, .. } => {
                for arg in args {
                    self.check_expr(arg, ExprCtx::General, sink);
                }
            }
        }
    }

    fn check_pattern(&self, pattern: &Pattern, sink: &mut DiagnosticsSink) {
        for part in &pattern.parts {
            self.check_pattern_part(part, sink);
        }
    }

    fn check_pattern_part(&self, part: &PatternPart, sink: &mut DiagnosticsSink) {
        // Path binder must be VarKind::Path (lowering assigns this; sanity
        // check here so any manual construction is caught).
        if let Some(path_var) = part.named_as
            && let Some(kind) = self.kind_of(path_var)
            && kind != VarKind::Path
        {
            let name = self.name_of(path_var);
            let span = dummy_span();
            sink.push(Diagnostic::error(
                DiagCode::E2008,
                span,
                format!(
                    "variable `{name}` used as path binder but has kind `{}`; \
                     expected `Path`",
                    kind_name(kind)
                ),
            ));
        }

        for elem in &part.elements {
            match elem {
                PatternElement::Node {
                    bind, props, span, ..
                } => {
                    // A node-pattern binder must have kind Node.
                    if let Some(var) = bind
                        && let Some(kind) = self.kind_of(*var)
                        && !matches!(kind, VarKind::Node)
                    {
                        let name = self.name_of(*var);
                        sink.push(Diagnostic::error(
                            DiagCode::E2008,
                            *span,
                            format!(
                                "variable `{name}` used in node-pattern position \
                                 but has kind `{}`; expected `Node`",
                                kind_name(kind)
                            ),
                        ));
                    }
                    if let Some(p) = props {
                        self.check_expr(p, ExprCtx::General, sink);
                    }
                }
                PatternElement::Rel {
                    bind, props, span, ..
                } => {
                    // A rel-pattern binder must have kind Relationship.
                    if let Some(var) = bind
                        && let Some(kind) = self.kind_of(*var)
                        && !matches!(kind, VarKind::Relationship)
                    {
                        let name = self.name_of(*var);
                        sink.push(Diagnostic::error(
                            DiagCode::E2008,
                            *span,
                            format!(
                                "variable `{name}` used in relationship-pattern \
                                 position but has kind `{}`; expected `Relationship`",
                                kind_name(kind)
                            ),
                        ));
                    }
                    if let Some(p) = props {
                        self.check_expr(p, ExprCtx::General, sink);
                    }
                }
            }
        }
    }

    fn check_expr(&self, expr: &Expr, ctx: ExprCtx, sink: &mut DiagnosticsSink) {
        match expr {
            // ---------------------------------------------------------------
            // Variable references — the main kind-check site.
            // ---------------------------------------------------------------
            Expr::Var(var) => {
                self.check_var_in_ctx(*var, ctx, sink);
            }
            // ---------------------------------------------------------------
            // Arithmetic binary operations — operands must be Value-kinded.
            // ---------------------------------------------------------------
            Expr::BinOp { op, lhs, rhs } => {
                let inner_ctx = if is_arithmetic_op(*op) {
                    ExprCtx::Arithmetic
                } else {
                    ExprCtx::General
                };
                self.check_expr(lhs, inner_ctx, sink);
                self.check_expr(rhs, inner_ctx, sink);
            }

            // ---------------------------------------------------------------
            // Recursive cases — propagate context downward.
            // ---------------------------------------------------------------
            Expr::UnaryOp { op: _, operand } => {
                // Unary minus is arithmetic; NOT is boolean (general).
                self.check_expr(operand, ctx, sink);
            }
            Expr::Prop { target, prop } => {
                // Property access on a non-Value var: check that target is
                // not a Path (which has no properties). Exception (cy-zo9):
                // the synthetic `.id` pseudo-property is valid on Nodes,
                // Relationships, *and* Paths — so skip the Path rejection
                // in that case and let `infer.rs` type it as INTEGER.
                let inner_ctx = if prop.as_str() == "id" {
                    ExprCtx::General
                } else {
                    ExprCtx::PropAccess
                };
                self.check_expr(target, inner_ctx, sink);
            }
            Expr::Index { target, index } => {
                self.require_indexable(target, /*op=*/ "index", sink);
                self.check_expr(target, ExprCtx::General, sink);
                self.check_expr(index, ExprCtx::General, sink);
            }
            Expr::Slice { target, start, end } => {
                self.require_indexable(target, /*op=*/ "slice", sink);
                self.check_expr(target, ExprCtx::General, sink);
                if let Some(s) = start {
                    self.check_expr(s, ExprCtx::General, sink);
                }
                if let Some(e) = end {
                    self.check_expr(e, ExprCtx::General, sink);
                }
            }
            Expr::List(items) => {
                for e in items {
                    self.check_expr(e, ExprCtx::General, sink);
                }
            }
            Expr::Map(pairs) => {
                for (_, e) in pairs {
                    self.check_expr(e, ExprCtx::General, sink);
                }
            }
            Expr::Call { args, .. } => {
                for a in args {
                    self.check_expr(a, ExprCtx::General, sink);
                }
            }
            Expr::InList { operand, list } => {
                self.check_expr(operand, ExprCtx::General, sink);
                self.check_expr(list, ExprCtx::General, sink);
            }
            Expr::IsNull { operand, .. } => {
                self.check_expr(operand, ExprCtx::General, sink);
            }
            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => {
                if let Some(s) = scrutinee {
                    self.check_expr(s, ExprCtx::General, sink);
                }
                for (cond, result) in arms {
                    self.check_expr(cond, ExprCtx::General, sink);
                    self.check_expr(result, ExprCtx::General, sink);
                }
                if let Some(o) = otherwise {
                    self.check_expr(o, ExprCtx::General, sink);
                }
            }
            Expr::ListComprehension {
                iterable,
                filter,
                map_expr,
                ..
            } => {
                // Structural guard: iterable must plausibly be a list.
                // Non-list-shaped atoms (scalars, maps, Node/Rel/Path vars,
                // pattern predicates) are rejected under the same E5010
                // class used for list-indexing (cy-7s6.1) — the error class
                // is identical: "this place requires a list". cy-5gh.
                self.require_indexable(iterable, "iterate over", sink);
                self.check_expr(iterable, ExprCtx::General, sink);
                if let Some(f) = filter {
                    self.check_expr(f, ExprCtx::General, sink);
                }
                self.check_expr(map_expr, ExprCtx::General, sink);
            }
            // cy-8x5 — list-predicate type rules (spec §19 row
            // "List predicates"):
            //   iterable must be `LIST<T>`     → E5011 on structural mismatch
            //   predicate, if present, Bool    → E2011 on a bare non-bool literal
            //   result type is always Bool     (enforced by infer.rs)
            Expr::ListPredicate {
                iterable,
                predicate,
                ..
            } => {
                self.require_list_iterable(iterable, sink);
                self.check_expr(iterable, ExprCtx::General, sink);
                if let Some(p) = predicate {
                    self.require_bool_predicate(p, sink);
                    self.check_expr(p, ExprCtx::General, sink);
                }
            }
            Expr::MapProjection { base, items } => {
                self.check_expr(base, ExprCtx::PropAccess, sink);
                for item in items {
                    use cypher_hir::MapProjectionItem;
                    match item {
                        MapProjectionItem::Computed { value, .. }
                        | MapProjectionItem::Aliased { value, .. } => {
                            self.check_expr(value, ExprCtx::General, sink);
                        }
                        MapProjectionItem::PropCopy { .. }
                        | MapProjectionItem::VarShorthand { .. } => {}
                    }
                }
            }
            // Pattern predicates — variables inside use Graph context.
            Expr::PatternPredicate(pat) => {
                self.check_pattern(pat, sink);
            }
            // Unresolved vars, literals, and params carry no kind-check.
            // Unresolved refs already have E1001 from name resolution.
            Expr::Unresolved(_)
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::String(_)
            | Expr::Param(_) => {}
        }
    }

    /// Emit a diagnostic if `var`'s kind is incompatible with `ctx`.
    fn check_var_in_ctx(&self, var: VarId, ctx: ExprCtx, sink: &mut DiagnosticsSink) {
        let Some(kind) = self.kind_of(var) else {
            return;
        };
        let span = dummy_span();
        match ctx {
            ExprCtx::Arithmetic => {
                // Only Value variables make sense in arithmetic.
                if !matches!(kind, VarKind::Value) {
                    let name = self.name_of(var);
                    sink.push(Diagnostic::error(
                        DiagCode::E2007,
                        span,
                        format!(
                            "variable `{name}` has kind `{}` but is used in an arithmetic \
                             expression; only `Value` variables are valid here",
                            kind_name(kind)
                        ),
                    ));
                }
            }
            ExprCtx::PropAccess => {
                // Property access / map projection is invalid on a Path variable.
                if matches!(kind, VarKind::Path) {
                    let name = self.name_of(var);
                    sink.push(Diagnostic::error(
                        DiagCode::E2007,
                        span,
                        format!(
                            "variable `{name}` has kind `Path` and does not support \
                             property access; use `nodes()` or `relationships()` to \
                             extract graph elements first"
                        ),
                    ));
                }
            }
            ExprCtx::General => {
                // No kind restriction in general expression position.
            }
        }
    }

    /// Structurally check that `target` is something that can be indexed or
    /// sliced (i.e. plausibly a list). Emits `E5010` when the target is
    /// provably non-list — a literal scalar, a Node / Relationship / Path
    /// variable, a map literal, or a pattern predicate.
    ///
    /// The check is intentionally narrow: schema-free inference cannot
    /// tell "variable holding Any" apart from "variable holding a list",
    /// so those cases are left to the schema-aware pass (out of scope for
    /// cy-7s6.1). v1 scope is LIST only; STRING indexing is deferred and
    /// currently errors under this code (spec §19 / §20 follow-up).
    fn require_indexable(&self, target: &Expr, op: &str, sink: &mut DiagnosticsSink) {
        let reason: Option<&str> = match target {
            Expr::Bool(_) => Some("Bool"),
            Expr::Int(_) => Some("Int"),
            Expr::Float(_) => Some("Float"),
            Expr::String(_) => Some("String"),
            Expr::Null => Some("Null"),
            Expr::Map(_) => Some("Map"),
            Expr::PatternPredicate(_) => Some("Pattern"),
            Expr::Var(var) => match self.kind_of(*var) {
                Some(VarKind::Node) => Some("Node"),
                Some(VarKind::Relationship) => Some("Relationship"),
                Some(VarKind::Path) => Some("Path"),
                // Value-kinded variables may hold a list at runtime — no
                // structural evidence to the contrary here.
                Some(VarKind::Value) | None => None,
            },
            // Calls, params, other indexing / slicing expressions, list
            // literals, list comprehensions, map projections, and
            // arithmetic / logical expressions are not *structurally*
            // non-list — defer to schema-aware / runtime checks.
            _ => None,
        };

        if let Some(ty) = reason {
            sink.push(Diagnostic::error(
                DiagCode::E5010,
                dummy_span(),
                format!(
                    "cannot {op} non-list expression of type `{ty}`; \
                     list indexing and slicing require a list target"
                ),
            ));
        }
    }

    /// Emit E5011 if `iterable` is structurally non-list.
    ///
    /// Mirrors `require_indexable` in scope — schema-free, structural:
    /// scalar literals, `Node` / `Relationship` / `Path` variables, map
    /// literals, and pattern predicates are provably non-list.
    /// `Value`-kinded variables, calls, and other composite expressions
    /// are deferred to schema-aware / runtime inspection.
    fn require_list_iterable(&self, iterable: &Expr, sink: &mut DiagnosticsSink) {
        let reason: Option<&str> = match iterable {
            Expr::Bool(_) => Some("Bool"),
            Expr::Int(_) => Some("Int"),
            Expr::Float(_) => Some("Float"),
            Expr::String(_) => Some("String"),
            Expr::Null => Some("Null"),
            Expr::Map(_) => Some("Map"),
            Expr::PatternPredicate(_) => Some("Pattern"),
            Expr::Var(var) => match self.kind_of(*var) {
                Some(VarKind::Node) => Some("Node"),
                Some(VarKind::Relationship) => Some("Relationship"),
                Some(VarKind::Path) => Some("Path"),
                Some(VarKind::Value) | None => None,
            },
            _ => None,
        };

        if let Some(ty) = reason {
            sink.push(Diagnostic::error(
                DiagCode::E5011,
                dummy_span(),
                format!(
                    "list-predicate iterable must be a list but found `{ty}`; \
                     ANY/ALL/NONE/SINGLE require a `LIST<T>` source"
                ),
            ));
        }
    }

    /// Emit E2011 if `predicate` is a bare non-boolean literal.
    ///
    /// Richer nested-expression bool checks are already covered by the
    /// inference pass (which emits E2011 on `AND`/`OR`/`NOT` operands).
    /// This helper catches the obvious case `ALL(x IN xs WHERE 1)`.
    #[allow(clippy::unused_self)]
    fn require_bool_predicate(&self, predicate: &Expr, sink: &mut DiagnosticsSink) {
        let reason: Option<&str> = match predicate {
            Expr::Int(_) => Some("Int"),
            Expr::Float(_) => Some("Float"),
            Expr::String(_) => Some("String"),
            // `Null` is treated as Bool by openCypher ternary logic;
            // leave it alone.
            _ => None,
        };

        if let Some(ty) = reason {
            sink.push(Diagnostic::error(
                DiagCode::E2011,
                dummy_span(),
                format!("list-predicate WHERE clause requires a `Bool` predicate but found `{ty}`"),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Expression context — governs which variable kinds are permitted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExprCtx {
    /// Unconstrained (default).
    General,
    /// Arithmetic operand: only `Value`-kinded variables allowed.
    Arithmetic,
    /// Property-access / map-projection target: `Path` is illegal.
    PropAccess,
}

/// Return `true` if `op` requires numeric / arithmetic operands.
fn is_arithmetic_op(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
    )
}

fn kind_name(k: VarKind) -> &'static str {
    match k {
        VarKind::Node => "Node",
        VarKind::Relationship => "Relationship",
        VarKind::Path => "Path",
        VarKind::Value => "Value",
    }
}

/// Placeholder span used when the exact source span is not available from
/// the HIR.  Callers that have a concrete span should pass it directly
/// once the HIR carries per-expression spans.
fn dummy_span() -> HirSpan {
    HirSpan::empty(HirOffset::new(0))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use cypher_diag::DiagnosticsSink;
    use cypher_hir::{
        BinOp, Binding, Clause, Direction, Expr, HirOffset, HirSpan, Pattern, PatternElement,
        PatternPart, Projection, RelLength, Statement, VarId, VarKind,
    };
    use smol_str::SmolStr;

    fn zero_range() -> HirSpan {
        HirSpan::empty(HirOffset::new(0))
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

    fn dummy_syntax() -> cypher_syntax::SyntaxNode {
        cypher_syntax::parse("x").syntax()
    }

    fn alloc(stmt: &mut Statement) -> cypher_hir::HirId {
        stmt.alloc_id(dummy_syntax())
    }

    fn run(stmt: &Statement) -> String {
        let mut sink = DiagnosticsSink::new();
        check_kinds(stmt, &mut sink);
        let diags = sink.into_sorted();
        let mut out = String::new();
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
        }
        out
    }

    // -----------------------------------------------------------------------
    // Test 1: Node variable bound at node pattern position — no error.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_node_kind_in_node_pattern_ok() {
        let mut stmt = Statement::new(zero_range());
        let n = intern_var(&mut stmt, "n", VarKind::Node);
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
                        bind: Some(n),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(n),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_node_in_node_pattern_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 2: Relationship variable bound at rel pattern position — no error.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_rel_kind_in_rel_pattern_ok() {
        let mut stmt = Statement::new(zero_range());
        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let r = intern_var(&mut stmt, "r", VarKind::Relationship);
        let b = intern_var(&mut stmt, "b", VarKind::Node);
        let id = alloc(&mut stmt);
        let nid_a = alloc(&mut stmt);
        let rid = alloc(&mut stmt);
        let nid_b = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: Some(a),
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: Some(r),
                            types: vec![],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: Some(b),
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(r),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_rel_in_rel_pattern_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 3: Value variable bound via UNWIND — no error in RETURN.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_value_kind_in_return_ok() {
        let mut stmt = Statement::new(zero_range());
        let x = intern_var(&mut stmt, "x", VarKind::Value);
        let uid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Unwind {
            id: uid,
            list: Expr::List(vec![]),
            bind: x,
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(x),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_value_in_return_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 4 (mismatch): Node variable used in arithmetic expression → E2007.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_node_in_arithmetic_mismatch() {
        let mut stmt = Statement::new(zero_range());
        let n = intern_var(&mut stmt, "n", VarKind::Node);

        // Simulate: MATCH (n) RETURN n + 1
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
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Var(n)),
                    rhs: Box::new(Expr::Int(1)),
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_node_in_arithmetic_mismatch", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 5 (mismatch): Relationship variable used in arithmetic → E2007.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_rel_in_arithmetic_mismatch() {
        let mut stmt = Statement::new(zero_range());
        let r = intern_var(&mut stmt, "r", VarKind::Relationship);

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
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: Some(r),
                            types: vec![],
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
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::BinOp {
                    op: BinOp::Mul,
                    lhs: Box::new(Expr::Var(r)),
                    rhs: Box::new(Expr::Int(2)),
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_rel_in_arithmetic_mismatch", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 6 (mismatch): Value variable used in node-pattern position → E2008.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_value_in_node_pattern_mismatch() {
        let mut stmt = Statement::new(zero_range());
        // A variable declared as Value but placed into a node-pattern binder.
        let x = intern_var(&mut stmt, "x", VarKind::Value);

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
                        bind: Some(x), // Value var in node position!
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(x),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_value_in_node_pattern_mismatch", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 7 (mismatch): Path variable used in property access → E2007.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_path_in_prop_access_mismatch() {
        let mut stmt = Statement::new(zero_range());
        let p = intern_var(&mut stmt, "p", VarKind::Path);

        // Simulate: RETURN p.length  (property access on a Path)
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
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
        });
        insta::assert_snapshot!("kinds_path_in_prop_access_mismatch", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // Test 8 (mismatch): Node variable in rel-pattern position → E2008.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_node_in_rel_pattern_mismatch() {
        let mut stmt = Statement::new(zero_range());
        // A variable declared as Node but placed into a rel-pattern binder.
        let n = intern_var(&mut stmt, "n", VarKind::Node);

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
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: Some(n), // Node var in rel position!
                            types: vec![],
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
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(n),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("kinds_node_in_rel_pattern_mismatch", run(&stmt));
    }
}
