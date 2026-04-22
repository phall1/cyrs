//! Integration tests for the `cypher-project.toml` loader (spec 0003).

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use cypher_project::{
    DialectDefault, LintLevel, ProjectFile, ProjectLoadError, ProjectManifest, load_from_toml_path,
    load_from_toml_str,
};

/// Fixture TOML: exercises every top-level table in spec 0003 §4.
fn fixture_toml() -> String {
    r#"
[project]
name        = "cyrs-demo"
version     = "0.1.0"
description = "Fixture project for the cypher-project.toml loader."

[project.dialect]
default = "GqlAligned"

[project.members]
include = ["samples/*.cyp"]
exclude = ["**/target/**", "**/.git/**"]

[project.lint]
"dead-pattern-var"     = "warn"
"unused-import-schema" = "deny"
"#
    .to_owned()
}

/// Scaffold a directory with a manifest file plus a `samples/` tree.
fn scaffold_with_samples(root: &Path) {
    let samples = root.join("samples");
    fs::create_dir_all(&samples).unwrap();
    fs::write(samples.join("a.cyp"), "MATCH (n) RETURN n\n").unwrap();
    fs::write(samples.join("b.cyp"), "MATCH (m) RETURN m\n").unwrap();
    // Decoy: should not appear in members.
    fs::write(root.join("README.md"), "docs\n").unwrap();
}

#[test]
fn loads_fixture_manifest_and_expands_members() {
    let tmp = tempfile::tempdir().unwrap();
    scaffold_with_samples(tmp.path());
    fs::write(tmp.path().join("cypher-project.toml"), fixture_toml()).unwrap();

    let m = load_from_toml_path(&tmp.path().join("cypher-project.toml")).unwrap();
    assert_eq!(m.name, "cyrs-demo");
    assert_eq!(m.version.as_deref(), Some("0.1.0"));
    assert_eq!(m.dialect.default, DialectDefault::GqlAligned);
    assert_eq!(m.members.len(), 2, "members: {:?}", m.members);
    assert_eq!(m.lint_levels.len(), 2);
    assert_eq!(
        m.lint_levels.get("dead-pattern-var"),
        Some(&LintLevel::Warn)
    );
    assert_eq!(
        m.lint_levels.get("unused-import-schema"),
        Some(&LintLevel::Deny)
    );
    assert!(m.schema.is_none());
}

#[test]
fn round_trip_parse_serialise_parse_is_semantic() {
    let tmp = tempfile::tempdir().unwrap();
    scaffold_with_samples(tmp.path());
    let toml = fixture_toml();
    let m1 = load_from_toml_str(&toml, Some(tmp.path())).unwrap();

    let file = ProjectFile::from_manifest(&m1);
    let reserialised = toml::to_string_pretty(&file).unwrap();
    let m2 = load_from_toml_str(&reserialised, Some(tmp.path())).unwrap();

    assert_eq!(m1.name, m2.name);
    assert_eq!(m1.version, m2.version);
    assert_eq!(m1.description, m2.description);
    assert_eq!(m1.dialect, m2.dialect);
    assert_eq!(m1.include, m2.include);
    assert_eq!(m1.exclude, m2.exclude);
    assert_eq!(m1.lint_levels, m2.lint_levels);
    assert_eq!(m1.members, m2.members);
}

#[test]
fn malformed_dialect_is_unknown_dialect_error() {
    let bad = r#"
[project]
name = "x"

[project.dialect]
default = "Cypher25"
"#;
    let tmp = tempfile::tempdir().unwrap();
    let err = load_from_toml_str(bad, Some(tmp.path())).unwrap_err();
    match err {
        ProjectLoadError::UnknownDialect(s) => assert_eq!(s.as_str(), "Cypher25"),
        other => panic!("expected UnknownDialect, got: {other:?}"),
    }
}

#[test]
fn unknown_lint_rule_is_rejected() {
    let bad = r#"
[project]
name = "x"

[project.lint]
"dead-pattern-var" = "warn"
"not-a-real-rule"  = "deny"
"#;
    let tmp = tempfile::tempdir().unwrap();
    let err = load_from_toml_str(bad, Some(tmp.path())).unwrap_err();
    match err {
        ProjectLoadError::UnknownLintRule(s) => assert_eq!(s.as_str(), "not-a-real-rule"),
        other => panic!("expected UnknownLintRule, got: {other:?}"),
    }
}

#[test]
fn missing_schema_path_is_schema_error() {
    // Valid manifest syntax but the referenced schema file doesn't exist.
    let toml = r#"
[project]
name = "x"

[project.schema]
path = "does-not-exist.toml"
"#;
    let tmp = tempfile::tempdir().unwrap();
    let err = load_from_toml_str(toml, Some(tmp.path())).unwrap_err();
    match err {
        ProjectLoadError::Schema { .. } => {}
        other => panic!("expected Schema error, got: {other:?}"),
    }
}

#[test]
fn bad_glob_is_glob_error() {
    let toml = r#"
[project]
name = "x"

[project.members]
include = ["samples/["]
exclude = []
"#;
    let tmp = tempfile::tempdir().unwrap();
    let err = load_from_toml_str(toml, Some(tmp.path())).unwrap_err();
    match err {
        ProjectLoadError::GlobError { pattern, .. } => {
            assert!(pattern.contains("samples/["), "got: {pattern}");
        }
        other => panic!("expected GlobError, got: {other:?}"),
    }
}

#[test]
fn schema_path_loads_from_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = r#"
[[label]]
name = "Movie"
properties = [{ name = "title", type = "STRING", required = true }]
"#;
    fs::write(tmp.path().join("schema.toml"), schema).unwrap();

    let toml = r#"
[project]
name = "x"

[project.schema]
path = "schema.toml"
"#;
    let m: ProjectManifest = load_from_toml_str(toml, Some(tmp.path())).unwrap();
    let s = m.schema.expect("schema loaded");
    assert_eq!(s.label_count(), 1);
    assert_eq!(m.schema_path.as_deref(), Some(Path::new("schema.toml")));
}

#[test]
fn load_from_toml_path_reports_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("cypher-project.toml");
    let err = load_from_toml_path(&missing).unwrap_err();
    match err {
        ProjectLoadError::ManifestNotFound(p) => assert_eq!(p, missing),
        other => panic!("expected ManifestNotFound, got: {other:?}"),
    }
}
