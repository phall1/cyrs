//! Opt-in [`Visitor`] trait for traversing a lowered plan IR (spec 0001 §12).
//!
//! This module gives embedders a recursive, default-traversing visitor over
//! [`ReadOp`] and [`WriteOp`], modelled on `syn`'s `visit::Visit`. The point is
//! to reduce maintenance burden as the `#[non_exhaustive]` operator enums
//! grow: an embedder implements [`Visitor`], overrides only the `visit_*`
//! methods for the variants it cares about, and still gets a full traversal of
//! the rest of the plan for free.
//!
//! # Shape
//!
//! Every method has a default body, so an empty `impl Visitor for MyType {}`
//! is a complete (no-op-but-traversing) visitor. The defaults delegate to free
//! `walk_*` functions which perform the recursion. This is the same split
//! `syn` uses: a `visit_*` method is the *override point*, and the matching
//! `walk_*` free function is the *recursion*. Overriding `visit_filter`
//! without calling [`walk_filter`] prunes the traversal below that node;
//! calling it continues the descent.
//!
//! Because the plan stores read operators in a flat arena
//! ([`PlanStatement::ops`]) and children are referenced by [`OpId`], every
//! method threads the [`PlanStatement`] through so the walker can resolve
//! child operators. The two embedded sub-trees in the IR — the boxed
//! `pattern` of [`ReadOp::OptionalJoin`] and of [`Expr::Exists`] — are walked
//! directly via [`walk_read_op_node`].
//!
//! # Forward compatibility
//!
//! [`ReadOp`] and [`WriteOp`] are `#[non_exhaustive]`. Code *inside* this
//! crate still sees every variant in a `match`, so [`walk_read_op`] /
//! [`walk_write_op`] dispatch exhaustively today. But a downstream embedder
//! that pinned an older `cyrs-plan` and then upgraded may meet a variant its
//! `Visitor` impl predates. To give that case a defined path, dispatch routes
//! any variant **not** otherwise recognised to [`Visitor::visit_unknown_read_op`]
//! / [`Visitor::visit_unknown_write_op`]. These hooks are the documented
//! extension point: override them to log, error, or conservatively bail when a
//! new operator kind appears. With the current enum definitions they are never
//! reached; they exist so that adding a variant in a future `cyrs-plan`
//! release degrades gracefully instead of silently skipping work.
//!
//! # Example
//!
//! An embedder that counts every `Source` scan and collects the labels of
//! every `CreateNode`:
//!
//! ```
//! use cyrs_plan::visit::{self, Visitor};
//! use cyrs_plan::lower::PlanStatement;
//! use cyrs_plan::{ReadOp, WriteOp, LabelSet, VarId};
//! use smol_str::SmolStr;
//!
//! #[derive(Default)]
//! struct Stats {
//!     sources: usize,
//!     created_labels: Vec<SmolStr>,
//! }
//!
//! impl Visitor for Stats {
//!     fn visit_source(&mut self, plan: &PlanStatement, label: &Option<LabelSet>, bind: VarId) {
//!         self.sources += 1;
//!         // `Source` is a leaf, so there is nothing more to walk; but
//!         // calling the default walker keeps the override future-proof.
//!         visit::walk_source(self, plan, label, bind);
//!     }
//!
//!     fn visit_create_node(
//!         &mut self,
//!         plan: &PlanStatement,
//!         op: &WriteOp,
//!     ) {
//!         if let WriteOp::CreateNode { labels, .. } = op {
//!             self.created_labels.extend(labels.iter().cloned());
//!         }
//!         visit::walk_create_node(self, plan, op);
//!     }
//! }
//!
//! fn count(plan: &PlanStatement) -> Stats {
//!     let mut stats = Stats::default();
//!     stats.visit_plan(plan);
//!     stats
//! }
//! ```

use crate::lower::PlanStatement;
use crate::{
    AggExpr, Expr, LabelSet, OpId, OrderKey, Projection, ReadOp, RelSpec, UnionKind, VarId, WriteOp,
};

/// Recursive visitor over a lowered plan ([`PlanStatement`]).
///
/// Implement this trait and override the `visit_*` methods for the operator
/// variants of interest. Every method has a default body that performs a full
/// recursive traversal, so an empty impl walks the whole plan doing nothing.
///
/// See the [module documentation](self) for the design rationale, the
/// `visit_*` / `walk_*` split, and the forward-compatibility hooks.
#[allow(unused_variables)]
pub trait Visitor: Sized {
    // ── Entry points ──────────────────────────────────────────────────────────

    /// Visit an entire plan: the read-operator tree (rooted at the last entry
    /// of [`PlanStatement::ops`]) followed by every [`WriteOp`] in order.
    ///
    /// This is the normal way to start a traversal. Override it only to take
    /// control of the top-level ordering; otherwise rely on the default, which
    /// delegates to [`walk_plan`].
    fn visit_plan(&mut self, plan: &PlanStatement) {
        walk_plan(self, plan);
    }

    /// Visit the read operator stored at `id` in the arena.
    ///
    /// Resolves the [`OpId`] against [`PlanStatement::ops`] and dispatches to
    /// the variant-specific `visit_*` method via [`walk_read_op`].
    fn visit_read_op(&mut self, plan: &PlanStatement, id: OpId) {
        walk_read_op(self, plan, id);
    }

    /// Visit a read operator given by reference rather than by [`OpId`].
    ///
    /// Used for the embedded boxed sub-trees in the IR ([`ReadOp::OptionalJoin`]
    /// and [`Expr::Exists`]), which are not arena entries. Dispatches to the
    /// variant-specific `visit_*` method via [`walk_read_op_node`].
    fn visit_read_op_node(&mut self, plan: &PlanStatement, op: &ReadOp) {
        walk_read_op_node(self, plan, op);
    }

    /// Visit a single [`WriteOp`]. Dispatches via [`walk_write_op`].
    fn visit_write_op(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_write_op(self, plan, op);
    }

    // ── ReadOp variants ───────────────────────────────────────────────────────

    /// Visit a [`ReadOp::Source`] — an all-node or label scan (leaf).
    fn visit_source(&mut self, plan: &PlanStatement, label: &Option<LabelSet>, bind: VarId) {
        walk_source(self, plan, label, bind);
    }

    /// Visit a [`ReadOp::Expand`] — a relationship traversal.
    fn visit_expand(&mut self, plan: &PlanStatement, op: &ReadOp) {
        walk_expand(self, plan, op);
    }

    /// Visit a [`ReadOp::Filter`] — a predicate filter.
    fn visit_filter(&mut self, plan: &PlanStatement, input: OpId, predicate: &Expr) {
        walk_filter(self, plan, input, predicate);
    }

    /// Visit a [`ReadOp::Project`] — a column projection.
    fn visit_project(&mut self, plan: &PlanStatement, input: OpId, items: &[Projection]) {
        walk_project(self, plan, input, items);
    }

    /// Visit a [`ReadOp::Aggregate`] — a grouping / aggregation.
    fn visit_aggregate(
        &mut self,
        plan: &PlanStatement,
        input: OpId,
        keys: &[Expr],
        aggs: &[AggExpr],
    ) {
        walk_aggregate(self, plan, input, keys, aggs);
    }

    /// Visit a [`ReadOp::OrderBy`] — a sort.
    fn visit_order_by(&mut self, plan: &PlanStatement, input: OpId, keys: &[OrderKey]) {
        walk_order_by(self, plan, input, keys);
    }

    /// Visit a [`ReadOp::Skip`] — a row skip.
    fn visit_skip(&mut self, plan: &PlanStatement, input: OpId, count: &Expr) {
        walk_skip(self, plan, input, count);
    }

    /// Visit a [`ReadOp::Limit`] — a row limit.
    fn visit_limit(&mut self, plan: &PlanStatement, input: OpId, count: &Expr) {
        walk_limit(self, plan, input, count);
    }

    /// Visit a [`ReadOp::Distinct`] — duplicate-row removal.
    fn visit_distinct(&mut self, plan: &PlanStatement, input: OpId) {
        walk_distinct(self, plan, input);
    }

    /// Visit a [`ReadOp::Unwind`] — list flattening.
    fn visit_unwind(&mut self, plan: &PlanStatement, input: OpId, list: &Expr, bind: VarId) {
        walk_unwind(self, plan, input, list, bind);
    }

    /// Visit a [`ReadOp::Union`] — concatenation of two sub-plans.
    fn visit_union(&mut self, plan: &PlanStatement, left: OpId, right: OpId, kind: UnionKind) {
        walk_union(self, plan, left, right, kind);
    }

    /// Visit a [`ReadOp::With`] — a scope-resetting projection with an
    /// optional `WHERE` filter.
    fn visit_with(
        &mut self,
        plan: &PlanStatement,
        input: OpId,
        items: &[Projection],
        filter: &Option<Expr>,
    ) {
        walk_with(self, plan, input, items, filter);
    }

    /// Visit a [`ReadOp::OptionalJoin`] — a left-outer join over an embedded
    /// sub-tree.
    fn visit_optional_join(&mut self, plan: &PlanStatement, input: OpId, pattern: &ReadOp) {
        walk_optional_join(self, plan, input, pattern);
    }

    /// Visit a [`ReadOp::ShortestPath`] — a shortest-path search.
    fn visit_shortest_path(&mut self, plan: &PlanStatement, op: &ReadOp) {
        walk_shortest_path(self, plan, op);
    }

    /// Fallback for a [`ReadOp`] variant this `Visitor` does not recognise.
    ///
    /// With the current `cyrs-plan` enum definitions this is unreachable —
    /// [`walk_read_op_node`] matches every variant. It exists as the
    /// documented extension point for forward compatibility: should a future
    /// `cyrs-plan` release add a `#[non_exhaustive]` [`ReadOp`] variant,
    /// dispatch routes it here. The default body does nothing (and walks
    /// nothing, since the variant's children are unknown to this code).
    /// Override it to log, error, or otherwise handle the new variant.
    fn visit_unknown_read_op(&mut self, plan: &PlanStatement, op: &ReadOp) {}

    // ── WriteOp variants ──────────────────────────────────────────────────────

    /// Visit a [`WriteOp::CreateNode`].
    fn visit_create_node(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_create_node(self, plan, op);
    }

    /// Visit a [`WriteOp::CreateRel`].
    fn visit_create_rel(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_create_rel(self, plan, op);
    }

    /// Visit a [`WriteOp::MergeNode`]. The default walker descends into the
    /// `on_create` and `on_match` nested [`WriteOp`] lists.
    fn visit_merge_node(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_merge_node(self, plan, op);
    }

    /// Visit a [`WriteOp::MergeRel`]. The default walker descends into the
    /// `on_create` and `on_match` nested [`WriteOp`] lists.
    fn visit_merge_rel(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_merge_rel(self, plan, op);
    }

    /// Visit a [`WriteOp::SetProperty`].
    fn visit_set_property(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_set_property(self, plan, op);
    }

    /// Visit a [`WriteOp::SetLabels`].
    fn visit_set_labels(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_set_labels(self, plan, op);
    }

    /// Visit a [`WriteOp::RemoveProperty`].
    fn visit_remove_property(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_remove_property(self, plan, op);
    }

    /// Visit a [`WriteOp::RemoveLabels`].
    fn visit_remove_labels(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_remove_labels(self, plan, op);
    }

    /// Visit a [`WriteOp::Delete`].
    fn visit_delete(&mut self, plan: &PlanStatement, op: &WriteOp) {
        walk_delete(self, plan, op);
    }

    /// Fallback for a [`WriteOp`] variant this `Visitor` does not recognise.
    ///
    /// The write-side analogue of [`Visitor::visit_unknown_read_op`]; see that
    /// method for the forward-compatibility contract. Unreachable with the
    /// current enum definitions.
    fn visit_unknown_write_op(&mut self, plan: &PlanStatement, op: &WriteOp) {}

    // ── Expression / nested-tree hooks ────────────────────────────────────────

    /// Visit an [`Expr`] reachable from an operator.
    ///
    /// The default walker descends through every sub-expression and, on
    /// reaching an [`Expr::Exists`], walks its embedded read sub-tree via
    /// [`Visitor::visit_read_op_node`]. Override this to inspect expression
    /// IR; most operator-counting visitors leave it as the default (or as a
    /// no-op, by overriding without calling [`walk_expr`], which prunes the
    /// descent into expressions).
    fn visit_expr(&mut self, plan: &PlanStatement, expr: &Expr) {
        walk_expr(self, plan, expr);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// walk_* — recursion. The default body of each visit_* method calls into here.
// ──────────────────────────────────────────────────────────────────────────────

/// Walk an entire plan: the read tree rooted at the last arena entry, then
/// every write operator in order. Default body of [`Visitor::visit_plan`].
pub fn walk_plan<V: Visitor>(v: &mut V, plan: &PlanStatement) {
    if let Some(root) = plan.ops.len().checked_sub(1) {
        #[allow(clippy::cast_possible_truncation)]
        v.visit_read_op(plan, OpId(root as u32));
    }
    for op in &plan.write_ops {
        v.visit_write_op(plan, op);
    }
}

/// Resolve `id` against [`PlanStatement::ops`] and dispatch to the matching
/// `visit_*` method. Default body of [`Visitor::visit_read_op`].
///
/// If `id` is out of bounds the call is a no-op — the plan is malformed and a
/// visitor should not panic on it.
pub fn walk_read_op<V: Visitor>(v: &mut V, plan: &PlanStatement, id: OpId) {
    if let Some(op) = plan.ops.get(id.0 as usize) {
        v.visit_read_op_node(plan, op);
    }
}

/// Dispatch a [`ReadOp`] reference to the matching `visit_*` method. Default
/// body of [`Visitor::visit_read_op_node`].
///
/// The `match` is exhaustive over the variants known to this crate; any future
/// `#[non_exhaustive]` variant falls through to
/// [`Visitor::visit_unknown_read_op`].
pub fn walk_read_op_node<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &ReadOp) {
    // `ReadOp` is `#[non_exhaustive]`. The wildcard arm below is unreachable
    // *today* (this crate sees every variant), but is retained deliberately:
    // when a future release adds a variant, the dispatch keeps compiling and
    // routes it to `visit_unknown_read_op` instead of failing to build.
    #[allow(unreachable_patterns)]
    match op {
        ReadOp::Source { label, bind } => v.visit_source(plan, label, *bind),
        ReadOp::Expand { .. } => v.visit_expand(plan, op),
        ReadOp::Filter { input, predicate } => v.visit_filter(plan, *input, predicate),
        ReadOp::Project { input, items } => v.visit_project(plan, *input, items),
        ReadOp::Aggregate { input, keys, aggs } => v.visit_aggregate(plan, *input, keys, aggs),
        ReadOp::OrderBy { input, keys } => v.visit_order_by(plan, *input, keys),
        ReadOp::Skip { input, count } => v.visit_skip(plan, *input, count),
        ReadOp::Limit { input, count } => v.visit_limit(plan, *input, count),
        ReadOp::Distinct { input } => v.visit_distinct(plan, *input),
        ReadOp::Unwind { input, list, bind } => v.visit_unwind(plan, *input, list, *bind),
        ReadOp::Union { left, right, kind } => v.visit_union(plan, *left, *right, *kind),
        ReadOp::With {
            input,
            items,
            filter,
        } => v.visit_with(plan, *input, items, filter),
        ReadOp::OptionalJoin { input, pattern } => {
            v.visit_optional_join(plan, *input, pattern);
        }
        ReadOp::ShortestPath { .. } => v.visit_shortest_path(plan, op),
        // Forward-compat: a future `#[non_exhaustive]` variant lands here.
        _ => v.visit_unknown_read_op(plan, op),
    }
}

/// Dispatch a [`WriteOp`] to the matching `visit_*` method. Default body of
/// [`Visitor::visit_write_op`].
///
/// The `match` is exhaustive over the variants known to this crate; any future
/// `#[non_exhaustive]` variant falls through to
/// [`Visitor::visit_unknown_write_op`].
pub fn walk_write_op<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    // See the note in `walk_read_op_node`: the wildcard arm is unreachable
    // today but kept for forward compatibility with `#[non_exhaustive]`.
    #[allow(unreachable_patterns)]
    match op {
        WriteOp::CreateNode { .. } => v.visit_create_node(plan, op),
        WriteOp::CreateRel { .. } => v.visit_create_rel(plan, op),
        WriteOp::MergeNode { .. } => v.visit_merge_node(plan, op),
        WriteOp::MergeRel { .. } => v.visit_merge_rel(plan, op),
        WriteOp::SetProperty { .. } => v.visit_set_property(plan, op),
        WriteOp::SetLabels { .. } => v.visit_set_labels(plan, op),
        WriteOp::RemoveProperty { .. } => v.visit_remove_property(plan, op),
        WriteOp::RemoveLabels { .. } => v.visit_remove_labels(plan, op),
        WriteOp::Delete { .. } => v.visit_delete(plan, op),
        // Forward-compat: a future `#[non_exhaustive]` variant lands here.
        _ => v.visit_unknown_write_op(plan, op),
    }
}

// ── ReadOp walkers ────────────────────────────────────────────────────────────

/// Walk a [`ReadOp::Source`]. A leaf operator: nothing to recurse into.
/// Default body of [`Visitor::visit_source`].
pub fn walk_source<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    label: &Option<LabelSet>,
    bind: VarId,
) {
    let _ = (v, plan, label, bind);
}

/// Walk a [`ReadOp::Expand`]: descend into `input` and the relationship /
/// node property predicates. Default body of [`Visitor::visit_expand`].
pub fn walk_expand<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &ReadOp) {
    if let ReadOp::Expand { input, rel, to, .. } = op {
        v.visit_read_op(plan, *input);
        walk_rel_spec(v, plan, rel);
        if let Some(props) = &to.properties {
            v.visit_expr(plan, props);
        }
    }
}

/// Walk a [`ReadOp::Filter`]: descend into `input` and the predicate
/// expression. Default body of [`Visitor::visit_filter`].
pub fn walk_filter<V: Visitor>(v: &mut V, plan: &PlanStatement, input: OpId, predicate: &Expr) {
    v.visit_read_op(plan, input);
    v.visit_expr(plan, predicate);
}

/// Walk a [`ReadOp::Project`]: descend into `input` and every projection
/// expression. Default body of [`Visitor::visit_project`].
pub fn walk_project<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    input: OpId,
    items: &[Projection],
) {
    v.visit_read_op(plan, input);
    for item in items {
        v.visit_expr(plan, &item.expr);
    }
}

/// Walk a [`ReadOp::Aggregate`]: descend into `input`, the grouping keys, and
/// every aggregate argument. Default body of [`Visitor::visit_aggregate`].
pub fn walk_aggregate<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    input: OpId,
    keys: &[Expr],
    aggs: &[AggExpr],
) {
    v.visit_read_op(plan, input);
    for key in keys {
        v.visit_expr(plan, key);
    }
    for agg in aggs {
        for arg in &agg.args {
            v.visit_expr(plan, arg);
        }
    }
}

/// Walk a [`ReadOp::OrderBy`]: descend into `input` and every sort key
/// expression. Default body of [`Visitor::visit_order_by`].
pub fn walk_order_by<V: Visitor>(v: &mut V, plan: &PlanStatement, input: OpId, keys: &[OrderKey]) {
    v.visit_read_op(plan, input);
    for key in keys {
        v.visit_expr(plan, &key.expr);
    }
}

/// Walk a [`ReadOp::Skip`]: descend into `input` and the count expression.
/// Default body of [`Visitor::visit_skip`].
pub fn walk_skip<V: Visitor>(v: &mut V, plan: &PlanStatement, input: OpId, count: &Expr) {
    v.visit_read_op(plan, input);
    v.visit_expr(plan, count);
}

/// Walk a [`ReadOp::Limit`]: descend into `input` and the count expression.
/// Default body of [`Visitor::visit_limit`].
pub fn walk_limit<V: Visitor>(v: &mut V, plan: &PlanStatement, input: OpId, count: &Expr) {
    v.visit_read_op(plan, input);
    v.visit_expr(plan, count);
}

/// Walk a [`ReadOp::Distinct`]: descend into `input`. Default body of
/// [`Visitor::visit_distinct`].
pub fn walk_distinct<V: Visitor>(v: &mut V, plan: &PlanStatement, input: OpId) {
    v.visit_read_op(plan, input);
}

/// Walk a [`ReadOp::Unwind`]: descend into `input` and the list expression.
/// Default body of [`Visitor::visit_unwind`].
pub fn walk_unwind<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    input: OpId,
    list: &Expr,
    bind: VarId,
) {
    let _ = bind;
    v.visit_read_op(plan, input);
    v.visit_expr(plan, list);
}

/// Walk a [`ReadOp::Union`]: descend into both `left` and `right` sub-plans.
/// Default body of [`Visitor::visit_union`].
pub fn walk_union<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    left: OpId,
    right: OpId,
    kind: UnionKind,
) {
    let _ = kind;
    v.visit_read_op(plan, left);
    v.visit_read_op(plan, right);
}

/// Walk a [`ReadOp::With`]: descend into `input`, every projection
/// expression, and the optional filter. Default body of [`Visitor::visit_with`].
pub fn walk_with<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    input: OpId,
    items: &[Projection],
    filter: &Option<Expr>,
) {
    v.visit_read_op(plan, input);
    for item in items {
        v.visit_expr(plan, &item.expr);
    }
    if let Some(f) = filter {
        v.visit_expr(plan, f);
    }
}

/// Walk a [`ReadOp::OptionalJoin`]: descend into `input` and the embedded
/// boxed `pattern` sub-tree. Default body of [`Visitor::visit_optional_join`].
pub fn walk_optional_join<V: Visitor>(
    v: &mut V,
    plan: &PlanStatement,
    input: OpId,
    pattern: &ReadOp,
) {
    v.visit_read_op(plan, input);
    v.visit_read_op_node(plan, pattern);
}

/// Walk a [`ReadOp::ShortestPath`]: descend into `input` and the relationship
/// property predicate. Default body of [`Visitor::visit_shortest_path`].
pub fn walk_shortest_path<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &ReadOp) {
    if let ReadOp::ShortestPath { input, rel, .. } = op {
        v.visit_read_op(plan, *input);
        walk_rel_spec(v, plan, rel);
    }
}

/// Walk the optional inline property predicate of a [`RelSpec`].
fn walk_rel_spec<V: Visitor>(v: &mut V, plan: &PlanStatement, rel: &RelSpec) {
    if let Some(props) = &rel.properties {
        v.visit_expr(plan, props);
    }
}

// ── WriteOp walkers ───────────────────────────────────────────────────────────

/// Walk a [`WriteOp::CreateNode`]: descend into the `props` map expression.
/// Default body of [`Visitor::visit_create_node`].
pub fn walk_create_node<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    if let WriteOp::CreateNode { props, .. } = op {
        v.visit_expr(plan, props);
    }
}

/// Walk a [`WriteOp::CreateRel`]: descend into the `props` map expression.
/// Default body of [`Visitor::visit_create_rel`].
pub fn walk_create_rel<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    if let WriteOp::CreateRel { props, .. } = op {
        v.visit_expr(plan, props);
    }
}

/// Walk a [`WriteOp::MergeNode`]: descend into `props` and the nested
/// `on_create` / `on_match` [`WriteOp`] lists. Default body of
/// [`Visitor::visit_merge_node`].
pub fn walk_merge_node<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    if let WriteOp::MergeNode {
        props,
        on_create,
        on_match,
        ..
    } = op
    {
        v.visit_expr(plan, props);
        for sub in on_create {
            v.visit_write_op(plan, sub);
        }
        for sub in on_match {
            v.visit_write_op(plan, sub);
        }
    }
}

/// Walk a [`WriteOp::MergeRel`]: descend into `props` and the nested
/// `on_create` / `on_match` [`WriteOp`] lists. Default body of
/// [`Visitor::visit_merge_rel`].
pub fn walk_merge_rel<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    if let WriteOp::MergeRel {
        props,
        on_create,
        on_match,
        ..
    } = op
    {
        v.visit_expr(plan, props);
        for sub in on_create {
            v.visit_write_op(plan, sub);
        }
        for sub in on_match {
            v.visit_write_op(plan, sub);
        }
    }
}

/// Walk a [`WriteOp::SetProperty`]: descend into the `value` expression.
/// Default body of [`Visitor::visit_set_property`].
pub fn walk_set_property<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    if let WriteOp::SetProperty { value, .. } = op {
        v.visit_expr(plan, value);
    }
}

/// Walk a [`WriteOp::SetLabels`]. A leaf operator: nothing to recurse into.
/// Default body of [`Visitor::visit_set_labels`].
pub fn walk_set_labels<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    let _ = (v, plan, op);
}

/// Walk a [`WriteOp::RemoveProperty`]. A leaf operator: nothing to recurse
/// into. Default body of [`Visitor::visit_remove_property`].
pub fn walk_remove_property<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    let _ = (v, plan, op);
}

/// Walk a [`WriteOp::RemoveLabels`]. A leaf operator: nothing to recurse into.
/// Default body of [`Visitor::visit_remove_labels`].
pub fn walk_remove_labels<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    let _ = (v, plan, op);
}

/// Walk a [`WriteOp::Delete`]: descend into every target expression. Default
/// body of [`Visitor::visit_delete`].
pub fn walk_delete<V: Visitor>(v: &mut V, plan: &PlanStatement, op: &WriteOp) {
    if let WriteOp::Delete { targets, .. } = op {
        for target in targets {
            v.visit_expr(plan, target);
        }
    }
}

// ── Expr walker ───────────────────────────────────────────────────────────────

/// Walk an [`Expr`], descending through every sub-expression. On reaching an
/// [`Expr::Exists`] the embedded read sub-tree is walked via
/// [`Visitor::visit_read_op_node`]. Default body of [`Visitor::visit_expr`].
pub fn walk_expr<V: Visitor>(v: &mut V, plan: &PlanStatement, expr: &Expr) {
    // `Expr` is `#[non_exhaustive]`; the wildcard arm is unreachable today but
    // kept so a future variant does not break this walker (see the note in
    // `walk_read_op_node`). The leaf arm is enumerated explicitly — rather than
    // folded into the wildcard — to document which `Expr` variants are leaves.
    #[allow(unreachable_patterns, clippy::match_same_arms)]
    match expr {
        // Leaves: no sub-expressions.
        Expr::Null
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::Param { .. } => {}
        Expr::Prop { target, .. } => v.visit_expr(plan, target),
        Expr::Index { target, index } => {
            v.visit_expr(plan, target);
            v.visit_expr(plan, index);
        }
        Expr::Slice { target, start, end } => {
            v.visit_expr(plan, target);
            if let Some(s) = start {
                v.visit_expr(plan, s);
            }
            if let Some(e) = end {
                v.visit_expr(plan, e);
            }
        }
        Expr::List(items) => {
            for item in items {
                v.visit_expr(plan, item);
            }
        }
        Expr::Map(entries) => {
            for (_k, value) in entries {
                v.visit_expr(plan, value);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                v.visit_expr(plan, arg);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            v.visit_expr(plan, lhs);
            v.visit_expr(plan, rhs);
        }
        Expr::UnaryOp { operand, .. } | Expr::IsNull { operand, .. } => {
            v.visit_expr(plan, operand);
        }
        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            if let Some(s) = scrutinee {
                v.visit_expr(plan, s);
            }
            for (when, then) in arms {
                v.visit_expr(plan, when);
                v.visit_expr(plan, then);
            }
            if let Some(o) = otherwise {
                v.visit_expr(plan, o);
            }
        }
        Expr::InList { operand, list } => {
            v.visit_expr(plan, operand);
            v.visit_expr(plan, list);
        }
        Expr::ListPredicate {
            iterable,
            predicate,
            ..
        } => {
            v.visit_expr(plan, iterable);
            if let Some(p) = predicate {
                v.visit_expr(plan, p);
            }
        }
        Expr::Exists { pattern } => v.visit_read_op_node(plan, pattern),
        // Forward-compat: a future `#[non_exhaustive]` `Expr` variant is
        // simply not descended into. Operator-level forward-compat is handled
        // by `visit_unknown_read_op` / `visit_unknown_write_op`.
        _ => {}
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::lower_statement;

    /// Lower a Cypher source string to a resolved, desugared HIR statement.
    fn hir_from(src: &str) -> cyrs_hir::Statement {
        let hir = cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(src))
            .expect("lower_parse is infallible");
        cyrs_hir::desugar::desugar_statement(hir)
    }

    /// Lower a Cypher source string to a [`PlanStatement`] for testing.
    fn plan_from(src: &str) -> PlanStatement {
        lower_statement(&hir_from(src)).expect("lowering should succeed")
    }

    /// A visitor that tallies every operator and expression kind it meets.
    #[derive(Default)]
    struct Counter {
        read_ops: usize,
        write_ops: usize,
        sources: usize,
        expands: usize,
        filters: usize,
        projects: usize,
        unwinds: usize,
        create_nodes: usize,
        create_rels: usize,
        merge_nodes: usize,
        set_props: usize,
        deletes: usize,
        exprs: usize,
        unknown_read: usize,
        unknown_write: usize,
    }

    impl Visitor for Counter {
        fn visit_read_op_node(&mut self, plan: &PlanStatement, op: &ReadOp) {
            self.read_ops += 1;
            walk_read_op_node(self, plan, op);
        }

        fn visit_write_op(&mut self, plan: &PlanStatement, op: &WriteOp) {
            self.write_ops += 1;
            walk_write_op(self, plan, op);
        }

        fn visit_source(&mut self, plan: &PlanStatement, label: &Option<LabelSet>, bind: VarId) {
            self.sources += 1;
            walk_source(self, plan, label, bind);
        }

        fn visit_expand(&mut self, plan: &PlanStatement, op: &ReadOp) {
            self.expands += 1;
            walk_expand(self, plan, op);
        }

        fn visit_filter(&mut self, plan: &PlanStatement, input: OpId, predicate: &Expr) {
            self.filters += 1;
            walk_filter(self, plan, input, predicate);
        }

        fn visit_project(&mut self, plan: &PlanStatement, input: OpId, items: &[Projection]) {
            self.projects += 1;
            walk_project(self, plan, input, items);
        }

        fn visit_unwind(&mut self, plan: &PlanStatement, input: OpId, list: &Expr, bind: VarId) {
            self.unwinds += 1;
            walk_unwind(self, plan, input, list, bind);
        }

        fn visit_create_node(&mut self, plan: &PlanStatement, op: &WriteOp) {
            self.create_nodes += 1;
            walk_create_node(self, plan, op);
        }

        fn visit_create_rel(&mut self, plan: &PlanStatement, op: &WriteOp) {
            self.create_rels += 1;
            walk_create_rel(self, plan, op);
        }

        fn visit_merge_node(&mut self, plan: &PlanStatement, op: &WriteOp) {
            self.merge_nodes += 1;
            walk_merge_node(self, plan, op);
        }

        fn visit_set_property(&mut self, plan: &PlanStatement, op: &WriteOp) {
            self.set_props += 1;
            walk_set_property(self, plan, op);
        }

        fn visit_delete(&mut self, plan: &PlanStatement, op: &WriteOp) {
            self.deletes += 1;
            walk_delete(self, plan, op);
        }

        fn visit_expr(&mut self, plan: &PlanStatement, expr: &Expr) {
            self.exprs += 1;
            walk_expr(self, plan, expr);
        }

        fn visit_unknown_read_op(&mut self, _plan: &PlanStatement, _op: &ReadOp) {
            self.unknown_read += 1;
        }

        fn visit_unknown_write_op(&mut self, _plan: &PlanStatement, _op: &WriteOp) {
            self.unknown_write += 1;
        }
    }

    #[test]
    fn empty_impl_traverses_without_panic() {
        // An empty `impl Visitor` is a complete no-op-but-traversing visitor.
        struct Noop;
        impl Visitor for Noop {}

        let plan = plan_from("MATCH (n:Person) WHERE n.age > 18 RETURN n.name");
        Noop.visit_plan(&plan);
    }

    #[test]
    fn counts_simple_read_plan() {
        // Source -> Filter -> Project.
        let plan = plan_from("MATCH (n:Person) WHERE n.age > 18 RETURN n.name");
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.sources, 1, "one Source scan");
        assert_eq!(c.filters, 1, "one Filter");
        assert_eq!(c.projects, 1, "one Project");
        assert_eq!(c.read_ops, plan.ops.len(), "every arena op visited once");
        assert_eq!(c.write_ops, 0, "pure read query");
        assert!(c.exprs > 0, "expressions are walked");
    }

    #[test]
    fn counts_expand_chain() {
        let plan = plan_from("MATCH (a)-[r:KNOWS]->(b)-[s:KNOWS]->(c) RETURN a, c");
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.sources, 1, "one Source seeds the chain");
        assert_eq!(c.expands, 2, "two Expand operators");
        assert_eq!(c.read_ops, plan.ops.len());
    }

    #[test]
    fn counts_unwind() {
        let plan = plan_from("UNWIND [1, 2, 3] AS x RETURN x");
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.unwinds, 1, "one Unwind");
        assert_eq!(c.read_ops, plan.ops.len());
    }

    #[test]
    fn counts_create_write_ops() {
        let plan = plan_from("CREATE (a:Person {name: 'Alice'})-[r:KNOWS]->(b:Person)");
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.create_nodes, 2, "two CreateNode ops");
        assert_eq!(c.create_rels, 1, "one CreateRel op");
        assert_eq!(
            c.write_ops,
            plan.write_ops.len(),
            "every top-level write op visited"
        );
    }

    #[test]
    fn descends_into_merge_on_create_and_on_match() {
        // MERGE's nested on_create / on_match WriteOp lists must be walked.
        let plan = plan_from(
            "MERGE (n:Person {email: 'a@b.c'}) \
             ON CREATE SET n.created = true \
             ON MATCH SET n.seen = true",
        );
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.merge_nodes, 1, "one MergeNode");
        assert_eq!(
            c.set_props, 2,
            "both the on-create and on-match SetProperty are reached"
        );
        // The nested SetProperty ops count toward the write-op tally too.
        assert!(
            c.write_ops >= 3,
            "merge + 2 nested writes, got {}",
            c.write_ops
        );
    }

    #[test]
    fn descends_into_delete_targets() {
        let plan = plan_from("MATCH (n) DETACH DELETE n");
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.deletes, 1, "one Delete");
        assert!(c.exprs > 0, "delete target expression is walked");
    }

    #[test]
    fn descends_into_optional_join_subtree() {
        // OPTIONAL MATCH lowers to a `ReadOp::OptionalJoin` whose `pattern` is
        // a boxed sub-tree. The inner `Expand` is reachable *only* through that
        // boxed pattern — nothing in the arena references it by `OpId` — so
        // counting it proves the embedded sub-tree was walked.
        let plan = plan_from("MATCH (a:Person) OPTIONAL MATCH (a)-[r:KNOWS]->(b) RETURN a, b");
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert!(
            c.expands >= 1,
            "the Expand inside the OPTIONAL MATCH boxed pattern is reached, got {}",
            c.expands
        );
    }

    #[test]
    fn overriding_without_walk_prunes_traversal() {
        // A visitor that overrides visit_filter WITHOUT calling walk_filter
        // must not descend below the Filter.
        struct Pruner {
            sources: usize,
            filters: usize,
        }
        impl Visitor for Pruner {
            fn visit_source(
                &mut self,
                _plan: &PlanStatement,
                _label: &Option<LabelSet>,
                _bind: VarId,
            ) {
                self.sources += 1;
            }
            fn visit_filter(&mut self, _plan: &PlanStatement, _input: OpId, _predicate: &Expr) {
                self.filters += 1;
                // Intentionally NOT calling walk_filter — prune here.
            }
        }

        let plan = plan_from("MATCH (n:Person) WHERE n.age > 18 RETURN n.name");
        let mut p = Pruner {
            sources: 0,
            filters: 0,
        };
        p.visit_plan(&plan);

        assert_eq!(p.filters, 1, "the Filter itself is visited");
        assert_eq!(
            p.sources, 0,
            "pruning at Filter means the Source below is never reached"
        );
    }

    #[test]
    fn unknown_hooks_unreached_for_current_enums() {
        // With today's enum definitions every variant has a dedicated
        // visit_* method, so the forward-compat fallbacks stay at zero.
        let plan = plan_from(
            "MATCH (n:Person) WHERE n.age > 18 \
             CREATE (m:Robot) MERGE (k:Key {id: 1}) RETURN n",
        );
        let mut c = Counter::default();
        c.visit_plan(&plan);

        assert_eq!(c.unknown_read, 0, "no unknown ReadOp with current enums");
        assert_eq!(c.unknown_write, 0, "no unknown WriteOp with current enums");
    }

    #[test]
    fn unknown_read_hook_is_callable_and_overridable() {
        // We cannot construct a genuinely unknown `ReadOp` variant from
        // within this crate (the crate sees every variant), so the dispatch
        // fallback is unreachable today. This test instead pins the contract
        // of the forward-compat hook itself: its default body is a no-op, and
        // an override is observed when the hook is invoked.
        struct Probe(bool);
        impl Visitor for Probe {
            fn visit_unknown_read_op(&mut self, _plan: &PlanStatement, _op: &ReadOp) {
                self.0 = true;
            }
        }
        // Default body is a no-op; calling it directly must not panic.
        let plan = PlanStatement::empty();
        let mut probe = Probe(false);
        let op = ReadOp::Distinct { input: OpId(0) };
        probe.visit_unknown_read_op(&plan, &op);
        assert!(probe.0, "the hook ran and set its flag");
    }

    #[test]
    fn walk_plan_on_empty_plan_is_noop() {
        let plan = PlanStatement::empty();
        let mut c = Counter::default();
        c.visit_plan(&plan);
        assert_eq!(c.read_ops, 0);
        assert_eq!(c.write_ops, 0);
    }

    #[test]
    fn out_of_bounds_opid_is_silently_ignored() {
        // A malformed plan with a dangling OpId must not panic the walker.
        let mut plan = PlanStatement::empty();
        plan.ops.push(ReadOp::Filter {
            input: OpId(99), // dangling
            predicate: Expr::Bool(true),
        });
        let mut c = Counter::default();
        c.visit_plan(&plan);
        assert_eq!(c.filters, 1, "the Filter is visited");
        // The dangling child resolves to nothing — no panic, no extra op.
        assert_eq!(c.read_ops, 1);
    }

    #[test]
    fn descends_into_exists_pattern_in_expression() {
        // `Expr::Exists` embeds a boxed `ReadOp` sub-tree. The expr walker
        // must descend through it via `visit_read_op_node`. We hand-build a
        // plan with such an expression to exercise the path directly.
        let mut plan = PlanStatement::empty();
        let source = plan.ops.len();
        plan.ops.push(ReadOp::Source {
            label: None,
            bind: VarId(0),
        });
        #[allow(clippy::cast_possible_truncation)]
        let exists_pattern = ReadOp::Filter {
            input: OpId(source as u32),
            predicate: Expr::Bool(true),
        };
        // A Project whose projection expression carries an `Expr::Exists`.
        plan.ops.push(ReadOp::Project {
            input: OpId(0),
            items: vec![Projection {
                expr: Expr::Exists {
                    pattern: Box::new(exists_pattern),
                },
                alias: "ex".into(),
            }],
        });

        let mut c = Counter::default();
        c.visit_plan(&plan);
        // The Source and Project are arena entries; the Filter is reachable
        // only through the `Expr::Exists` pattern, so seeing it proves the
        // expr walker descended into the embedded sub-tree.
        assert_eq!(
            c.filters, 1,
            "Filter inside Expr::Exists pattern is reached"
        );
        assert_eq!(c.sources, 2, "the Source is visited via arena + Exists box");
    }

    #[test]
    fn manual_dispatch_helpers_are_usable_directly() {
        // walk_read_op / visit_read_op are usable as standalone entry points.
        let plan = plan_from("MATCH (n) RETURN n");
        let mut c = Counter::default();
        let root = OpId(u32::try_from(plan.ops.len() - 1).expect("small arena"));
        c.visit_read_op(&plan, root);
        assert_eq!(c.read_ops, plan.ops.len());
    }

    #[test]
    fn covers_order_by_skip_limit_distinct() {
        #[derive(Default)]
        struct All {
            order_by: usize,
            skip: usize,
            limit: usize,
            distinct: usize,
        }
        impl Visitor for All {
            fn visit_order_by(&mut self, plan: &PlanStatement, input: OpId, keys: &[OrderKey]) {
                self.order_by += 1;
                walk_order_by(self, plan, input, keys);
            }
            fn visit_skip(&mut self, plan: &PlanStatement, input: OpId, count: &Expr) {
                self.skip += 1;
                walk_skip(self, plan, input, count);
            }
            fn visit_limit(&mut self, plan: &PlanStatement, input: OpId, count: &Expr) {
                self.limit += 1;
                walk_limit(self, plan, input, count);
            }
            fn visit_distinct(&mut self, plan: &PlanStatement, input: OpId) {
                self.distinct += 1;
                walk_distinct(self, plan, input);
            }
        }
        let plan = plan_from(
            "MATCH (n:Person) RETURN DISTINCT n.name AS nm \
             ORDER BY n.name SKIP 2 LIMIT 5",
        );
        let mut all = All::default();
        all.visit_plan(&plan);
        assert_eq!(all.order_by, 1, "one OrderBy");
        assert_eq!(all.skip, 1, "one Skip");
        assert_eq!(all.limit, 1, "one Limit");
        assert_eq!(all.distinct, 1, "one Distinct");
    }

    #[test]
    fn covers_union_and_with() {
        #[derive(Default)]
        struct Uw {
            unions: usize,
            withs: usize,
        }
        impl Visitor for Uw {
            fn visit_union(
                &mut self,
                plan: &PlanStatement,
                left: OpId,
                right: OpId,
                kind: UnionKind,
            ) {
                self.unions += 1;
                walk_union(self, plan, left, right, kind);
            }
            fn visit_with(
                &mut self,
                plan: &PlanStatement,
                input: OpId,
                items: &[Projection],
                filter: &Option<Expr>,
            ) {
                self.withs += 1;
                walk_with(self, plan, input, items, filter);
            }
        }
        // `ReadOp::Union` is produced by `lower_union_pair`, which joins two
        // already-split arms — the visitor must reach both sub-plans.
        let left = hir_from("MATCH (a:Person) WITH a WHERE a.age > 1 RETURN a.name AS x");
        let right = hir_from("MATCH (b:Robot) RETURN b.name AS y");
        let plan = crate::lower::lower_union_pair(&left, &right, UnionKind::Distinct)
            .expect("union lowering should succeed");
        let mut uw = Uw::default();
        uw.visit_plan(&plan);
        assert_eq!(uw.unions, 1, "one Union at the root");
        assert_eq!(uw.withs, 1, "the With clause in the left arm is reached");
    }

    #[test]
    fn covers_aggregate() {
        struct Agg(usize);
        impl Visitor for Agg {
            fn visit_aggregate(
                &mut self,
                plan: &PlanStatement,
                input: OpId,
                keys: &[Expr],
                aggs: &[AggExpr],
            ) {
                self.0 += 1;
                walk_aggregate(self, plan, input, keys, aggs);
            }
        }
        let plan = plan_from("MATCH (n:Person) RETURN n.city AS c, count(*) AS n");
        let mut agg = Agg(0);
        agg.visit_plan(&plan);
        assert_eq!(agg.0, 1, "one Aggregate");
    }

    #[test]
    fn covers_shortest_path() {
        struct Sp(usize);
        impl Visitor for Sp {
            fn visit_shortest_path(&mut self, plan: &PlanStatement, op: &ReadOp) {
                self.0 += 1;
                walk_shortest_path(self, plan, op);
            }
        }
        let plan = plan_from(
            "MATCH (a:Person), (b:Person) \
             MATCH p = shortestPath((a)-[:KNOWS*]->(b)) RETURN p",
        );
        let mut sp = Sp(0);
        sp.visit_plan(&plan);
        assert_eq!(sp.0, 1, "one ShortestPath operator");
    }

    #[test]
    fn covers_set_remove_write_ops() {
        #[derive(Default)]
        struct SetRm {
            set_labels: usize,
            set_props: usize,
            rm_labels: usize,
            rm_props: usize,
        }
        impl Visitor for SetRm {
            fn visit_set_labels(&mut self, plan: &PlanStatement, op: &WriteOp) {
                self.set_labels += 1;
                walk_set_labels(self, plan, op);
            }
            fn visit_set_property(&mut self, plan: &PlanStatement, op: &WriteOp) {
                self.set_props += 1;
                walk_set_property(self, plan, op);
            }
            fn visit_remove_labels(&mut self, plan: &PlanStatement, op: &WriteOp) {
                self.rm_labels += 1;
                walk_remove_labels(self, plan, op);
            }
            fn visit_remove_property(&mut self, plan: &PlanStatement, op: &WriteOp) {
                self.rm_props += 1;
                walk_remove_property(self, plan, op);
            }
        }
        let plan = plan_from(
            "MATCH (n:Person) SET n:Admin SET n.age = 30 \
             REMOVE n:Person REMOVE n.tmp",
        );
        let mut sr = SetRm::default();
        sr.visit_plan(&plan);
        assert_eq!(sr.set_labels, 1, "one SetLabels");
        assert_eq!(sr.set_props, 1, "one SetProperty");
        assert_eq!(sr.rm_labels, 1, "one RemoveLabels");
        assert_eq!(sr.rm_props, 1, "one RemoveProperty");
    }

    #[test]
    fn covers_merge_rel_nested_writes() {
        #[derive(Default)]
        struct Mr {
            merge_rels: usize,
            on_create_sets: usize,
            on_match_sets: usize,
        }
        impl Visitor for Mr {
            fn visit_merge_rel(&mut self, plan: &PlanStatement, op: &WriteOp) {
                self.merge_rels += 1;
                walk_merge_rel(self, plan, op);
            }
            fn visit_set_property(&mut self, plan: &PlanStatement, op: &WriteOp) {
                // The two nested SetProperty ops differ by their `prop` key.
                if let WriteOp::SetProperty { prop, .. } = op {
                    match prop.as_str() {
                        "fresh" => self.on_create_sets += 1,
                        "seen" => self.on_match_sets += 1,
                        _ => {}
                    }
                }
                walk_set_property(self, plan, op);
            }
        }
        // Hand-build a MergeRel whose on_create / on_match lists each carry a
        // SetProperty, to confirm `walk_merge_rel` descends into both.
        let mut plan = PlanStatement::empty();
        plan.write_ops.push(WriteOp::MergeRel {
            from: VarId(0),
            to: VarId(1),
            rel_type: "KNOWS".into(),
            props: Expr::Map(vec![]),
            key_props: vec![],
            on_create: vec![WriteOp::SetProperty {
                target: VarId(2),
                prop: "fresh".into(),
                value: Expr::Bool(true),
            }],
            on_match: vec![WriteOp::SetProperty {
                target: VarId(2),
                prop: "seen".into(),
                value: Expr::Bool(true),
            }],
            bind: Some(VarId(2)),
        });
        let mut mr = Mr::default();
        mr.visit_plan(&plan);
        assert_eq!(mr.merge_rels, 1, "one MergeRel");
        assert_eq!(mr.on_create_sets, 1, "the on-create SetProperty is reached");
        assert_eq!(mr.on_match_sets, 1, "the on-match SetProperty is reached");
    }

    #[test]
    fn expr_walker_reaches_nested_subexpressions() {
        // Count Var references — there are several buried in the predicate.
        struct VarCounter(usize);
        impl Visitor for VarCounter {
            fn visit_expr(&mut self, plan: &PlanStatement, expr: &Expr) {
                if matches!(expr, Expr::Var(_)) {
                    self.0 += 1;
                }
                walk_expr(self, plan, expr);
            }
        }
        // Build a plan whose predicate nests many Expr variants and confirm
        // the expr walker reaches a deeply-buried leaf.
        let plan = plan_from(
            "MATCH (n:Person) \
             WHERE (n.age + 1) > 18 AND n.name IN ['a', 'b'] \
             RETURN n",
        );
        let mut vc = VarCounter(0);
        vc.visit_plan(&plan);
        assert!(
            vc.0 >= 2,
            "nested Var references inside the predicate are reached, got {}",
            vc.0
        );
    }
}
