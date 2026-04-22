//! cy-f2t regression — HIR → Plan lowering must not panic on pattern
//! parts produced by the parser's error-recovery pass.
//!
//! Spec 0001 §12, §17.4. The P0 bug was surfaced by cy-h07's fuzz smoke:
//! the 5-byte input `MATCH` round-trips through `cypher_hir::lower` as a
//! `Clause::Match` whose [`cypher_hir::PatternPart`] carries zero
//! [`cypher_hir::PatternElement`]s. The previous lowerer answered that
//! shape with `.expect("pattern part must have at least one element")`
//! at `crates/cypher-plan/src/lower.rs:682`. The fix surfaces the
//! recovery shape as `PlanLowerError::EmptyPatternPart { .. }` (see
//! `cypher_plan::error`) instead.

#![cfg(test)]

use cypher_hir::{Clause, HirId, HirSpan, Pattern, PatternElement, PatternPart, Statement};
use cypher_plan::PlanLowerError;
use cypher_plan::lower::lower_statement;

/// Lowering a `MATCH` clause with a single empty pattern part must
/// surface `EmptyPatternPart` rather than panicking.
///
/// This shape is exactly what `cypher_hir::lower::lower_statement("MATCH")`
/// produces (see cy-h07's smoke finding and this bead's description).
#[test]
fn lower_statement_rejects_empty_pattern_part_from_bare_match() {
    let span = HirSpan::default();
    let mut stmt = Statement::new(span);
    stmt.clauses.push(Clause::Match {
        id: HirId::DUMMY,
        optional: false,
        pattern: Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![],
            }],
        },
        span,
    });

    let err = lower_statement(&stmt).expect_err("empty pattern part must be rejected");
    match err {
        PlanLowerError::EmptyPatternPart { span: err_span } => {
            assert_eq!(
                err_span, span,
                "error span must match the MATCH clause span"
            );
        }
        other => panic!("expected EmptyPatternPart, got {other:?}"),
    }
}

/// A `Pattern` with zero parts is fine — there is nothing to walk.
/// Only the per-part shape is a precondition violation. Locking in this
/// distinction keeps the error variant precise.
#[test]
fn lower_statement_accepts_pattern_with_no_parts() {
    let span = HirSpan::default();
    let mut stmt = Statement::new(span);
    stmt.clauses.push(Clause::Match {
        id: HirId::DUMMY,
        optional: false,
        pattern: Pattern { parts: vec![] },
        span,
    });

    let plan = lower_statement(&stmt).expect("zero-part pattern must lower cleanly");
    // No parts → no Source / Expand ops required. Depending on the
    // pattern-free fallback the lowerer may emit a degenerate all-node
    // Source or nothing; both are acceptable. We only care that it did
    // not error / panic.
    let _ = plan.ops;
}

/// A `PatternPart` that begins with a `Rel` (the other reachable
/// recovery shape — e.g. `MATCH -[:R]->(n)`) must also surface cleanly
/// as `EmptyPatternPart` rather than reaching the in-body
/// `.expect(...)` sites that previously guarded `last_node_var` and
/// `last_op`.
#[test]
fn lower_statement_rejects_rel_first_pattern_part() {
    use cypher_hir::{Direction, RelLength};
    let span = HirSpan::default();
    let mut stmt = Statement::new(span);
    stmt.clauses.push(Clause::Match {
        id: HirId::DUMMY,
        optional: false,
        pattern: Pattern {
            parts: vec![PatternPart {
                named_as: None,
                elements: vec![
                    PatternElement::Rel {
                        id: HirId::DUMMY,
                        bind: None,
                        types: vec![],
                        direction: Direction::Outgoing,
                        length: RelLength::Single,
                        props: None,
                        span,
                    },
                    PatternElement::Node {
                        id: HirId::DUMMY,
                        bind: None,
                        labels: vec![],
                        props: None,
                        span,
                    },
                ],
            }],
        },
        span,
    });

    let err = lower_statement(&stmt).expect_err("rel-first pattern must be rejected");
    assert!(
        matches!(err, PlanLowerError::EmptyPatternPart { .. }),
        "expected EmptyPatternPart, got {err:?}",
    );
}
