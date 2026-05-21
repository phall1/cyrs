//! L2 — redundant `MATCH` ([`W6012`](cyrs_diag::DiagCode::W6012)).
//!
//! Flags a `MATCH` clause that re-matches a pattern an earlier `MATCH`
//! in the same statement has already matched: the later clause adds no
//! new constraint and binds the same variables, so it can be deleted.
//!
//! # Conservative by design
//!
//! A precise "this pattern is subsumed by that one" check is a graph
//! sub-isomorphism problem and easy to get wrong. This lint instead
//! fires **only** on an exact structural duplicate:
//!
//! - both clauses are plain `MATCH` (not `OPTIONAL MATCH`);
//! - their patterns produce the *same structural fingerprint* — same
//!   parts in the same order, same node labels / relationship types,
//!   same directions and variable-length bounds, and the same binder
//!   *names* at every position.
//!
//! Anything subtler (a pattern that is a strict super-set, reordered
//! parts, an equivalent-but-differently-written direction) is left
//! un-flagged. The lint therefore never warns wrongly — it just misses
//! the harder cases (spec 0003 §6: "sound conservative version").
//!
//! Property maps on pattern elements are deliberately *ignored* by the
//! fingerprint: two `MATCH (n {a: 1})` clauses are structurally the
//! same shape, and re-stating the same inline-property filter is still
//! redundant. (A pattern with *different* property filters produces the
//! same fingerprint only when the bound names also match, which is
//! itself already a redundant re-match of the binders.)

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::{
    Clause, Direction, Pattern, PatternElement, PatternPart, RelLength, Statement, VarId,
};
use std::fmt::Write as _;

/// Run L2 over `stmt`.
pub fn check(stmt: &Statement, sink: &mut DiagnosticsSink) {
    let mut seen: Vec<(String, cyrs_hir::HirSpan)> = Vec::new();
    for clause in &stmt.clauses {
        let Clause::Match {
            optional: false,
            pattern,
            span,
            ..
        } = clause
        else {
            continue;
        };
        let fp = fingerprint(stmt, pattern);
        if let Some((_, first_span)) = seen.iter().find(|(f, _)| f == &fp) {
            sink.push(
                Diagnostic::warning(
                    DiagCode::W6012,
                    *span,
                    "redundant MATCH — this pattern is already matched by an earlier MATCH",
                )
                .with_label(*first_span, "first matched here")
                .with_note(
                    "delete this MATCH clause; it binds the same variables \
                     and adds no new constraint",
                ),
            );
        } else {
            seen.push((fp, *span));
        }
    }
}

/// Structural fingerprint of a pattern: identical text ⇒ identical
/// shape. Binder identity is keyed on the bound *name* (looked up in
/// `stmt.bindings`) so the same fingerprint across clauses means the
/// same author-visible variables.
fn fingerprint(stmt: &Statement, pattern: &Pattern) -> String {
    let mut s = String::new();
    for part in &pattern.parts {
        fingerprint_part(stmt, part, &mut s);
        s.push(';');
    }
    s
}

fn fingerprint_part(stmt: &Statement, part: &PatternPart, s: &mut String) {
    write!(s, "shortest={:?}", part.shortest).unwrap();
    if let Some(p) = part.named_as {
        write!(s, "|path={}", var_name(stmt, p)).unwrap();
    }
    for elem in &part.elements {
        s.push('/');
        match elem {
            PatternElement::Node { bind, labels, .. } => {
                write!(s, "N(").unwrap();
                if let Some(b) = bind {
                    write!(s, "{}", var_name(stmt, *b)).unwrap();
                }
                let mut ls: Vec<&str> = labels.iter().map(smol_str::SmolStr::as_str).collect();
                ls.sort_unstable();
                write!(s, ":{})", ls.join(",")).unwrap();
            }
            PatternElement::Rel {
                bind,
                types,
                direction,
                length,
                ..
            } => {
                write!(s, "R(").unwrap();
                if let Some(b) = bind {
                    write!(s, "{}", var_name(stmt, *b)).unwrap();
                }
                let mut ts: Vec<&str> = types.iter().map(smol_str::SmolStr::as_str).collect();
                ts.sort_unstable();
                write!(s, ":{}", ts.join(",")).unwrap();
                write!(s, "{}", dir_tag(*direction)).unwrap();
                match length {
                    RelLength::Single => write!(s, "*1").unwrap(),
                    RelLength::Variable { min, max } => {
                        write!(s, "*{min:?}..{max:?}").unwrap();
                    }
                    // `RelLength` is `#[non_exhaustive]`; an unknown
                    // length variant gets a distinct, stable tag so two
                    // such patterns still compare equal to themselves
                    // but never collide with the modelled variants.
                    other => write!(s, "*?{other:?}").unwrap(),
                }
                s.push(')');
            }
        }
    }
}

fn dir_tag(d: Direction) -> &'static str {
    match d {
        Direction::Outgoing => "->",
        Direction::Incoming => "<-",
        Direction::Undirected => "--",
        // `Direction` is `#[non_exhaustive]`; a future variant gets a
        // catch-all tag — conservatively distinct from the modelled
        // directions so it never spuriously matches.
        _ => "?dir",
    }
}

fn var_name(stmt: &Statement, v: VarId) -> String {
    stmt.bindings
        .get(&v)
        .map_or_else(|| format!("#{}", v.0), |b| b.name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lints::test_support::run_one;

    #[test]
    fn snap_redundant_match_fires() {
        insta::assert_snapshot!(run_one(check, "MATCH (n:Person) MATCH (n:Person) RETURN n",));
    }

    #[test]
    fn snap_distinct_matches_clean() {
        insta::assert_snapshot!(run_one(
            check,
            "MATCH (n:Person) MATCH (m:Company) RETURN n, m",
        ));
    }

    #[test]
    fn different_labels_not_redundant() {
        let out = run_one(check, "MATCH (n:Person) MATCH (n:Company) RETURN n");
        assert!(out.contains("diagnostics: 0"), "{out}");
    }

    #[test]
    fn optional_match_never_flagged() {
        // OPTIONAL MATCH has different semantics — never a redundant
        // duplicate of a plain MATCH.
        let out = run_one(check, "MATCH (n:Person) OPTIONAL MATCH (n:Person) RETURN n");
        assert!(out.contains("diagnostics: 0"), "{out}");
    }
}
