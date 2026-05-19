//! HIR → Plan lowering (spec 0001 §12).
//!
//! Entry point: [`lower_statement`]. One call per Cypher statement.
//!
//! # Pre-conditions
//!
//! The HIR passed in **must** be post-resolve and post-desugar:
//!
//! - Name resolution (cy-nres / cy-b4b) must have run so that every
//!   variable reference is `cyrs_hir::Expr::Var(VarId)` — not
//!   `Expr::Unresolved`.
//! - HIR desugaring (cy-mla / `cyrs_hir::desugar`) must have run so
//!   that `ListComprehension`, `MapProjection`, and `PatternPredicate`
//!   nodes are absent.
//!
//! These pre-conditions are enforced at the entry point by a pre-lowering
//! sanity scan (bead cy-wlr): a stray `Expr::Unresolved` or un-desugared
//! construct now yields `Err(PlanLowerError::UnresolvedName)` or
//! `Err(PlanLowerError::UndesugaredExpr)` respectively (see
//! [`crate::PlanLowerError`]), rather than a deep panic. The
//! `debug_assert!`s that guard the same conditions inside the private
//! `LowerCtx::lower_expr` remain as belt-and-braces checks for defense.
//!
//! If you hand this function a freshly-constructed HIR without running
//! those passes first, you will get one of those errors rather than an
//! incorrect or incomplete plan.
//!
//! # Output shape
//!
//! Returns a [`PlanStatement`] whose `ops` vec is the operator arena.
//! Operators reference each other via [`crate::OpId`] (dense index into
//! `ops`). The last element of `ops` is the root (i.e. the final
//! consumer-visible operator). Write operators are collected in
//! `write_ops` and are applied in order after every read-phase row.
//! `var_map` translates plan-scoped [`crate::VarId`]s back to HIR
//! [`cyrs_hir::VarId`]s for diagnostics.

use indexmap::IndexMap;
use smol_str::SmolStr;

use cyrs_hir::{
    Clause, Direction as HirDir, Expr as HirExpr, HirSpan, ListPredKind as HirListPredKind,
    OrderItem, Pattern, PatternElement, PatternPart, Projection, RelLength as HirRelLen,
    RemoveItem, SetItem, Statement, VarId as HirVarId,
};

use crate::{
    AggExpr, BinOp, Direction, Expr, LabelSet, ListPredKind, NodeSpec, OpId, OrderKey,
    PlanLowerError, Projection as PlanProj, ReadOp, RelLength, RelSpec, SortDir, UnaryOp,
    UnionKind, VarId, WriteOp,
};

// ── Public output type ────────────────────────────────────────────────────────

/// The result of lowering a single HIR [`Statement`] to a logical plan.
///
/// `ops` is the operator arena: each entry is a [`ReadOp`] and may
/// reference earlier entries via [`OpId`]. The last entry is the root.
/// If the statement has no read phase (e.g. bare `CREATE`), `ops` is
/// empty and the root is implicit (one write pass over an empty row).
///
/// `write_ops` are applied in order after every row produced by the read
/// phase. For a pure read query they are empty.
///
/// `var_map` maps plan-scoped [`VarId`]s back to HIR [`HirVarId`]s for
/// diagnostic purposes (spec §12.3).
#[derive(Debug, Clone)]
pub struct PlanStatement {
    /// Ordered flat arena of read operators. References use dense [`OpId`].
    pub ops: Vec<ReadOp>,
    /// Write operators applied after each read-phase row.
    pub write_ops: Vec<WriteOp>,
    /// Mapping from plan [`VarId`] → HIR [`HirVarId`]. Insertion-ordered
    /// for determinism (spec §17.14).
    pub var_map: IndexMap<VarId, HirVarId>,
}

impl PlanStatement {
    fn new() -> Self {
        Self::empty()
    }

    /// Construct an empty [`PlanStatement`] — no read or write operators
    /// and an empty `var_map`. Useful as a fallback when downstream
    /// callers need a plan shape for a malformed query (cy-wlr).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            ops: Vec::new(),
            write_ops: Vec::new(),
            var_map: IndexMap::new(),
        }
    }

    /// Push an operator and return its [`OpId`].
    fn push(&mut self, op: ReadOp) -> OpId {
        #[allow(clippy::cast_possible_truncation)]
        let id = OpId(self.ops.len() as u32);
        self.ops.push(op);
        id
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Lower a post-resolve, post-desugar HIR [`Statement`] into a logical
/// [`PlanStatement`].
///
/// # Errors
///
/// Before walking the HIR the entry point performs a pre-lowering sanity
/// scan (bead cy-wlr). It returns without building any plan operators when
/// it encounters:
///
/// - [`PlanLowerError::UnresolvedName`] — a
///   [`cyrs_hir::Expr::Unresolved`] node. Run name resolution
///   (`cyrs-sema::resolve` / cy-b4b) first.
/// - [`PlanLowerError::UndesugaredExpr`] — a
///   [`cyrs_hir::Expr::PatternPredicate`],
///   [`cyrs_hir::Expr::ListComprehension`], or
///   [`cyrs_hir::Expr::MapProjection`]. Run
///   [`cyrs_hir::desugar::desugar_statement`] (cy-mla) first.
///
/// The scan returns at the first offending node; other violations in the
/// same statement are not reported in a single call.
///
/// # Panics (debug)
///
/// The pre-scan makes the main lowering body sound for the accepted
/// subset of HIR. The `debug_assert!`s inside the private `lower_expr`
/// helper remain for defense; they must not fire in practice because the
/// scan catches the same conditions first.
pub fn lower_statement(stmt: &Statement) -> Result<PlanStatement, PlanLowerError> {
    precheck_statement(stmt)?;
    let mut ctx = LowerCtx::new(stmt);
    ctx.lower(stmt);
    Ok(ctx.into_plan())
}

// ── Pre-lowering sanity scan (cy-wlr) ─────────────────────────────────────────

/// Walk every expression reachable from `stmt.clauses` and return the first
/// precondition violation, if any. See [`lower_statement`] for the contract.
fn precheck_statement(stmt: &Statement) -> Result<(), PlanLowerError> {
    for clause in &stmt.clauses {
        let span = clause.span();
        match clause {
            Clause::Match { pattern, .. } | Clause::Create { pattern, .. } => {
                check_pattern(pattern, span)?;
            }
            Clause::Where { predicate, .. } => check_expr(predicate, span)?,
            Clause::With {
                projections,
                filter,
                ..
            } => {
                for p in projections {
                    check_expr(&p.expr, span)?;
                }
                if let Some(f) = filter {
                    check_expr(f, span)?;
                }
            }
            Clause::Return { projections, .. } => {
                for p in projections {
                    check_expr(&p.expr, span)?;
                }
            }
            Clause::Unwind { list, .. } => check_expr(list, span)?,
            Clause::Merge {
                pattern,
                on_create,
                on_match,
                ..
            } => {
                check_pattern(pattern, span)?;
                for item in on_create.iter().chain(on_match.iter()) {
                    check_set_item(item, span)?;
                }
            }
            Clause::Set { items, .. } => {
                for item in items {
                    check_set_item(item, span)?;
                }
            }
            Clause::Remove { items, .. } => {
                for item in items {
                    check_remove_item(item, span)?;
                }
            }
            Clause::Delete { targets, .. } => {
                for t in targets {
                    check_expr(t, span)?;
                }
            }
            Clause::Call { args, .. } => {
                for a in args {
                    check_expr(a, span)?;
                }
            }
        }
    }
    Ok(())
}

fn check_pattern(pattern: &Pattern, clause_span: HirSpan) -> Result<(), PlanLowerError> {
    for part in &pattern.parts {
        // cy-f2t: the parser's error-recovery pass can yield a `PatternPart`
        // with zero elements (e.g. bare `MATCH`) or a part whose first element
        // is a `Rel` (e.g. `MATCH -[:R]->(n)`). The Source + Expand walker in
        // `lower_pattern_part` assumes the first element is a `Node` and that
        // the part has at least one element; surface any violation here as a
        // clean error rather than a deep `.expect(...)` panic.
        match part.elements.first() {
            None => return Err(PlanLowerError::EmptyPatternPart { span: clause_span }),
            Some(PatternElement::Rel { .. }) => {
                return Err(PlanLowerError::EmptyPatternPart { span: clause_span });
            }
            Some(PatternElement::Node { .. }) => {}
        }
        for elem in &part.elements {
            let props = match elem {
                PatternElement::Node { props, .. } | PatternElement::Rel { props, .. } => {
                    props.as_ref()
                }
            };
            if let Some(p) = props {
                // Element spans are preferred over the clause span because the
                // HIR records them per element.
                check_expr(p, elem.span())?;
            }
        }
    }
    Ok(())
}

fn check_set_item(item: &SetItem, span: HirSpan) -> Result<(), PlanLowerError> {
    match item {
        SetItem::Property { target, value, .. } => {
            check_expr(target, span)?;
            check_expr(value, span)?;
        }
        SetItem::Labels { .. } => {}
        SetItem::AssignMap { map, .. } => check_expr(map, span)?,
    }
    Ok(())
}

fn check_remove_item(item: &RemoveItem, span: HirSpan) -> Result<(), PlanLowerError> {
    match item {
        RemoveItem::Property { target, .. } => check_expr(target, span)?,
        RemoveItem::Labels { .. } => {}
    }
    Ok(())
}

/// Recursively walk a HIR expression, returning the first precondition
/// violation encountered (see [`PlanLowerError`]).
///
/// `span` is the enclosing clause's span — the HIR does not carry
/// per-expression spans in v1, so sub-expressions inherit their clause
/// span for diagnostic purposes.
fn check_expr(expr: &HirExpr, span: HirSpan) -> Result<(), PlanLowerError> {
    match expr {
        // Leaf nodes with no sub-expressions.
        HirExpr::Null
        | HirExpr::Bool(_)
        | HirExpr::Int(_)
        | HirExpr::Float(_)
        | HirExpr::String(_)
        | HirExpr::Var(_)
        | HirExpr::Param(_) => Ok(()),

        // cy-863: `PatternPredicate` carries an embedded `Pattern` that
        // is lowered in-place by `lower_match_pattern` during plan
        // construction (cy-lve, see [`Expr::Exists`]). That machinery
        // calls `lower_expr` on element properties without first running
        // the pre-lowering scan, so any precondition violation hidden
        // inside the embedded pattern would surface as a deep
        // `debug_assert!` panic. Recurse into the embedded pattern here
        // so violations are reported as a clean `Err` from the outer
        // `lower_statement` call instead.
        HirExpr::PatternPredicate(pattern) => check_pattern(pattern, span),

        // Precondition violations.
        HirExpr::Unresolved(name) => Err(PlanLowerError::UnresolvedName {
            name: name.clone(),
            span,
        }),
        HirExpr::ListComprehension { .. } => Err(PlanLowerError::UndesugaredExpr {
            kind: "ListComprehension",
            span,
        }),
        HirExpr::MapProjection { .. } => Err(PlanLowerError::UndesugaredExpr {
            kind: "MapProjection",
            span,
        }),
        // cy-p1u5: EXISTS subquery is parser-accepted but sema-deferred
        // (spec §0 amendment 2026-05-19, §20 D1 / N4). It must never
        // reach Plan lowering — sema's dialect-gate pass owns the
        // primary E4017 diagnostic. Surfacing it as an
        // `UndesugaredExpr` here is a hard backstop so a caller that
        // bypasses sema cannot accidentally produce a Plan that
        // pretends the subquery is valid.
        HirExpr::ExistsSubqueryDeferred { .. } => Err(PlanLowerError::UndesugaredExpr {
            kind: "ExistsSubqueryDeferred",
            span,
        }),

        // Recursive cases.
        HirExpr::Prop { target, .. } => check_expr(target, span),
        HirExpr::Index { target, index } => {
            check_expr(target, span)?;
            check_expr(index, span)
        }
        HirExpr::Slice { target, start, end } => {
            check_expr(target, span)?;
            if let Some(s) = start {
                check_expr(s, span)?;
            }
            if let Some(e) = end {
                check_expr(e, span)?;
            }
            Ok(())
        }
        HirExpr::List(items) => {
            for item in items {
                check_expr(item, span)?;
            }
            Ok(())
        }
        HirExpr::Map(pairs) => {
            for (_, v) in pairs {
                check_expr(v, span)?;
            }
            Ok(())
        }
        HirExpr::Call { args, .. } => {
            for a in args {
                check_expr(a, span)?;
            }
            Ok(())
        }
        HirExpr::BinOp { lhs, rhs, .. } => {
            check_expr(lhs, span)?;
            check_expr(rhs, span)
        }
        HirExpr::UnaryOp { operand, .. } | HirExpr::IsNull { operand, .. } => {
            check_expr(operand, span)
        }
        HirExpr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            if let Some(s) = scrutinee {
                check_expr(s, span)?;
            }
            for (w, t) in arms {
                check_expr(w, span)?;
                check_expr(t, span)?;
            }
            if let Some(o) = otherwise {
                check_expr(o, span)?;
            }
            Ok(())
        }
        HirExpr::InList { operand, list } => {
            check_expr(operand, span)?;
            check_expr(list, span)
        }
        HirExpr::ListPredicate {
            iterable,
            predicate,
            ..
        } => {
            check_expr(iterable, span)?;
            if let Some(p) = predicate {
                check_expr(p, span)?;
            }
            Ok(())
        }
    }
}

// ── Lowering context ──────────────────────────────────────────────────────────

struct LowerCtx<'s> {
    plan: PlanStatement,
    /// Mapping from HIR `VarId` to plan `VarId` (allocated on first seen).
    hir_to_plan: IndexMap<HirVarId, VarId>,
    next_var: u32,
    _stmt: &'s Statement,
}

impl<'s> LowerCtx<'s> {
    fn new(stmt: &'s Statement) -> Self {
        Self {
            plan: PlanStatement::new(),
            hir_to_plan: IndexMap::new(),
            next_var: 0,
            _stmt: stmt,
        }
    }

    fn into_plan(self) -> PlanStatement {
        self.plan
    }

    // ── VarId mapping ─────────────────────────────────────────────────────────

    fn map_var(&mut self, hir_var: HirVarId) -> VarId {
        if let Some(&plan_var) = self.hir_to_plan.get(&hir_var) {
            return plan_var;
        }
        let plan_var = VarId(self.next_var);
        self.next_var += 1;
        self.hir_to_plan.insert(hir_var, plan_var);
        self.plan.var_map.insert(plan_var, hir_var);
        plan_var
    }

    // ── Top-level clause dispatch ─────────────────────────────────────────────

    fn lower(&mut self, stmt: &Statement) {
        // Handle UNION at statement level: a UNION query is two sub-statements
        // separated by UNION / UNION ALL. In the HIR the clauses of each arm
        // are simply concatenated — there is no explicit union clause node in
        // the current HIR shape. We therefore lower all clauses sequentially;
        // consumers that produce UNION must build paired PlanStatements
        // themselves. Union construction from two Statement arms is handled
        // by `lower_union_pair` (see below).
        //
        // For regular queries we walk the clause list and build up an operator
        // chain.
        let mut current_op: Option<OpId> = None;

        let mut i = 0;
        while i < stmt.clauses.len() {
            let clause = &stmt.clauses[i];
            match clause {
                Clause::Match {
                    pattern, optional, ..
                } => {
                    let (new_op, _) = self.lower_match_pattern(pattern, current_op, *optional);
                    current_op = Some(new_op);
                }
                Clause::Where { predicate, .. } => {
                    let pred = self.lower_expr(predicate);
                    let input = current_op.unwrap_or_else(|| self.push_source_all());
                    let op = self.plan.push(ReadOp::Filter {
                        input,
                        predicate: pred,
                    });
                    current_op = Some(op);
                }
                Clause::With {
                    projections,
                    filter,
                    order_by,
                    skip,
                    limit,
                    ..
                } => {
                    let input = current_op.unwrap_or_else(|| self.push_source_all());
                    let items = self.lower_projections(projections);
                    let filter_expr = filter.as_ref().map(|f| self.lower_expr(f));
                    let op = self.plan.push(ReadOp::With {
                        input,
                        items,
                        filter: filter_expr,
                    });
                    current_op =
                        Some(self.lower_trailers(op, order_by, skip.as_ref(), limit.as_ref()));
                }
                Clause::Return {
                    projections,
                    distinct,
                    order_by,
                    skip,
                    limit,
                    ..
                } => {
                    let input = current_op.unwrap_or_else(|| self.push_source_all());
                    let (items, agg_items) = self.split_projections_agg(projections);
                    let op = if agg_items.is_empty() {
                        let proj_items = self.lower_projections(projections);
                        self.plan.push(ReadOp::Project {
                            input,
                            items: proj_items,
                        })
                    } else {
                        // Aggregating RETURN: emit Aggregate then Project for
                        // the non-agg columns.
                        let keys: Vec<Expr> = items.iter().map(|p| p.expr.clone()).collect();
                        let agg_op = self.plan.push(ReadOp::Aggregate {
                            input,
                            keys,
                            aggs: agg_items,
                        });
                        // Project picks up both key cols and agg output cols;
                        // we project everything from the aggregate.
                        let all_items = self.lower_projections(projections);
                        self.plan.push(ReadOp::Project {
                            input: agg_op,
                            items: all_items,
                        })
                    };
                    let op = if *distinct {
                        self.plan.push(ReadOp::Distinct { input: op })
                    } else {
                        op
                    };
                    current_op =
                        Some(self.lower_trailers(op, order_by, skip.as_ref(), limit.as_ref()));
                }
                Clause::Unwind { list, bind, .. } => {
                    let input = current_op.unwrap_or_else(|| self.push_source_all());
                    let list_expr = self.lower_expr(list);
                    let bind_var = self.map_var(*bind);
                    let op = self.plan.push(ReadOp::Unwind {
                        input,
                        list: list_expr,
                        bind: bind_var,
                    });
                    current_op = Some(op);
                }
                Clause::Create { pattern, .. } => {
                    let write_ops = self.lower_create_pattern(pattern);
                    self.plan.write_ops.extend(write_ops);
                }
                Clause::Merge {
                    pattern,
                    on_create,
                    on_match,
                    ..
                } => {
                    let write_ops = self.lower_merge_pattern(pattern, on_create, on_match);
                    self.plan.write_ops.extend(write_ops);
                }
                Clause::Set { items, .. } => {
                    let write_ops = self.lower_set_items(items);
                    self.plan.write_ops.extend(write_ops);
                }
                Clause::Remove { items, .. } => {
                    let write_ops = self.lower_remove_items(items);
                    self.plan.write_ops.extend(write_ops);
                }
                Clause::Delete {
                    targets, detach, ..
                } => {
                    let exprs: Vec<Expr> = targets.iter().map(|e| self.lower_expr(e)).collect();
                    self.plan.write_ops.push(WriteOp::Delete {
                        targets: exprs,
                        detach: *detach,
                    });
                }
                Clause::Call { .. } => {
                    // CALL subquery / procedure call is out of v1 scope (spec §19/§20).
                    // Leave current_op unchanged.
                }
            }
            i += 1;
        }
    }

    /// Push a degenerate all-node Source (used when a clause appears without
    /// a preceding MATCH, e.g. a standalone RETURN).
    fn push_source_all(&mut self) -> OpId {
        self.plan.push(ReadOp::Source {
            label: None,
            bind: VarId(self.next_var),
        })
        // Note: we do NOT register this synthetic var in var_map because it
        // has no HIR counterpart.
    }

    // ── MATCH pattern → Source + Expand chain ────────────────────────────────

    /// Lower a [`Pattern`] into a Source + Expand chain. Returns the `OpId`
    /// of the outermost operator and a list of variable bindings introduced.
    ///
    /// If `optional` is true and there is an existing `current_op`, the
    /// chain is wrapped in an `OptionalJoin`.
    fn lower_match_pattern(
        &mut self,
        pattern: &Pattern,
        current_op: Option<OpId>,
        optional: bool,
    ) -> (OpId, Vec<VarId>) {
        let mut vars = Vec::new();
        let mut op: Option<OpId> = None;

        for part in &pattern.parts {
            let part_op = self.lower_pattern_part(part, &mut vars);
            op = Some(match op {
                None => part_op,
                Some(left) => {
                    // Multiple pattern parts in a single MATCH clause: treat
                    // as a cross-product by wrapping later parts as nested
                    // expands on the first. In practice, patterns with
                    // multiple parts are rare; we link them sequentially.
                    // The last part's root op is the join point.
                    let _ = left;
                    part_op
                }
            });
        }

        let inner_op = op.unwrap_or_else(|| {
            // Empty pattern — emit an all-node source anyway.
            let bind = VarId(self.next_var);
            self.next_var += 1;
            self.plan.push(ReadOp::Source { label: None, bind })
        });

        let final_op = if optional {
            if let Some(outer) = current_op {
                // Wrap the inner pattern in an OptionalJoin.
                let inner_root = self.plan.ops[inner_op.0 as usize].clone();
                self.plan.push(ReadOp::OptionalJoin {
                    input: outer,
                    pattern: Box::new(inner_root),
                })
            } else {
                inner_op
            }
        } else {
            inner_op
        };

        (final_op, vars)
    }

    fn lower_pattern_part(&mut self, part: &PatternPart, vars: &mut Vec<VarId>) -> OpId {
        // Walk elements; first node becomes Source, alternating
        // Rel+Node pairs become Expand.
        //
        // The entry-point pre-scan (`precheck_statement`, cy-f2t) guarantees
        // `part.elements` is non-empty and starts with a `Node` — the
        // previously-panicking `.expect(…)` sites in this function are
        // replaced with graceful fallbacks so that a consumer who skips the
        // pre-scan still gets a plan, not a panic.
        let mut last_op: Option<OpId> = None;
        let mut last_node_var: Option<VarId> = None;
        let mut last_rel: Option<&PatternElement> = None;

        for elem in &part.elements {
            match elem {
                PatternElement::Node {
                    bind,
                    labels,
                    props,
                    ..
                } => {
                    let bind_var = bind.map(|v| {
                        let pv = self.map_var(v);
                        vars.push(pv);
                        pv
                    });

                    if let (Some(rel_elem), Some(from), Some(input)) =
                        (last_rel.take(), last_node_var, last_op)
                    {
                        // We have a pending relationship + a preceding node
                        // bound → emit an Expand.
                        let bind_var = bind_var.unwrap_or_else(|| {
                            let v = VarId(self.next_var);
                            self.next_var += 1;
                            v
                        });
                        let bind_to = bind_var;

                        let (rel_spec, bind_rel) = self.lower_rel_element(rel_elem, vars);

                        let node_spec = NodeSpec {
                            labels: LabelSet(labels.clone()),
                            properties: props.as_ref().map(|e| self.lower_expr(e)),
                        };

                        let op = self.plan.push(ReadOp::Expand {
                            input,
                            from,
                            rel: rel_spec,
                            to: node_spec,
                            bind_rel,
                            bind_to,
                        });
                        last_node_var = Some(bind_to);
                        last_op = Some(op);
                    } else {
                        // First node (or malformed-but-recovered part whose
                        // leading Rel we silently drop): Source.
                        let label_set = if labels.is_empty() {
                            None
                        } else {
                            Some(LabelSet(labels.clone()))
                        };
                        let bind_var = bind_var.unwrap_or_else(|| {
                            let v = VarId(self.next_var);
                            self.next_var += 1;
                            v
                        });
                        let op = self.plan.push(ReadOp::Source {
                            label: label_set,
                            bind: bind_var,
                        });
                        // If there are inline props on the node, add a Filter.
                        let op = if let Some(prop_expr) = props.as_ref() {
                            let predicate = self.lower_expr(prop_expr);
                            self.plan.push(ReadOp::Filter {
                                input: op,
                                predicate,
                            })
                        } else {
                            op
                        };
                        last_node_var = Some(bind_var);
                        last_op = Some(op);
                    }
                }
                PatternElement::Rel { .. } => {
                    // Store for pairing with the next Node.
                    last_rel = Some(elem);
                }
            }
        }

        // Empty / leading-Rel pattern parts are rejected at the entry point
        // (see `precheck_statement`, cy-f2t). If a consumer bypasses the
        // pre-scan and hands us a part with no Node, degrade to a degenerate
        // all-node Source so lowering still produces a valid plan.
        last_op.unwrap_or_else(|| self.push_source_all())
    }

    fn lower_rel_element(
        &mut self,
        elem: &PatternElement,
        vars: &mut Vec<VarId>,
    ) -> (RelSpec, VarId) {
        match elem {
            PatternElement::Rel {
                bind,
                types,
                direction,
                length,
                props,
                ..
            } => {
                let bind_rel = bind
                    .map(|v| {
                        let pv = self.map_var(v);
                        vars.push(pv);
                        pv
                    })
                    .unwrap_or_else(|| {
                        let v = VarId(self.next_var);
                        self.next_var += 1;
                        v
                    });

                let dir = match direction {
                    HirDir::Outgoing => Direction::Outgoing,
                    HirDir::Incoming => Direction::Incoming,
                    HirDir::Undirected => Direction::Undirected,
                    // `Direction` is `#[non_exhaustive]` (cy-2i9.1).
                    _ => unreachable!("cyrs-plan::lower: unhandled Direction variant"),
                };

                let rel_len = match length {
                    HirRelLen::Single => RelLength::Single,
                    HirRelLen::Variable { min, max } => RelLength::Variable {
                        min: *min,
                        max: *max,
                    },
                    // `RelLength` is `#[non_exhaustive]` (cy-2i9.1).
                    _ => unreachable!("cyrs-plan::lower: unhandled RelLength variant"),
                };

                let rel_spec = RelSpec {
                    types: types.clone(),
                    direction: dir,
                    length: rel_len,
                    properties: props.as_ref().map(|e| self.lower_expr(e)),
                };

                (rel_spec, bind_rel)
            }
            PatternElement::Node { .. } => panic!("lower_rel_element called on a Node element"),
        }
    }

    // ── Projection lowering ───────────────────────────────────────────────────

    fn lower_projections(&mut self, projs: &[Projection]) -> Vec<PlanProj> {
        projs
            .iter()
            .map(|p| {
                let expr = self.lower_expr(&p.expr);
                let alias = p.alias.clone().unwrap_or_else(|| synthesise_alias(&p.expr));
                PlanProj { expr, alias }
            })
            .collect()
    }

    /// Materialise the `ORDER BY` / `SKIP` / `LIMIT` trailers carried on a
    /// `RETURN` (or `WITH`) clause as a chain of `OrderBy` → `Skip` → `Limit`
    /// operators on top of `input`. Returns the new root op (or `input` if no
    /// trailers are present). Mirrors [`apply_order_skip_limit`] for callers
    /// that drive the plan via `lower_statement` directly. Spec §12.1.
    fn lower_trailers(
        &mut self,
        input: OpId,
        order_by: &[OrderItem],
        skip: Option<&HirExpr>,
        limit: Option<&HirExpr>,
    ) -> OpId {
        let mut root = input;
        if !order_by.is_empty() {
            let keys = order_by
                .iter()
                .map(|item| OrderKey {
                    expr: self.lower_expr(&item.expr),
                    dir: if item.descending {
                        SortDir::Desc
                    } else {
                        SortDir::Asc
                    },
                })
                .collect();
            root = self.plan.push(ReadOp::OrderBy { input: root, keys });
        }
        if let Some(expr) = skip {
            let count = self.lower_expr(expr);
            root = self.plan.push(ReadOp::Skip { input: root, count });
        }
        if let Some(expr) = limit {
            let count = self.lower_expr(expr);
            root = self.plan.push(ReadOp::Limit { input: root, count });
        }
        root
    }

    /// Split projections into non-aggregate and aggregate groups.
    ///
    /// A projection is considered an aggregate call when it is a
    /// `HirExpr::Call` whose name is a known aggregate function
    /// (`count`, `sum`, `avg`, `min`, `max`, `collect`, `stdev`,
    /// `stdevp`, `percentileCont`, `percentileDisc`). This mirrors the
    /// function catalog entry `aggregate = true` (spec §8.3) without
    /// importing `cyrs-sema`.
    fn split_projections_agg(&mut self, projs: &[Projection]) -> (Vec<PlanProj>, Vec<AggExpr>) {
        let mut non_agg = Vec::new();
        let mut agg = Vec::new();

        for p in projs {
            if let HirExpr::Call {
                name,
                args,
                distinct,
            } = &p.expr
                && is_aggregate_func(name)
            {
                let plan_args: Vec<Expr> = args.iter().map(|a| self.lower_expr(a)).collect();
                agg.push(AggExpr {
                    func: name.clone(),
                    args: plan_args,
                    distinct: *distinct,
                });
                continue;
            }
            let expr = self.lower_expr(&p.expr);
            let alias = p.alias.clone().unwrap_or_else(|| synthesise_alias(&p.expr));
            non_agg.push(PlanProj { expr, alias });
        }

        (non_agg, agg)
    }

    // ── Write op lowering ─────────────────────────────────────────────────────

    fn lower_create_pattern(&mut self, pattern: &Pattern) -> Vec<WriteOp> {
        let mut ops = Vec::new();
        for part in &pattern.parts {
            // Use the two-pass pairing helper to correctly link rel from/to.
            let paired = create_pattern_pairs(part);
            for pair in paired {
                match pair {
                    CreatePair::Node {
                        labels,
                        props,
                        bind,
                    } => {
                        let bind_var = bind.map(|v| self.map_var(v));
                        let props_expr = if let Some(e) = props.as_ref() {
                            self.lower_expr(e)
                        } else {
                            Expr::Map(vec![])
                        };
                        ops.push(WriteOp::CreateNode {
                            labels,
                            props: props_expr,
                            bind: bind_var,
                        });
                    }
                    CreatePair::Rel {
                        from_bind,
                        to_bind,
                        rel_type,
                        props,
                        bind,
                    } => {
                        let from = self.map_var(from_bind);
                        let to = self.map_var(to_bind);
                        let bind_rel = bind.map(|v| self.map_var(v));
                        let props_expr = if let Some(e) = props.as_ref() {
                            self.lower_expr(e)
                        } else {
                            Expr::Map(vec![])
                        };
                        ops.push(WriteOp::CreateRel {
                            from,
                            to,
                            rel_type,
                            props: props_expr,
                            bind: bind_rel,
                        });
                    }
                }
            }
        }
        ops
    }

    fn lower_merge_pattern(
        &mut self,
        pattern: &Pattern,
        on_create: &[SetItem],
        on_match: &[SetItem],
    ) -> Vec<WriteOp> {
        let mut ops = Vec::new();
        let create_ops = self.lower_set_items(on_create);
        let match_ops = self.lower_set_items(on_match);

        for part in &pattern.parts {
            let paired = create_pattern_pairs(part);
            for pair in paired {
                match pair {
                    CreatePair::Node {
                        labels,
                        props,
                        bind,
                    } => {
                        let bind_var = bind.map(|v| self.map_var(v));
                        let props_expr = if let Some(e) = props.as_ref() {
                            self.lower_expr(e)
                        } else {
                            Expr::Map(vec![])
                        };
                        ops.push(WriteOp::MergeNode {
                            labels,
                            props: props_expr,
                            on_create: create_ops.clone(),
                            on_match: match_ops.clone(),
                            bind: bind_var,
                        });
                    }
                    CreatePair::Rel {
                        from_bind,
                        to_bind,
                        rel_type,
                        props,
                        bind,
                    } => {
                        let from = self.map_var(from_bind);
                        let to = self.map_var(to_bind);
                        let bind_rel = bind.map(|v| self.map_var(v));
                        let props_expr = if let Some(e) = props.as_ref() {
                            self.lower_expr(e)
                        } else {
                            Expr::Map(vec![])
                        };
                        ops.push(WriteOp::MergeRel {
                            from,
                            to,
                            rel_type,
                            props: props_expr,
                            on_create: create_ops.clone(),
                            on_match: match_ops.clone(),
                            bind: bind_rel,
                        });
                    }
                }
            }
        }
        ops
    }

    fn lower_set_items(&mut self, items: &[SetItem]) -> Vec<WriteOp> {
        items
            .iter()
            .flat_map(|item| self.lower_set_item(item))
            .collect()
    }

    fn lower_set_item(&mut self, item: &SetItem) -> Vec<WriteOp> {
        match item {
            SetItem::Property {
                target,
                prop,
                value,
            } => {
                // `target` is an expression; for the plan we need a VarId.
                // Extract a Var from the expression; fall back to a synthetic
                // VarId for non-Var targets.
                let target_var = if let Some(hir_var) = expr_to_var_id(target) {
                    self.map_var(hir_var)
                } else {
                    let v = VarId(self.next_var);
                    self.next_var += 1;
                    v
                };
                vec![WriteOp::SetProperty {
                    target: target_var,
                    prop: prop.clone(),
                    value: self.lower_expr(value),
                }]
            }
            SetItem::Labels { target, labels } => {
                let target_var = self.map_var(*target);
                vec![WriteOp::SetLabels {
                    target: target_var,
                    labels: labels.clone(),
                }]
            }
            SetItem::AssignMap {
                target,
                map: _,
                replace: _,
            } => {
                // Whole-map assignment (`n = {…}` or `n += {…}`) is not
                // representable as a single WriteOp in v1; emit SetLabels
                // with empty labels as a no-op placeholder. Consumers that
                // need full map assignment should handle this at the
                // cyrs-db layer.
                let target_var = self.map_var(*target);
                vec![WriteOp::SetLabels {
                    target: target_var,
                    labels: vec![],
                }]
            }
        }
    }

    fn lower_remove_items(&mut self, items: &[RemoveItem]) -> Vec<WriteOp> {
        items
            .iter()
            .map(|item| match item {
                RemoveItem::Property { target, prop } => {
                    let target_var = if let Some(hir_var) = expr_to_var_id(target) {
                        self.map_var(hir_var)
                    } else {
                        let v = VarId(self.next_var);
                        self.next_var += 1;
                        v
                    };
                    WriteOp::RemoveProperty {
                        target: target_var,
                        prop: prop.clone(),
                    }
                }
                RemoveItem::Labels { target, labels } => {
                    let target_var = self.map_var(*target);
                    WriteOp::RemoveLabels {
                        target: target_var,
                        labels: labels.clone(),
                    }
                }
            })
            .collect()
    }

    // ── Expression lowering ───────────────────────────────────────────────────

    /// Lower a HIR expression to a plan expression.
    ///
    /// # Contract
    ///
    /// - [`HirExpr::Unresolved`]: must not appear in a post-resolution HIR.
    ///   `debug_assert!`s in debug builds; returns [`Expr::Null`] in release.
    ///
    /// - [`HirExpr::PatternPredicate`] / [`HirExpr::ListComprehension`] /
    ///   [`HirExpr::MapProjection`]: must be desugared before lowering (see
    ///   cy-mla and `cyrs_hir::desugar`). `debug_assert!`s in debug builds;
    ///   returns [`Expr::Null`] in release.
    fn lower_expr(&mut self, expr: &HirExpr) -> Expr {
        match expr {
            HirExpr::Null => Expr::Null,
            HirExpr::Bool(b) => Expr::Bool(*b),
            HirExpr::Int(i) => Expr::Int(*i),
            HirExpr::Float(f) => Expr::Float(*f),
            HirExpr::String(s) => Expr::String(s.clone()),
            HirExpr::Var(v) => Expr::Var(self.map_var(*v)),
            HirExpr::Param(name) => Expr::Param { name: name.clone() },

            HirExpr::Prop { target, prop } => Expr::Prop {
                target: Box::new(self.lower_expr(target)),
                prop: prop.clone(),
            },
            HirExpr::Index { target, index } => Expr::Index {
                target: Box::new(self.lower_expr(target)),
                index: Box::new(self.lower_expr(index)),
            },
            HirExpr::Slice { target, start, end } => Expr::Slice {
                target: Box::new(self.lower_expr(target)),
                start: start.as_ref().map(|s| Box::new(self.lower_expr(s))),
                end: end.as_ref().map(|e| Box::new(self.lower_expr(e))),
            },
            HirExpr::List(items) => Expr::List(items.iter().map(|e| self.lower_expr(e)).collect()),
            HirExpr::Map(pairs) => Expr::Map(
                pairs
                    .iter()
                    .map(|(k, v)| (k.clone(), self.lower_expr(v)))
                    .collect(),
            ),
            HirExpr::Call {
                name,
                args,
                distinct: _,
            } => Expr::Call {
                func: name.clone(),
                args: args.iter().map(|a| self.lower_expr(a)).collect(),
            },
            HirExpr::BinOp { op, lhs, rhs } => Expr::BinOp {
                op: lower_bin_op(*op),
                lhs: Box::new(self.lower_expr(lhs)),
                rhs: Box::new(self.lower_expr(rhs)),
            },
            HirExpr::UnaryOp { op, operand } => Expr::UnaryOp {
                op: match op {
                    cyrs_hir::UnaryOp::Neg => UnaryOp::Neg,
                    cyrs_hir::UnaryOp::Not => UnaryOp::Not,
                },
                operand: Box::new(self.lower_expr(operand)),
            },
            HirExpr::Case {
                scrutinee,
                arms,
                otherwise,
            } => Expr::Case {
                scrutinee: scrutinee.as_ref().map(|s| Box::new(self.lower_expr(s))),
                arms: arms
                    .iter()
                    .map(|(w, t)| (self.lower_expr(w), self.lower_expr(t)))
                    .collect(),
                otherwise: otherwise.as_ref().map(|o| Box::new(self.lower_expr(o))),
            },
            HirExpr::IsNull { operand, negated } => Expr::IsNull {
                operand: Box::new(self.lower_expr(operand)),
                negated: *negated,
            },
            HirExpr::InList { operand, list } => Expr::InList {
                operand: Box::new(self.lower_expr(operand)),
                list: Box::new(self.lower_expr(list)),
            },

            // ── Constructs that require pre-lowering passes ──────────────────
            HirExpr::Unresolved(name) => {
                // Name resolution must run before HIR→Plan lowering.
                debug_assert!(
                    false,
                    "Unresolved variable `{name}` encountered in HIR→Plan lowering; \
                     run name resolution (cy-b4b) before calling lower_statement"
                );
                Expr::Null
            }

            HirExpr::PatternPredicate(pattern) => {
                // cy-lve: lower to plan `Expr::Exists` whose payload is
                // the pattern's read-sub-plan. The embedded `ReadOp`
                // mirrors the treatment of `OptionalJoin`: a fresh sub-
                // tree introduced in-place, not an `OpId` into the main
                // arena (spec §12.1 N13 note).
                let (sub_op, _sub_vars) =
                    self.lower_match_pattern(pattern, None, /* optional = */ false);
                let inner_root = self.plan.ops[sub_op.0 as usize].clone();
                Expr::Exists {
                    pattern: Box::new(inner_root),
                }
            }

            HirExpr::ListComprehension { .. } => {
                // List comprehensions must be desugared to Unwind + Filter
                // before lowering (see cy-mla).
                debug_assert!(
                    false,
                    "ListComprehension encountered in HIR→Plan lowering; \
                     run cyrs_hir::desugar::desugar_statement (cy-mla) first"
                );
                Expr::Null
            }

            HirExpr::ListPredicate {
                kind,
                var,
                iterable,
                predicate,
            } => Expr::ListPredicate {
                kind: lower_list_pred_kind(*kind),
                var: self.map_var(*var),
                iterable: Box::new(self.lower_expr(iterable)),
                predicate: predicate.as_ref().map(|p| Box::new(self.lower_expr(p))),
            },

            HirExpr::MapProjection { .. } => {
                // Map projections must be desugared to explicit Expr::Map
                // before lowering (see cy-mla).
                debug_assert!(
                    false,
                    "MapProjection encountered in HIR→Plan lowering; \
                     run cyrs_hir::desugar::desugar_statement (cy-mla) first"
                );
                Expr::Null
            }

            // cy-p1u5: parser-accepted EXISTS subquery — sema-deferred
            // per spec §0 amendment 2026-05-19 / §20 D1 / N4. Sema
            // fires `DiagCode::E4017` before Plan lowering runs; if a
            // caller bypasses sema and reaches us anyway,
            // [`check_expr`] (the precondition scanner that runs at
            // the entry point of `lower_statement`) returns
            // [`PlanLowerError::UndesugaredExpr`]. This match arm only
            // executes when `check_expr` was skipped (an internal
            // bug); produce a stable `Expr::Null` and `debug_assert!`
            // so the bug surfaces loudly in CI.
            HirExpr::ExistsSubqueryDeferred { .. } => {
                debug_assert!(
                    false,
                    "ExistsSubqueryDeferred encountered in HIR→Plan lowering; \
                     spec §20 D1 forbids it. sema's `exists_subquery` gate \
                     (E4017) must run before this point."
                );
                Expr::Null
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Synthesise a column alias for a bare expression when no explicit alias
/// was provided. Used to ensure every plan Projection has an explicit alias
/// (spec §12.1 N4 note).
fn synthesise_alias(expr: &HirExpr) -> SmolStr {
    match expr {
        HirExpr::Var(v) => SmolStr::new(format!("_v{}", v.0)),
        HirExpr::Prop { prop, .. } => prop.clone(),
        HirExpr::Call { name, .. } => name.clone(),
        _ => SmolStr::new("_"),
    }
}

/// Extract the [`HirVarId`] from a simple variable expression, if any.
fn expr_to_var_id(expr: &HirExpr) -> Option<HirVarId> {
    match expr {
        HirExpr::Var(v) => Some(*v),
        _ => None,
    }
}

/// Lower a HIR [`HirListPredKind`] to a plan [`ListPredKind`] (cy-8x5).
///
/// Both enums are `#[non_exhaustive]` at the public boundary; the
/// wildcard arm maps unknown future kinds to `ListPredKind::All` so
/// the plan stays well-typed. A later bead adding a new HIR kind also
/// bumps this mapping.
#[allow(clippy::match_same_arms)]
fn lower_list_pred_kind(kind: HirListPredKind) -> ListPredKind {
    match kind {
        HirListPredKind::Any => ListPredKind::Any,
        HirListPredKind::All => ListPredKind::All,
        HirListPredKind::None => ListPredKind::None,
        HirListPredKind::Single => ListPredKind::Single,
        _ => ListPredKind::All,
    }
}

/// Lower a HIR [`cyrs_hir::BinOp`] to a plan [`BinOp`].
fn lower_bin_op(op: cyrs_hir::BinOp) -> BinOp {
    match op {
        cyrs_hir::BinOp::Add => BinOp::Add,
        cyrs_hir::BinOp::Sub => BinOp::Sub,
        cyrs_hir::BinOp::Mul => BinOp::Mul,
        cyrs_hir::BinOp::Div => BinOp::Div,
        cyrs_hir::BinOp::Mod => BinOp::Mod,
        cyrs_hir::BinOp::Pow => BinOp::Pow,
        cyrs_hir::BinOp::Eq => BinOp::Eq,
        cyrs_hir::BinOp::Neq => BinOp::Neq,
        cyrs_hir::BinOp::Lt => BinOp::Lt,
        cyrs_hir::BinOp::Le => BinOp::Le,
        cyrs_hir::BinOp::Gt => BinOp::Gt,
        cyrs_hir::BinOp::Ge => BinOp::Ge,
        cyrs_hir::BinOp::And => BinOp::And,
        cyrs_hir::BinOp::Or => BinOp::Or,
        cyrs_hir::BinOp::Xor => BinOp::Xor,
        cyrs_hir::BinOp::StartsWith => BinOp::StartsWith,
        cyrs_hir::BinOp::EndsWith => BinOp::EndsWith,
        cyrs_hir::BinOp::Contains => BinOp::Contains,
        cyrs_hir::BinOp::RegexMatch => BinOp::RegexMatch,
        cyrs_hir::BinOp::Concat => BinOp::Concat,
    }
}

/// Returns true if `name` is a known aggregate function (spec §8.3).
fn is_aggregate_func(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "count"
            | "sum"
            | "avg"
            | "min"
            | "max"
            | "collect"
            | "stdev"
            | "stdevp"
            | "percentilecont"
            | "percentiledisc"
    )
}

// ── Create/Merge pattern decomposition helper ─────────────────────────────────

/// A decomposed write operation from a CREATE/MERGE pattern.
enum CreatePair<'a> {
    Node {
        labels: Vec<SmolStr>,
        props: Option<&'a HirExpr>,
        bind: Option<HirVarId>,
    },
    Rel {
        from_bind: HirVarId,
        to_bind: HirVarId,
        rel_type: SmolStr,
        props: Option<&'a HirExpr>,
        bind: Option<HirVarId>,
    },
}

/// Decompose a [`PatternPart`] into a sequence of node and relationship
/// creation pairs. Relationships reference their adjacent nodes by `HirVarId`.
/// Only nodes that have an explicit binding are usable as rel endpoints;
/// anonymous nodes in CREATE are given synthetic `VarIds` by the caller.
fn create_pattern_pairs(part: &PatternPart) -> Vec<CreatePair<'_>> {
    let mut result = Vec::new();
    let mut node_vars: Vec<Option<HirVarId>> = Vec::new();
    let mut elements = part.elements.iter().peekable();

    while let Some(elem) = elements.next() {
        match elem {
            PatternElement::Node {
                bind,
                labels,
                props,
                ..
            } => {
                node_vars.push(*bind);
                result.push(CreatePair::Node {
                    labels: labels.clone(),
                    props: props.as_ref(),
                    bind: *bind,
                });
            }
            PatternElement::Rel {
                bind, types, props, ..
            } => {
                // A relationship must follow a node; take the last node as
                // `from`. The `to` node is the *next* element.
                let Some(from_bind) = node_vars.last().copied().flatten() else {
                    continue; // malformed pattern
                };

                // Peek at the next node.
                let to_bind = match elements.peek() {
                    Some(PatternElement::Node { bind: Some(v), .. }) => {
                        let v = *v;
                        node_vars.push(Some(v));
                        // Consume the next node element here so that the outer
                        // loop doesn't double-emit it. We emit the Node first,
                        // then the Rel.
                        let next = elements.next().unwrap();
                        if let PatternElement::Node {
                            labels,
                            props,
                            bind,
                            ..
                        } = next
                        {
                            result.push(CreatePair::Node {
                                labels: labels.clone(),
                                props: props.as_ref(),
                                bind: *bind,
                            });
                        }
                        v
                    }
                    // Anonymous to-node — cannot reference it by VarId; skip
                    // the relationship in this case (caller provides binding).
                    _ => continue,
                };

                let rel_type = types.first().cloned().unwrap_or_default();
                result.push(CreatePair::Rel {
                    from_bind,
                    to_bind,
                    rel_type,
                    props: props.as_ref(),
                    bind: *bind,
                });
            }
        }
    }

    result
}

// ── Public helper: lower a UNION pair ────────────────────────────────────────

/// Lower two HIR statements joined by `UNION` / `UNION ALL` into a single
/// [`PlanStatement`] whose root is a [`ReadOp::Union`].
///
/// This helper is provided for callers that have already split a
/// `UNION`-joined Cypher query into its left and right arms (e.g. a parser
/// pass). Single-statement callers use [`lower_statement`] directly.
///
/// # Errors
///
/// Returns the first [`PlanLowerError`] produced by either arm; see
/// [`lower_statement`] for the precondition contract.
pub fn lower_union_pair(
    left: &Statement,
    right: &Statement,
    kind: UnionKind,
) -> Result<PlanStatement, PlanLowerError> {
    let mut left_plan = lower_statement(left)?;
    let right_plan = lower_statement(right)?;

    // The left plan's op arena is the base; we offset the right plan's OpIds.
    // Plan arenas are limited to u32::MAX ops in practice; use truncating cast
    // intentionally here — a plan with 4+ billion operators is unreachable.
    #[allow(clippy::cast_possible_truncation)]
    let offset = left_plan.ops.len() as u32;
    #[allow(clippy::cast_possible_truncation)]
    let right_root = OpId(right_plan.ops.len() as u32 - 1 + offset);

    // Append right ops (no OpId rewriting needed — Union references by index).
    left_plan.ops.extend(right_plan.ops);
    left_plan.write_ops.extend(right_plan.write_ops);
    // Merge var_maps (plan VarIds from the right are offset).
    for (plan_var, hir_var) in right_plan.var_map {
        left_plan
            .var_map
            .insert(VarId(plan_var.0 + offset), hir_var);
    }

    let left_root = OpId(offset - 1);
    left_plan.ops.push(ReadOp::Union {
        left: left_root,
        right: right_root,
        kind,
    });

    Ok(left_plan)
}

// ── Public helper: apply ORDER BY / SKIP / LIMIT ─────────────────────────────

/// Wrap the root operator of `plan` with `ORDER BY`, `SKIP`, and/or `LIMIT`
/// operators if the corresponding lists/values are non-empty / Some.
///
/// This is provided as a separate helper so callers that parse `ORDER BY` /
/// `SKIP` / `LIMIT` outside the clause list (e.g. as modifiers on `RETURN`)
/// can apply them after lowering.
pub fn apply_order_skip_limit(
    plan: &mut PlanStatement,
    order_keys: Vec<OrderKey>,
    skip: Option<Expr>,
    limit: Option<Expr>,
) {
    if plan.ops.is_empty() {
        return;
    }
    #[allow(clippy::cast_possible_truncation)]
    let mut root = OpId(plan.ops.len() as u32 - 1);

    if !order_keys.is_empty() {
        let op = ReadOp::OrderBy {
            input: root,
            keys: order_keys,
        };
        root = plan.push(op);
    }
    if let Some(count) = skip {
        let op = ReadOp::Skip { input: root, count };
        root = plan.push(op);
    }
    if let Some(count) = limit {
        let op = ReadOp::Limit { input: root, count };
        root = plan.push(op);
    }
    let _ = root;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SortDir;
    use cyrs_hir::desugar::desugar_statement;
    use cyrs_hir::lower::lower_statement as hir_lower;

    // Helper: lower from source Cypher → plan via HIR.
    fn plan_from(src: &str) -> PlanStatement {
        let hir = hir_lower(src);
        let hir = desugar_statement(hir);
        lower_statement(&hir).expect("plan_from: input HIR must be resolved and desugared")
    }

    // Helper: render a plan to a stable, readable string for snapshots.
    fn render(plan: &PlanStatement) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        writeln!(out, "read_ops: {}", plan.ops.len()).unwrap();
        writeln!(out, "write_ops: {}", plan.write_ops.len()).unwrap();
        writeln!(out, "var_map_entries: {}", plan.var_map.len()).unwrap();
        for (i, op) in plan.ops.iter().enumerate() {
            writeln!(out, "op[{i}]: {}", op_tag(op)).unwrap();
        }
        for (i, wop) in plan.write_ops.iter().enumerate() {
            writeln!(out, "write[{i}]: {}", write_op_tag(wop)).unwrap();
        }
        out
    }

    fn op_tag(op: &ReadOp) -> String {
        match op {
            ReadOp::Source { label, bind } => format!(
                "Source(label={}, bind={})",
                label
                    .as_ref()
                    .map_or("None".into(), |l| format!("{:?}", l.0)),
                bind.0
            ),
            ReadOp::Expand {
                from,
                bind_rel,
                bind_to,
                ..
            } => {
                format!(
                    "Expand(from={}, bind_rel={}, bind_to={})",
                    from.0, bind_rel.0, bind_to.0
                )
            }
            ReadOp::Filter { input, .. } => format!("Filter(input={})", input.0),
            ReadOp::Project { input, items } => {
                format!("Project(input={}, cols={})", input.0, items.len())
            }
            ReadOp::Aggregate { input, keys, aggs } => {
                format!(
                    "Aggregate(input={}, keys={}, aggs={})",
                    input.0,
                    keys.len(),
                    aggs.len()
                )
            }
            ReadOp::OrderBy { input, keys } => {
                format!("OrderBy(input={}, keys={})", input.0, keys.len())
            }
            ReadOp::Skip { input, .. } => format!("Skip(input={})", input.0),
            ReadOp::Limit { input, .. } => format!("Limit(input={})", input.0),
            ReadOp::Distinct { input } => format!("Distinct(input={})", input.0),
            ReadOp::Unwind { input, bind, .. } => {
                format!("Unwind(input={}, bind={})", input.0, bind.0)
            }
            ReadOp::Union { left, right, kind } => {
                format!("Union(left={}, right={}, kind={:?})", left.0, right.0, kind)
            }
            ReadOp::With {
                input,
                items,
                filter,
            } => {
                format!(
                    "With(input={}, cols={}, has_filter={})",
                    input.0,
                    items.len(),
                    filter.is_some()
                )
            }
            ReadOp::OptionalJoin { input, .. } => format!("OptionalJoin(input={})", input.0),
        }
    }

    fn write_op_tag(op: &WriteOp) -> String {
        match op {
            WriteOp::CreateNode { labels, bind, .. } => {
                format!(
                    "CreateNode(labels={:?}, bind={:?})",
                    labels,
                    bind.map(|v| v.0)
                )
            }
            WriteOp::CreateRel { rel_type, bind, .. } => {
                format!("CreateRel(type={rel_type}, bind={:?})", bind.map(|v| v.0))
            }
            WriteOp::MergeNode { labels, bind, .. } => {
                format!(
                    "MergeNode(labels={:?}, bind={:?})",
                    labels,
                    bind.map(|v| v.0)
                )
            }
            WriteOp::MergeRel { rel_type, bind, .. } => {
                format!("MergeRel(type={rel_type}, bind={:?})", bind.map(|v| v.0))
            }
            WriteOp::SetProperty { target, prop, .. } => {
                format!("SetProperty(target={}, prop={prop})", target.0)
            }
            WriteOp::SetLabels { target, labels } => {
                format!("SetLabels(target={}, labels={:?})", target.0, labels)
            }
            WriteOp::RemoveProperty { target, prop } => {
                format!("RemoveProperty(target={}, prop={prop})", target.0)
            }
            WriteOp::RemoveLabels { target, labels } => {
                format!("RemoveLabels(target={}, labels={:?})", target.0, labels)
            }
            WriteOp::Delete { detach, targets } => {
                format!("Delete(detach={detach}, targets={})", targets.len())
            }
        }
    }

    // ── Snapshot tests (15+) ─────────────────────────────────────────────────

    // 1. Single MATCH
    #[test]
    fn snap_single_match() {
        let plan = plan_from("MATCH (n) RETURN n");
        insta::assert_snapshot!("plan_single_match", render(&plan));
    }

    // 2. MATCH with label
    #[test]
    fn snap_match_with_label() {
        let plan = plan_from("MATCH (n:Person) RETURN n");
        insta::assert_snapshot!("plan_match_with_label", render(&plan));
    }

    // 3. MATCH + WHERE
    #[test]
    fn snap_match_where() {
        let plan = plan_from("MATCH (n) WHERE n.age > 18 RETURN n");
        insta::assert_snapshot!("plan_match_where", render(&plan));
    }

    // cy-ypm: canonical MATCH+WHERE must pretty-print as a proper
    // Project → Filter → Source chain, not an orphan Filter over
    // EMPTY_SOURCE.  This pins the end-to-end lowering shape.
    #[test]
    fn snap_match_where_pretty_tree() {
        use crate::pretty::pretty;
        let plan = plan_from("MATCH (a) WHERE a.x = 1 RETURN a");
        insta::assert_snapshot!("plan_match_where_pretty_tree", pretty(&plan));
    }

    // 4. MATCH + WITH
    #[test]
    fn snap_match_with() {
        let plan = plan_from("MATCH (n) WITH n RETURN n");
        insta::assert_snapshot!("plan_match_with", render(&plan));
    }

    // 5. MATCH + RETURN with property projection
    #[test]
    fn snap_match_return_projection() {
        let plan = plan_from("MATCH (n:Person) RETURN n.name, n.age");
        insta::assert_snapshot!("plan_match_return_projection", render(&plan));
    }

    // 6. RETURN DISTINCT
    #[test]
    fn snap_return_distinct() {
        let plan = plan_from("MATCH (n) RETURN DISTINCT n.name");
        insta::assert_snapshot!("plan_return_distinct", render(&plan));
    }

    // 7. UNWIND — build HIR directly because UNWIND's RETURN uses unresolved
    // `x` when going through the text path (name resolution cy-b4b not yet
    // run). We construct the HIR manually with resolved VarIds.
    #[test]
    fn snap_unwind() {
        use cyrs_hir::{
            Binding, Clause, Expr as HirExpr, HirSpan, Statement, VarId as HirVarId, VarKind,
        };
        let span = HirSpan::default();
        let mut stmt = Statement::new(span);
        // Synthesise a dummy HirId by using a minimal syntax node from the
        // HIR's own test helper — or just use a minimal approach with alloc_id.
        // Since Statement::alloc_id requires a SyntaxNode we use an internal
        // field instead by pushing a DUMMY id (OK for test — not in prod).
        let x_var = HirVarId(0);
        stmt.bindings.insert(
            x_var,
            Binding {
                id: x_var,
                name: "x".into(),
                kind: VarKind::Value,
                defined_at: span,
            },
        );
        stmt.clauses.push(Clause::Unwind {
            id: cyrs_hir::HirId::DUMMY,
            list: HirExpr::List(vec![HirExpr::Int(1), HirExpr::Int(2), HirExpr::Int(3)]),
            bind: x_var,
            span,
        });
        stmt.clauses.push(Clause::Return {
            id: cyrs_hir::HirId::DUMMY,
            projections: vec![cyrs_hir::Projection {
                expr: HirExpr::Var(x_var),
                alias: Some("x".into()),
                span,
            }],
            distinct: false,
            span,
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        let plan = lower_statement(&stmt).expect("manually-built HIR must be resolved");
        insta::assert_snapshot!("plan_unwind", render(&plan));
    }

    // 8. CREATE node
    #[test]
    fn snap_create_node() {
        let plan = plan_from("CREATE (n:Person)");
        insta::assert_snapshot!("plan_create_node", render(&plan));
    }

    // 9. CREATE relationship
    #[test]
    fn snap_create_rel() {
        let plan = plan_from("MATCH (a:Person), (b:Person) CREATE (a)-[:KNOWS]->(b)");
        insta::assert_snapshot!("plan_create_rel", render(&plan));
    }

    // 10. MERGE node
    #[test]
    fn snap_merge_node() {
        let plan = plan_from("MERGE (n:Person {name: 'Alice'})");
        insta::assert_snapshot!("plan_merge_node", render(&plan));
    }

    // 11. SET property
    #[test]
    fn snap_set_property() {
        let plan = plan_from("MATCH (n:Person) SET n.age = 30");
        insta::assert_snapshot!("plan_set_property", render(&plan));
    }

    // 12. REMOVE label
    #[test]
    fn snap_remove_label() {
        let plan = plan_from("MATCH (n:Person) REMOVE n:Person");
        insta::assert_snapshot!("plan_remove_label", render(&plan));
    }

    // 13. DELETE
    #[test]
    fn snap_delete() {
        let plan = plan_from("MATCH (n) DELETE n");
        insta::assert_snapshot!("plan_delete", render(&plan));
    }

    // 14. DETACH DELETE
    #[test]
    fn snap_detach_delete() {
        let plan = plan_from("MATCH (n) DETACH DELETE n");
        insta::assert_snapshot!("plan_detach_delete", render(&plan));
    }

    // 15. Aggregation — count
    #[test]
    fn snap_aggregation_count() {
        let plan = plan_from("MATCH (n) RETURN count(n)");
        insta::assert_snapshot!("plan_aggregation_count", render(&plan));
    }

    // 16. Aggregation — sum
    #[test]
    fn snap_aggregation_sum() {
        let plan = plan_from("MATCH (n) RETURN sum(n.age)");
        insta::assert_snapshot!("plan_aggregation_sum", render(&plan));
    }

    // 17. UNION ALL
    #[test]
    fn snap_union_all() {
        let left_hir = desugar_statement(hir_lower("MATCH (n:Person) RETURN n"));
        let right_hir = desugar_statement(hir_lower("MATCH (n:Animal) RETURN n"));
        let plan = lower_union_pair(&left_hir, &right_hir, UnionKind::All)
            .expect("UNION arms must be resolved/desugared");
        insta::assert_snapshot!("plan_union_all", render(&plan));
    }

    // 18. UNION (distinct)
    #[test]
    fn snap_union_distinct() {
        let left_hir = desugar_statement(hir_lower("MATCH (n:Person) RETURN n"));
        let right_hir = desugar_statement(hir_lower("MATCH (n:Animal) RETURN n"));
        let plan = lower_union_pair(&left_hir, &right_hir, UnionKind::Distinct)
            .expect("UNION arms must be resolved/desugared");
        insta::assert_snapshot!("plan_union_distinct", render(&plan));
    }

    // 19. OPTIONAL MATCH
    #[test]
    fn snap_optional_match() {
        let plan = plan_from("MATCH (n) OPTIONAL MATCH (n)-[:KNOWS]->(m) RETURN n, m");
        insta::assert_snapshot!("plan_optional_match", render(&plan));
    }

    // 20. MATCH relationship chain
    #[test]
    fn snap_match_rel_chain() {
        let plan = plan_from("MATCH (a)-[:KNOWS]->(b) RETURN a, b");
        insta::assert_snapshot!("plan_match_rel_chain", render(&plan));
    }

    // 21. apply_order_skip_limit helper
    #[test]
    fn snap_order_skip_limit() {
        let mut plan = plan_from("MATCH (n) RETURN n");
        apply_order_skip_limit(
            &mut plan,
            vec![OrderKey {
                expr: Expr::Var(VarId(0)),
                dir: SortDir::Desc,
            }],
            Some(Expr::Int(10)),
            Some(Expr::Int(5)),
        );
        insta::assert_snapshot!("plan_order_skip_limit", render(&plan));
    }

    // 22. RETURN trailers via lower_statement: source-string RETURN ... LIMIT 1
    // should already carry the Limit op without `apply_order_skip_limit` —
    // cyrs-hir lowers the trailer fields onto Clause::Return, and we consume
    // them via `lower_trailers` from `lower`. This is the contract that
    // unblocks pipeline::compile in lg-query-cyrs.
    #[test]
    fn return_limit_in_lowered_plan() {
        let plan = plan_from("MATCH (n) RETURN n LIMIT 1");
        assert!(
            plan.ops.iter().any(|op| matches!(op, ReadOp::Limit { .. })),
            "expected a Limit op in the lowered plan: {:?}",
            plan.ops
        );
    }

    #[test]
    fn return_order_skip_limit_chain_in_lowered_plan() {
        let plan = plan_from("MATCH (n) RETURN n ORDER BY n DESC SKIP 2 LIMIT 3");
        let kinds: Vec<&'static str> = plan
            .ops
            .iter()
            .map(|op| match op {
                ReadOp::Source { .. } => "Source",
                ReadOp::Project { .. } => "Project",
                ReadOp::OrderBy { .. } => "OrderBy",
                ReadOp::Skip { .. } => "Skip",
                ReadOp::Limit { .. } => "Limit",
                _ => "Other",
            })
            .collect();
        assert!(
            kinds.ends_with(&["OrderBy", "Skip", "Limit"]),
            "expected trailers in OrderBy → Skip → Limit order, got {kinds:?}"
        );
    }

    // ── Determinism check ────────────────────────────────────────────────────

    #[test]
    fn plan_lowering_is_deterministic() {
        let plan1 = plan_from("MATCH (n:Person) WHERE n.age > 18 RETURN n.name, n.age");
        let plan2 = plan_from("MATCH (n:Person) WHERE n.age > 18 RETURN n.name, n.age");
        assert_eq!(render(&plan1), render(&plan2));
    }

    // ── Structural correctness checks ────────────────────────────────────────

    #[test]
    fn single_match_returns_source_and_project() {
        let plan = plan_from("MATCH (n) RETURN n");
        assert!(plan.ops.len() >= 2);
        assert!(matches!(plan.ops[0], ReadOp::Source { .. }));
        assert!(matches!(plan.ops.last(), Some(ReadOp::Project { .. })));
    }

    #[test]
    fn match_where_inserts_filter() {
        let plan = plan_from("MATCH (n) WHERE n.age > 18 RETURN n");
        let has_filter = plan
            .ops
            .iter()
            .any(|op| matches!(op, ReadOp::Filter { .. }));
        assert!(has_filter, "expected Filter op in plan");
    }

    #[test]
    fn create_node_emits_write_op() {
        // Build HIR directly: the cy-nom parser stubs CREATE clauses as ERROR
        // nodes so we test the lowering path by constructing the HIR manually.
        use cyrs_hir::{
            Binding, Clause, HirSpan, Pattern, PatternElement, PatternPart, Statement,
            VarId as HirVarId, VarKind,
        };
        let span = HirSpan::default();
        let mut stmt = Statement::new(span);
        let n_var = HirVarId(0);
        stmt.bindings.insert(
            n_var,
            Binding {
                id: n_var,
                name: "n".into(),
                kind: VarKind::Node,
                defined_at: span,
            },
        );
        stmt.clauses.push(Clause::Create {
            id: cyrs_hir::HirId::DUMMY,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: cyrs_hir::HirId::DUMMY,
                        bind: Some(n_var),
                        labels: vec!["Person".into()],
                        props: None,
                        span,
                    }],
                }],
            },
            span,
        });
        let plan = lower_statement(&stmt).expect("manually-built HIR must be resolved");
        assert!(
            plan.write_ops
                .iter()
                .any(|w| matches!(w, WriteOp::CreateNode { .. })),
            "expected CreateNode write op; write_ops={:?}",
            plan.write_ops.iter().map(write_op_tag).collect::<Vec<_>>()
        );
    }

    #[test]
    fn delete_emits_write_op() {
        // Build HIR directly since the cy-nom parser stubs DELETE as ERROR nodes.
        use cyrs_hir::{
            Binding, Clause, Expr as HirExpr, HirSpan, Pattern, PatternElement, PatternPart,
            Statement, VarId as HirVarId, VarKind,
        };
        let span = HirSpan::default();
        let mut stmt = Statement::new(span);
        let n_var = HirVarId(0);
        stmt.bindings.insert(
            n_var,
            Binding {
                id: n_var,
                name: "n".into(),
                kind: VarKind::Node,
                defined_at: span,
            },
        );
        stmt.clauses.push(Clause::Match {
            id: cyrs_hir::HirId::DUMMY,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: cyrs_hir::HirId::DUMMY,
                        bind: Some(n_var),
                        labels: vec![],
                        props: None,
                        span,
                    }],
                }],
            },
            span,
        });
        stmt.clauses.push(Clause::Delete {
            id: cyrs_hir::HirId::DUMMY,
            targets: vec![HirExpr::Var(n_var)],
            detach: false,
            span,
        });
        let plan = lower_statement(&stmt).expect("manually-built HIR must be resolved");
        assert!(
            plan.write_ops
                .iter()
                .any(|w| matches!(w, WriteOp::Delete { detach: false, .. })),
            "expected Delete(detach=false) write op"
        );
    }

    #[test]
    fn detach_delete_emits_write_op() {
        // Build HIR directly since the cy-nom parser stubs DETACH DELETE as
        // ERROR nodes.
        use cyrs_hir::{
            Binding, Clause, Expr as HirExpr, HirSpan, Pattern, PatternElement, PatternPart,
            Statement, VarId as HirVarId, VarKind,
        };
        let span = HirSpan::default();
        let mut stmt = Statement::new(span);
        let n_var = HirVarId(0);
        stmt.bindings.insert(
            n_var,
            Binding {
                id: n_var,
                name: "n".into(),
                kind: VarKind::Node,
                defined_at: span,
            },
        );
        stmt.clauses.push(Clause::Match {
            id: cyrs_hir::HirId::DUMMY,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: cyrs_hir::HirId::DUMMY,
                        bind: Some(n_var),
                        labels: vec![],
                        props: None,
                        span,
                    }],
                }],
            },
            span,
        });
        stmt.clauses.push(Clause::Delete {
            id: cyrs_hir::HirId::DUMMY,
            targets: vec![HirExpr::Var(n_var)],
            detach: true,
            span,
        });
        let plan = lower_statement(&stmt).expect("manually-built HIR must be resolved");
        assert!(
            plan.write_ops
                .iter()
                .any(|w| matches!(w, WriteOp::Delete { detach: true, .. })),
            "expected Delete(detach=true) write op"
        );
    }

    #[test]
    fn var_map_populated_for_bound_variables() {
        let plan = plan_from("MATCH (n) RETURN n");
        assert!(
            !plan.var_map.is_empty(),
            "var_map should be populated for bound variables"
        );
    }

    // cy-v31: WHERE after WITH must survive end-to-end (source → HIR → plan)
    // and materialise in `ReadOp::With { filter: Some(_), .. }`.
    #[test]
    fn with_where_threads_filter_into_plan() {
        let plan = plan_from(
            "MATCH (a) UNWIND a.aliases AS alias \
             WITH a, alias WHERE alias CONTAINS 'Fancy' \
             RETURN DISTINCT a.canonical_name",
        );
        let has_with_filter = plan.ops.iter().any(|op| {
            matches!(
                op,
                ReadOp::With {
                    filter: Some(_),
                    ..
                }
            )
        });
        assert!(
            has_with_filter,
            "expected ReadOp::With with a Some(filter); plan ops = {:#?}",
            plan.ops
        );
    }

    // ── cy-wlr: precondition violations surface as Err, not panic ────────────

    /// Build a skeletal statement whose single RETURN projects `expr`.
    fn stmt_with_return_expr(expr: HirExpr) -> Statement {
        use cyrs_hir::HirSpan;
        let span = HirSpan::default();
        let mut stmt = Statement::new(span);
        stmt.clauses.push(Clause::Return {
            id: cyrs_hir::HirId::DUMMY,
            projections: vec![Projection {
                expr,
                alias: Some("x".into()),
                span,
            }],
            distinct: false,
            span,
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        stmt
    }

    /// An `Expr::Unresolved` surviving into `lower_statement` must not
    /// panic — it must return `Err(UnresolvedName { name, .. })`.
    #[test]
    fn lower_statement_returns_err_on_unresolved_name() {
        let stmt = stmt_with_return_expr(HirExpr::Unresolved("foo".into()));
        let err = lower_statement(&stmt).expect_err("unresolved name must be rejected");
        match err {
            PlanLowerError::UnresolvedName { name, .. } => assert_eq!(name, "foo"),
            other => panic!("expected UnresolvedName, got {other:?}"),
        }
    }

    /// Un-desugared `ListComprehension` must surface as `UndesugaredExpr`.
    #[test]
    fn lower_statement_returns_err_on_listcomp() {
        let expr = HirExpr::ListComprehension {
            filter_var: HirVarId(0),
            iterable: Box::new(HirExpr::List(vec![HirExpr::Int(1)])),
            filter: None,
            map_expr: Box::new(HirExpr::Var(HirVarId(0))),
        };
        let stmt = stmt_with_return_expr(expr);
        let err = lower_statement(&stmt).expect_err("list comprehension must be rejected");
        match err {
            PlanLowerError::UndesugaredExpr { kind, .. } => assert_eq!(kind, "ListComprehension"),
            other => panic!("expected UndesugaredExpr(ListComprehension), got {other:?}"),
        }
    }

    /// Un-desugared `MapProjection` must surface as `UndesugaredExpr`.
    #[test]
    fn lower_statement_returns_err_on_mapprojection() {
        let expr = HirExpr::MapProjection {
            base: Box::new(HirExpr::Var(HirVarId(0))),
            items: vec![],
        };
        let stmt = stmt_with_return_expr(expr);
        let err = lower_statement(&stmt).expect_err("map projection must be rejected");
        match err {
            PlanLowerError::UndesugaredExpr { kind, .. } => assert_eq!(kind, "MapProjection"),
            other => panic!("expected UndesugaredExpr(MapProjection), got {other:?}"),
        }
    }

    /// cy-863: an `Expr::Unresolved` hidden inside a `PatternPredicate`'s
    /// embedded pattern (e.g. an unresolved name in a node-property
    /// expression) must be reported via the same `UnresolvedName` error
    /// path as a top-level unresolved name — not surface as a deep
    /// `debug_assert!` panic from `lower_expr`.
    #[test]
    fn lower_statement_returns_err_on_unresolved_inside_patternpredicate() {
        let element = PatternElement::Node {
            id: cyrs_hir::HirId::DUMMY,
            bind: None,
            labels: vec![],
            props: Some(HirExpr::Map(vec![(
                "k".into(),
                HirExpr::Unresolved("vaext".into()),
            )])),
            span: HirSpan::default(),
        };
        let pattern = cyrs_hir::Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![element],
            }],
        };
        let stmt = stmt_with_return_expr(HirExpr::PatternPredicate(pattern));
        let err = lower_statement(&stmt)
            .expect_err("unresolved name inside PatternPredicate must be rejected");
        match err {
            PlanLowerError::UnresolvedName { name, .. } => assert_eq!(name, "vaext"),
            other => panic!("expected UnresolvedName, got {other:?}"),
        }
    }

    /// cy-863 (text path): exercise the same code path the `fuzz_plan`
    /// harness uses (parse → HIR lower → desugar → plan lower) on a
    /// snippet that puts an unresolved name inside a pattern predicate's
    /// node properties. Without the precheck recursion this triggered a
    /// `debug_assert!` panic; now it must surface as a clean `Err` (or
    /// `Ok` if upstream lowering happens to bind the name some other
    /// way — the oracle is "no panic", same as the fuzz target).
    #[test]
    fn lower_statement_no_panic_on_unresolved_inside_patternpredicate_text() {
        let s = "MATCH (n) WHERE (n {k: vaext})-->() RETURN n\n";
        let stmt = hir_lower(s);
        let stmt = desugar_statement(stmt);
        // Must not panic; either Ok (resolved by HIR lowering) or Err.
        let _ = lower_statement(&stmt);
    }

    /// Pattern predicates are now accepted by plan lowering (cy-lve) and
    /// emerge as `Expr::Exists { pattern }`. This test locks the new
    /// behaviour: an empty pattern still yields a plan, and the
    /// projection carries the `Expr::Exists` variant.
    #[test]
    fn lower_statement_accepts_patternpredicate_as_exists() {
        let expr = HirExpr::PatternPredicate(cyrs_hir::Pattern { parts: vec![] });
        let stmt = stmt_with_return_expr(expr);
        let plan = lower_statement(&stmt).expect("pattern predicate must lower to Exists");
        // Walk every projection: at least one must be `Expr::Exists { .. }`.
        let mut saw_exists = false;
        for op in &plan.ops {
            if let ReadOp::Project { items, .. } = op {
                for item in items {
                    if matches!(item.expr, Expr::Exists { .. }) {
                        saw_exists = true;
                    }
                }
            }
        }
        assert!(
            saw_exists,
            "expected plan to carry Expr::Exists after PatternPredicate lowering, got {plan:?}"
        );
    }
}
