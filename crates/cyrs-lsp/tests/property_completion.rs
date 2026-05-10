//! In-process integration tests for property-key completion (spec §14.2,
//! bead cy-2pk).
//!
//! The harness drives the server via `lsp_server::Connection::memory()`
//! the same way `integration.rs` does; the duplicated shim keeps this
//! file self-contained so the cy-2pk bead can be reviewed / bisected in
//! isolation.  If a second feature suite later needs the same harness,
//! lift the shim into a shared `tests/common.rs` module.

use std::thread;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};

const RECV_TIMEOUT: Duration = Duration::from_secs(2);

struct TestHarness {
    client: Connection,
    server_thread: Option<thread::JoinHandle<anyhow::Result<()>>>,
    next_id: i32,
}

impl TestHarness {
    fn new() -> Self {
        let (server, client) = Connection::memory();
        let server_thread = thread::Builder::new()
            .name("cyrs-lsp-prop-completion-test".into())
            .spawn(move || cyrs_lsp::serve(&server))
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

    fn recv(&self) -> Message {
        self.client
            .receiver
            .recv_timeout(RECV_TIMEOUT)
            .expect("server response within timeout")
    }

    fn recv_notification(&self, method: &str) -> Notification {
        match self.recv() {
            Message::Notification(n) if n.method == method => n,
            other => panic!("expected notification {method:?}, got {other:?}"),
        }
    }

    fn recv_response(&self, expected_id: &RequestId) -> Response {
        match self.recv() {
            Message::Response(r) if &r.id == expected_id => r,
            other => panic!("expected response for {expected_id:?}, got {other:?}"),
        }
    }

    fn initialize_with_options(&mut self, options: Value) {
        let id = self.send_request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "capabilities": {},
                "initializationOptions": options,
            }),
        );
        let resp = self.recv_response(&id);
        assert!(resp.error.is_none(), "initialize error: {:?}", resp.error);
        // Assert the `.` trigger character is advertised — if a merge
        // accidentally drops it the property-completion flow below
        // would still work (we fall back to the char-before-cursor
        // heuristic) but real editors would stop asking.
        let caps = resp.result.expect("initialize result");
        let triggers = caps["capabilities"]["completionProvider"]["triggerCharacters"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(
            triggers.contains(&"."),
            "completionProvider must advertise `.` as a trigger character; got {triggers:?}"
        );
        self.send_notification("initialized", json!({}));
    }

    fn initialize_no_schema(&mut self) {
        self.initialize_with_options(json!({ "schemaSource": "none" }));
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

    /// Request completion at (line, character) with a `.` trigger
    /// context — the shape the client sends when the user types `.`.
    fn completion_after_dot(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let id = self.send_request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": {
                    "triggerKind": 2,            // TriggerCharacter
                    "triggerCharacter": ".",
                },
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
        let (dummy_sender, _) = crossbeam_channel::unbounded();
        let sender = std::mem::replace(&mut self.client.sender, dummy_sender);
        drop(sender);
        if let Some(handle) = self.server_thread.take() {
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
// Fixtures
// ---------------------------------------------------------------------------

/// Write a tempfile JSON schema declaring `Person{ name, age }` and
/// return it so the test keeps the file alive for the lifetime of the
/// LSP session.
fn person_schema() -> tempfile::NamedTempFile {
    let tmp = tempfile::NamedTempFile::new().expect("create tmp file");
    std::fs::write(
        tmp.path(),
        r#"{
            "labels": ["Person"],
            "rel_types": [],
            "node_properties": {
                "Person": [
                    {"name": "name", "type": "String", "required": true},
                    {"name": "age", "type": "Int", "required": false}
                ]
            }
        }"#,
    )
    .expect("write schema");
    tmp
}

fn init_with_schema(h: &mut TestHarness, schema_path: &std::path::Path) {
    h.initialize_with_options(json!({
        "schemaSource": "file",
        "schemaPath": schema_path.to_string_lossy(),
        "dialect": "GqlAligned",
    }));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Schema loaded with `Person{name, age}`; query `MATCH (n:Person)
/// WHERE n.<cursor>` → property keys `name` and `age`.
#[test]
fn property_completion_surfaces_schema_properties() {
    let schema = person_schema();
    let mut h = TestHarness::new();
    init_with_schema(&mut h, schema.path());

    let uri = "file:///tmp/prop_completion_person.cyp";
    let source = "MATCH (n:Person) WHERE n.";
    h.did_open(uri, source);
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    // Cursor sits right after the `.`, i.e. column = source.len().
    let col = u32::try_from(source.len()).expect("fixture fits in u32");
    let result = h.completion_after_dot(uri, 0, col);
    let items = result.as_array().expect("completion returns an array");
    let labels: std::collections::BTreeSet<&str> =
        items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert_eq!(
        labels,
        ["age", "name"].into_iter().collect(),
        "property completion must return exactly the schema's Person properties; got {labels:?}"
    );
    // Each item is a field-kind completion.
    for item in items {
        assert_eq!(
            item["kind"],
            json!(5),
            "CompletionItemKind::FIELD is 5; got {item}"
        );
    }

    h.shutdown_exit();
}

/// No schema loaded: even a perfectly bound `n:Person` returns empty
/// completions — the engine never guesses.
#[test]
fn property_completion_without_schema_is_empty() {
    let mut h = TestHarness::new();
    h.initialize_no_schema();

    let uri = "file:///tmp/prop_completion_no_schema.cyp";
    let source = "MATCH (n:Person) WHERE n.";
    h.did_open(uri, source);
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    let col = u32::try_from(source.len()).expect("fixture fits in u32");
    let result = h.completion_after_dot(uri, 0, col);
    let items = result.as_array().expect("array");
    assert!(
        items.is_empty(),
        "without a schema, property completion must be empty; got {items:?}"
    );

    h.shutdown_exit();
}

/// Schema is loaded but the identifier before `.` is unbound
/// (`ghost.`); completion is empty.
#[test]
fn property_completion_unbound_identifier_is_empty() {
    let schema = person_schema();
    let mut h = TestHarness::new();
    init_with_schema(&mut h, schema.path());

    let uri = "file:///tmp/prop_completion_unbound.cyp";
    // `ghost` is never bound in the query.
    let source = "MATCH (n:Person) WHERE ghost.";
    h.did_open(uri, source);
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    let col = u32::try_from(source.len()).expect("fixture fits in u32");
    let result = h.completion_after_dot(uri, 0, col);
    let items = result.as_array().expect("array");
    assert!(
        items.is_empty(),
        "property completion on an unbound identifier must be empty; got {items:?}"
    );

    h.shutdown_exit();
}

/// Schema loaded but the binding's label is not in the schema: we do
/// not hallucinate properties, so completion is empty.
#[test]
fn property_completion_unknown_label_is_empty() {
    // Schema declares `Person` only; query binds `Widget` which the
    // schema does not know about.
    let schema = person_schema();
    let mut h = TestHarness::new();
    init_with_schema(&mut h, schema.path());

    let uri = "file:///tmp/prop_completion_unknown_label.cyp";
    let source = "MATCH (n:Widget) WHERE n.";
    h.did_open(uri, source);
    let _ = h.recv_notification("textDocument/publishDiagnostics");

    let col = u32::try_from(source.len()).expect("fixture fits in u32");
    let result = h.completion_after_dot(uri, 0, col);
    let items = result.as_array().expect("array");
    assert!(
        items.is_empty(),
        "property completion on an unknown label must be empty; got {items:?}"
    );

    h.shutdown_exit();
}
