//! `cypher-tck` — openCypher TCK harness. Spec 0001 §17.5.
//!
//! The TCK is a feature-tagged suite of scenarios. This crate provides
//! the scaffolding to fetch, parse, and execute those scenarios against
//! the front-end. Feature tags map to conformance groups (§17.5); a
//! feature is considered green only when every scenario under that tag
//! passes.
//!
//! v1 scaffolding: exposes [`FeatureGate`] + `run_all` stubs. Scenario
//! fetching and per-tag conformance tracking land alongside the grammar.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-tck/0.0.1")]

/// A named openCypher TCK feature tag. Kept as a string so new tags can
/// be introduced without a crate version bump.
#[derive(Debug, Clone)]
pub struct FeatureGate {
    /// openCypher feature tag (e.g. `@MATCH`, `@OPTIONAL-MATCH`).
    pub tag: &'static str,
    /// Conformance bucket this tag falls into for the active release.
    pub expected: Expected,
}

/// Conformance bucket for a feature tag.  Green scenarios must pass;
/// red scenarios must be rejected (typically with a parse or sema
/// error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// Tag must pass every non-ignored scenario it covers.
    Green,
    /// Tag is explicitly unsupported in v1; scenarios covered by the
    /// tag must be rejected.
    Red,
}

/// Feature gates required by spec §17.5 for v1.
#[must_use]
pub fn v1_gates() -> Vec<FeatureGate> {
    vec![
        FeatureGate {
            tag: "@MATCH",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@OPTIONAL-MATCH",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@WHERE",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@RETURN",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@WITH",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@UNWIND",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@CREATE",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@MERGE",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@SET",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@REMOVE",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@DELETE",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@AGGREGATIONS",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@STRINGS",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@LISTS",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@MAPS",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@PATTERNS",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@NULL",
            expected: Expected::Green,
        },
        FeatureGate {
            tag: "@CALL-SUBQUERY",
            expected: Expected::Red,
        },
        FeatureGate {
            tag: "@EXISTS-SUBQUERY",
            expected: Expected::Red,
        },
        FeatureGate {
            tag: "@LOAD-CSV",
            expected: Expected::Red,
        },
    ]
}
