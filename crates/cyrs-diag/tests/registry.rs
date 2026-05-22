//! Registry invariants for `DiagCode` (spec §10.2).
//!
//! These tests enforce the stability guarantees the spec requires:
//! every code is unique, well-formatted, and falls inside its declared
//! range. The `ALL` array is the enumeration; every enum variant MUST
//! appear in it.

#![forbid(unsafe_code)]

use cyrs_diag::DiagCode;
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

/// The `E4500..=E4999` block is permanently reserved for external
/// embedders (e.g. a host that embeds cyrs and mints its own codes for
/// storage-model rejections cyrs cannot diagnose). cyrs's own
/// `DiagCode` enum must never assign a code in that range — its native
/// dialect / compatibility codes are confined to `E4000..=E4499`.
///
/// This test mechanically enforces the policy documented on
/// `DiagCode`: adding a native variant in the reserved range fails CI
/// here. See feat-request §3.1 / `docs/embedder-issues/0003`.
#[test]
fn embedder_reserved_range_is_empty() {
    const RESERVED: std::ops::RangeInclusive<u32> = 4500..=4999;
    for c in DiagCode::ALL {
        let n = *c as u32;
        assert!(
            !RESERVED.contains(&n),
            "code {} ({n}) falls in the embedder-reserved range \
             E4500..=E4999 — native cyrs codes must stay below E4500",
            c.as_str(),
        );
    }
}

#[test]
fn all_count_pinned() {
    // When you add a variant to `DiagCode`, append it to `ALL` and
    // bump this number. This ensures the registry stays exhaustive.
    //
    // Breakdown (spec §10.2):
    //   77  syntax         (E0001–E0082, cy-a4d + cy-3xz + cy-7s6.1 + cy-8x5 + cy-5gh + cy-41u + cy-7lf + cy-01q)
    //    2  name-res       (E1001–E1002, cy-heh)
    //    7  schema-free    (E2007–E2013, cy-b4b + cy-raq)
    //   10  schema-aware   (E3001–E3004, E3006–E3011, cy-36u + cy-0ek + cy-e45)
    //   14  dialect        (E4001 + E4010–E4023, cy-z49 + cy-v5u6 + cy-5js)
    //    4  type           (E5003, E5010, E5011, E5012, cy-c6g + cy-7s6.1 + cy-8x5 + cy-zo9.1)
    //   14  style          (W6001–W6007, W6010–W6016, cy-0ek + cy-4yy)
    //    4  perf           (W7001–W7004)
    //    3  notes          (N8001–N8003)
    //  ---
    //  132  total
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
    // cy-0ek: added E3010 (opaque/unresolved schema type), E3011 (self-
    //         referential rel type), and W6010 (unreachable label) for
    //         the `cypher schema check` linter (spec 0002 §9).
    // cy-41u: added E0070 (expected THEN in CASE arm) and E0071
    //         (expected END to close CASE) for CASE expression parsing
    //         (spec §19 row "CASE").
    // cy-7lf: registered E0072 (expected `)` to close EXISTS pattern
    //         predicate) — emitted by cy-lve's parser path but not
    //         carried into the registry at that time. The bare-pattern
    //         form (cy-7lf) reuses the NODE_PATTERN-level recovery and
    //         does not need a new code.
    // cy-01q: added E0078–E0082 for map projection parser recovery
    //         (spec §6.1 / §19 row "Map projection"). Distinct from the
    //         map-literal recovery codes (E0049–E0052) so tools can
    //         tell projection from literal apart.
    // cy-8z3: added E0083 for `INSERT` pattern recovery — the
    //         GQL-distinct write-clause spelling (ISO/IEC 39075:2024
    //         §13.4) parses analogously to CREATE but emits a separate
    //         code so dialect-aware tools can hint accordingly.
    // cy-auh: added E0086 for `RETURN ... EXCLUDE` field-list recovery
    //         — the GQL-distinct return-projection trailer per ISO/IEC
    //         39075:2024 §14.13.4. Slots E0084 / E0085 are pre-reserved
    //         for parallel GQL beads (FILTER / OPTIONAL CALL).
    // cy-pnp: added E0088 / E0089 for GQL type-assertion recovery — the
    //         `IS [NOT] TYPED <Type>` predicate and the `<expr> :: <Type>`
    //         typed-value shorthand (ISO/IEC 39075:2024 §6.5.2).
    // cy-q2g: added E0090 for the `MATCH` path-mode prefix
    //         (`REPEATABLE ELEMENTS` / `DIFFERENT EDGES`, ISO/IEC
    //         39075:2024 §10.6.3).
    // cy-r50: added E0084 for the GQL `FILTER` post-projection clause
    //         (ISO/IEC 39075:2024 §14.10).
    // cy-9kzx: added E0091–E0098 for the GQL `SESSION SET` top-level
    //          statement category (ISO/IEC 39075:2024 §14.15).
    // cy-lp3y: added E4020 (`session_set` dialect gate) for the
    //          `SESSION SET` HIR + sema follow-up (spec §0 amendment
    //          2026-05-19 cy-5e3f).
    // cy-v5u6: added E4021 (GQL catalog DDL used in OpenCypherV9
    //          dialect) and E4022 (catalog op malformed) for HIR
    //          lowering + sema of `CREATE GRAPH` / `CREATE SCHEMA`
    //          (ISO/IEC 39075:2024 §14.14; rebased E4020/E4021 →
    //          E4021/E4022, ceded E4020 to cy-lp3y).
    // cy-e45: added E3009 (MERGE key not backed by a declared uniqueness
    //         constraint) for schema-aware MERGE validation against
    //         `SchemaProvider::{label,rel_type}_unique_props` (feat-
    //         request §2.2; spec §7.5). E3005 stays retired (cy-va1).
    // cy-5js: added E4023 (incompatible multi-label combination) for the
    //         `SchemaProvider::labels_compatible` check on `CREATE` node
    //         patterns and label-adding `SET` items (feat-request §2.3).
    // cy-4yy: added W6011–W6016 for the clippy-equivalent lint starter
    //         pack (spec 0003 §6 / §20) — L1 unused pattern variable,
    //         L2 redundant MATCH, L3 unrestricted pattern (schema-aware),
    //         L4 implicit cartesian product, L5 wide `RETURN *`, L6
    //         OPTIONAL MATCH with a WHERE on the optional binding.
    //         Emitted by `cyrs-sema::lints`, opt-in via `cypher
    //         check --lints`.
    const EXPECTED: usize = 150;
    assert_eq!(DiagCode::ALL.len(), EXPECTED);
}
