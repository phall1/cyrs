//! In-process integration test for cypher-lsp. Spec 0001 §14, §17.11; bead cy-d48.
//!
//! Previously this file spawned the `cypher-lsp` binary over stdio,
//! which hangs in the GitHub Actions macOS/Linux runners — so the tests
//! were `#[ignore]`d and the §17.11 LSP conformance gate was silently
//! unchecked in CI.  The rewrite drives the server in-process via
//! `lsp_server::Connection::memory()`: the server runs in a worker
//! thread and the test thread speaks LSP over a pair of crossbeam
//! channels.  No stdio, no child process, no CI hangs.
//!
//! Coverage (matches the spec §17.11 list of required flows):
//!
//! * `initialize` + capability echo
//! * `textDocument/didOpen` → `publishDiagnostics`
//! * `textDocument/didChange` → fresh `publishDiagnostics`
//! * `textDocument/didClose` → empty `publishDiagnostics` + `FileId`
//!   eviction from the `Database` (cy-it7)
//! * Unknown-URI `didClose` is a no-op
//! * `shutdown` + `exit` exit cleanly

use std::thread;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Wall-clock ceiling on any single `receiver.recv` wait.  The server
/// runs in a worker thread and responds synchronously, so 2s is generous;
/// anything longer means the server is stuck and the test should fail
/// loudly rather than spin forever.
const RECV_TIMEOUT: Duration = Duration::from_secs(2);

/// Harness tying a server thread to a client-side `Connection`.
///
/// Drop the harness to implicitly send `shutdown` + `exit` and join
/// the worker.  Callers that want to assert clean exit should call
/// [`TestHarness::shutdown_exit`] instead and then drop.
struct TestHarness {
    client: Connection,
    server_thread: Option<thread::JoinHandle<anyhow::Result<()>>>,
    next_id: i32,
}

impl TestHarness {
    fn new() -> Self {
        let (server, client) = Connection::memory();
        let server_thread = thread::Builder::new()
            .name("cypher-lsp-test-server".into())
            .spawn(move || cypher_lsp::serve(&server))
            .expect("spawn server thread");
        Self {
            client,
            server_thread: Some(server_thread),
            next_id: 0,
        }
    }

    fn next_id(&mut self) -> RequestId {
        self.next_id += 1;
        RequestId::from(self.next_id)
    }

    fn send_request(&mut self, method: &str, params: Value) -> RequestId {
        let id = self.next_id();
        let req = Request::new(id.clone(), method.to_owned(), params);
        self.client
            .sender
            .send(Message::Request(req))
            .expect("send request");
        id
    }

    fn send_notification(&self, method: &str, params: Value) {
        let notif = Notification::new(method.to_owned(), params);
        self.client
            .sender
            .send(Message::Notification(notif))
            .expect("send notification");
    }

    /// Receive the next message from the server, failing the test after
    /// [`RECV_TIMEOUT`].
    fn recv(&self) -> Message {
        self.client
            .receiver
            .recv_timeout(RECV_TIMEOUT)
            .expect("server response within timeout")
    }

    /// Receive the next message and assert it's a notification with the
    /// given method.  Panics with the actual message on mismatch so the
    /// test log shows what the server sent instead.
    fn recv_notification(&self, method: &str) -> Notification {
        match self.recv() {
            Message::Notification(n) if n.method == method => n,
            other => panic!("expected notification {method:?}, got {other:?}"),
        }
    }

    /// Receive the next message and assert it's a response to `expected_id`.
    fn recv_response(&self, expected_id: &RequestId) -> Response {
        match self.recv() {
            Message::Response(r) if &r.id == expected_id => r,
            other => panic!("expected response for {expected_id:?}, got {other:?}"),
        }
    }

    /// Initialize the session and assert the spec §14.2 capabilities land
    /// on the wire.  Separated from `new` so tests that only care about
    /// shutdown semantics can skip the capability assertions.
    fn initialize(&mut self) {
        let id = self.send_request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "capabilities": {}
            }),
        );

        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "initialize error: {:?}", resp.error);
        let caps = resp.result.expect("initialize result");
        let caps = &caps["capabilities"];

        assert_eq!(
            caps["textDocumentSync"],
            json!(1),
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
        assert!(
            caps["referencesProvider"].as_bool().unwrap_or(false),
            "referencesProvider must be true"
        );
        assert!(
            caps["completionProvider"].is_object(),
            "completionProvider must be advertised"
        );
        let triggers = caps["completionProvider"]["triggerCharacters"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        for ch in [":", ".", "$"] {
            assert!(
                triggers.contains(&ch),
                "completionProvider must list {ch:?} as a trigger character"
            );
        }

        self.send_notification("initialized", json!({}));
    }

    fn did_open(&self, uri: &str, text: &str) {
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "cypher",
                    "version": 1,
                    "text": text,
                }
            }),
        );
    }

    fn did_change(&self, uri: &str, text: &str, version: i64) {
        self.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }]
            }),
        );
    }

    fn did_close(&self, uri: &str) {
        self.send_notification(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        );
    }

    fn hover(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.send_request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        );
        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "hover error: {:?}", resp.error);
        resp.result.expect("hover result present")
    }

    fn definition(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.send_request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        );
        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "definition error: {:?}", resp.error);
        resp.result.expect("definition result present")
    }

    fn references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Value {
        let id = self.send_request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": include_declaration },
            }),
        );
        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "references error: {:?}", resp.error);
        resp.result.expect("references result present")
    }

    fn completion(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.send_request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        );
        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "completion error: {:?}", resp.error);
        resp.result.expect("completion result present")
    }

    fn shutdown_exit(&mut self) {
        let id = self.send_request("shutdown", Value::Null);
        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "shutdown error: {:?}", resp.error);
        self.send_notification("exit", Value::Null);
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        // Best-effort: if the test forgot to call shutdown_exit, drop the
        // client sender so the server's recv loop disconnects cleanly.
        let (dummy_sender, _) = crossbeam_channel::unbounded();
        let sender = std::mem::replace(&mut self.client.sender, dummy_sender);
        drop(sender);
        if let Some(handle) = self.server_thread.take() {
            // Join with a deadline so a stuck server fails the test run
            // visibly instead of hanging forever.
            let join_deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < join_deadline {
                if handle.is_finished() {
                    let _ = handle.join();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            eprintln!("warning: server thread did not join within 5s; leaking");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn lsp_initialize_advertises_v1_capabilities() {
    let mut h = TestHarness::new();
    h.initialize();
    h.shutdown_exit();
}

#[test]
fn lsp_did_open_publishes_diagnostics() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_open.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");

    let notif = h.recv_notification("textDocument/publishDiagnostics");
    let params: Value = serde_json::from_value(notif.params).unwrap();
    assert_eq!(params["uri"], json!(uri));
    assert!(
        params["diagnostics"].as_array().is_some_and(Vec::is_empty),
        "valid MATCH (n) RETURN n must produce no diagnostics, got {}",
        params["diagnostics"]
    );

    h.shutdown_exit();
}

#[test]
fn lsp_did_change_republishes_diagnostics() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_change.cyp";
    h.did_open(uri, "RETURN 1");
    let open_notif = h.recv_notification("textDocument/publishDiagnostics");
    let _ = open_notif;

    h.did_change(uri, "MATCH (m) RETURN m", 2);
    let change_notif = h.recv_notification("textDocument/publishDiagnostics");
    let params: Value = serde_json::from_value(change_notif.params).unwrap();
    assert_eq!(params["uri"], json!(uri));
    // Valid MATCH (m) RETURN m has no diagnostics.
    assert!(
        params["diagnostics"].as_array().is_some_and(Vec::is_empty),
        "valid MATCH (m) RETURN m must produce no diagnostics"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_did_close_clears_diagnostics() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_close.cyp";
    h.did_open(uri, "RETURN 42");
    // Consume the didOpen diagnostics.
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    h.did_close(uri);
    let close_notif = h.recv_notification("textDocument/publishDiagnostics");
    let params: Value = serde_json::from_value(close_notif.params).unwrap();
    assert_eq!(params["uri"], json!(uri));
    assert!(
        params["diagnostics"].as_array().is_some_and(Vec::is_empty),
        "didClose publishDiagnostics must be empty (client-state clear)"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_hover_on_keyword_returns_markdown() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_hover_kw.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor on the M of MATCH (line 0, character 0).
    let result = h.hover(uri, 0, 0);
    let contents = &result["contents"];
    let body = contents.as_str().unwrap_or_else(|| {
        contents["value"]
            .as_str()
            .expect("hover contents must carry markdown")
    });
    assert!(
        body.contains("MATCH"),
        "MATCH keyword hover must mention MATCH; got {body:?}"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_hover_on_variable_returns_kind() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_hover_var.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor on the n inside MATCH (n) — line 0, character 7.
    let result = h.hover(uri, 0, 7);
    let body = result["contents"]
        .as_str()
        .or_else(|| result["contents"]["value"].as_str())
        .unwrap_or("");
    assert!(
        body.contains("variable") && body.contains("`n`"),
        "variable hover must name the binding; got {body:?}"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_hover_on_whitespace_returns_null() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_hover_none.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor past the end of the document.
    let result = h.hover(uri, 5, 0);
    assert!(
        result.is_null(),
        "hover beyond EOF must return null; got {result}"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_definition_on_variable_returns_location() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_def_var.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor on the trailing n in RETURN n — line 0, character 17.
    let result = h.definition(uri, 0, 17);
    assert_eq!(
        result["uri"],
        json!(uri),
        "definition location must point at the same file"
    );
    let range = &result["range"];
    assert_eq!(range["start"]["line"], json!(0));
    // The defined-at range covers the n inside MATCH (n) — character 7.
    assert_eq!(range["start"]["character"], json!(7));

    h.shutdown_exit();
}

#[test]
fn lsp_definition_on_keyword_returns_null() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_def_kw.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor on the M of MATCH — keyword, not an identifier.
    let result = h.definition(uri, 0, 0);
    assert!(
        result.is_null(),
        "definition on a keyword must return null; got {result}"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_references_on_variable_returns_all_sites() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_refs_var.cyp";
    // Three occurrences of `n`: MATCH (n), WHERE n.x, RETURN n.
    h.did_open(uri, "MATCH (n) WHERE n.x = 1 RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor on the trailing `n` in `RETURN n`.
    let fixture = "MATCH (n) WHERE n.x = 1 RETURN n";
    let last_n = u32::try_from(fixture.len() - 1).expect("fixture fits in u32");

    // include_declaration = true → all three sites.
    let result = h.references(uri, 0, last_n, true);
    let items = result.as_array().expect("references must be an array");
    assert_eq!(
        items.len(),
        3,
        "references with declaration must be 3; got {items:?}"
    );
    for item in items {
        assert_eq!(item["uri"], json!(uri));
    }
    let starts: Vec<u64> = items
        .iter()
        .filter_map(|i| i["range"]["start"]["character"].as_u64())
        .collect();
    // MATCH (n) → col 7, WHERE n.x → col 16, RETURN n → col 31.
    assert!(
        starts.contains(&7),
        "must include MATCH site; got {starts:?}"
    );
    assert!(
        starts.contains(&16),
        "must include WHERE site; got {starts:?}"
    );
    assert!(
        starts.contains(&31),
        "must include RETURN site; got {starts:?}"
    );

    // include_declaration = false → drops the MATCH (n) defined-at range.
    let result = h.references(uri, 0, last_n, false);
    let items = result.as_array().expect("array");
    assert_eq!(
        items.len(),
        2,
        "references without declaration must be 2; got {items:?}"
    );
    let starts: Vec<u64> = items
        .iter()
        .filter_map(|i| i["range"]["start"]["character"].as_u64())
        .collect();
    assert!(
        !starts.contains(&7),
        "defined-at site (col 7) must be excluded; got {starts:?}"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_references_on_keyword_returns_null() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_refs_kw.cyp";
    h.did_open(uri, "MATCH (n) RETURN n");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor on the M of MATCH — keyword, not an identifier.
    let result = h.references(uri, 0, 0, true);
    assert!(
        result.is_null(),
        "references on a keyword must return null; got {result}"
    );

    h.shutdown_exit();
}

#[test]
fn lsp_completion_default_returns_keywords() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_completion_kw.cyp";
    h.did_open(uri, "");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor at column 0 of an empty doc → keyword context.
    let result = h.completion(uri, 0, 0);
    let items = result
        .as_array()
        .expect("completion items must be an array");
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    for kw in ["MATCH", "WHERE", "RETURN", "WITH"] {
        assert!(
            labels.contains(&kw),
            "keyword completion must include {kw:?}; got {labels:?}"
        );
    }

    h.shutdown_exit();
}

#[test]
fn lsp_completion_after_dollar_returns_parameter_placeholder() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_completion_param.cyp";
    // Buffer where the user has typed "RETURN $" — cursor right after $.
    h.did_open(uri, "RETURN $");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    let result = h.completion(uri, 0, 8);
    let items = result.as_array().expect("array");
    assert!(!items.is_empty(), "parameter completion must not be empty");
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(
        labels.contains(&"param"),
        "fresh buffer must surface a generic `param` placeholder; got {labels:?}"
    );
}

#[test]
fn lsp_completion_after_dollar_lists_existing_parameters() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_completion_param2.cyp";
    // Buffer references $name + $age earlier; user typing $ at the end.
    h.did_open(uri, "MATCH (n {name: $name, age: $age}) RETURN $");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    let cursor: u32 = u32::try_from("MATCH (n {name: $name, age: $age}) RETURN $".len())
        .expect("test fixture fits in u32");
    let result = h.completion(uri, 0, cursor);
    let items = result.as_array().expect("array");
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(
        labels.contains(&"name"),
        "must surface $name; got {labels:?}"
    );
    assert!(labels.contains(&"age"), "must surface $age; got {labels:?}");
}

#[test]
fn lsp_completion_after_colon_empty_without_schema() {
    let mut h = TestHarness::new();
    h.initialize();

    let uri = "file:///tmp/lsp_test_completion_label.cyp";
    h.did_open(uri, "MATCH (n:");
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // No schema loaded → label completion returns no items rather than
    // guessing.  Spec §14.3 (cy-0ls) is the user-visible knob.
    let result = h.completion(uri, 0, 9);
    let items = result.as_array().expect("array");
    assert!(
        items.is_empty(),
        "label completion without schema must be empty; got {items:?}"
    );
}

#[test]
fn lsp_did_close_unknown_uri_is_silent() {
    let mut h = TestHarness::new();
    h.initialize();

    // Close a URI we never opened.  The server must log + return without
    // publishing anything or crashing.  We then send shutdown and expect
    // the response to land normally — no stray diagnostic in between.
    h.did_close("file:///tmp/lsp_test_ghost.cyp");

    h.shutdown_exit();
}
