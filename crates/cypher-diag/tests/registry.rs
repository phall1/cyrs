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
    //
    // Breakdown (spec §10.2):
    //   69  syntax         (E0001–E0069, cy-a4d + cy-3xz + cy-7s6.1 + cy-8x5 + cy-5gh)
    //    2  name-res       (E1001–E1002, cy-heh)
    //    7  schema-free    (E2007–E2013, cy-b4b + cy-raq)
    //    7  schema-aware   (E3001–E3004, E3006–E3008, cy-36u)
    //   11  dialect        (E4001 + E4010–E4019, cy-z49)
    //    4  type           (E5003, E5010, E5011, E5012, cy-c6g + cy-7s6.1 + cy-8x5 + cy-zo9.1)
    //    7  style          (W6001–W6007)
    //    4  perf           (W7001–W7004)
    //    3  notes          (N8001–N8003)
    //  ---
    //  114  total
    //
    // cy-va1: removed unemitted dead codes E1003–E1005, E2001–E2006,
    //         E3005, E4002, E5001–E5002 (spec §10.2 — registry must
    //         match emission sites).
    // cy-3xz: added E0047–E0052 for list/map literal grammar.
    // cy-7s6.1: added E0064 (unclosed index bracket) and E5010
    //           (index / slice of non-list) for list-indexing support.
    // cy-8x5: added E0065–E0067 (list-predicate parser recovery) and
    //         E5011 (list-predicate iterable is not a list).
    // cy-5gh: added E0068 (expected IN in list comp) and E0069
    //         (expected `|` or `]` in list comp) for list comprehensions.
    // cy-zo9.1: added E5012 (builtin argument kind mismatch).
    const EXPECTED: usize = 114;
    assert_eq!(DiagCode::ALL.len(), EXPECTED);
}
