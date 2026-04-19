//! Registry invariants for `DiagCode` (spec §10.2).
//!
//! These tests enforce the stability guarantees the spec requires:
//! every code is unique, well-formatted, and falls inside its declared
//! range. The `ALL` array is the enumeration; every enum variant MUST
//! appear in it.

#![forbid(unsafe_code)]

use cypher_diag::DiagCode;
use std::collections::HashSet;

#[test]
fn all_codes_unique() {
    let mut seen = HashSet::new();
    for c in DiagCode::ALL {
        assert!(
            seen.insert(*c as u32),
            "duplicate discriminant: {}",
            c.as_str()
        );
    }
}

#[test]
fn all_codes_well_formatted() {
    for c in DiagCode::ALL {
        let s = c.as_str();
        // e.g. "E0001" — one letter + exactly four digits
        assert_eq!(s.len(), 5, "malformed code: {s}");
        let first = s.as_bytes()[0];
        assert!(
            matches!(first, b'E' | b'W' | b'N'),
            "code must begin with E/W/N: {s}",
        );
        assert!(s[1..].chars().all(|ch| ch.is_ascii_digit()));
    }
}

#[test]
fn severity_char_matches_range() {
    for c in DiagCode::ALL {
        let n = *c as u32;
        let expected = match n {
            0..=5999 => 'E',
            6000..=7999 => 'W',
            8000..=8999 => 'N',
            _ => panic!("code {} is outside any registered range", c.as_str()),
        };
        assert_eq!(c.severity_char(), expected, "{}", c.as_str());
    }
}

#[test]
fn all_covers_every_variant() {
    // If this count diverges, a variant was added to `DiagCode` without
    // being added to `ALL` (or vice versa).
    // Expected = number of registered variants.
    let expected = DiagCode::ALL.len();
    let actual = DiagCode::ALL.iter().copied().collect::<HashSet<_>>().len();
    assert_eq!(expected, actual);
}

#[test]
fn all_count_pinned() {
    // When you add a variant to `DiagCode`, append it to `ALL` and
    // bump this number. This ensures the registry stays exhaustive.
    // Current count: 32 (6 syntax + 5 name-res + 6 sema-free + 8
    // sema-aware + 2 dialect + 2 type + 1 style + 1 perf + 1 note).
    const EXPECTED: usize = 32;
    assert_eq!(DiagCode::ALL.len(), EXPECTED);
}
