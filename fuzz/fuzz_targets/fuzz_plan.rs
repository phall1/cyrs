//! fuzz_plan — lowers HIR to plan; oracle: no panic (spec §17.4).
//!
//! We parse → lower to HIR → lower to plan. The plan lowering only sees
//! HIR that the HIR lowerer produced, so we exercise the plan layer
//! against whatever HIR the parser+lowerer yields for arbitrary input.

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Produce HIR.
    let stmt = cypher_hir::lower::lower_statement(s);

    // Lower to plan — must not panic on any HIR shape. Pre-condition
    // violations (un-resolved names, un-desugared expressions) surface as
    // `Err(PlanLowerError::…)` per cy-wlr; the oracle is "no panic", so
    // we discard the `Result` rather than asserting it is `Ok`.
    let _ = cypher_plan::lower::lower_statement(&stmt);
});
