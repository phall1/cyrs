//! Cross-file LSP navigation fixture (spec §14.2, bead cy-kkw).
//!
//! Builds a small on-disk workspace rooted by a `cypher-project.toml`
//! with a `schema.toml` and three `.cyp` members, then drives the
//! server in-process via `lsp_server::Connection::memory()` to assert:
//!
//! * `textDocument/references` on `:Person` returns the schema
//!   declaration plus every `.cyp` use site across files.
//! * `textDocument/definition` on `:Person` jumps into `schema.toml`.
//! * `workspace/symbol` with the substring `"Perso"` returns a
//!   `Person` entry.
//!
//! The harness is a trimmed copy of `integration.rs` — the shared
//! shim could be lifted into `tests/common.rs` if a third suite
//! appears but for now copy-paste keeps each test file independently
//! reviewable, mirroring the pattern cy-2pk already set.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{Value, json};
use tempfile::TempDir;

const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// In-process LSP harness.  Spawns the server in a worker thread and
/// talks to it over a pair of crossbeam channels (no stdio, no
/// children), the same way `integration.rs` does.
struct TestHarness {
    client: Connection,
    server_thread: Option<thread::JoinHandle<anyhow::Result<()>>>,
    next_id: i32,
}

impl TestHarness {
    fn new() -> Self {
        let (server, client) = Connection::memory();
        let server_thread = thread::Builder::new()
            .name("cypher-lsp-workspace-test".into())
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

    fn recv(&self) -> Message {
        self.client
            .receiver
            .recv_timeout(RECV_TIMEOUT)
            .expect("server response within timeout")
    }

    fn drain_notifications_until_response(&self, expected: &RequestId) -> Response {
        loop {
            if let Message::Response(r) = self.recv()
                && &r.id == expected
            {
                return r;
            }
        }
    }

    fn initialize(&mut self) {
        let id = self.send_request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "features": {}
            }),
        );
        let resp = self.drain_notifications_until_response(&id);
        assert!(resp.error.is_none(), "initialize error: {:?}", resp.error);
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

    fn shutdown(&mut self) {
        let id = self.send_request("shutdown", json!(null));
        let _ = self.drain_notifications_until_response(&id);
        self.send_notification("exit", json!(null));
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        // Best-effort: emit shutdown+exit if the test didn't.  We
        // ignore send errors (the receiver may already be gone) and
        // we swap out the sender after so the server's receive loop
        // gets a closed channel.  Then join — the server exits on
        // the first shutdown/exit it sees.  Without this, a panic
        // mid-test would hang the Drop forever.
        let _ = self.client.sender.send(Message::Request(Request::new(
            RequestId::from(i32::MAX),
            "shutdown".to_owned(),
            json!(null),
        )));
        let _ = self
            .client
            .sender
            .send(Message::Notification(Notification::new(
                "exit".to_owned(),
                json!(null),
            )));
        if let Some(handle) = self.server_thread.take() {
            let _ = handle.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace scaffold
// ---------------------------------------------------------------------------

struct Workspace {
    _dir: TempDir,
    schema_path: PathBuf,
    a_path: PathBuf,
    b_path: PathBuf,
    c_path: PathBuf,
    a_text: String,
    b_text: String,
    c_text: String,
}

impl Workspace {
    fn build() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();

        // cypher-project.toml — includes every .cyp at the root, wires
        // the schema.toml so navigation can resolve label decls.
        fs::write(
            root.join("cypher-project.toml"),
            "[project]\nname = \"kkw-fixture\"\n\n[project.schema]\npath = \"schema.toml\"\n",
        )
        .expect("write cypher-project.toml");

        // schema.toml — declares a single label `Person` and a
        // rel-type `KNOWS`.  Keep the lines short so the label-line
        // scan in workspace_nav produces a small, predictable range.
        let schema_contents = "[meta]\ncyrs_schema_version = \"0.1.0\"\n\n[[label]]\nname = \"Person\"\n\n[[rel_type]]\nname = \"KNOWS\"\nstart_labels = []\nend_labels = []\n";
        let schema_path = root.join("schema.toml");
        fs::write(&schema_path, schema_contents).expect("write schema.toml");

        // Three member .cyp files, each using `:Person` one or more
        // times so we can assert cross-file references.
        let a_text = "MATCH (p:Person) RETURN p\n".to_string();
        let b_text = "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a, b\n".to_string();
        let c_text = "CREATE (c:Person {name: 'c'}) RETURN c\n".to_string();
        let a_path = root.join("a.cyp");
        let b_path = root.join("b.cyp");
        let c_path = root.join("c.cyp");
        fs::write(&a_path, &a_text).expect("write a.cyp");
        fs::write(&b_path, &b_text).expect("write b.cyp");
        fs::write(&c_path, &c_text).expect("write c.cyp");

        // `root` was used only to build the other paths; we don't
        // need it at test time.
        let _ = root;

        Self {
            _dir: dir,
            schema_path,
            a_path,
            b_path,
            c_path,
            a_text,
            b_text,
            c_text,
        }
    }

    fn uri_for(p: &Path) -> String {
        format!("file://{}", p.display())
    }

    /// Byte offset of `needle` in `text`, converted into a UTF-16
    /// LSP Position.  LSP speaks UTF-16 code units, but our fixtures
    /// are ASCII so byte == char == UTF-16 unit.
    fn position_of(text: &str, needle: &str) -> (u32, u32) {
        let byte = text.find(needle).expect("needle not found");
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in text.char_indices() {
            if i == byte {
                return (line, col);
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn workspace_references_on_person_span_all_files() {
    let ws = Workspace::build();
    let mut h = TestHarness::new();
    h.initialize();

    // Open all three .cyp files so the server learns about them.
    h.did_open(&Workspace::uri_for(&ws.a_path), &ws.a_text);
    h.did_open(&Workspace::uri_for(&ws.b_path), &ws.b_text);
    h.did_open(&Workspace::uri_for(&ws.c_path), &ws.c_text);

    // Cursor inside `:Person` in a.cyp.
    let (line, ch) = Workspace::position_of(&ws.a_text, "Person");
    let id = h.send_request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": Workspace::uri_for(&ws.a_path) },
            "position": { "line": line, "character": ch + 1 },
            "context": { "includeDeclaration": true }
        }),
    );
    let resp = h.drain_notifications_until_response(&id);
    assert!(resp.error.is_none(), "references error: {:?}", resp.error);
    let result = resp.result.expect("references result");
    let arr = result.as_array().expect("references must be an array");

    // Collect the URIs of every location so we can assert coverage.
    let uris: Vec<String> = arr
        .iter()
        .filter_map(|v| v["uri"].as_str().map(str::to_owned))
        .collect();

    let schema_uri = Workspace::uri_for(&ws.schema_path);
    let a_uri = Workspace::uri_for(&ws.a_path);
    let b_uri = Workspace::uri_for(&ws.b_path);
    let c_uri = Workspace::uri_for(&ws.c_path);

    assert!(
        uris.iter().any(|u| u == &schema_uri),
        "schema.toml must appear in references; got {uris:?}"
    );
    assert!(
        uris.iter().any(|u| u == &a_uri),
        "a.cyp must appear; got {uris:?}"
    );
    assert!(
        uris.iter().any(|u| u == &b_uri),
        "b.cyp must appear; got {uris:?}"
    );
    assert!(
        uris.iter().any(|u| u == &c_uri),
        "c.cyp must appear; got {uris:?}"
    );
    // b.cyp uses `:Person` twice — make sure both show up.
    let b_count = uris.iter().filter(|u| *u == &b_uri).count();
    assert!(
        b_count >= 2,
        "b.cyp references must include both `:Person` sites; got count={b_count}"
    );

    h.shutdown();
}

#[test]
fn workspace_definition_jumps_to_schema_toml() {
    let ws = Workspace::build();
    let mut h = TestHarness::new();
    h.initialize();

    h.did_open(&Workspace::uri_for(&ws.a_path), &ws.a_text);

    let (line, ch) = Workspace::position_of(&ws.a_text, "Person");
    let id = h.send_request(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": Workspace::uri_for(&ws.a_path) },
            "position": { "line": line, "character": ch + 1 }
        }),
    );
    let resp = h.drain_notifications_until_response(&id);
    assert!(resp.error.is_none(), "definition error: {:?}", resp.error);
    let result = resp.result.expect("definition result");

    // The LSP `Definition` response can be `Location`, `Location[]`,
    // or `LocationLink[]`.  The server returns the scalar variant.
    let uri = result["uri"].as_str().or_else(|| {
        result
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v["uri"].as_str())
    });
    assert_eq!(
        uri,
        Some(Workspace::uri_for(&ws.schema_path).as_str()),
        "goto_definition on :Person must point at schema.toml; got {result}"
    );

    h.shutdown();
}

#[test]
fn workspace_symbol_query_finds_person() {
    let ws = Workspace::build();
    let mut h = TestHarness::new();
    h.initialize();

    // At least one .cyp opened so the server discovers the project.
    h.did_open(&Workspace::uri_for(&ws.a_path), &ws.a_text);

    let id = h.send_request(
        "workspace/symbol",
        json!({
            "query": "Perso"
        }),
    );
    let resp = h.drain_notifications_until_response(&id);
    assert!(
        resp.error.is_none(),
        "workspace/symbol error: {:?}",
        resp.error
    );
    let result = resp.result.expect("workspace/symbol result");
    let arr = result.as_array().expect("symbols must be array");

    let names: Vec<&str> = arr.iter().filter_map(|v| v["name"].as_str()).collect();
    assert!(
        names.contains(&"Person"),
        "workspace_symbols query 'Perso' must surface Person; got {names:?}"
    );

    // And the location should point at schema.toml.
    if let Some(entry) = arr.iter().find(|v| v["name"] == "Person") {
        assert_eq!(
            entry["location"]["uri"].as_str(),
            Some(Workspace::uri_for(&ws.schema_path).as_str()),
            "Person symbol must be located in schema.toml"
        );
    }

    // Keep the workspace dir alive until after the request — dropping
    // TempDir mid-request would race the server's file read.
    drop(ws);
    h.shutdown();
}

#[test]
fn workspace_references_single_file_fallback_still_works() {
    // Regression: when the cursor is on a variable binding (not a
    // workspace symbol), the LSP must still hand off to the
    // single-file references engine (cy-vhe) and return the local
    // uses — not [].
    let ws = Workspace::build();
    let mut h = TestHarness::new();
    h.initialize();

    h.did_open(&Workspace::uri_for(&ws.a_path), &ws.a_text);

    // Cursor on the trailing `p` in `RETURN p` — end-of-file so the
    // rowan token pick unambiguously lands on the IDENT, not the
    // following whitespace / previous `(`.  `a_text` =
    // "MATCH (p:Person) RETURN p\n", so the last `p` is at
    // `len - 2` bytes.
    let last_p_byte = u32::try_from(ws.a_text.len() - 2).expect("fits in u32");
    let id = h.send_request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": Workspace::uri_for(&ws.a_path) },
            "position": { "line": 0, "character": last_p_byte },
            "context": { "includeDeclaration": true }
        }),
    );
    let resp = h.drain_notifications_until_response(&id);
    assert!(resp.error.is_none(), "references error: {:?}", resp.error);
    let result = resp.result.expect("references result");
    // Must be a non-null array with ≥1 entry.
    let arr = result.as_array().unwrap_or_else(|| {
        panic!("references response not array; got {result}");
    });
    assert!(
        !arr.is_empty(),
        "single-file fallback must yield at least one reference for `p`"
    );

    h.shutdown();
}
