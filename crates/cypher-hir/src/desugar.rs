//! HIR desugaring pass (spec 0001 §6.1).
//!
//! After AST→HIR lowering, this pass transforms syntactic-sugar constructs
//! into canonical HIR forms. The transformations are purely structural/
//! syntactic — no name resolution is performed here (that is deferred to
//! `cypher-sema`).
//!
//! # Desugaring rules
//!
//! | Source construct | Canonical HIR form |
//! |---|---|
//! | `MATCH (a {name: $n})` — shorthand property map in a pattern node | `MATCH (a) WHERE a.name = $n` appended after the MATCH clause |
//! | `[x IN xs WHERE p(x) \| e(x)]` — list comprehension | `Expr::ListComprehension { filter_var, iterable, filter, map_expr }` |
//! | `[x IN xs WHERE p(x)]` — filter-only comprehension | `Expr::ListComprehension` with `map_expr = Var(filter_var)` |
//! | `a { .name, .age, key: f(a) }` — map projection | `Expr::MapProjection { base, items }` |
//! | `(a)-->(:X)` in boolean context — pattern predicate | `Expr::PatternPredicate(Pattern { .. })` |
//! | `n["prop"]` subscript with string literal key | Normalized to `Expr::Prop { target, prop }` |
//! | `n.prop` already canonical | Unchanged — `Expr::Prop` |
//!
//! The entry point is [`desugar_statement`]. All transforms are in-place
//! on an owned [`Statement`].

use smol_str::SmolStr;

use cypher_syntax::TextRange;

use crate::{
    BinOp, Clause, Expr, HirId, MapProjectionItem, Pattern, PatternElement, PatternPart, Statement,
    VarId,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Apply all desugaring passes to `stmt` and return the transformed
/// statement. This is a pure function: the input is consumed and a new
/// (or mutated) statement is returned.
///
/// Passes, in order:
/// 1. `desugar_shorthand_props` — lift inline property maps on pattern
///    nodes into explicit `WHERE` predicates.
/// 2. `desugar_exprs` — recursively desugar expressions (subscript-to-
///    prop, list comprehension, map projection normalization).
pub fn desugar_statement(mut stmt: Statement) -> Statement {
    // Pass 1: shorthand property maps → WHERE predicates.
    desugar_shorthand_props(&mut stmt);
    // Pass 2: expression-level desugaring.
    desugar_exprs_in_clauses(&mut stmt);
    stmt
}

// ---------------------------------------------------------------------------
// Pass 1 — shorthand property maps → WHERE predicates
// ---------------------------------------------------------------------------

/// For every `MATCH`/`OPTIONAL MATCH` clause, scan each pattern node that
/// carries an inline `props` expression. For each property `key: val`
/// pair in the map, emit a `WHERE a.key = val` predicate and strip the
/// inline map from the pattern element.
///
/// Example:
/// ```text
/// MATCH (a:Person {name: $n})
/// ```
/// becomes:
/// ```text
/// MATCH (a:Person)  +  [WHERE appended:]  WHERE a.name = $n
/// ```
///
/// Multiple properties produce `AND`-chained predicates.
fn desugar_shorthand_props(stmt: &mut Statement) {
    // Collect (insert_after_index, where_clause) pairs to splice in.
    let mut inserts: Vec<(usize, Clause)> = Vec::new();

    for (clause_idx, clause) in stmt.clauses.iter_mut().enumerate() {
        let (Clause::Match { pattern, .. }
        | Clause::Create { pattern, .. }
        | Clause::Merge { pattern, .. }) = clause
        else {
            continue;
        };

        let mut predicates: Vec<Expr> = Vec::new();

        for part in &mut pattern.parts {
            for elem in &mut part.elements {
                if let PatternElement::Node {
                    bind,
                    props,
                    span: _,
                    ..
                } = elem
                    && let Some(prop_map) = props.take()
                {
                    // Extract pairs from the Expr::Map.
                    if let Expr::Map(pairs) = prop_map {
                        for (key, val) in pairs {
                            // Build `a.key = val` predicate if we have a binding.
                            let pred = if let Some(var_id) = bind {
                                Expr::BinOp {
                                    op: BinOp::Eq,
                                    lhs: Box::new(Expr::Prop {
                                        target: Box::new(Expr::Var(*var_id)),
                                        prop: key.clone(),
                                    }),
                                    rhs: Box::new(val),
                                }
                            } else {
                                // Anonymous node — can't generate a name-keyed predicate.
                                // Re-wrap as a single-key map with a dummy target.
                                Expr::Map(vec![(key, val)])
                            };
                            predicates.push(pred);
                        }
                    } else {
                        // Non-map prop expression — keep it in place (already lowered
                        // from parameter, etc.). The spec only defines desugaring for
                        // literal map shapes.
                        *props = Some(prop_map);
                    }
                }
            }
        }

        if !predicates.is_empty() {
            // Fold predicates into a single AND chain.
            let combined = predicates
                .into_iter()
                .reduce(|acc, p| Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(acc),
                    rhs: Box::new(p),
                })
                .unwrap(); // safe: non-empty

            // Use HirId::DUMMY for synthetic clauses — a real id would
            // require a syntax node (which we don't have for generated nodes).
            // The node_map simply has no entry for HirId::DUMMY, which is
            // the documented sentinel.
            let span = TextRange::default();

            inserts.push((
                clause_idx,
                Clause::Where {
                    id: HirId::DUMMY,
                    predicate: combined,
                    span,
                },
            ));
        }
    }

    // Splice WHERE clauses in reverse order so indices stay valid.
    for (after_idx, where_clause) in inserts.into_iter().rev() {
        stmt.clauses.insert(after_idx + 1, where_clause);
    }
}

// ---------------------------------------------------------------------------
// Pass 2 — expression-level desugaring
// ---------------------------------------------------------------------------

/// Walk every expression in every clause and apply:
/// - `n["prop"]` → `n.prop` when the subscript key is a string literal.
/// - [`Expr::ListComprehension`] — already in canonical form from lowering;
///   sub-expressions are recursively desugared.
/// - [`Expr::MapProjection`] — already in canonical form; sub-expressions
///   are recursively desugared.
fn desugar_exprs_in_clauses(stmt: &mut Statement) {
    for clause in &mut stmt.clauses {
        desugar_exprs_in_clause(clause);
    }
}

fn desugar_exprs_in_clause(clause: &mut Clause) {
    match clause {
        Clause::Where { predicate, .. } => {
            desugar_expr(predicate);
        }
        Clause::Return { projections, .. } | Clause::With { projections, .. } => {
            for p in projections {
                desugar_expr(&mut p.expr);
            }
        }
        Clause::Unwind { list, .. } => {
            desugar_expr(list);
        }
        Clause::Set { items, .. } => {
            for item in items {
                use crate::SetItem;
                match item {
                    SetItem::Property { target, value, .. } => {
                        desugar_expr(target);
                        desugar_expr(value);
                    }
                    SetItem::AssignMap { map, .. } => {
                        desugar_expr(map);
                    }
                    SetItem::Labels { .. } => {}
                }
            }
        }
        Clause::Delete { targets, .. } => {
            for t in targets {
                desugar_expr(t);
            }
        }
        Clause::Call { args, .. } => {
            for a in args {
                desugar_expr(a);
            }
        }
        // MATCH/CREATE/MERGE patterns — props were handled in pass 1; no more
        // inline prop maps remain. But expressions inside projections etc. may
        // exist on CREATE patterns in future; skip for now.
        Clause::Match { .. }
        | Clause::Create { .. }
        | Clause::Merge { .. }
        | Clause::Remove { .. } => {}
    }
}

/// Recursively desugar a single expression node in place.
fn desugar_expr(expr: &mut Expr) {
    match expr {
        // ----------------------------------------------------------------
        // `n["prop"]` → `n.prop` when index is a string literal.
        // ----------------------------------------------------------------
        Expr::Index { target, index } => {
            desugar_expr(target);
            desugar_expr(index);
            if let Expr::String(key) = index.as_ref() {
                let key = key.clone();
                // Replace self with Prop.
                let inner_target = std::mem::replace(target.as_mut(), Expr::Null);
                *expr = Expr::Prop {
                    target: Box::new(inner_target),
                    prop: key,
                };
            }
        }
        // ----------------------------------------------------------------
        // Recursive cases — descend into sub-expressions.
        // ----------------------------------------------------------------
        Expr::Prop { target, .. } => {
            desugar_expr(target);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            desugar_expr(lhs);
            desugar_expr(rhs);
        }
        Expr::UnaryOp { operand, .. } | Expr::IsNull { operand, .. } => {
            desugar_expr(operand);
        }
        Expr::Call { args, .. } => {
            for a in args {
                desugar_expr(a);
            }
        }
        Expr::List(items) => {
            for i in items {
                desugar_expr(i);
            }
        }
        Expr::Map(pairs) => {
            for (_, v) in pairs {
                desugar_expr(v);
            }
        }
        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            if let Some(s) = scrutinee {
                desugar_expr(s);
            }
            for (cond, then) in arms {
                desugar_expr(cond);
                desugar_expr(then);
            }
            if let Some(o) = otherwise {
                desugar_expr(o);
            }
        }
        Expr::InList { operand, list } => {
            desugar_expr(operand);
            desugar_expr(list);
        }
        // `Expr::Slice` has no desugar rewrite yet — recurse into bounds
        // so any nested `["prop"]` → `.prop` normalisation still fires.
        Expr::Slice { target, start, end } => {
            desugar_expr(target);
            if let Some(s) = start {
                desugar_expr(s);
            }
            if let Some(e) = end {
                desugar_expr(e);
            }
        }
        // ----------------------------------------------------------------
        // Desugared list comprehension — recurse into sub-expressions.
        // ----------------------------------------------------------------
        Expr::ListComprehension {
            iterable,
            filter,
            map_expr,
            ..
        } => {
            desugar_expr(iterable);
            if let Some(f) = filter {
                desugar_expr(f);
            }
            desugar_expr(map_expr);
        }
        // List predicates survive desugaring as-is — they have no
        // canonical rewrite in terms of simpler HIR forms (unlike
        // `MapProjection` which expands to explicit `Expr::Map`).
        // Recurse into iterable + predicate for nested normalisation.
        Expr::ListPredicate {
            iterable,
            predicate,
            ..
        } => {
            desugar_expr(iterable);
            if let Some(p) = predicate {
                desugar_expr(p);
            }
        }
        // ----------------------------------------------------------------
        // Desugared map projection — recurse into item expressions.
        // ----------------------------------------------------------------
        Expr::MapProjection { base, items } => {
            desugar_expr(base);
            for item in items {
                match item {
                    MapProjectionItem::Computed { value, .. }
                    | MapProjectionItem::Aliased { value, .. } => {
                        desugar_expr(value);
                    }
                    MapProjectionItem::PropCopy { .. } | MapProjectionItem::VarShorthand { .. } => {
                    }
                }
            }
        }
        // Leaves — nothing to do.
        Expr::Null
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::Param(_)
        | Expr::PatternPredicate(_)
        | Expr::Unresolved(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers — build synthetic HIR without a syntax node
// ---------------------------------------------------------------------------

/// Build an `Expr::Prop` from a `VarId` and a property key.
pub fn prop_of_var(var: VarId, prop: impl Into<SmolStr>) -> Expr {
    Expr::Prop {
        target: Box::new(Expr::Var(var)),
        prop: prop.into(),
    }
}

/// Build a binary-equality predicate `lhs = rhs`.
pub fn eq_pred(lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// Build an `AND`-chain predicate from a slice of predicates.
/// Returns `Expr::Null` for an empty slice (should not happen in practice).
pub fn and_chain(mut preds: Vec<Expr>) -> Expr {
    if preds.is_empty() {
        return Expr::Null;
    }
    let first = preds.remove(0);
    preds.into_iter().fold(first, |acc, p| Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(acc),
        rhs: Box::new(p),
    })
}

/// Build a canonical list comprehension expression directly (used in
/// tests and by passes that construct HIR synthetically).
pub fn list_comprehension(
    filter_var: VarId,
    iterable: Expr,
    filter: Option<Expr>,
    map_expr: Expr,
) -> Expr {
    Expr::ListComprehension {
        filter_var,
        iterable: Box::new(iterable),
        filter: filter.map(Box::new),
        map_expr: Box::new(map_expr),
    }
}

/// Build a canonical map projection expression directly.
pub fn map_projection(base: Expr, items: Vec<MapProjectionItem>) -> Expr {
    Expr::MapProjection {
        base: Box::new(base),
        items,
    }
}

/// Build a pattern predicate expression from a pattern.
pub fn pattern_predicate(parts: Vec<PatternPart>) -> Expr {
    Expr::PatternPredicate(Pattern { parts })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Binding, Direction, PatternElement, Projection, RelLength, VarId, VarKind,
        lower::lower_statement,
    };
    use cypher_syntax::parse;
    use smol_str::SmolStr;

    // --- Test helpers -------------------------------------------------------

    fn render_stmt(stmt: &Statement) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        writeln!(out, "clauses: {}", stmt.clauses.len()).unwrap();
        writeln!(out, "bindings: {}", stmt.bindings.len()).unwrap();
        for (i, clause) in stmt.clauses.iter().enumerate() {
            writeln!(out, "clause[{i}]: {}", clause_tag(clause)).unwrap();
        }
        for (_, b) in &stmt.bindings {
            writeln!(out, "binding: {} ({:?})", b.name, b.kind).unwrap();
        }
        out
    }

    fn render_expr(expr: &Expr) -> String {
        match expr {
            Expr::Null => "Null".to_string(),
            Expr::Bool(b) => format!("Bool({b})"),
            Expr::Int(i) => format!("Int({i})"),
            Expr::Float(f) => format!("Float({f})"),
            Expr::String(s) => format!("String({s:?})"),
            Expr::Var(v) => format!("Var({})", v.0),
            Expr::Param(p) => format!("Param({p})"),
            Expr::Prop { target, prop } => format!("Prop({}.{})", render_expr(target), prop),
            Expr::Index { target, index } => {
                format!("Index({}[{}])", render_expr(target), render_expr(index))
            }
            Expr::Slice { target, start, end } => {
                let s = start.as_ref().map_or_else(String::new, |e| render_expr(e));
                let e = end.as_ref().map_or_else(String::new, |e| render_expr(e));
                format!("Slice({}[{s}..{e}])", render_expr(target))
            }
            Expr::List(items) => {
                let inner: Vec<_> = items.iter().map(render_expr).collect();
                format!("List([{}])", inner.join(", "))
            }
            Expr::Map(pairs) => {
                let inner: Vec<_> = pairs
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", render_expr(v)))
                    .collect();
                format!("Map({{{}}}", inner.join(", "))
            }
            Expr::Call {
                name,
                args,
                distinct,
            } => {
                let inner: Vec<_> = args.iter().map(render_expr).collect();
                let d = if *distinct { "DISTINCT " } else { "" };
                format!("Call({name}({d}{}))", inner.join(", "))
            }
            Expr::BinOp { op, lhs, rhs } => {
                format!(
                    "BinOp({:?}, {}, {})",
                    op,
                    render_expr(lhs),
                    render_expr(rhs)
                )
            }
            Expr::UnaryOp { op, operand } => {
                format!("UnaryOp({op:?}, {})", render_expr(operand))
            }
            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => {
                let s = scrutinee
                    .as_ref()
                    .map_or_else(|| "None".to_string(), |e| render_expr(e));
                format!(
                    "Case(scrutinee={s}, arms={}, else={})",
                    arms.len(),
                    otherwise
                        .as_ref()
                        .map_or_else(|| "None".to_string(), |e| render_expr(e))
                )
            }
            Expr::IsNull { operand, negated } => {
                format!("IsNull({}, negated={negated})", render_expr(operand))
            }
            Expr::InList { operand, list } => {
                format!("InList({} IN {})", render_expr(operand), render_expr(list))
            }
            Expr::PatternPredicate(pat) => {
                format!("PatternPredicate(parts={})", pat.parts.len())
            }
            Expr::ListComprehension {
                filter_var,
                iterable,
                filter,
                map_expr,
            } => {
                let f = filter
                    .as_ref()
                    .map_or_else(|| "None".to_string(), |e| render_expr(e));
                format!(
                    "ListComp(var={}, iterable={}, filter={f}, map={})",
                    filter_var.0,
                    render_expr(iterable),
                    render_expr(map_expr)
                )
            }
            Expr::ListPredicate {
                kind,
                var,
                iterable,
                predicate,
            } => {
                let p = predicate
                    .as_ref()
                    .map_or_else(|| "None".to_string(), |e| render_expr(e));
                format!(
                    "ListPred({kind:?}, var={}, iterable={}, pred={p})",
                    var.0,
                    render_expr(iterable)
                )
            }
            Expr::MapProjection { base, items } => {
                let rendered: Vec<_> = items.iter().map(render_proj_item).collect();
                format!("MapProj({}, [{}])", render_expr(base), rendered.join(", "))
            }
            Expr::Unresolved(n) => format!("Unresolved({n})"),
        }
    }

    fn render_proj_item(item: &MapProjectionItem) -> String {
        match item {
            MapProjectionItem::PropCopy { prop } => format!(".{prop}"),
            MapProjectionItem::Computed { key, value } => {
                format!("{key}: {}", render_expr(value))
            }
            MapProjectionItem::VarShorthand { var, name } => {
                format!("${name}({})", var.0)
            }
            MapProjectionItem::Aliased { key, value } => {
                format!("{key} AS {}", render_expr(value))
            }
        }
    }

    fn clause_tag(c: &Clause) -> String {
        match c {
            Clause::Match { optional, .. } => {
                if *optional {
                    "OptionalMatch".to_string()
                } else {
                    "Match".to_string()
                }
            }
            Clause::Where { predicate, .. } => format!("Where({})", render_expr(predicate)),
            Clause::Return {
                projections,
                distinct,
                ..
            } => {
                let exprs: Vec<_> = projections.iter().map(|p| render_expr(&p.expr)).collect();
                format!("Return(distinct={distinct}, [{}])", exprs.join(", "))
            }
            Clause::With { projections, .. } => {
                let exprs: Vec<_> = projections.iter().map(|p| render_expr(&p.expr)).collect();
                format!("With([{}])", exprs.join(", "))
            }
            Clause::Unwind { list, .. } => format!("Unwind({})", render_expr(list)),
            Clause::Create { .. } => "Create".to_string(),
            Clause::Merge { .. } => "Merge".to_string(),
            Clause::Set { .. } => "Set".to_string(),
            Clause::Remove { .. } => "Remove".to_string(),
            Clause::Delete { .. } => "Delete".to_string(),
            Clause::Call { procedure, .. } => format!("Call({procedure})"),
        }
    }

    // --- Synthetic HIR helpers ----------------------------------------------

    fn make_stmt_with_match_node_props() -> Statement {
        // Synthetic: MATCH (a:Person {name: "Alice"}) RETURN a
        // We build this directly in HIR rather than parsing, because the
        // parser doesn't yet emit inline property maps on pattern nodes
        // with the desugaring syntax.
        let src = parse("MATCH (a) RETURN a");
        let root = src.syntax();
        let span = root.text_range();
        let mut stmt = Statement::new(span);

        let var_id = VarId(0);
        stmt.bindings.insert(
            var_id,
            Binding {
                id: var_id,
                name: SmolStr::new("a"),
                kind: VarKind::Node,
                defined_at: span,
            },
        );

        let node_id = stmt.alloc_id(root.clone());
        let match_id = stmt.alloc_id(root.clone());
        let return_id = stmt.alloc_id(root.clone());

        stmt.clauses.push(Clause::Match {
            id: match_id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: node_id,
                        bind: Some(var_id),
                        labels: vec![SmolStr::new("Person")],
                        props: Some(Expr::Map(vec![
                            (SmolStr::new("name"), Expr::String(SmolStr::new("Alice"))),
                            (SmolStr::new("age"), Expr::Int(30)),
                        ])),
                        span,
                    }],
                }],
            },
            span,
        });
        stmt.clauses.push(Clause::Return {
            id: return_id,
            projections: vec![Projection {
                expr: Expr::Var(var_id),
                alias: None,
                span,
            }],
            distinct: false,
            span,
        });
        stmt
    }

    // --- Snapshot tests: shorthand property map desugaring ------------------

    #[test]
    fn snap_desugar_shorthand_single_prop() {
        let stmt = make_stmt_with_match_node_props();
        let desugared = desugar_statement(stmt);
        insta::assert_snapshot!("desugar_shorthand_single_prop", render_stmt(&desugared));
    }

    #[test]
    fn snap_desugar_shorthand_prop_where_content() {
        let stmt = make_stmt_with_match_node_props();
        let desugared = desugar_statement(stmt);
        // The second clause should be a Where produced by desugaring.
        let where_clause_rendered = clause_tag(&desugared.clauses[1]);
        insta::assert_snapshot!(
            "desugar_shorthand_prop_where_content",
            where_clause_rendered
        );
    }

    #[test]
    fn snap_desugar_shorthand_no_props_unchanged() {
        // MATCH (a) RETURN a — no props, no WHERE should be inserted.
        let stmt = lower_statement("MATCH (a) RETURN a");
        let before_count = stmt.clauses.len();
        let desugared = desugar_statement(stmt);
        insta::assert_snapshot!(
            "desugar_shorthand_no_props_unchanged",
            format!("before: {before_count}, after: {}", desugared.clauses.len())
        );
    }

    // --- Snapshot tests: subscript-to-prop normalization --------------------

    #[test]
    fn snap_desugar_subscript_string_to_prop() {
        // n["name"] → n.name
        let mut expr = Expr::Index {
            target: Box::new(Expr::Var(VarId(0))),
            index: Box::new(Expr::String(SmolStr::new("name"))),
        };
        desugar_expr(&mut expr);
        insta::assert_snapshot!("desugar_subscript_string_to_prop", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_subscript_int_unchanged() {
        // n[0] stays as Index (integer key)
        let mut expr = Expr::Index {
            target: Box::new(Expr::Var(VarId(0))),
            index: Box::new(Expr::Int(0)),
        };
        desugar_expr(&mut expr);
        insta::assert_snapshot!("desugar_subscript_int_unchanged", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_subscript_nested() {
        // a["x"]["y"] — both normalized
        let mut expr = Expr::Index {
            target: Box::new(Expr::Index {
                target: Box::new(Expr::Var(VarId(0))),
                index: Box::new(Expr::String(SmolStr::new("x"))),
            }),
            index: Box::new(Expr::String(SmolStr::new("y"))),
        };
        desugar_expr(&mut expr);
        insta::assert_snapshot!("desugar_subscript_nested", render_expr(&expr));
    }

    // --- Snapshot tests: slice desugar (cy-7s6.1) ---------------------------

    #[test]
    fn snap_desugar_slice_bounded() {
        // xs[0..3] — both bounds present, no rewrite.
        let mut expr = Expr::Slice {
            target: Box::new(Expr::Var(VarId(0))),
            start: Some(Box::new(Expr::Int(0))),
            end: Some(Box::new(Expr::Int(3))),
        };
        desugar_expr(&mut expr);
        insta::assert_snapshot!("desugar_slice_bounded", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_slice_elided_start() {
        // xs[..3] — elided start stays `None`.
        let mut expr = Expr::Slice {
            target: Box::new(Expr::Var(VarId(0))),
            start: None,
            end: Some(Box::new(Expr::Int(3))),
        };
        desugar_expr(&mut expr);
        insta::assert_snapshot!("desugar_slice_elided_start", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_slice_elided_end() {
        // xs[0..] — elided end stays `None`.
        let mut expr = Expr::Slice {
            target: Box::new(Expr::Var(VarId(0))),
            start: Some(Box::new(Expr::Int(0))),
            end: None,
        };
        desugar_expr(&mut expr);
        insta::assert_snapshot!("desugar_slice_elided_end", render_expr(&expr));
    }

    // --- Snapshot tests: list comprehension ---------------------------------

    #[test]
    fn snap_desugar_list_comp_filter_and_map() {
        // [x IN xs WHERE x > 0 | x * 2]
        let filter_var = VarId(1);
        let expr = list_comprehension(
            filter_var,
            Expr::Var(VarId(0)), // xs
            Some(Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Var(filter_var)),
                rhs: Box::new(Expr::Int(0)),
            }),
            Expr::BinOp {
                op: BinOp::Mul,
                lhs: Box::new(Expr::Var(filter_var)),
                rhs: Box::new(Expr::Int(2)),
            },
        );
        insta::assert_snapshot!("desugar_list_comp_filter_and_map", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_list_comp_filter_only() {
        // [x IN xs WHERE x > 0] — map_expr = Var(filter_var)
        let filter_var = VarId(1);
        let expr = list_comprehension(
            filter_var,
            Expr::Var(VarId(0)),
            Some(Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Var(filter_var)),
                rhs: Box::new(Expr::Int(0)),
            }),
            Expr::Var(filter_var), // identity map
        );
        insta::assert_snapshot!("desugar_list_comp_filter_only", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_list_comp_map_only() {
        // [x IN xs | x.name] — no filter
        let filter_var = VarId(1);
        let expr = list_comprehension(
            filter_var,
            Expr::Var(VarId(0)),
            None,
            Expr::Prop {
                target: Box::new(Expr::Var(filter_var)),
                prop: SmolStr::new("name"),
            },
        );
        insta::assert_snapshot!("desugar_list_comp_map_only", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_list_comp_identity() {
        // [x IN xs] — no filter, no map = identity collect
        let filter_var = VarId(1);
        let expr = list_comprehension(filter_var, Expr::Var(VarId(0)), None, Expr::Var(filter_var));
        insta::assert_snapshot!("desugar_list_comp_identity", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_list_comp_nested_expr() {
        // [x IN xs WHERE x.active | x.name + "_suffix"]
        let filter_var = VarId(1);
        let expr = list_comprehension(
            filter_var,
            Expr::Var(VarId(0)),
            Some(Expr::Prop {
                target: Box::new(Expr::Var(filter_var)),
                prop: SmolStr::new("active"),
            }),
            Expr::BinOp {
                op: BinOp::Concat,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(filter_var)),
                    prop: SmolStr::new("name"),
                }),
                rhs: Box::new(Expr::String(SmolStr::new("_suffix"))),
            },
        );
        insta::assert_snapshot!("desugar_list_comp_nested_expr", render_expr(&expr));
    }

    // --- Snapshot tests: map projection -------------------------------------

    #[test]
    fn snap_desugar_map_proj_prop_copies() {
        // a { .name, .age }
        let expr = map_projection(
            Expr::Var(VarId(0)),
            vec![
                MapProjectionItem::PropCopy {
                    prop: SmolStr::new("name"),
                },
                MapProjectionItem::PropCopy {
                    prop: SmolStr::new("age"),
                },
            ],
        );
        insta::assert_snapshot!("desugar_map_proj_prop_copies", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_map_proj_computed() {
        // a { computed: f(a) }
        let expr = map_projection(
            Expr::Var(VarId(0)),
            vec![MapProjectionItem::Computed {
                key: SmolStr::new("computed"),
                value: Expr::Call {
                    name: SmolStr::new("f"),
                    args: vec![Expr::Var(VarId(0))],
                    distinct: false,
                },
            }],
        );
        insta::assert_snapshot!("desugar_map_proj_computed", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_map_proj_mixed() {
        // a { .name, .age, computed: toUpper(a.name) }
        let expr = map_projection(
            Expr::Var(VarId(0)),
            vec![
                MapProjectionItem::PropCopy {
                    prop: SmolStr::new("name"),
                },
                MapProjectionItem::PropCopy {
                    prop: SmolStr::new("age"),
                },
                MapProjectionItem::Computed {
                    key: SmolStr::new("upper_name"),
                    value: Expr::Call {
                        name: SmolStr::new("toUpper"),
                        args: vec![Expr::Prop {
                            target: Box::new(Expr::Var(VarId(0))),
                            prop: SmolStr::new("name"),
                        }],
                        distinct: false,
                    },
                },
            ],
        );
        insta::assert_snapshot!("desugar_map_proj_mixed", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_map_proj_var_shorthand() {
        // a { b } — include b as shorthand
        let expr = map_projection(
            Expr::Var(VarId(0)),
            vec![MapProjectionItem::VarShorthand {
                var: VarId(1),
                name: SmolStr::new("b"),
            }],
        );
        insta::assert_snapshot!("desugar_map_proj_var_shorthand", render_expr(&expr));
    }

    // --- Snapshot tests: pattern predicate ----------------------------------

    #[test]
    fn snap_desugar_pattern_predicate_empty() {
        // Pattern predicate with no elements (stub from parser).
        let expr = pattern_predicate(vec![]);
        insta::assert_snapshot!("desugar_pattern_predicate_empty", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_pattern_predicate_with_pattern() {
        // (a)-[:R]->(b) in boolean context
        let span = TextRange::default();
        let expr = pattern_predicate(vec![PatternPart {
            named_as: None,
            elements: vec![
                PatternElement::Node {
                    id: HirId(1),
                    bind: Some(VarId(0)),
                    labels: vec![],
                    props: None,
                    span,
                },
                PatternElement::Rel {
                    id: HirId(2),
                    bind: None,
                    types: vec![SmolStr::new("R")],
                    direction: Direction::Outgoing,
                    length: RelLength::Single,
                    props: None,
                    span,
                },
                PatternElement::Node {
                    id: HirId(3),
                    bind: Some(VarId(1)),
                    labels: vec![],
                    props: None,
                    span,
                },
            ],
        }]);
        insta::assert_snapshot!("desugar_pattern_predicate_with_pattern", render_expr(&expr));
    }

    // --- Snapshot tests: and_chain / helpers --------------------------------

    #[test]
    fn snap_desugar_and_chain_two() {
        let preds = vec![
            Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Var(VarId(0))),
                rhs: Box::new(Expr::Int(0)),
            },
            Expr::BinOp {
                op: BinOp::Lt,
                lhs: Box::new(Expr::Var(VarId(0))),
                rhs: Box::new(Expr::Int(100)),
            },
        ];
        let expr = and_chain(preds);
        insta::assert_snapshot!("desugar_and_chain_two", render_expr(&expr));
    }

    #[test]
    fn snap_desugar_and_chain_single() {
        let preds = vec![Expr::Bool(true)];
        let expr = and_chain(preds);
        insta::assert_snapshot!("desugar_and_chain_single", render_expr(&expr));
    }

    // --- Round-trip: desugar is idempotent on already-desugared HIR ---------

    #[test]
    fn desugar_is_idempotent_on_simple_match() {
        let stmt = lower_statement("MATCH (n) RETURN n");
        let once = desugar_statement(stmt.clone());
        let twice = desugar_statement(once.clone());
        // Clause count should be identical on second pass.
        assert_eq!(once.clauses.len(), twice.clauses.len());
    }

    // --- EXISTS(<pattern>) end-to-end lowering (cy-lve, spec §6.1) ----------

    /// `RETURN EXISTS((a)-->(b))` must parse, lower, and land as an
    /// `Expr::PatternPredicate` in the Return projection.
    #[test]
    fn lower_exists_pattern_predicate_in_return() {
        let stmt = lower_statement("RETURN EXISTS((a)-->(b))");
        assert_eq!(stmt.clauses.len(), 1);
        let Clause::Return { projections, .. } = &stmt.clauses[0] else {
            panic!("expected Return clause, got {:?}", stmt.clauses[0]);
        };
        assert_eq!(projections.len(), 1);
        let Expr::PatternPredicate(pat) = &projections[0].expr else {
            panic!(
                "expected Expr::PatternPredicate, got {:?}",
                projections[0].expr
            );
        };
        assert_eq!(pat.parts.len(), 1);
        assert_eq!(pat.parts[0].elements.len(), 3);
    }

    /// `MATCH (a) WHERE EXISTS((a)-->(:Movie)) RETURN a` — WHERE clause
    /// carries the pattern predicate.
    #[test]
    fn lower_exists_pattern_predicate_in_where() {
        let stmt = lower_statement("MATCH (a) WHERE EXISTS((a)-->(:Movie)) RETURN a");
        // Expect: Match, Where (carries the pattern predicate), Return.
        let where_clause = stmt
            .clauses
            .iter()
            .find(|c| matches!(c, Clause::Where { .. }))
            .expect("expected a Where clause");
        let Clause::Where { predicate, .. } = where_clause else {
            unreachable!();
        };
        let Expr::PatternPredicate(pat) = predicate else {
            panic!("expected WHERE predicate to be Expr::PatternPredicate, got {predicate:?}");
        };
        assert_eq!(pat.parts.len(), 1);
    }
}
