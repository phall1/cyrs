//! fuzz_formatter — fuzzes `cypher-fmt::format` against arbitrary strings.
//!
//! Oracle: idempotence — `fmt(fmt(s)) == fmt(s)` for all inputs (spec §13,
//! §17.4). No panic on any input.

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // format() must never panic regardless of input.
    let first = cypher_fmt::format(s);
    // Idempotence invariant: formatting the result again must be identical.
    let second = cypher_fmt::format(&first);
    assert_eq!(
        first, second,
        "formatter is not idempotent: fmt(fmt(s)) != fmt(s)"
    );
});
