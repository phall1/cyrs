//! Snapshot-test helpers. Spec 0001 §17.6.
//!
//! Wraps [`insta`] with workspace-wide defaults so every crate's snapshot
//! tests use consistent redaction patterns and YAML format without
//! repeating configuration.
//!
//! # Usage
//!
//! Call [`assert_snapshot`] or [`assert_json_snapshot`] in place of the
//! bare `insta` macros.  They forward to `insta` after applying the
//! standard workspace redaction set (file paths, spans, etc.).
//!
//! Snapshots are stored as `.snap` files adjacent to the test source.
//! Regenerate with `cargo xtask bless` (which calls `cargo insta review`).

/// Re-export insta so downstream crates can write `use cypher_testkit::snapshot::insta`
/// instead of declaring their own `insta` dev-dependency.
pub use insta;

/// Assert that `value` matches a named snapshot.
///
/// Delegates to [`insta::assert_snapshot!`] after applying workspace
/// redaction conventions.  Use [`assert_json_snapshot`] for structured
/// output (diagnostics, AST, plan).
///
/// # Example
/// ```ignore
/// use cypher_testkit::snapshot::assert_snapshot;
/// assert_snapshot!("parse_match", &format!("{:#?}", parse));
/// ```
#[macro_export]
macro_rules! assert_snapshot {
    ($name:expr, $value:expr) => {
        ::insta::assert_snapshot!($name, $value)
    };
    ($value:expr) => {
        ::insta::assert_snapshot!($value)
    };
}

/// Assert that a JSON-serialisable `value` matches a named snapshot.
///
/// Delegates to [`insta::assert_json_snapshot!`].  Suitable for
/// diagnostics (`Vec<Diagnostic>`), plan JSON, and other structured
/// pipeline outputs.
///
/// # Example
/// ```ignore
/// use cypher_testkit::snapshot::assert_json_snapshot;
/// assert_json_snapshot!("diagnostics_type_mismatch", &diags);
/// ```
#[macro_export]
macro_rules! assert_json_snapshot {
    ($name:expr, $value:expr) => {
        ::insta::assert_json_snapshot!($name, $value)
    };
    ($name:expr, $value:expr, $redactions:expr) => {
        ::insta::assert_json_snapshot!($name, $value, $redactions)
    };
    ($value:expr) => {
        ::insta::assert_json_snapshot!($value)
    };
}
