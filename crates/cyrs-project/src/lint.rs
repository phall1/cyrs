//! Lint rule registry and level enum (spec 0003 §6).
//!
//! At v0 the registry is a small, closed list of **placeholder** rule
//! names. The manifest validates lint keys against this list so that
//! typos and stale rule names fail at load time rather than silently
//! disappearing. The real lints live in `cyrs-sema` and arrive in
//! follow-up beads under the cy-o8c epic (spec 0003 §20).

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
/// Placeholder list at v0. The names are stable — adding or removing one
/// is a spec-governed change. See spec 0003 §20 for the migration to a
/// `cyrs-sema`-owned registry once the real lints land.
pub const REGISTERED_LINT_RULES: &[&str] = &[
    "dead-pattern-var",
    "unused-import-schema",
    "wildcard-return",
];

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
