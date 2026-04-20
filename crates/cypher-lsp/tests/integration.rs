//! Integration test for cypher-lsp stdio transport. Spec 0001 §14.
//!
//! Spawns the binary, sends JSON-RPC messages over stdin, and reads
//! responses + notifications from stdout to verify the server behaves
//! correctly for the v1 capability set.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Encode a JSON-RPC message with the LSP Content-Length framing.
fn encode(msg: &Value) -> Vec<u8> {
    let body = serde_json::to_string(msg).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Read one LSP message (Content-Length + body) from a `BufReader`.
fn read_message(reader: &mut BufReader<impl std::io::Read>) -> Value {
    use std::io::Read;
    // Read headers until we get the blank separator line.
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read header line");
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length: ") {
            content_length = rest.trim().parse().expect("parse Content-Length");
        }
    }
    assert!(content_length > 0, "Content-Length must be > 0");

    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf).expect("read body");
    serde_json::from_slice(&buf).expect("parse JSON body")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
#[ignore = "spawned-process LSP integration hangs in CI harness; manually verified. Follow-up: rewrite against in-process lsp-server::Connection::pair()."]
fn lsp_initialize_didopen_diagnostics() {
    // Build the binary first.
    let bin_path = {
        let output = Command::new("cargo")
            .args(["build", "-p", "cypher-lsp", "--message-format=json"])
            .output()
            .expect("cargo build");

        assert!(
            output.status.success(),
            "cargo build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Find the binary path from cargo's JSON messages.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut bin = None;
        for line in stdout.lines() {
            if let Ok(msg) = serde_json::from_str::<Value>(line)
                && msg["reason"] == "compiler-artifact"
                && msg["target"]["name"] == "cypher-lsp"
                && msg["target"]["kind"]
                    .as_array()
                    .is_some_and(|k| k.contains(&json!("bin")))
                && let Some(exe) = msg["executable"].as_str()
            {
                bin = Some(exe.to_owned());
            }
        }
        bin.expect("could not find cypher-lsp binary in cargo output")
    };

    let mut child = Command::new(&bin_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cypher-lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // 1. initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {}
        }
    });
    stdin
        .write_all(&encode(&init_req))
        .expect("write initialize");

    // Read the initialize response.
    let init_resp = read_message(&mut reader);
    assert_eq!(init_resp["id"], json!(1), "initialize response id");
    let caps = &init_resp["result"]["capabilities"];
    assert_eq!(
        caps["textDocumentSync"],
        json!(1), // Full = 1
        "textDocumentSync must be Full"
    );
    assert!(
        caps["documentFormattingProvider"]
            .as_bool()
            .unwrap_or(false),
        "documentFormattingProvider must be true"
    );
    assert!(
        caps["hoverProvider"].as_bool().unwrap_or(false),
        "hoverProvider must be true"
    );
    assert!(
        caps["definitionProvider"].as_bool().unwrap_or(false),
        "definitionProvider must be true"
    );

    // 2. initialized notification (no-op)
    let initialized = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    stdin
        .write_all(&encode(&initialized))
        .expect("write initialized");

    // 3. textDocument/didOpen with a simple MATCH query
    let did_open = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tmp/test.cyp",
                "languageId": "cypher",
                "version": 1,
                "text": "MATCH (n) RETURN n"
            }
        }
    });
    stdin.write_all(&encode(&did_open)).expect("write didOpen");

    // Expect a textDocument/publishDiagnostics notification.
    let diag_notif = read_message(&mut reader);
    assert_eq!(
        diag_notif["method"],
        json!("textDocument/publishDiagnostics"),
        "must receive publishDiagnostics"
    );
    assert_eq!(
        diag_notif["params"]["uri"],
        json!("file:///tmp/test.cyp"),
        "diagnostics URI must match"
    );
    // A valid MATCH...RETURN should have no diagnostics.
    let diags = &diag_notif["params"]["diagnostics"];
    assert!(
        diags.as_array().is_some_and(std::vec::Vec::is_empty),
        "valid MATCH (n) RETURN n must produce no diagnostics, got: {diags}"
    );

    // 4. Shutdown sequence.
    let shutdown_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    });
    stdin
        .write_all(&encode(&shutdown_req))
        .expect("write shutdown");

    let shutdown_resp = read_message(&mut reader);
    assert_eq!(shutdown_resp["id"], json!(2), "shutdown response id");

    let exit_notif = json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    stdin.write_all(&encode(&exit_notif)).expect("write exit");
    drop(stdin);

    let status = child.wait().expect("wait");
    assert!(status.success(), "cypher-lsp must exit cleanly: {status}");
}

/// Verify that `textDocument/didClose` causes the server to publish empty
/// diagnostics (clearing client-side highlights) and does not crash.
///
/// Also verifies that closing an unknown URI is silently ignored (no crash,
/// server continues to respond to subsequent requests).
///
/// Marked `#[ignore]` because spawning a stdio process hangs in the CI
/// harness (see cy-9nr / existing test above).  Run manually with:
///   cargo test -p cypher-lsp -- --ignored `lsp_did_close_eviction`
#[test]
#[ignore = "spawned-process LSP integration hangs in CI harness; manually verified. See cy-it7."]
fn lsp_did_close_eviction() {
    let bin_path = {
        let output = Command::new("cargo")
            .args(["build", "-p", "cypher-lsp", "--message-format=json"])
            .output()
            .expect("cargo build");

        assert!(output.status.success(), "cargo build failed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut bin = None;
        for line in stdout.lines() {
            if let Ok(msg) = serde_json::from_str::<Value>(line)
                && msg["reason"] == "compiler-artifact"
                && msg["target"]["name"] == "cypher-lsp"
                && msg["target"]["kind"]
                    .as_array()
                    .is_some_and(|k| k.contains(&json!("bin")))
                && let Some(exe) = msg["executable"].as_str()
            {
                bin = Some(exe.to_owned());
            }
        }
        bin.expect("could not find cypher-lsp binary")
    };

    let mut child = Command::new(&bin_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cypher-lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // initialize
    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "initialize",
            "params": { "processId": null, "rootUri": null, "capabilities": {} }
        })))
        .unwrap();
    let _init_resp = read_message(&mut reader);

    // initialized
    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        })))
        .unwrap();

    // didOpen
    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///tmp/evict_test.cyp",
                    "languageId": "cypher",
                    "version": 1,
                    "text": "RETURN 42"
                }
            }
        })))
        .unwrap();
    // consume publishDiagnostics from didOpen
    let _diag = read_message(&mut reader);

    // didClose — must receive empty publishDiagnostics to clear client state
    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {
                "textDocument": { "uri": "file:///tmp/evict_test.cyp" }
            }
        })))
        .unwrap();
    let close_diag = read_message(&mut reader);
    assert_eq!(
        close_diag["method"],
        json!("textDocument/publishDiagnostics"),
        "didClose must emit publishDiagnostics to clear client highlights"
    );
    assert!(
        close_diag["params"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "didClose publishDiagnostics must be empty"
    );

    // didClose on unknown URI — server must NOT crash; subsequent shutdown works
    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {
                "textDocument": { "uri": "file:///tmp/ghost.cyp" }
            }
        })))
        .unwrap();
    // No message expected (server logs warn and returns without publishing).

    // shutdown + exit — server must still respond cleanly
    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "shutdown",
            "params": null
        })))
        .unwrap();
    let shutdown_resp = read_message(&mut reader);
    assert_eq!(shutdown_resp["id"], json!(2), "shutdown response id");

    stdin
        .write_all(&encode(&json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        })))
        .unwrap();
    drop(stdin);

    let status = child.wait().expect("wait");
    assert!(
        status.success(),
        "cypher-lsp must exit cleanly after eviction: {status}"
    );
}
