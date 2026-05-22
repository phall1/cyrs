//! L4 — implicit cartesian product
//! ([`W6014`](cyrs_diag::DiagCode::W6014)).
//!
//! Flags two `MATCH` clauses that bind *disjoint* sets of variables with
//! no predicate joining them: the engine must form the full cross
//! product of the two row sets, which is rarely intended and scales
//! quadratically.
//!
//! # Conservative by design
//!
//! A precise cartesian-product analysis would track connectivity
//! through every clause (`WITH` re-projections, path variables shared
//! between parts, predicates buried in `OPTIONAL MATCH`, …). To stay
//! sound — never warn wrongly — this lint fires only when **all** of
//! the following hold:
//!
//! - the statement contains at least two plain `MATCH` clauses;
//! - no `WITH` clause appears (a `WITH` re-scopes variables, so
//!   "disjoint binders" stops being a reliable proxy for "unjoined");
//! - two `MATCH` clauses bind variable sets that share no variable;
//! - **and** no predicate anywhere — a `WHERE`, a `WITH ... WHERE`
//!   filter, or an inline pattern-element property expression —
//!   references a variable from *both* of those sets.
//!
//! The last condition is the join detector: a `WHERE a.id = b.id`
//! mentioning a variable from each component is exactly the joining
//! predicate that makes the product intentional, so the lint stays
//! silent. Anything more elaborate than this simple shape is left
//! un-flagged (spec 0003 §6: "sound conservative version").
//!
//! Only the first offending pair is reported, to keep the output terse.

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::{Clause, Expr, Statement, VarId, Visitor, walk_expr};
use std::collections::BTreeSet;

/// Run L4 over `stmt`.
pub fn check(stmt: &Statement, sink: &mut DiagnosticsSink) {
    // A `WITH` barrier re-scopes variables; bail out conservatively.
    if stmt
        .clauses
        .iter()
        .any(|c| matches!(c, Clause::With { .. }))
    {
        return;
    }

    // One bound-variable set per plain MATCH clause, keeping the span.
    let matches: Vec<(BTreeSet<VarId>, cyrs_hir::HirSpan)> = stmt
        .clauses
        .iter()
        .filter_map(|c| match c {
            Clause::Match {
                optional: false,
                pattern,
                span,
                ..
            } => {
                let mut vars = BTreeSet::new();
                collect_pattern_binders(pattern, &mut vars);
                (!vars.is_empty()).then_some((vars, *span))
            }
            _ => None,
        })
        .collect();

    if matches.len() < 2 {
        return;
    }

    // Every variable pair (a, b) joined by some predicate in the query.
    let joins = collect_joined_pairs(stmt);

    for i in 0..matches.len() {
        for j in (i + 1)..matches.len() {
            let (a, span_a) = &matches[i];
            let (b, span_b) = &matches[j];
            // Share a binder ⇒ already connected.
            if a.intersection(b).next().is_some() {
                continue;
            }
            // A predicate references a variable from each ⇒ joined.
            let joined = joins
                .iter()
                .any(|(x, y)| (a.contains(x) && b.contains(y)) || (a.contains(y) && b.contains(x)));
            if joined {
                continue;
            }
            sink.push(
                Diagnostic::warning(
                    DiagCode::W6014,
                    *span_b,
                    "implicit cartesian product — this MATCH shares no variable \
                     or join predicate with an earlier MATCH",
                )
                .with_label(*span_a, "disjoint from this MATCH")
                .with_note(
                    "add a relationship pattern or a WHERE predicate relating \
                     the two MATCH clauses, or merge them into one pattern",
                ),
            );
            return;
        }
    }
}

fn collect_pattern_binders(pattern: &cyrs_hir::Pattern, out: &mut BTreeSet<VarId>) {
    for part in &pattern.parts {
        if let Some(p) = part.named_as {
            out.insert(p);
        }
        for elem in &part.elements {
            match elem {
                cyrs_hir::PatternElement::Node { bind, .. }
                | cyrs_hir::PatternElement::Rel { bind, .. } => {
                    if let Some(b) = bind {
                        out.insert(*b);
                    }
                }
            }
        }
    }
}

/// For every predicate expression in the statement, record each
/// unordered pair of distinct variables it co-references. If a `WHERE`
/// names both `a` and `b`, the pair `(a, b)` is a join candidate.
fn collect_joined_pairs(stmt: &Statement) -> BTreeSet<(VarId, VarId)> {
    let mut pairs = BTreeSet::new();
    for clause in &stmt.clauses {
        match clause {
            Clause::Where { predicate, .. } => add_pairs_from_expr(predicate, &mut pairs),
            // Inline property maps on pattern elements can also relate
            // two binders (rare, but cheap to honour).
            Clause::Match { pattern, .. } => {
                for part in &pattern.parts {
                    for elem in &part.elements {
                        if let cyrs_hir::PatternElement::Node { props: Some(e), .. }
                        | cyrs_hir::PatternElement::Rel { props: Some(e), .. } = elem
                        {
                            add_pairs_from_expr(e, &mut pairs);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    pairs
}

fn add_pairs_from_expr(expr: &Expr, pairs: &mut BTreeSet<(VarId, VarId)>) {
    let mut collector = VarRefs::default();
    collector.visit_expr(expr);
    let vars: Vec<VarId> = collector.vars.into_iter().collect();
    for i in 0..vars.len() {
        for j in (i + 1)..vars.len() {
            pairs.insert((vars[i], vars[j]));
        }
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
    fn snap_cartesian_product_fires() {
        insta::assert_snapshot!(run_one(
            check,
            "MATCH (a:Person) MATCH (b:Company) RETURN a, b",
        ));
    }

    #[test]
    fn snap_joined_matches_clean() {
        // `WHERE a.id = b.id` joins the two components.
        insta::assert_snapshot!(run_one(
            check,
            "MATCH (a:Person) MATCH (b:Company) WHERE a.id = b.id RETURN a, b",
        ));
    }

    #[test]
    fn shared_variable_not_cartesian() {
        let out = run_one(check, "MATCH (a:Person) MATCH (a)-[:KNOWS]->(b) RETURN b");
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn single_match_clean() {
        let out = run_one(check, "MATCH (a:Person), (b:Company) RETURN a, b");
        // Two parts in ONE MATCH is a cartesian product too, but this
        // conservative lint only compares separate MATCH clauses.
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn with_barrier_suppresses_lint() {
        let out = run_one(
            check,
            "MATCH (a:Person) WITH a MATCH (b:Company) RETURN a, b",
        );
        assert!(out.contains("diagnostics: 0"), "{out}");
    }
}
