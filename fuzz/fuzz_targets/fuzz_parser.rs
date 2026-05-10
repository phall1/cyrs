//! fuzz_parser — fuzzes the `cyrs-syntax` recovering parser.
//!
//! Oracle: no panic; parsed tree is lossless (roundtrip == input);
//! memory is bounded (no cycle/blowup). Spec §17.4.

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    let parsed = cyrs_syntax::parse(s);

    // Lossless invariant: the printed text of the CST equals the input.
    let root = parsed.syntax();
    assert_eq!(
        root.text().to_string(),
        s,
        "parser produced a lossy CST: input bytes were dropped or duplicated"
    );
});
