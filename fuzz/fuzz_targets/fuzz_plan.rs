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

    // Produce HIR. `lower_parse` (cy-cfi) lowers best-effort from the
    // recovered parse tree, so the plan layer is still exercised for
    // inputs the parser reported errors on — `lower_statement` would
    // short-circuit those to `Err` and skip plan lowering entirely.
    let Ok(stmt) = cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(s)) else {
        return;
    };

    // Lower to plan — must not panic on any HIR shape. Pre-condition
    // violations (un-resolved names, un-desugared expressions) surface as
    // `Err(PlanLowerError::…)` per cy-wlr; the oracle is "no panic", so
    // we discard the `Result` rather than asserting it is `Ok`.
    let _ = cyrs_plan::lower::lower_statement(&stmt);
});
