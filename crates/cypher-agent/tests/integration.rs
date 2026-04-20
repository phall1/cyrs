//! Integration test: spawn the cypher-agent binary, pipe 3 requests,
//! verify 3 responses.
//!
//! Spec 0001 §15 — one-request-per-line, one-response-per-line protocol.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn binary_path() -> std::path::PathBuf {
    // `CARGO_BIN_EXE_cypher-agent` is set by Cargo when running integration
    // tests if the package declares a [[bin]] with that name.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_cypher-agent") {
        return std::path::PathBuf::from(p);
    }
    // Fallback: walk from the integration test binary's location to the
    // target directory and look for the main binary alongside the deps dir.
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop(); // remove test binary name
    // deps/ → pop to reach build profile dir
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("cypher-agent");
    p
}

/// Spawn `cypher-agent`, send `requests` line-by-line, collect that many
/// response lines, then send `{"op":"shutdown"}` and wait for exit.
fn run_requests(requests: &[&str]) -> Vec<serde_json::Value> {
    let bin = binary_path();
    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", bin.display()));

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    // Write all requests then shutdown.
    for req in requests {
        writeln!(stdin, "{req}").unwrap();
    }
    writeln!(stdin, r#"{{"op":"shutdown"}}"#).unwrap();
    drop(stdin);

    // Collect response lines (one per request + one for shutdown).
    let reader = BufReader::new(stdout);
    let mut responses = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("bad JSON from agent: {e}\nline: {line}"));
        let is_shutdown = v["op"] == "shutdown";
        responses.push(v);
        if is_shutdown {
            break;
        }
    }

    child.wait().unwrap();
    // Drop the shutdown response; return only the request responses.
    responses.pop(); // remove shutdown
    responses
}

#[test]
fn three_requests_get_three_responses() {
    let reqs = [
        r#"{"op":"parse","text":"RETURN 1"}"#,
        r#"{"op":"check","text":"MATCH (n) RETURN n"}"#,
        r#"{"op":"format","text":"return 1"}"#,
    ];

    let responses = run_requests(&reqs);

    assert_eq!(
        responses.len(),
        3,
        "expected 3 responses, got {}",
        responses.len()
    );

    // Response 0: parse
    assert_eq!(responses[0]["op"], "parse", "first response must be parse");
    assert!(
        responses[0]["cst_string"].is_string(),
        "parse response must have cst_string"
    );
    assert!(
        responses[0]["syntax_errors"].is_array(),
        "parse response must have syntax_errors"
    );

    // Response 1: check
    assert_eq!(responses[1]["op"], "check", "second response must be check");
    assert!(
        responses[1]["diagnostics"].is_array(),
        "check response must have diagnostics"
    );

    // Response 2: format
    assert_eq!(
        responses[2]["op"], "format",
        "third response must be format"
    );
    assert!(
        responses[2]["formatted"].is_string(),
        "format response must have formatted"
    );
}

#[test]
fn malformed_line_returns_error_not_crash() {
    let reqs = [r#"this is not json at all"#];
    let responses = run_requests(&reqs);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["op"], "error");
    assert!(responses[0]["message"].is_string());
}

#[test]
fn schema_set_then_check() {
    let reqs = [
        r#"{"op":"schema_set","schema_json":{"labels":["Person"],"rel_types":[]}}"#,
        r#"{"op":"check","text":"MATCH (n:Person) RETURN n"}"#,
    ];
    let responses = run_requests(&reqs);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["op"], "schema_set");
    assert_eq!(responses[0]["ok"], true);
    assert_eq!(responses[1]["op"], "check");
    assert!(responses[1]["diagnostics"].is_array());
}
