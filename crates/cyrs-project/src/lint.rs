//! Lint rule registry and level enum (spec 0003 §6).
//!
//! The registry is a stable, closed list of lint rule names. The
//! manifest validates lint keys against this list so that typos and
//! stale rule names fail at load time rather than silently
//! disappearing.
//!
//! As of bead cy-4yy the registry pairs each name with a real lint in
//! `cyrs_sema::lints` (the clippy-equivalent starter pack). Each rule
//! name maps to a `W6xxx` diagnostic code:
//!
//! | Rule name                  | Code  | Lint |
//! | -------------------------- | ----- | ---- |
//! | `unused-pattern-var`       | W6011 | L1 — variable bound but never used |
//! | `redundant-match`          | W6012 | L2 — MATCH duplicating an earlier one |
//! | `unrestricted-pattern`     | W6013 | L3 — pattern with no label / rel-type |
//! | `cartesian-product`        | W6014 | L4 — implicit cartesian product |
//! | `wildcard-return`          | W6015 | L5 — wide `RETURN *` |
//! | `optional-match-where`     | W6016 | L6 — WHERE on an OPTIONAL MATCH binding |
//!
//! The legacy placeholder names (`dead-pattern-var`,
//! `unused-import-schema`) are retained so existing manifests keep
//! loading; see [`REGISTERED_LINT_RULES`].

use serde::{Deserialize, Serialize};

/// The level at which a lint rule fires.
///
/// Wire format: lowercase strings `"allow" | "warn" | "deny"`. Any other
/// string is rejected at TOML-parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LintLevel {
    /// The rule is disabled; no diagnostic is emitted.
    Allow,
    /// The rule emits a warning-severity diagnostic.
    Warn,
    /// The rule emits an error-severity diagnostic.
    Deny,
}

/// Registered lint rule names (spec 0003 §6).
///
/// Kept sorted for deterministic diagnostic output (enforced by a unit
/// test). The names are stable — adding or removing one is a
/// spec-governed change.
///
/// The first six entries are the cy-4yy clippy-equivalent starter pack
/// (each backed by a real lint in `cyrs_sema::lints`); the last two
/// (`dead-pattern-var`, `unused-import-schema`) are retained legacy
/// placeholder names so pre-cy-4yy manifests keep loading.
pub const REGISTERED_LINT_RULES: &[&str] = &[
    "cartesian-product",
    "dead-pattern-var",
    "optional-match-where",
    "redundant-match",
    "unrestricted-pattern",
    "unused-import-schema",
    "unused-pattern-var",
    "wildcard-return",
];

/// Map a registered lint rule name to its stable diagnostic code
/// string, or `None` for a legacy placeholder name with no backing
/// lint (`dead-pattern-var`, `unused-import-schema`).
///
/// The codes mirror `cyrs_diag::DiagCode`'s `W6011..=W6016` block,
/// which the `cyrs-sema` lint pass emits.
#[must_use]
pub fn lint_rule_code(name: &str) -> Option<&'static str> {
    match name {
        "unused-pattern-var" => Some("W6011"),
        "redundant-match" => Some("W6012"),
        "unrestricted-pattern" => Some("W6013"),
        "cartesian-product" => Some("W6014"),
        "wildcard-return" => Some("W6015"),
        "optional-match-where" => Some("W6016"),
        _ => None,
    }
}

/// Is `name` a known lint rule name?
#[must_use]
pub fn is_registered_lint_rule(name: &str) -> bool {
    REGISTERED_LINT_RULES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_sorted_and_unique() {
        // Sorted for deterministic diagnostic output.
        let mut sorted: Vec<&&str> = REGISTERED_LINT_RULES.iter().collect();
        sorted.sort();
        let sorted_owned: Vec<&str> = sorted.iter().map(|s| **s).collect();
        let registry_owned: Vec<&str> = REGISTERED_LINT_RULES.to_vec();
        assert_eq!(sorted_owned, registry_owned);

        // Unique.
        let mut seen: Vec<&str> = Vec::new();
        for r in REGISTERED_LINT_RULES {
            assert!(!seen.contains(r), "duplicate rule `{r}`");
            seen.push(r);
        }
    }

    #[test]
    fn is_registered_lint_rule_matches_every_entry() {
        for r in REGISTERED_LINT_RULES {
            assert!(is_registered_lint_rule(r));
        }
        assert!(!is_registered_lint_rule("not-a-rule"));
    }

    #[test]
    fn lint_rule_code_covers_the_starter_pack() {
        // The six cy-4yy lints each map to a sequential W601x code.
        assert_eq!(lint_rule_code("unused-pattern-var"), Some("W6011"));
        assert_eq!(lint_rule_code("redundant-match"), Some("W6012"));
        assert_eq!(lint_rule_code("unrestricted-pattern"), Some("W6013"));
        assert_eq!(lint_rule_code("cartesian-product"), Some("W6014"));
        assert_eq!(lint_rule_code("wildcard-return"), Some("W6015"));
        assert_eq!(lint_rule_code("optional-match-where"), Some("W6016"));
        // Legacy placeholders have no backing lint.
        assert_eq!(lint_rule_code("dead-pattern-var"), None);
        assert_eq!(lint_rule_code("unused-import-schema"), None);
        assert_eq!(lint_rule_code("not-a-rule"), None);
    }

    #[test]
    fn every_coded_rule_is_registered() {
        // Any name `lint_rule_code` recognises must be in the registry.
        for r in REGISTERED_LINT_RULES {
            if let Some(code) = lint_rule_code(r) {
                assert!(code.starts_with("W601"), "rule {r} → {code}");
            }
        }
    }

    #[test]
    fn lint_level_serde_is_lowercase() {
        use std::collections::BTreeMap;
        let mut map: BTreeMap<String, LintLevel> = BTreeMap::new();
        map.insert("dead-pattern-var".into(), LintLevel::Warn);
        map.insert("wildcard-return".into(), LintLevel::Deny);
        let s = toml::to_string(&map).unwrap();
        assert!(s.contains("\"warn\""), "got: {s}");
        assert!(s.contains("\"deny\""), "got: {s}");
    }
}
