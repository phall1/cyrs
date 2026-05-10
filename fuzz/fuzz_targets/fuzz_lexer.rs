//! fuzz_lexer — fuzzes the `cyrs-syntax` lexer against arbitrary bytes.
//!
//! Oracle: no panic, no hang (libfuzzer timeout kills hanging runs).
//! Sanitizers: ASan + UBSan enabled via cargo-fuzz default flags (spec §17.4).

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The lexer works on &str; skip inputs that are not valid UTF-8.
    // Invalid-UTF-8 handling is a separate concern (tokenise_bytes, future).
    if let Ok(s) = std::str::from_utf8(data) {
        // lex() must never panic regardless of input (spec §17.4).
        let tokens = cyrs_syntax::lex(s);
        // validate_tokens() runs secondary diagnostics — must not panic.
        let _errors = cyrs_syntax::validate_tokens(s, &tokens);
    }
});
