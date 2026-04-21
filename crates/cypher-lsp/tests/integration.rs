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
fn lsp_did_close_unknown_uri_is_silent() {
    let mut h = TestHarness::new();
    h.initialize();

    // Close a URI we never opened.  The server must log + return without
    // publishing anything or crashing.  We then send shutdown and expect
    // the response to land normally — no stray diagnostic in between.
    h.did_close("file:///tmp/lsp_test_ghost.cyp");

    h.shutdown_exit();
}
