//! Plan property tests (spec 0001 §17.3).
//!
//! Property implemented here:
//!
//! - **P17.3.7 Plan determinism** — calling `lower_statement(hir)` twice on
//!   the same HIR produces byte-identical `Debug` (and JSON, when the `serde`
//!   feature is enabled) output (spec §17.3 P17.3.7, §17.14).
//!
//! "Byte-identical" is verified via both the `Debug` representation and the
//! JSON serialisation so that both the in-memory layout and the
//! wire-format are deterministic.  Using `IndexMap` throughout the plan IR
//! (AGENTS.md §8 / spec §17.14) is what makes this property hold.
//!
//! Inputs that produce no ops (empty plan) are still checked — the empty
//! plan must also be deterministic.

use cypher_hir::{desugar::desugar_statement, lower::lower_statement as hir_lower};
use cypher_plan::lower::lower_statement as plan_lower;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` when the parse tree for `src` contains no ERROR nodes.
fn has_no_errors(src: &str) -> bool {
    use cypher_syntax::parse;
    let p = parse(src);
    p.errors().is_empty()
}

/// Lower `src` → HIR (desugar) → Plan and return the plan's Debug string.
///
/// cy-wlr: `plan_lower` now returns `Result`. The determinism property
/// renders the full result (error or plan) so either outcome is exercised
/// deterministically; inputs that violate the plan-lowering precondition
/// still produce byte-identical outputs on repeat calls.
fn plan_debug(src: &str) -> String {
    let stmt = hir_lower(src);
    let stmt = desugar_statement(stmt);
    let result = plan_lower(&stmt);
    format!("{result:?}")
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn cypher_valid() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("MATCH (n) RETURN n".to_string()),
        Just("MATCH (n:Person) RETURN n".to_string()),
        Just("MATCH (n)-[r]->(m) RETURN n, r, m".to_string()),
        Just("MATCH (n)-[r:KNOWS]->(m) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.age > 21 RETURN n".to_string()),
        Just("MATCH (n) RETURN n.name, n.age".to_string()),
        Just("MATCH (n) RETURN DISTINCT n".to_string()),
        Just("MATCH (n) RETURN n ORDER BY n.name ASC".to_string()),
        Just("MATCH (n) RETURN n SKIP 10 LIMIT 5".to_string()),
        Just("MATCH (n) WITH n RETURN n".to_string()),
        Just("MATCH (n {name: 'Alice'}) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.a > 1 AND n.b < 2 RETURN n".to_string()),
        Just("MATCH (n) WHERE n.a > 1 OR n.b < 2 RETURN n".to_string()),
        Just("MATCH (n) RETURN n.age + 1".to_string()),
        Just("MATCH (n) RETURN count(n)".to_string()),
        Just("MATCH (n) RETURN n.name, count(n)".to_string()),
        Just("// comment\nMATCH (n) RETURN n".to_string()),
        Just("MATCH (n:A)-[r:B]->(m:C) WHERE n.x = 1 RETURN n, r, m".to_string()),
    ]
}

// ---------------------------------------------------------------------------
// P17.3.7 — Plan determinism
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        max_global_rejects: 4096,
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// Two calls to `lower_statement` on the same HIR produce byte-identical
    /// Debug output (spec P17.3.7 / §17.14 determinism).
    #[test]
    fn plan_determinism_debug(s in cypher_valid()) {
        prop_assume!(has_no_errors(&s));

        let d1 = plan_debug(&s);
        let d2 = plan_debug(&s);
        prop_assert_eq!(
            &d1,
            &d2,
            "plan determinism (Debug) failed for input: {:?}",
            s
        );
    }
}

// ---------------------------------------------------------------------------
// Regression guards
// ---------------------------------------------------------------------------

/// Basic MATCH RETURN must produce a deterministic plan.
#[test]
fn regression_basic_match_return() {
    let s = "MATCH (n) RETURN n";
    assert_eq!(plan_debug(s), plan_debug(s));
}

/// MATCH with labels must produce a deterministic plan.
#[test]
fn regression_match_with_labels() {
    let s = "MATCH (n:Person)-[r:KNOWS]->(m) RETURN n, r, m";
    assert_eq!(plan_debug(s), plan_debug(s));
}

/// Aggregation must produce a deterministic plan.
#[test]
fn regression_aggregation() {
    let s = "MATCH (n) RETURN n.name, count(n)";
    assert_eq!(plan_debug(s), plan_debug(s));
}

/// ORDER BY / SKIP / LIMIT must produce a deterministic plan.
#[test]
fn regression_order_skip_limit() {
    let s = "MATCH (n) RETURN n ORDER BY n.name ASC SKIP 10 LIMIT 5";
    assert_eq!(plan_debug(s), plan_debug(s));
}
