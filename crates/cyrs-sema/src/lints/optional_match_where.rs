//! L6 — `OPTIONAL MATCH` followed by a `WHERE` on the optional binding
//! ([`W6016`](cyrs_diag::DiagCode::W6016)).
//!
//! Flags an `OPTIONAL MATCH` immediately followed by a top-level `WHERE`
//! whose predicate constrains a variable the `OPTIONAL MATCH` itself
//! bound. This is almost always a bug: a `WHERE` after the optional
//! match runs *after* the optional join, so any row where the optional
//! pattern failed (and the binding is `NULL`) is discarded by the
//! predicate — turning the `OPTIONAL MATCH` back into a strict `MATCH`.
//!
//! The author who wrote `OPTIONAL MATCH` clearly wanted the "keep the
//! row even when the pattern misses" semantics; the trailing `WHERE`
//! silently defeats that. The fixes are either to move the predicate
//! *inside* the `OPTIONAL MATCH` (`OPTIONAL MATCH (n) WHERE ...` — a
//! pattern-scoped filter that does not drop rows) or, if a strict match
//! was intended, to drop the `OPTIONAL` keyword.
//!
//! # Detection
//!
//! The HIR lowers a clause-level `WHERE` to a standalone
//! [`Clause::Where`]. The lint walks consecutive clause pairs: an
//! `OPTIONAL MATCH` directly followed by a `Where` whose predicate
//! references at least one variable *newly bound by that `OPTIONAL
//! MATCH`* fires. A variable that the optional pattern merely re-uses
//! as a join point (it was introduced by an earlier `MATCH`) is *not*
//! a fresh optional binding — constraining it is not the bug, so it
//! does not trigger the lint. "Newly bound here" is decided by the
//! binding's `defined_at` span lying inside the `OPTIONAL MATCH`.

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::{Clause, Expr, Statement, VarId, Visitor, walk_expr};
use std::collections::BTreeSet;

/// Run L6 over `stmt`.
pub fn check(stmt: &Statement, sink: &mut DiagnosticsSink) {
    for pair in stmt.clauses.windows(2) {
        let [
            Clause::Match {
                optional: true,
                pattern,
                span: match_span,
                ..
            },
            Clause::Where {
                predicate,
                span: where_span,
                ..
            },
        ] = pair
        else {
            continue;
        };

        // Variables *newly* bound by this OPTIONAL MATCH. A variable
        // the pattern only re-uses as a join point (introduced by an
        // earlier MATCH) is excluded: its binding `defined_at` span
        // lies outside this clause.
        let mut optional_binders = BTreeSet::new();
        let mut consider = |bind: Option<VarId>| {
            if let Some(v) = bind
                && let Some(b) = stmt.bindings.get(&v)
                && match_span.contains_range(b.defined_at)
            {
                optional_binders.insert(v);
            }
        };
        for part in &pattern.parts {
            consider(part.named_as);
            for elem in &part.elements {
                match elem {
                    cyrs_hir::PatternElement::Node { bind, .. }
                    | cyrs_hir::PatternElement::Rel { bind, .. } => consider(*bind),
                }
            }
        }

        // Variables the WHERE predicate references.
        let mut refs = VarRefs::default();
        refs.visit_expr(predicate);

        let constrained: Vec<VarId> = refs.vars.intersection(&optional_binders).copied().collect();
        if constrained.is_empty() {
            continue;
        }

        let names: Vec<String> = constrained
            .iter()
            .map(|v| {
                stmt.bindings
                    .get(v)
                    .map_or_else(|| format!("#{}", v.0), |b| format!("`{}`", b.name))
            })
            .collect();
        sink.push(
            Diagnostic::warning(
                DiagCode::W6016,
                *where_span,
                format!(
                    "WHERE constrains the OPTIONAL MATCH binding {} — \
                     this discards unmatched rows, defeating OPTIONAL",
                    names.join(", ")
                ),
            )
            .with_label(*match_span, "OPTIONAL MATCH here")
            .with_note(
                "move the predicate into the OPTIONAL MATCH \
                 (`OPTIONAL MATCH (...) WHERE ...`) to keep unmatched rows, \
                 or drop OPTIONAL if a strict MATCH was intended",
            ),
        );
    }
}

#[derive(Default)]
struct VarRefs {
    vars: BTreeSet<VarId>,
}

impl Visitor for VarRefs {
    fn visit_expr(&mut self, e: &Expr) {
        if let Expr::Var(v) = e {
            self.vars.insert(*v);
        }
        walk_expr(self, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::test_support::run_one;

    #[test]
    fn snap_optional_match_where_fires() {
        insta::assert_snapshot!(run_one(
            check,
            "MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) WHERE m.age > 30 RETURN n, m",
        ));
    }

    #[test]
    fn snap_optional_match_no_where_clean() {
        insta::assert_snapshot!(run_one(
            check,
            "MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) RETURN n, m",
        ));
    }

    #[test]
    fn where_on_earlier_binding_not_flagged() {
        // The WHERE constrains `n` (bound by the strict MATCH), not the
        // optional binding `m` — no lint.
        let out = run_one(
            check,
            "MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) WHERE n.age > 30 RETURN n, m",
        );
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn strict_match_where_not_flagged() {
        let out = run_one(check, "MATCH (n:Person) WHERE n.age > 30 RETURN n");
        assert!(out.contains("diagnostics: 0"), "{out}");
    }
}
