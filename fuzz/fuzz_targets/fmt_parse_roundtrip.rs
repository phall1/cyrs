//! fmt_parse_roundtrip — differential fuzzer for `parse ∘ fmt ∘ parse`
//! (bead cy-h07, spec §17.3 P17.3.3 / P17.3.4 / P17.3.1).
//!
//! Oracle, for every UTF-8 input `s`:
//!
//! 1. `parse(s)` never panics and produces a CST whose text is `s`
//!    (byte-level losslessness, P17.3.2).
//! 2. If `parse(s)` has no errors, `fmt(s)` must:
//!    a. not panic,
//!    b. itself parse cleanly (parser must not choke on its own
//!       formatter's output), and
//!    c. be idempotent: `fmt(fmt(s)) == fmt(s)` (P17.3.3).
//! 3. The trivia-stripped CST shape of `parse(s)` equals the shape of
//!    `parse(fmt(s))` (P17.3.4 — formatting preserves semantics up to
//!    whitespace/comment rewriting).
//!
//! This target differs from `fuzz_structured_parse` in two ways:
//!   - Input is arbitrary UTF-8 (byte-level), not an RNG seed that is
//!     guaranteed to produce valid Cypher. Invalid-by-parse inputs skip
//!     the formatter oracle but still exercise the lossless invariant.
//!   - The structural comparison is explicit: we walk both CSTs in
//!     lockstep and compare kinds, ignoring WHITESPACE / COMMENT /
//!     LINE_COMMENT / BLOCK_COMMENT tokens. A mismatch is a P0 bug
//!     against `cyrs-fmt`.
//!
//! If this target ever crashes, treat it as a blocker — §17.4 forbids
//! any new panic on any input.

#![no_main]

use cyrs_fuzz::roundtrip::{contains_error, shape};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // -------- Oracle 1: parse is total + lossless --------
    let parsed = cyrs_syntax::parse(s);
    let root = parsed.syntax();
    assert_eq!(
        root.text().to_string(),
        s,
        "parser produced a lossy CST: input bytes were dropped or duplicated"
    );

    // The roundtrip oracle only makes sense on inputs the parser
    // accepted without recovery; feeding recovery-laden input through
    // the formatter is tested by `fuzz_formatter` separately.
    if !parsed.errors().is_empty() || contains_error(&root) {
        return;
    }

    // -------- Oracle 2: formatter is total + idempotent --------
    let once = cyrs_fmt::format(s);
    let twice = cyrs_fmt::format(&once);
    assert_eq!(
        once, twice,
        "formatter not idempotent:\n  src: {s:?}\n  once: {once:?}\n  twice: {twice:?}"
    );

    // -------- Oracle 3: parse(fmt(s)) is still clean --------
    let reparsed = cyrs_syntax::parse(&once);
    assert!(
        reparsed.errors().is_empty(),
        "fmt(src) no longer parses:\n  src: {s:?}\n  fmt: {once:?}\n  errors: {:?}",
        reparsed.errors()
    );
    let reparsed_root = reparsed.syntax();
    assert!(
        !contains_error(&reparsed_root),
        "fmt(src) reparses with ERROR nodes:\n  src: {s:?}\n  fmt: {once:?}"
    );

    // -------- Oracle 4: structural equality (P17.3.4) --------
    let src_shape = shape(&root);
    let fmt_shape = shape(&reparsed_root);
    assert_eq!(
        src_shape, fmt_shape,
        "structural equality broken:\n  src: {s:?}\n  fmt: {once:?}\n  src_shape: {src_shape}\n  fmt_shape: {fmt_shape}"
    );
});
