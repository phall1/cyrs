//! Integration tests for `cypher` CLI (spec §15, §16).

use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
// `protobuf::Message` lives inside the `index_scip_on_workspace_fixture_emits_valid_file`
// test but clippy `items_after_statements` forbids per-test imports,
// so we hoist it to the top of the module.
use protobuf::Message as _;

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

/// Spec 0003 §6 (bead cy-4yy): `cypher check --lints` runs the
/// clippy-equivalent lint pass and surfaces a `W6xxx` lint to stderr.
/// `MATCH (n), (m) RETURN n` leaves `m` unused (L1 / W6011).
#[test]
fn check_lints_flag_emits_lint_diagnostics() {
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check", "--lints", "-"])
        .write_stdin("MATCH (n), (m) RETURN n")
        .assert()
        // Lints are warning-severity — they never set a non-zero exit.
        .success()
        .stderr(predicate::str::contains("W6011"))
        .stderr(predicate::str::contains("lints:"));
}

/// Without `--lints`, the same query produces no lint output — lints
/// are off by default (spec 0003 §6).
#[test]
fn check_without_lints_flag_is_silent() {
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check", "-"])
        .write_stdin("MATCH (n), (m) RETURN n")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// `cypher check --lints` on a clean query reports zero lints and still
/// exits 0.
#[test]
fn check_lints_flag_clean_query_reports_zero() {
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check", "--lints", "-"])
        .write_stdin("MATCH (n) WHERE n.age > 1 RETURN n")
        .assert()
        .success()
        .stderr(predicate::str::contains("0 lints emitted"));
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

/// Spec 0001 §8 + §11.4: the workspace schema is shared across every
/// member. A query using an undeclared label raises E3001, proving the
/// manifest's schema is actually propagated through the Database.
#[test]
fn check_project_schema_detects_unknown_label_across_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("samples")).unwrap();
    std::fs::write(
        root.join("schema.toml"),
        r#"
[[label]]
name = "Person"
"#,
    )
    .unwrap();
    // `Person` is declared; `Ghost` is not.
    std::fs::write(root.join("samples/ok.cyp"), "MATCH (p:Person) RETURN p\n").unwrap();
    std::fs::write(root.join("samples/bad.cyp"), "MATCH (g:Ghost) RETURN g\n").unwrap();
    std::fs::write(
        root.join("cypher-project.toml"),
        r#"
[project]
name = "ws"

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
        .code(1)
        .stderr(predicate::str::contains("E3001"))
        .stderr(predicate::str::contains("bad.cyp"));
}

/// Spec 0003 §2 + acceptance of cy-o8c tranche 1: the
/// `tests/workspace/` on-disk fixture loads cleanly. Three member files
/// share a single `schema.toml`; `cypher check` must resolve labels
/// declared in the schema from every member regardless of which file
/// first needs them.
#[test]
fn check_project_static_workspace_fixture() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("workspace");
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["check"])
        .arg(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "checked 3 files in project 'tranche1-fixture'",
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
/// Spec 0002 §9: `cypher schema check <path>` runs the linter and
/// emits a one-line issue summary with stable codes.
#[test]
fn schema_check_emits_lint_issues_with_codes() {
    let mut tf = tempfile::NamedTempFile::new().expect("tempfile");
    tf.write_all(
        br#"
[[label]]
name = "Team"

[[label]]
name = "Orphan"

[[rel_type]]
name = "REPORTS_TO"
start_labels = ["Team"]
end_labels   = ["Team"]
"#,
    )
    .unwrap();
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["schema", "check"])
        .arg(tf.path())
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("error[E3011]")
                .and(predicate::str::contains("REPORTS_TO"))
                .and(predicate::str::contains("warning[W6010]"))
                .and(predicate::str::contains("Orphan")),
        )
        .stdout(predicate::str::contains("2 issue(s)"));
}

/// Spec 0002 §9: `cypher schema check` on a clean schema exits 0.
#[test]
fn schema_check_clean_schema_exits_zero() {
    let mut tf = tempfile::NamedTempFile::new().expect("tempfile");
    tf.write_all(
        br#"
[[label]]
name = "Person"

[[label]]
name = "Movie"

[[rel_type]]
name = "ACTED_IN"
start_labels = ["Person"]
end_labels   = ["Movie"]
"#,
    )
    .unwrap();
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["schema", "check"])
        .arg(tf.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("0 issue(s)"));
}

/// Spec 0002 §9: `cypher schema diff old.toml new.toml` emits a stable
/// JSON report. The report is snapshot-tested via `insta` for format
/// stability — downstream CI gates consume this output verbatim.
#[test]
fn schema_diff_emits_stable_json_report() {
    let old = tempfile::NamedTempFile::new().expect("tempfile");
    let new = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(
        old.path(),
        r#"
[[label]]
name = "Person"
properties = [
    { name = "name", type = "STRING", required = true },
    { name = "age",  type = "INTEGER" },
]

[[label]]
name = "Movie"
properties = [
    { name = "title", type = "STRING", required = true },
]

[[rel_type]]
name = "ACTED_IN"
start_labels = ["Person"]
end_labels   = ["Movie"]

[[parameter]]
name    = "since_year"
type    = "INTEGER"
default = 1990
"#,
    )
    .unwrap();
    std::fs::write(
        new.path(),
        r#"
[[label]]
name = "Person"
properties = [
    { name = "name",  type = "STRING", required = true },
    { name = "age",   type = "STRING" },
    { name = "email", type = "STRING" },
]

[[label]]
name = "Director"

[[rel_type]]
name = "ACTED_IN"
start_labels = ["Person"]
end_labels   = ["Person"]

[[parameter]]
name = "since_year"
type = "INTEGER"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["schema", "diff"])
        .arg(old.path())
        .arg(new.path())
        .assert()
        .code(1) // breaking changes present
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).expect("utf-8");
    insta::assert_snapshot!("schema_diff_report", stdout);
}

/// Identical schemas produce an empty diff and exit 0 — the gate's
/// "happy path" for CI usage.
#[test]
fn schema_diff_identical_schemas_empty_report() {
    let old = tempfile::NamedTempFile::new().expect("tempfile");
    let new = tempfile::NamedTempFile::new().expect("tempfile");
    let src = r#"
[[label]]
name = "A"

[[rel_type]]
name = "R"
start_labels = ["A"]
end_labels   = ["A"]
"#;
    std::fs::write(old.path(), src).unwrap();
    std::fs::write(new.path(), src).unwrap();
    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["schema", "diff"])
        .arg(old.path())
        .arg(new.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"adds\": []")
                .and(predicate::str::contains("\"removes\": []"))
                .and(predicate::str::contains("\"breaking\": []")),
        );
}

/// Spec §14, bead cy-o8c tranche 3 / cy-k2r: `cypher index scip` on the
/// tranche 1 workspace fixture writes a non-empty `.scip` file that
/// round-trips through the `scip` crate's proto reader.
#[test]
fn index_scip_on_workspace_fixture_emits_valid_file() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("workspace");
    let out = tempfile::NamedTempFile::new().expect("tempfile");
    let out_path = out.path().to_path_buf();
    // Drop the handle so the bin can overwrite the path; keep the guard
    // alive via `_out_guard` so the file is reaped on test exit.
    let _out_guard = out;

    Command::cargo_bin("cypher")
        .expect("binary exists")
        .args(["index", "scip"])
        .arg(&root)
        .args(["--output"])
        .arg(&out_path)
        .assert()
        .success()
        .stderr(predicate::str::contains("scip: wrote"));

    let bytes = std::fs::read(&out_path).expect("scip file exists");
    assert!(!bytes.is_empty(), "scip file must be non-empty");

    let index = scip::types::Index::parse_from_bytes(&bytes).expect("round-trip");
    assert!(!index.documents.is_empty(), "index must have documents");
    assert!(
        index
            .documents
            .iter()
            .flat_map(|d| d.symbols.iter())
            .any(|s| s.display_name == "Person"),
        "Person label must be indexed"
    );
}
