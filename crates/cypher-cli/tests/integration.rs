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

/// Spec 0003 §2: `cypher check <dir>` discovers the manifest, loads every
/// member, and exits 0 when all files are clean.
#[test]
fn check_project_clean_exits_zero() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("samples")).unwrap();
    std::fs::write(root.join("samples/a.cyp"), "MATCH (n) RETURN n\n").unwrap();
    std::fs::write(root.join("samples/b.cyp"), "RETURN 1\n").unwrap();
    std::fs::write(
        root.join("cypher-project.toml"),
        r#"
[project]
name = "ws"

[project.members]
include = ["samples/*.cyp"]
"#,
    )
    .unwrap();

    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check"])
        .arg(root)
        .assert()
        .success()
        .stderr(predicate::str::contains("checked 2 files in project 'ws'"));
}

/// Spec 0001 §16: `cypher check <dir>` surfaces errors from any member
/// file and exits 1.
#[test]
fn check_project_surfaces_member_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("samples")).unwrap();
    std::fs::write(root.join("samples/ok.cyp"), "MATCH (n) RETURN n\n").unwrap();
    std::fs::write(root.join("samples/bad.cyp"), "MATCH (n RETURN n\n").unwrap();
    std::fs::write(
        root.join("cypher-project.toml"),
        r#"
[project]
name = "ws"

[project.members]
include = ["samples/*.cyp"]
"#,
    )
    .unwrap();

    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check"])
        .arg(root)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("bad.cyp"))
        .stderr(predicate::str::contains("error[E"));
}

/// Spec 0003 §2: `cypher check <dir>` errors cleanly when no manifest
/// exists at or above the target directory.
#[test]
fn check_project_errors_when_no_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check"])
        .arg(tmp.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("no cypher-project.toml"));
}

/// Spec 0003 §4.4 + spec 0001 §8: when the manifest declares a schema,
/// cross-file analysis consults it. A file referencing a label declared
/// in the shared `schema.toml` parses clean; the same query without the
/// schema raises no unknown-label diagnostic either — we only assert the
/// workspace loads end-to-end and all member files are checked.
#[test]
fn check_project_loads_schema_across_members() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("samples")).unwrap();
    std::fs::write(
        root.join("schema.toml"),
        r#"
[[label]]
name = "Movie"
properties = [{ name = "title", type = "STRING", required = true }]

[[label]]
name = "Person"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("samples/movies.cyp"),
        "MATCH (m:Movie) RETURN m.title\n",
    )
    .unwrap();
    std::fs::write(
        root.join("samples/people.cyp"),
        "MATCH (p:Person) RETURN p\n",
    )
    .unwrap();
    std::fs::write(
        root.join("cypher-project.toml"),
        r#"
[project]
name = "wsschema"

[project.members]
include = ["samples/*.cyp"]

[project.schema]
path = "schema.toml"
"#,
    )
    .unwrap();

    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check"])
        .arg(root)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "checked 2 files in project 'wsschema'",
        ));
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
