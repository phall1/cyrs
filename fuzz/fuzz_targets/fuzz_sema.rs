//! fuzz_sema — parses, lowers to HIR, then runs `cyrs_sema::analyse`.
//!
//! Oracle: no panic; diagnostic spans lie within the input range (spec §17.4).

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Lower to HIR. `lower_parse` (cy-cfi) lowers best-effort from the
    // recovered parse tree so sema is exercised even for inputs the
    // parser reported errors on (the common fuzz case); `lower_statement`
    // would short-circuit those to `Err` and skip analysis.
    let Ok(stmt) = cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(s)) else {
        return;
    };

    // Run semantic analysis — must not panic.
    let mut sink = cyrs_diag::DiagnosticsSink::new();
    let opts = cyrs_sema::SemaOptions::default();
    cyrs_sema::analyse(&stmt, None, &opts, &mut sink);

    // Span invariant: every diagnostic primary span must lie within input.
    let src_len = s.len() as u32;
    for diag in sink.into_sorted() {
        let start: u32 = diag.primary.range.start().into();
        let end: u32 = diag.primary.range.end().into();
        assert!(
            start <= src_len && end <= src_len,
            "diagnostic span [{start}, {end}) out of input range [0, {src_len})"
        );
    }
});
