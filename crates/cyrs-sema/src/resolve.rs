//! Name resolution pass (spec 0001 §6.2).
//!
//! Entry point: [`resolve()`].  Walks the clauses of a
//! `cyrs_hir::Statement` in order, building a `ScopeGraph` and a
//! `ResolvedNames` table, emitting diagnostics into a [`DiagnosticsSink`].
//!
//! ## Scope construction rules (§6.2)
//!
//! 1. Each statement starts with a single `Root` scope.
//! 2. MATCH / OPTIONAL MATCH / CREATE / MERGE introduce a `Match` /
//!    `Create` / `Merge` child scope and bind pattern variables into it.
//! 3. UNWIND introduces an `Unwind` scope and binds its variable.
//! 4. CALL introduces a `Call` scope and binds YIELD columns.
//! 5. WHERE clauses do **not** introduce a new scope — their expressions
//!    are resolved against the current (innermost) scope.
//! 6. WITH introduces a `WithBarrier` scope that records the projected
//!    names/aliases.  After the WITH, a fresh child scope is pushed;
//!    only the projected names are visible in that child and beyond.
//! 7. RETURN introduces a `Return` scope (terminal).
//!
//! ## Unresolved references
//!
//! Every `Expr::Unresolved(name)` that survives lowering is looked up in
//! the current scope chain.  If found, `ResolvedNames` records a
//! `Resolution::Resolved(_)`.  If not found, `Resolution::Unresolved` is
//! recorded and an `E1001` diagnostic is emitted.
//!
//! ## Shadowing
//!
//! Re-binding a name that is already visible in the current scope chain
//! emits `E1002`. The new binding wins.
//!
//! ## WITH barrier detail
//!
//! Projection aliases (`WITH a AS b`) create a fresh `Value` binding for
//! `b` in the scope that follows the barrier; `a` is accessible inside
//! the WITH's own expression but not after.  Plain projections (`WITH a`)
//! re-bind `a` with its original kind.

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::{
    Binding, Clause, Expr, HirOffset, HirSpan, MapProjectionItem, Pattern, PatternElement,
    Projection, Resolution, ResolvedBinding, ResolvedNames, ScopeGraph, ScopeId, ScopeKind,
    Statement, VarId, VarKind,
};
use indexmap::IndexMap;
use smol_str::SmolStr;

/// Result of the name-resolution pass for one statement.
#[derive(Debug)]
pub struct ResolveResult {
    /// The scope forest for the statement.
    pub scope_graph: ScopeGraph,
    /// Use-site → definition table.
    pub resolved_names: ResolvedNames,
}

/// Name-resolution pass.  Builds the `ScopeGraph`, populates
/// `ResolvedNames`, and emits `E1001` / `E1002` diagnostics.
///
/// `warn_shadowing`: when `true`, re-binding an already-visible name
/// emits `E1002`.  Default is `false` (matches `SemaOptions`).
pub fn resolve(
    stmt: &Statement,
    warn_shadowing: bool,
    sink: &mut DiagnosticsSink,
) -> ResolveResult {
    let mut ctx = ResolveCtx {
        graph: ScopeGraph::new(),
        resolved: ResolvedNames::new(),
        bindings: &stmt.bindings,
        warn_shadowing,
    };

    // Root scope — parent of everything. Its span covers the full
    // statement so `scope_at_offset` can fall back to Root when the
    // cursor is between clauses (whitespace / trivia).
    let root = ctx
        .graph
        .add_scope_with_span(ScopeKind::Root, None, stmt.span);
    // `current` is the scope into which new bindings land and against
    // which use-site lookups are performed.
    let mut current = root;

    for clause in &stmt.clauses {
        current = ctx.lower_clause(clause, current, sink);
    }

    ResolveResult {
        scope_graph: ctx.graph,
        resolved_names: ctx.resolved,
    }
}

// ---------------------------------------------------------------------------
// Internal context
// ---------------------------------------------------------------------------

struct ResolveCtx<'a> {
    graph: ScopeGraph,
    resolved: ResolvedNames,
    bindings: &'a IndexMap<VarId, Binding>,
    warn_shadowing: bool,
}

impl ResolveCtx<'_> {
    /// Process one clause and return the scope that should be considered
    /// "current" for subsequent clauses.
    fn lower_clause(
        &mut self,
        clause: &Clause,
        current: ScopeId,
        sink: &mut DiagnosticsSink,
    ) -> ScopeId {
        match clause {
            // ------------------------------------------------------------------
            // MATCH / OPTIONAL MATCH
            // ------------------------------------------------------------------
            Clause::Match {
                optional,
                pattern,
                span,
                ..
            } => {
                let scope = self.graph.add_scope_with_span(
                    ScopeKind::Match {
                        optional: *optional,
                    },
                    Some(current),
                    *span,
                );
                self.bind_pattern(pattern, scope, sink);
                // WHERE inline is emitted as a separate Where clause by the
                // lowerer; handled below.
                scope
            }

            // ------------------------------------------------------------------
            // CREATE
            // ------------------------------------------------------------------
            Clause::Create { pattern, span, .. } => {
                let scope = self
                    .graph
                    .add_scope_with_span(ScopeKind::Create, Some(current), *span);
                self.bind_pattern(pattern, scope, sink);
                scope
            }

            // ------------------------------------------------------------------
            // MERGE
            // ------------------------------------------------------------------
            Clause::Merge { pattern, span, .. } => {
                let scope = self
                    .graph
                    .add_scope_with_span(ScopeKind::Merge, Some(current), *span);
                self.bind_pattern(pattern, scope, sink);
                scope
            }

            // ------------------------------------------------------------------
            // UNWIND
            // ------------------------------------------------------------------
            Clause::Unwind { bind, span, .. } => {
                let scope = self
                    .graph
                    .add_scope_with_span(ScopeKind::Unwind, Some(current), *span);
                let name = self.var_name(*bind);
                if self
                    .graph
                    .bind(scope, name.clone(), *bind, VarKind::Value)
                    .is_some()
                    && self.warn_shadowing
                {
                    sink.push(Diagnostic::warning(
                        DiagCode::E1002,
                        *span,
                        format!("variable `{name}` shadows an earlier binding"),
                    ));
                }
                scope
            }

            // ------------------------------------------------------------------
            // CALL … YIELD …
            // ------------------------------------------------------------------
            Clause::Call { yields, span, .. } => {
                let scope = self
                    .graph
                    .add_scope_with_span(ScopeKind::Call, Some(current), *span);
                for yi in yields {
                    let col_name: SmolStr = yi.alias.clone().unwrap_or_else(|| yi.name.clone());
                    // Find the VarId for this yield column — it must have
                    // been allocated by the lowerer.  Locate by name.
                    if let Some((&var, _)) = self
                        .bindings
                        .iter()
                        .find(|(_, b)| b.name == col_name && b.kind == VarKind::Value)
                        && self
                            .graph
                            .bind(scope, col_name.clone(), var, VarKind::Value)
                            .is_some()
                        && self.warn_shadowing
                    {
                        sink.push(Diagnostic::warning(
                            DiagCode::E1002,
                            *span,
                            format!("variable `{col_name}` shadows an earlier binding"),
                        ));
                    }
                }
                scope
            }

            // ------------------------------------------------------------------
            // WITH — scope barrier
            // ------------------------------------------------------------------
            Clause::With {
                projections,
                filter,
                span,
                ..
            } => {
                // 1. Resolve projection expressions against the *current* scope.
                for proj in projections {
                    self.resolve_expr(&proj.expr, current, sink);
                }
                if let Some(f) = filter {
                    self.resolve_expr(f, current, sink);
                }

                // 2. Build the projected list: (visible_name, source_var).
                let projected: Vec<(SmolStr, VarId)> = projections
                    .iter()
                    .filter_map(|p| {
                        Self::projection_name_var(p, self.bindings, current, &self.graph)
                    })
                    .collect();

                // 3. Introduce the barrier scope.
                let barrier = self.graph.add_scope_with_span(
                    ScopeKind::WithBarrier {
                        projected: projected.clone(),
                    },
                    Some(current),
                    *span,
                );

                // 4. Create a fresh child scope that only sees projected names.
                //    Bind the projected names into it. Its span starts at
                //    the end of the WITH and runs to the end of the
                //    statement (set when the parent `resolve` wraps up).
                let post_span = cyrs_hir::HirSpan::new(span.end(), span.end());
                let post =
                    self.graph
                        .add_scope_with_span(ScopeKind::Root, Some(barrier), post_span);
                for (proj_name, source_var) in &projected {
                    let kind = self
                        .bindings
                        .get(source_var)
                        .map_or(VarKind::Value, |b| b.kind);
                    self.graph.bind(post, proj_name.clone(), *source_var, kind);
                }
                post
            }

            // ------------------------------------------------------------------
            // RETURN
            // ------------------------------------------------------------------
            Clause::Return {
                projections, span, ..
            } => {
                let scope = self
                    .graph
                    .add_scope_with_span(ScopeKind::Return, Some(current), *span);
                for proj in projections {
                    self.resolve_expr(&proj.expr, current, sink);
                }
                scope
            }

            // ------------------------------------------------------------------
            // WHERE — no new scope, just resolve the predicate.
            // ------------------------------------------------------------------
            Clause::Where { predicate, .. } => {
                self.resolve_expr(predicate, current, sink);
                current
            }

            // ------------------------------------------------------------------
            // SET / REMOVE / DELETE — resolve exprs, no new bindings.
            // ------------------------------------------------------------------
            Clause::Set { items, .. } => {
                use cyrs_hir::SetItem;
                for item in items {
                    match item {
                        SetItem::Property { target, value, .. } => {
                            self.resolve_expr(target, current, sink);
                            self.resolve_expr(value, current, sink);
                        }
                        SetItem::AssignMap { map, .. } => {
                            self.resolve_expr(map, current, sink);
                        }
                        SetItem::Labels { .. } => {}
                    }
                }
                current
            }

            Clause::Remove { items, .. } => {
                use cyrs_hir::RemoveItem;
                for item in items {
                    match item {
                        RemoveItem::Property { target, .. } => {
                            self.resolve_expr(target, current, sink);
                        }
                        RemoveItem::Labels { .. } => {}
                    }
                }
                current
            }

            Clause::Delete { targets, .. } => {
                for expr in targets {
                    self.resolve_expr(expr, current, sink);
                }
                current
            }
        }
    }

    // -----------------------------------------------------------------------
    // Pattern binding helpers
    // -----------------------------------------------------------------------

    fn bind_pattern(&mut self, pattern: &Pattern, scope: ScopeId, sink: &mut DiagnosticsSink) {
        for part in &pattern.parts {
            // Path binder.
            if let Some(path_var) = part.named_as {
                let name = self.var_name(path_var);
                if self
                    .graph
                    .bind(scope, name.clone(), path_var, VarKind::Path)
                    .is_some()
                    && self.warn_shadowing
                {
                    // Use a zero-width span — we don't have the part span.
                    let span = HirSpan::empty(HirOffset::new(0));
                    sink.push(Diagnostic::warning(
                        DiagCode::E1002,
                        span,
                        format!("variable `{name}` shadows an earlier binding"),
                    ));
                }
            }

            for elem in &part.elements {
                match elem {
                    PatternElement::Node {
                        bind, props, span, ..
                    } => {
                        if let Some(var) = bind {
                            let name = self.var_name(*var);
                            // Check if already visible (pattern re-use is OK in
                            // Cypher — same variable in multi-hop patterns —
                            // shadowing is a new name overshadowing an outer one).
                            let already_visible = self.graph.resolve_at(scope, &name).is_some();
                            if !already_visible
                                && self
                                    .graph
                                    .bind(scope, name.clone(), *var, VarKind::Node)
                                    .is_some()
                                && self.warn_shadowing
                            {
                                sink.push(Diagnostic::warning(
                                    DiagCode::E1002,
                                    *span,
                                    format!("variable `{name}` shadows an earlier binding"),
                                ));
                            }
                        }
                        if let Some(p) = props {
                            self.resolve_expr(p, scope, sink);
                        }
                    }
                    PatternElement::Rel {
                        bind, props, span, ..
                    } => {
                        if let Some(var) = bind {
                            let name = self.var_name(*var);
                            let already_visible = self.graph.resolve_at(scope, &name).is_some();
                            if !already_visible
                                && self
                                    .graph
                                    .bind(scope, name.clone(), *var, VarKind::Relationship)
                                    .is_some()
                                && self.warn_shadowing
                            {
                                sink.push(Diagnostic::warning(
                                    DiagCode::E1002,
                                    *span,
                                    format!("variable `{name}` shadows an earlier binding"),
                                ));
                            }
                        }
                        if let Some(p) = props {
                            self.resolve_expr(p, scope, sink);
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expression resolution
    // -----------------------------------------------------------------------

    fn resolve_expr(&mut self, expr: &Expr, scope: ScopeId, sink: &mut DiagnosticsSink) {
        match expr {
            Expr::Var(var) => {
                // Lowered var — must be in bindings; record as resolved.
                if let Some(b) = self.bindings.get(var) {
                    self.resolved.insert(
                        *var,
                        Resolution::Resolved(ResolvedBinding {
                            defining_var: *var,
                            kind: b.kind.into(),
                            via_projection: false,
                        }),
                    );
                }
            }
            Expr::Unresolved(name) => {
                // Use a synthetic `VarId` sentinel based on the name hash for
                // tracking; the actual `VarId` space for unresolved refs uses
                // u32::MAX range working downward.
                let diag_var = Self::find_or_make_unresolved_var(name);
                if let Some(def_var) = self.graph.resolve_at(scope, name) {
                    let kind = self
                        .bindings
                        .get(&def_var)
                        .map_or(cyrs_hir::BindingKind::Value, |b| b.kind.into());
                    self.resolved.insert(
                        diag_var,
                        Resolution::Resolved(ResolvedBinding {
                            defining_var: def_var,
                            kind,
                            via_projection: false,
                        }),
                    );
                } else {
                    // Not found — unresolved reference.
                    self.resolved.insert(diag_var, Resolution::Unresolved);
                    // Emit E1001 only for real variable names (not star).
                    if name != "*" {
                        // We don't have the exact span here (the lowerer
                        // discards it for Unresolved). Use a zero-width
                        // span at offset 0; callers can overlay with the
                        // node_map if needed.
                        let span = HirSpan::empty(HirOffset::new(0));
                        sink.push(Diagnostic::error(
                            DiagCode::E1001,
                            span,
                            format!("unresolved variable `{name}`"),
                        ));
                    }
                }
            }
            Expr::Prop { target, .. }
            | Expr::IsNull {
                operand: target, ..
            } => {
                self.resolve_expr(target, scope, sink);
            }
            Expr::Index { target, index } => {
                self.resolve_expr(target, scope, sink);
                self.resolve_expr(index, scope, sink);
            }
            Expr::Slice { target, start, end } => {
                self.resolve_expr(target, scope, sink);
                if let Some(s) = start {
                    self.resolve_expr(s, scope, sink);
                }
                if let Some(e) = end {
                    self.resolve_expr(e, scope, sink);
                }
            }
            Expr::List(items) => {
                for e in items {
                    self.resolve_expr(e, scope, sink);
                }
            }
            Expr::Map(pairs) => {
                for (_, e) in pairs {
                    self.resolve_expr(e, scope, sink);
                }
            }
            Expr::Call { args, .. } => {
                for a in args {
                    self.resolve_expr(a, scope, sink);
                }
            }
            Expr::BinOp { lhs, rhs, .. }
            | Expr::InList {
                operand: lhs,
                list: rhs,
            } => {
                self.resolve_expr(lhs, scope, sink);
                self.resolve_expr(rhs, scope, sink);
            }
            Expr::UnaryOp { operand, .. } => {
                self.resolve_expr(operand, scope, sink);
            }
            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => {
                if let Some(s) = scrutinee {
                    self.resolve_expr(s, scope, sink);
                }
                for (cond, result) in arms {
                    self.resolve_expr(cond, scope, sink);
                    self.resolve_expr(result, scope, sink);
                }
                if let Some(o) = otherwise {
                    self.resolve_expr(o, scope, sink);
                }
            }
            Expr::ListComprehension {
                iterable,
                filter,
                map_expr,
                ..
            } => {
                self.resolve_expr(iterable, scope, sink);
                if let Some(f) = filter {
                    self.resolve_expr(f, scope, sink);
                }
                self.resolve_expr(map_expr, scope, sink);
            }
            // cy-8x5 — list predicates. Traversal mirrors the
            // list-comprehension shape: resolve iterable in the outer
            // scope, then the optional predicate (which may reference
            // the binder `var`). A dedicated child scope for the
            // binder is added when the sema pass starts tracking
            // expression-level scopes — for now the binder is already
            // interned at HIR lowering so any reference resolves to
            // the right `VarId`.
            Expr::ListPredicate {
                iterable,
                predicate,
                ..
            } => {
                self.resolve_expr(iterable, scope, sink);
                if let Some(p) = predicate {
                    self.resolve_expr(p, scope, sink);
                }
            }
            Expr::MapProjection { base, items } => {
                self.resolve_expr(base, scope, sink);
                for item in items {
                    match item {
                        MapProjectionItem::Computed { value, .. }
                        | MapProjectionItem::Aliased { value, .. } => {
                            self.resolve_expr(value, scope, sink);
                        }
                        MapProjectionItem::PropCopy { .. }
                        | MapProjectionItem::VarShorthand { .. } => {}
                    }
                }
            }
            // Literals, params, and pattern predicates (deferred in v1)
            // carry no variable references.
            Expr::PatternPredicate(_)
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::String(_)
            | Expr::Param(_)
            // cy-p1u5: ExistsSubqueryDeferred is opaque to name
            // resolution. The body is not lowered, so no variable
            // references inside it need to be (or can be) resolved
            // here. The dialect-gate pass emits E4017.
            | Expr::ExistsSubqueryDeferred { .. } => {}
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn var_name(&self, var: VarId) -> SmolStr {
        self.bindings.get(&var).map_or_else(
            || SmolStr::new(format!("__var{}", var.0)),
            |b| b.name.clone(),
        )
    }

    /// Derive a `VarId` key for an unresolved name so we can record it in
    /// `ResolvedNames`. We use a high-range id that won't collide with
    /// real lowered vars (which start at 0). Real `VarId`s are dense from 0;
    /// we park unresolved keys at `u32::MAX - hash(name)`.
    fn find_or_make_unresolved_var(name: &str) -> VarId {
        // Simple deterministic hash: FNV-1a-lite.
        let mut h: u32 = 2_166_136_261;
        for b in name.bytes() {
            h = h.wrapping_mul(16_777_619) ^ u32::from(b);
        }
        // Keep in the upper half of u32 space to avoid collisions.
        VarId(u32::MAX - (h & 0x3FFF_FFFF))
    }

    /// Extract `(visible_name, source_var)` from a Projection.
    fn projection_name_var(
        proj: &Projection,
        bindings: &IndexMap<VarId, Binding>,
        current: ScopeId,
        graph: &ScopeGraph,
    ) -> Option<(SmolStr, VarId)> {
        // Alias takes priority as the visible name.
        if let Some(alias) = &proj.alias {
            // The source var: whatever the expr resolves to.
            let source_var = expr_direct_var(&proj.expr)?;
            return Some((alias.clone(), source_var));
        }
        // No alias — plain `WITH x`.
        match &proj.expr {
            Expr::Var(v) => {
                let name = bindings.get(v).map(|b| b.name.clone())?;
                Some((name, *v))
            }
            Expr::Unresolved(name) => {
                // Look up in current scope.
                let v = graph.resolve_at(current, name)?;
                Some((name.clone(), v))
            }
            _ => None,
        }
    }
}

/// Extract the `VarId` from a direct [`Expr::Var`].
fn expr_direct_var(expr: &Expr) -> Option<VarId> {
    if let Expr::Var(v) = expr {
        Some(*v)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use cyrs_diag::DiagnosticsSink;
    use cyrs_hir::{
        Binding, Clause, HirOffset, HirSpan, Pattern, PatternElement, PatternPart, Projection,
        Statement, VarId, VarKind,
    };
    use smol_str::SmolStr;

    // `cyrs_hir::lower::lower_statement` is fallible since cy-cfi; these
    // resolve tests feed it source that may not parse cleanly, so use the
    // infallible `lower_parse` primitive.
    fn lower_statement(src: &str) -> Statement {
        cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(src)).expect("lower_parse is infallible")
    }

    // -----------------------------------------------------------------------
    // Render helpers
    // -----------------------------------------------------------------------

    fn render_scope_kind(k: &ScopeKind) -> String {
        match k {
            ScopeKind::Root => "Root".to_string(),
            ScopeKind::Match { optional } => {
                if *optional {
                    "OptionalMatch".into()
                } else {
                    "Match".into()
                }
            }
            ScopeKind::Create => "Create".into(),
            ScopeKind::Merge => "Merge".into(),
            ScopeKind::Unwind => "Unwind".into(),
            ScopeKind::Call => "Call".into(),
            ScopeKind::WithBarrier { projected } => {
                let names: Vec<_> = projected.iter().map(|(n, _)| n.as_str()).collect();
                format!("WithBarrier[{}]", names.join(","))
            }
            ScopeKind::Return => "Return".into(),
        }
    }

    fn render_result(stmt: &Statement, warn: bool) -> String {
        let mut sink = DiagnosticsSink::new();
        let result = resolve(stmt, warn, &mut sink);
        let diags = sink.into_sorted();

        let mut out = String::new();

        writeln!(out, "scopes: {}", result.scope_graph.len()).unwrap();
        for node in result.scope_graph.iter() {
            let kind = render_scope_kind(&node.kind);
            let mut bindings: Vec<String> = node.bindings.keys().map(ToString::to_string).collect();
            bindings.sort();
            writeln!(
                out,
                "  scope[{}] kind={kind} parent={:?} bindings={bindings:?}",
                node.id.0,
                node.parent.map(|p| p.0)
            )
            .unwrap();
        }
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
        }
        out
    }

    fn render_src(src: &str) -> String {
        let stmt = lower_statement(src);
        render_result(&stmt, false)
    }

    // -----------------------------------------------------------------------
    // Statement builder (borrow-safe: returns values, does not hold &mut stmt)
    // -----------------------------------------------------------------------

    fn zero_range() -> HirSpan {
        HirSpan::empty(HirOffset::new(0))
    }

    /// Add a variable to the statement bindings, returning its `VarId`.
    fn intern_var(stmt: &mut Statement, name: &str, kind: VarKind) -> VarId {
        for (id, b) in &stmt.bindings {
            if b.name.as_str() == name {
                return *id;
            }
        }
        let id = VarId(u32::try_from(stmt.bindings.len()).expect("var count overflow"));
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

    fn dummy_syntax() -> cyrs_syntax::SyntaxNode {
        cyrs_syntax::parse("x").syntax()
    }

    fn alloc(stmt: &mut Statement) -> cyrs_hir::HirId {
        stmt.alloc_id(dummy_syntax())
    }

    fn node_pattern(bind_var: VarId, hir_id: cyrs_hir::HirId) -> Pattern {
        Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![PatternElement::Node {
                    id: hir_id,
                    bind: Some(bind_var),
                    labels: vec![],
                    props: None,
                    span: zero_range(),
                }],
            }],
        }
    }

    // -----------------------------------------------------------------------
    // Test 1: simple MATCH → RETURN (parser-based)
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_match_return() {
        insta::assert_snapshot!("scope_match_return", render_src("MATCH (n) RETURN n"));
    }

    // -----------------------------------------------------------------------
    // Test 2: MATCH + WHERE (no extra scope)
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_match_where_return() {
        insta::assert_snapshot!(
            "scope_match_where_return",
            render_src("MATCH (n) WHERE n.age > 18 RETURN n")
        );
    }

    // -----------------------------------------------------------------------
    // Test 3: OPTIONAL MATCH
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_optional_match() {
        insta::assert_snapshot!(
            "scope_optional_match",
            render_src("OPTIONAL MATCH (a:Person) RETURN a")
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: two sequential MATCHes
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_two_matches() {
        insta::assert_snapshot!(
            "scope_two_matches",
            render_src("MATCH (a) MATCH (b) RETURN a")
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: relationship pattern
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_rel_pattern() {
        insta::assert_snapshot!(
            "scope_rel_pattern",
            render_src("MATCH (a)-[r:KNOWS]->(b) RETURN a, r, b")
        );
    }

    // -----------------------------------------------------------------------
    // Test 6: unresolved variable → E1001
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_unresolved_ref() {
        insta::assert_snapshot!("scope_unresolved_ref", render_src("MATCH (n) RETURN z"));
    }

    // -----------------------------------------------------------------------
    // Test 7: property access on bound var (no errors)
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_prop_access_resolved() {
        insta::assert_snapshot!(
            "scope_prop_access_resolved",
            render_src("MATCH (n) RETURN n.age")
        );
    }

    // -----------------------------------------------------------------------
    // Test 8: function call with unresolved arg → E1001
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_func_unresolved_arg() {
        insta::assert_snapshot!(
            "scope_func_unresolved_arg",
            render_src("RETURN count(missing)")
        );
    }

    // -----------------------------------------------------------------------
    // Test 9: MATCH + WHERE + resolved var in predicate
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_match_where_resolved() {
        insta::assert_snapshot!(
            "scope_match_where_resolved",
            render_src("MATCH (n) WHERE n.name = 'Alice' RETURN n")
        );
    }

    // -----------------------------------------------------------------------
    // Test 10: RETURN is terminal — produces Return scope
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_return_terminal() {
        insta::assert_snapshot!(
            "scope_return_terminal",
            render_src("MATCH (n:Person) RETURN n.name")
        );
    }

    // -----------------------------------------------------------------------
    // Test 11: WITH barrier — only projected var visible after
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_with_barrier_basic() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let b = intern_var(&mut stmt, "b", VarKind::Node);

        for v in [a, b] {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            let pat = node_pattern(v, nid);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: pat,
                span: zero_range(),
            });
        }
        // WITH a   (b not projected)
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

        insta::assert_snapshot!("scope_with_barrier_basic", render_result(&stmt, false));
    }

    // -----------------------------------------------------------------------
    // Test 12: WITH barrier — dropped var → E1001 in RETURN
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_with_barrier_dropped_var() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let b = intern_var(&mut stmt, "b", VarKind::Node);

        for v in [a, b] {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            let pat = node_pattern(v, nid);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: pat,
                span: zero_range(),
            });
        }
        // WITH a — drops b
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
        // RETURN b — b not visible → E1001
        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::Return {
                id,
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
        }

        insta::assert_snapshot!(
            "scope_with_barrier_dropped_var",
            render_result(&stmt, false)
        );
    }

    // -----------------------------------------------------------------------
    // Test 13: WITH alias — `WITH a AS b` — only `b` visible after
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_with_alias() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let b = intern_var(&mut stmt, "b", VarKind::Value);

        // MATCH (a)
        {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            let pat = node_pattern(a, nid);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: pat,
                span: zero_range(),
            });
        }
        // WITH a AS b
        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::With {
                id,
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
        }
        // RETURN b
        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::Return {
                id,
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
        }

        insta::assert_snapshot!("scope_with_alias", render_result(&stmt, false));
    }

    // -----------------------------------------------------------------------
    // Test 14: UNWIND binds a Value variable
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_unwind() {
        let mut stmt = Statement::new(zero_range());

        let x = intern_var(&mut stmt, "x", VarKind::Value);

        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::Unwind {
                id,
                list: Expr::List(vec![]),
                bind: x,
                span: zero_range(),
            });
        }
        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::Return {
                id,
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
        }

        insta::assert_snapshot!("scope_unwind", render_result(&stmt, false));
    }

    // -----------------------------------------------------------------------
    // Test 15: double WITH barrier — a survives both; b dropped after first
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_double_with_barrier() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let b = intern_var(&mut stmt, "b", VarKind::Node);
        let c = intern_var(&mut stmt, "c", VarKind::Node);

        // MATCH (a), MATCH (b)
        for v in [a, b] {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: node_pattern(v, nid),
                span: zero_range(),
            });
        }
        // WITH a   (b dropped)
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
        // MATCH (c)
        {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: node_pattern(c, nid),
                span: zero_range(),
            });
        }
        // WITH a, c
        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::With {
                id,
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
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::Return {
                id,
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

        insta::assert_snapshot!("scope_double_with_barrier", render_result(&stmt, false));
    }

    // -----------------------------------------------------------------------
    // Test 16: shadowing warning — rebind `a` after WITH, warn=true
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_shadowing_warn() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);

        // MATCH (a)
        {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: node_pattern(a, nid),
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
        // MATCH (a) again — rebind in post-WITH scope → shadowing
        {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: node_pattern(a, nid),
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

        insta::assert_snapshot!("scope_shadowing_warn", render_result(&stmt, true));
    }

    // -----------------------------------------------------------------------
    // Test 17: CREATE introduces a Create scope and binds vars
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_create_binds() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);

        {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            stmt.clauses.push(Clause::Create {
                id,
                pattern: Pattern {
                    parts: vec![PatternPart {
                        named_as: None,
                        elements: vec![PatternElement::Node {
                            id: nid,
                            bind: Some(a),
                            labels: vec![SmolStr::new("Person")],
                            props: None,
                            span: zero_range(),
                        }],
                    }],
                },
                span: zero_range(),
            });
        }
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

        insta::assert_snapshot!("scope_create_binds", render_result(&stmt, false));
    }

    // -----------------------------------------------------------------------
    // Test 18: WITH barrier — b is invisible after barrier
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_with_barrier_invisible() {
        let mut stmt = Statement::new(zero_range());

        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let b = intern_var(&mut stmt, "b", VarKind::Node);

        for v in [a, b] {
            let id = alloc(&mut stmt);
            let nid = alloc(&mut stmt);
            stmt.clauses.push(Clause::Match {
                id,
                optional: false,
                pattern: node_pattern(v, nid),
                span: zero_range(),
            });
        }
        // WITH a only
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
        // RETURN a, b — b should be unresolved → E1001
        {
            let id = alloc(&mut stmt);
            stmt.clauses.push(Clause::Return {
                id,
                projections: vec![
                    Projection {
                        expr: Expr::Var(a),
                        alias: None,
                        span: zero_range(),
                    },
                    Projection {
                        expr: Expr::Unresolved(SmolStr::new("b")),
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

        insta::assert_snapshot!("scope_with_barrier_invisible", render_result(&stmt, false));
    }

    // -----------------------------------------------------------------------
    // Test 19: pattern re-use — `a` appears as both node in two positions
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_pattern_reuse() {
        insta::assert_snapshot!(
            "scope_pattern_reuse",
            render_src("MATCH (a)-[r]->(b) RETURN a, r, b")
        );
    }

    // -----------------------------------------------------------------------
    // Test 20: empty RETURN (no projections)
    // -----------------------------------------------------------------------
    #[test]
    fn snap_scope_empty_return() {
        let mut stmt = Statement::new(zero_range());
        let id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id,
            projections: vec![],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        insta::assert_snapshot!("scope_empty_return", render_result(&stmt, false));
    }
}
