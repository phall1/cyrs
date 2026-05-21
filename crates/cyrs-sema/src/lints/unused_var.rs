//! L1 — unused pattern variable ([`W6011`](cyrs_diag::DiagCode::W6011)).
//!
//! Flags a variable that is *bound* by a pattern (a node, relationship,
//! or path binder in `MATCH` / `OPTIONAL MATCH`) but never *referenced*
//! anywhere downstream — not in a `WHERE` predicate, not in a `RETURN`
//! projection, not in a `SET` / `WITH` expression.
//!
//! Such a variable is dead weight: the pattern would behave identically
//! with the binder removed (`MATCH (unused:Person)` → `MATCH (:Person)`).
//! The bead names this "unused variable in WHERE"; in practice the most
//! common shape is a binder the author meant to filter on but forgot.
//!
//! # What counts as a reference
//!
//! Any [`Expr::Var`] occurrence reachable from a clause body, plus the
//! `target` of a label-set [`SetItem`] / [`RemoveItem`] (which name a
//! variable directly rather than via an `Expr`). The binder site itself
//! does *not* count — a node/rel binder lives in the pattern's `bind`
//! field, not as an `Expr::Var`, so the walk never mistakes a definition
//! for a use.
//!
//! # Scope
//!
//! Only `MATCH` / `OPTIONAL MATCH` binders are linted. `CREATE` /
//! `MERGE` binders, `UNWIND ... AS v`, `WITH ... AS v`, and `CALL ...
//! YIELD v` are intentionally excluded: a write-clause binder or a
//! projection alias that is unused is a separate concern (`WITH` dead
//! projections already have [`W6001`](cyrs_diag::DiagCode::W6001)).

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::visit::{walk_remove_item, walk_set_item};
use cyrs_hir::{Clause, Expr, RemoveItem, SetItem, Statement, VarId, VarKind, Visitor, walk_expr};
use std::collections::BTreeSet;

/// Run L1 over `stmt`.
pub fn check(stmt: &Statement, sink: &mut DiagnosticsSink) {
    // 1. Every variable referenced anywhere in the statement.
    let mut used = UsedVars::default();
    used.visit_statement(stmt);

    // 2. Every variable that is *bound by a MATCH pattern at its
    //    defining occurrence*. A `bind` slot in a pattern element is the
    //    var's definition only when the binding's `defined_at` span
    //    falls inside that element — otherwise the element merely
    //    re-uses an earlier variable as a join point, which is a *use*,
    //    not a definition. We record:
    //      - `match_binders`  — vars defined by a MATCH pattern (the
    //        lint candidates), each with its defining span;
    //      - `rejoined`       — vars re-used as a join point (a use).
    let mut match_binders: Vec<VarId> = Vec::new();
    let mut rejoined: BTreeSet<VarId> = BTreeSet::new();
    for clause in &stmt.clauses {
        let Clause::Match { pattern, .. } = clause else {
            continue;
        };
        for part in &pattern.parts {
            // A named-path binder (`p = (a)-[]->(b)`) is a binder too;
            // a path variable is always introduced where it is written.
            if let Some(path_var) = part.named_as
                && !match_binders.contains(&path_var)
            {
                match_binders.push(path_var);
            }
            for elem in &part.elements {
                let Some(v) = elem_binder(elem) else {
                    continue;
                };
                let defined_here = stmt
                    .bindings
                    .get(&v)
                    .is_some_and(|b| elem.span().contains_range(b.defined_at));
                if defined_here {
                    if !match_binders.contains(&v) {
                        match_binders.push(v);
                    }
                } else {
                    rejoined.insert(v);
                }
            }
        }
    }

    // 3. A MATCH binder is unused when it is neither referenced in an
    //    expression nor re-used as a later join point.
    for var in match_binders {
        if !used.vars.contains(&var) && !rejoined.contains(&var) {
            emit(stmt, var, sink);
        }
    }
}

fn elem_binder(elem: &cyrs_hir::PatternElement) -> Option<VarId> {
    match elem {
        cyrs_hir::PatternElement::Node { bind, .. }
        | cyrs_hir::PatternElement::Rel { bind, .. } => *bind,
    }
}

fn emit(stmt: &Statement, var: VarId, sink: &mut DiagnosticsSink) {
    let Some(binding) = stmt.bindings.get(&var) else {
        return;
    };
    let kind = match binding.kind {
        VarKind::Node => "node",
        VarKind::Relationship => "relationship",
        VarKind::Path => "path",
        VarKind::Value => "value",
    };
    let name = &binding.name;
    sink.push(
        Diagnostic::warning(
            DiagCode::W6011,
            binding.defined_at,
            format!("{kind} variable `{name}` is bound but never used"),
        )
        .with_note(format!(
            "remove the unused binder — `{name}` is not referenced in any \
             WHERE, RETURN, or other downstream clause"
        )),
    );
}

/// Visitor collecting every *referenced* [`VarId`].
#[derive(Default)]
struct UsedVars {
    vars: BTreeSet<VarId>,
}

impl Visitor for UsedVars {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::Var(v) = e {
            self.vars.insert(*v);
        }
        walk_expr(self, e);
    }

    fn visit_set_item(&mut self, item: &SetItem) {
        // `SET n:Label` / `SET n += {...}` name a variable directly.
        match item {
            SetItem::Labels { target, .. } | SetItem::AssignMap { target, .. } => {
                self.vars.insert(*target);
            }
            SetItem::Property { .. } => {}
        }
        walk_set_item(self, item);
    }

    fn visit_remove_item(&mut self, item: &RemoveItem) {
        if let RemoveItem::Labels { target, .. } = item {
            self.vars.insert(*target);
        }
        walk_remove_item(self, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::test_support::run_one;

    #[test]
    fn snap_unused_node_var_fires() {
        // `m` is bound but never used anywhere.
        insta::assert_snapshot!(run_one(
            check,
            "MATCH (n:Person), (m:Company) WHERE n.age > 30 RETURN n",
        ));
    }

    #[test]
    fn snap_all_vars_used_clean() {
        insta::assert_snapshot!(run_one(check, "MATCH (n:Person) WHERE n.age > 30 RETURN n",));
    }

    #[test]
    fn unused_relationship_binder_fires() {
        let out = run_one(check, "MATCH (n)-[r:KNOWS]->(m) RETURN n, m");
        assert!(out.contains("W6011"), "{out}");
        assert!(out.contains("`r`"), "{out}");
    }

    #[test]
    fn binder_used_only_in_where_is_not_unused() {
        let out = run_one(check, "MATCH (n:Person) WHERE n.age > 1 RETURN 1");
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn variable_reused_as_join_point_is_not_unused() {
        // `n` is bound by the first MATCH and re-used as a join point in
        // the second pattern — that re-use is a use, and the var must
        // be flagged exactly zero times (no false positive, no
        // duplicate emission).
        let out = run_one(check, "MATCH (n:Person) MATCH (n)-[:KNOWS]->(m) RETURN m");
        assert!(
            !out.contains("`n`"),
            "join-point re-use of `n` must not be flagged: {out}"
        );
        // Only `m` is unused here (it is never returned-... actually
        // `m` IS returned) — so the query is fully clean.
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn each_unused_binder_is_reported_once() {
        // `a` re-used as a join point (a use); `b` and `r` unused. The
        // re-bound `a` must not produce a duplicate W6011.
        let out = run_one(check, "MATCH (a), (b) MATCH (a)-[r]->(c) RETURN c");
        assert_eq!(out.matches("W6011").count(), 2, "{out}");
        assert!(out.contains("`b`") && out.contains("`r`"), "{out}");
        assert!(!out.contains("`a`"), "{out}");
    }
}
