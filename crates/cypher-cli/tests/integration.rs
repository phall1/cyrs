//! Integration tests for `cypher` CLI (spec §15, §16).

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
