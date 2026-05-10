//! In-process LSP diagnostic-round-trip latency budget (cy-bod /
//! cy-od5).
//!
//! cy-od5 acceptance includes "demo page renders + diagnoses a
//! 200-line file in <100 ms p95". The demo page is `demo/web/`,
//! which loads `crates/cyrs-lsp` as a wasm artifact via
//! `cargo xtask lsp-web-build`. The same Rust server code runs in
//! the worker; the only added latency is wasm-bindgen marshalling +
//! `postMessage` hops, both small and bounded.
//!
//! We do not run a browser harness in CI (Playwright would be a heavy
//! new dependency for one assertion). Instead, this test exercises
//! the same `cyrs-lsp` round-trip end-to-end **in-process** —
//! `textDocument/didChange` → `publishDiagnostics` — over a 200-line
//! corpus and asserts a p95 budget of 75 ms. The 25 ms headroom up
//! to the demo's 100 ms public budget is reserved for:
//!
//! * wasm-bindgen JSON marshalling on each direction (~3-5 ms each
//!   for a typical message),
//! * the `postMessage` Worker hop on each direction (~1-3 ms each in
//!   modern browsers),
//! * Monaco's marker-application paint cycle.
//!
//! If this test fails, the demo page will *also* fail its 100 ms
//! budget — fix the regression here before changing the public
//! number. If only the demo regresses, suspect wasm-bindgen output
//! size, worker bundle changes, or Monaco — none of which this test
//! covers.

use std::fmt::Write as _;
use std::thread;
use std::time::{Duration, Instant};

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{Value, json};

const RECV_TIMEOUT: Duration = Duration::from_secs(10);
const ITERATIONS: usize = 60;
const WARMUP_ITERATIONS: usize = 10;
/// In-process budget — lower than the demo's 100 ms public number to
/// leave headroom for wasm-bindgen + postMessage + Monaco. See module
/// docs for the breakdown.
const P95_BUDGET_MS: u128 = 75;

struct Harness {
    client: Connection,
    server_thread: Option<thread::JoinHandle<anyhow::Result<()>>>,
    next_id: i32,
}

impl Harness {
    fn new() -> Self {
        let (server, client) = Connection::memory();
        let server_thread = thread::Builder::new()
            .name("cyrs-lsp-demo-latency".into())
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

    fn initialize(&mut self) {
        let id = self.send_request(
            "initialize",
            json!({ "processId": null, "rootUri": null, "capabilities": {} }),
        );
        loop {
            if let Message::Response(r) = self.recv()
                && r.id == id
            {
                assert!(r.error.is_none(), "initialize error: {:?}", r.error);
                break;
            }
        }
        self.send_notification("initialized", json!({}));
    }

    /// Wait for the next `publishDiagnostics` notification matching `uri`.
    /// Drops other messages.
    fn await_publish_diagnostics(&self, uri: &str) {
        loop {
            match self.recv() {
                Message::Notification(n) if n.method == "textDocument/publishDiagnostics" => {
                    if n.params
                        .get("uri")
                        .and_then(Value::as_str)
                        .is_some_and(|u| u == uri)
                    {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn shutdown(&mut self) {
        let id = self.send_request("shutdown", json!(null));
        loop {
            if let Message::Response(r) = self.recv()
                && r.id == id
            {
                break;
            }
        }
        self.send_notification("exit", json!(null));
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
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

/// Build a 200-line Cypher corpus. Each line is a small, syntactically
/// valid statement so the parser does real work but diagnostics stay
/// stable across edits.
fn corpus_200_lines() -> String {
    let mut s = String::with_capacity(8192);
    for i in 0..200 {
        // `write!` on a `String` is infallible — discarding the
        // `fmt::Result` here is safe.
        let _ = writeln!(
            s,
            "MATCH (p{i}:Person {{name: 'p{i}'}})-[:KNOWS]->(f{i}) RETURN p{i}.name AS n{i}, f{i}.name AS m{i};"
        );
    }
    s
}

#[test]
fn demo_round_trip_p95_under_budget() {
    let uri = "inmemory:///demo.cyp";
    let mut h = Harness::new();
    h.initialize();

    // Open the 200-line corpus and wait for the first publishDiagnostics
    // so the server is warm before timing.
    let mut text = corpus_200_lines();
    h.send_notification(
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
    h.await_publish_diagnostics(uri);

    // Drive ITERATIONS + WARMUP_ITERATIONS edits. Each iteration
    // appends a single character to the corpus (a no-op identifier
    // suffix on the last line) and times the round-trip.
    let mut samples: Vec<u128> = Vec::with_capacity(ITERATIONS);
    for i in 0..(ITERATIONS + WARMUP_ITERATIONS) {
        text.push('x');
        let start = Instant::now();
        // Iteration count is bounded by ITERATIONS + WARMUP_ITERATIONS;
        // both are `usize` constants well under `i32::MAX`.
        let version = i32::try_from(i).expect("iteration fits in i32") + 2;
        h.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        );
        h.await_publish_diagnostics(uri);
        let elapsed_ms = start.elapsed().as_millis();
        if i >= WARMUP_ITERATIONS {
            samples.push(elapsed_ms);
        }
    }

    samples.sort_unstable();
    let p95_index = (samples.len() * 95).div_ceil(100) - 1;
    let p95_ms = samples[p95_index];
    let median_ms = samples[samples.len() / 2];
    let max_ms = *samples.last().unwrap();

    eprintln!(
        "demo_round_trip latency over {ITERATIONS} iterations (200-line corpus): \
         median={median_ms}ms p95={p95_ms}ms max={max_ms}ms (budget {P95_BUDGET_MS}ms)"
    );

    assert!(
        p95_ms <= P95_BUDGET_MS,
        "demo round-trip p95 regressed: {p95_ms} ms exceeds in-process budget {P95_BUDGET_MS} ms. \
         The demo page's 100 ms public budget reserves 25 ms headroom for wasm-bindgen + \
         postMessage + Monaco; if this test fails the demo will too. \
         Re-run with `cargo test -p cyrs-lsp --test demo_latency -- --nocapture` to see all samples."
    );

    h.shutdown();
}
