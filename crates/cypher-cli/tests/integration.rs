//! Integration tests for `cypher` CLI (spec §15, §16).

use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn parse_produces_output() {
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["parse", "-"])
        .write_stdin("MATCH (n) RETURN n")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

/// Spec §16: `cypher check` on a clean query exits 0 with no diagnostic output.
#[test]
fn check_clean_query_exits_zero() {
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check", "-"])
        .write_stdin("MATCH (n) RETURN n")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// Spec §16: `cypher check` on a syntactically broken query emits a
/// rustc-style diagnostic to stderr with a stable `E`-code and exits 1.
#[test]
fn check_bad_syntax_reports_error_and_exits_one() {
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check", "-"])
        .write_stdin("MATCH (n RETURN n")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("error[E"));
}

/// Spec 0002 §12: `cypher schema load <path>` prints a one-line human-
/// readable summary and exits 0.
#[test]
fn schema_load_prints_summary_on_success() {
    let mut tf = tempfile::NamedTempFile::new().expect("tempfile");
    tf.write_all(
        br#"
[[label]]
name = "A"

[[label]]
name = "B"
properties = [{ name = "p", type = "STRING" }]

[[rel_type]]
name = "R"
start_labels = ["A"]
end_labels   = ["B"]

[[parameter]]
name = "x"
type = "INTEGER"
"#,
    )
    .unwrap();
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["schema", "load"])
        .arg(tf.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "loaded schema: 2 labels, 1 rel_types, 1 parameters",
        ));
}

/// Spec 0003 §12: `cypher project load <path>` prints a one-line summary
/// of name, members, dialect, schema, and lint-rule counts; exits 0 on
/// success.
#[test]
fn project_load_prints_summary_on_success() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("samples")).unwrap();
    std::fs::write(root.join("samples/a.cyp"), "MATCH (n) RETURN n\n").unwrap();
    std::fs::write(root.join("samples/b.cyp"), "MATCH (m) RETURN m\n").unwrap();
    std::fs::write(
        root.join("cypher-project.toml"),
        r#"
[project]
name = "demo"

[project.dialect]
default = "GqlAligned"

[project.members]
include = ["samples/*.cyp"]

[project.lint]
"dead-pattern-var" = "warn"
"#,
    )
    .unwrap();

    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["project", "load"])
        .arg(root.join("cypher-project.toml"))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "loaded project 'demo': 2 members, dialect=GqlAligned, schema: none, lint rules: 1",
        ));
}

/// Spec 0003 §7: a manifest with an unknown dialect fails to load; the
/// error goes to stderr and the binary exits 1.
#[test]
fn project_load_reports_error_on_bad_manifest() {
    let mut tf = tempfile::NamedTempFile::new().expect("tempfile");
    tf.write_all(
        br#"
[project]
name = "x"

[project.dialect]
default = "Cypher25"
"#,
    )
    .unwrap();
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["project", "load"])
        .arg(tf.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown dialect"));
}

/// Spec 0002 §11: load errors print to stderr and exit 1.
#[test]
fn schema_load_reports_error_on_bad_file() {
    let mut tf = tempfile::NamedTempFile::new().expect("tempfile");
    tf.write_all(
        br#"
[[label]]
name = "Dup"

[[label]]
name = "Dup"
"#,
    )
    .unwrap();
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["schema", "load"])
        .arg(tf.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("duplicate label"));
}
