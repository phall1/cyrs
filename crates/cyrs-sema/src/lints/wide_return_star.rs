//! L5 — wide `RETURN *` ([`W6015`](cyrs_diag::DiagCode::W6015)).
//!
//! Flags `RETURN *` (the wildcard projection) in a statement that binds
//! more than a configurable number of variables. `RETURN *` expands to
//! every variable in scope; once that set is large the result shape
//! becomes hard to predict and brittle against future edits to the
//! `MATCH` clauses. Spelling the columns out explicitly is the style fix.
//!
//! # Threshold
//!
//! The cut-off is [`LintOptions::return_star_max_bindings`] (default 5).
//! The lint fires once a statement binds *strictly more* than that many
//! variables. A small `RETURN *` is fine and stays quiet.
//!
//! # How `RETURN *` is represented
//!
//! HIR lowering has no dedicated star node: a `RETURN *` item is lowered
//! to a projection whose expression is `Expr::Unresolved("*")` (see
//! `cyrs_hir::lower`). This lint detects exactly that shape.

use cyrs_diag::{Applicability, DiagCode, Diagnostic, DiagnosticsSink, FixIt, TextEdit};
use cyrs_hir::{Clause, Expr, Projection, Statement};
use smol_str::SmolStr;

use super::LintOptions;

/// Run L5 over `stmt`.
pub fn check(stmt: &Statement, options: &LintOptions, sink: &mut DiagnosticsSink) {
    let binding_count = stmt.bindings.len();
    if binding_count <= options.return_star_max_bindings {
        return;
    }

    for clause in &stmt.clauses {
        let Clause::Return { projections, .. } = clause else {
            continue;
        };
        for proj in projections {
            if !is_star(proj) {
                continue;
            }
            // Build the explicit replacement: every bound variable name,
            // in binding order. Used both for the hint text and a
            // machine-applicable quick-fix.
            let names: Vec<&str> = stmt.bindings.values().map(|b| b.name.as_str()).collect();
            let explicit = names.join(", ");
            sink.push(
                Diagnostic::warning(
                    DiagCode::W6015,
                    proj.span,
                    format!(
                        "`RETURN *` expands to {binding_count} variables — \
                         prefer an explicit projection"
                    ),
                )
                .with_note(format!(
                    "replace `*` with the columns you need, e.g. `RETURN {explicit}`"
                ))
                .with_fix(FixIt {
                    id: SmolStr::new("cy-fix.expand-return-star"),
                    title: SmolStr::new("expand `*` to the bound variables"),
                    applicability: Applicability::MaybeIncorrect,
                    edits: vec![TextEdit {
                        range: proj.span,
                        replacement: SmolStr::new(explicit),
                    }],
                }),
            );
        }
    }
}

fn is_star(proj: &Projection) -> bool {
    matches!(&proj.expr, Expr::Unresolved(s) if s.as_str() == "*")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::test_support::render;
    use cyrs_diag::DiagnosticsSink;
    use cyrs_hir::lower_statement;

    fn run(src: &str, max: usize) -> String {
        let stmt = lower_statement(src).expect("test input lowers");
        let mut sink = DiagnosticsSink::new();
        let opts = LintOptions {
            return_star_max_bindings: max,
        };
        check(&stmt, &opts, &mut sink);
        render(src, sink)
    }

    #[test]
    fn snap_wide_return_star_fires() {
        // 6 bound node variables, threshold 5 → fires.
        insta::assert_snapshot!(run("MATCH (a), (b), (c), (d), (e), (f) RETURN *", 5,));
    }

    #[test]
    fn snap_narrow_return_star_clean() {
        // 2 bound variables, threshold 5 → clean.
        insta::assert_snapshot!(run("MATCH (a), (b) RETURN *", 5));
    }

    #[test]
    fn explicit_return_never_flagged() {
        let out = run("MATCH (a), (b), (c), (d), (e), (f) RETURN a, b", 5);
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn threshold_is_strict() {
        // Exactly `max` bindings → still clean (strictly-more rule).
        let out = run("MATCH (a), (b), (c) RETURN *", 3);
        assert!(out.contains("diagnostics: 0"), "{out}");
    }
}
