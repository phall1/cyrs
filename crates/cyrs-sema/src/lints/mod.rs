//! Lint pass — the clippy-equivalent starter pack (spec 0003 §6 / §20).
//!
//! Where the rest of `cyrs-sema` reports *errors* (constructs that are
//! definitionally wrong), this module reports *lints*: warning-severity
//! diagnostics for queries that parse and analyse cleanly but are
//! stylistically poor or — more usefully — likely a bug. Every lint
//! carries a `note:` hint describing the fix.
//!
//! # The starter pack
//!
//! | Lint | Code  | What it flags |
//! |------|-------|---------------|
//! | L1   | [`W6011`] | A pattern variable bound but never referenced. |
//! | L2   | [`W6012`] | A `MATCH` re-matching a pattern an earlier `MATCH` already covers. |
//! | L3   | [`W6013`] | A node/relationship pattern with no label / type restriction (schema-aware). |
//! | L4   | [`W6014`] | Two `MATCH` clauses with disjoint variables and no joining predicate. |
//! | L5   | [`W6015`] | `RETURN *` in a statement binding more than N variables. |
//! | L6   | [`W6016`] | `OPTIONAL MATCH` followed by a `WHERE` on the optional binding. |
//!
//! # Opt-in
//!
//! Lints are **off by default**. They are *not* run by [`crate::analyse`];
//! a caller must invoke [`run_lints`] explicitly. The `cypher` CLI gates
//! them behind `cypher check --lints`; the LSP surfaces them with
//! `Information` severity (see `cyrs-diag`'s LSP converter).
//!
//! # Soundness over completeness
//!
//! L2 (redundant `MATCH`) and L4 (cartesian product) are deliberately
//! *conservative*: they fire only on cases that are unambiguously the
//! flagged pattern, preferring to miss real cases over emitting a false
//! positive. See each module for the precise predicate.
//!
//! [`W6011`]: cyrs_diag::DiagCode::W6011
//! [`W6012`]: cyrs_diag::DiagCode::W6012
//! [`W6013`]: cyrs_diag::DiagCode::W6013
//! [`W6014`]: cyrs_diag::DiagCode::W6014
//! [`W6015`]: cyrs_diag::DiagCode::W6015
//! [`W6016`]: cyrs_diag::DiagCode::W6016

use cyrs_diag::DiagnosticsSink;
use cyrs_hir::Statement;
use cyrs_schema::SchemaProvider;

mod cartesian;
mod optional_match_where;
mod redundant_match;
mod unrestricted_pattern;
mod unused_var;
mod wide_return_star;

/// Knobs for the lint pass.
///
/// Added rather than changed; new fields land with a `Default` so
/// existing callers keep compiling.
#[derive(Debug, Clone)]
pub struct LintOptions {
    /// L5 threshold: `RETURN *` fires once a statement binds *more than*
    /// this many variables. Default: 5.
    pub return_star_max_bindings: usize,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            return_star_max_bindings: 5,
        }
    }
}

/// Run the lint starter pack against a resolved HIR statement.
///
/// Schema-free lints (L1, L2, L4, L5, L6) always run. The schema-aware
/// lint L3 ([`W6013`](cyrs_diag::DiagCode::W6013)) runs only when
/// `schema` is `Some` — without a schema there is no notion of an
/// "unknown" label, so an unrestricted pattern is not actionable.
///
/// No lint short-circuits; every applicable lint walks the whole
/// statement and emits into `sink`. Diagnostics are warning-severity.
pub fn run_lints(
    stmt: &Statement,
    schema: Option<&dyn SchemaProvider>,
    options: &LintOptions,
    sink: &mut DiagnosticsSink,
) {
    unused_var::check(stmt, sink);
    redundant_match::check(stmt, sink);
    if let Some(s) = schema {
        unrestricted_pattern::check(stmt, s, sink);
    }
    cartesian::check(stmt, sink);
    wide_return_star::check(stmt, options, sink);
    optional_match_where::check(stmt, sink);
}

/// Shared helpers for the per-lint snapshot tests.
#[cfg(test)]
pub(crate) mod test_support {
    use cyrs_diag::DiagnosticsSink;
    use cyrs_hir::{Statement, lower_statement};
    use std::fmt::Write as _;

    /// Lower `src`, run a single schema-free lint `check` over it, and
    /// render the resulting diagnostics into a stable, snapshot-friendly
    /// string.
    pub(crate) fn run_one(check: impl Fn(&Statement, &mut DiagnosticsSink), src: &str) -> String {
        let stmt = lower_statement(src).expect("test input lowers cleanly");
        let mut sink = DiagnosticsSink::new();
        check(&stmt, &mut sink);
        render(src, sink)
    }

    /// Render a sink for snapshotting: count + one line per diagnostic
    /// (code, message, each `note:`).
    pub(crate) fn render(src: &str, sink: DiagnosticsSink) -> String {
        let diags = sink.into_sorted();
        let mut out = String::new();
        writeln!(out, "query: {src}").unwrap();
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
            for note in &d.notes {
                writeln!(out, "    note: {note}").unwrap();
            }
            for fix in &d.fixes {
                writeln!(out, "    help: {} [{}]", fix.title, fix.id).unwrap();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrs_hir::lower_statement;

    #[test]
    fn run_lints_clean_query_is_silent() {
        let stmt = lower_statement("MATCH (n:Person) WHERE n.age > 30 RETURN n")
            .expect("clean input lowers");
        let mut sink = DiagnosticsSink::new();
        run_lints(&stmt, None, &LintOptions::default(), &mut sink);
        assert!(sink.is_empty(), "expected no lints, got {}", sink.len());
    }

    #[test]
    fn run_lints_empty_statement_is_silent() {
        let stmt = lower_statement("RETURN 1").expect("clean input lowers");
        let mut sink = DiagnosticsSink::new();
        run_lints(&stmt, None, &LintOptions::default(), &mut sink);
        assert!(sink.is_empty());
    }
}
