//! Web-LSP entry point (spec 0004 §7).
//!
//! Compiled only when the `web-lsp` Cargo feature is enabled and the
//! target is `wasm32-unknown-unknown`.  The native stdio build never
//! pulls this module in; the feature-gate at the top of `lib.rs`
//! enforces that.
//!
//! # Topology
//!
//! ```text
//!   Browser main thread                 Dedicated Worker
//!     Monaco editor  ──postMessage──▶   cypher_lsp::start_lsp()
//!                    ◀──postMessage──    (runs the LSP loop)
//! ```
//!
//! `start_lsp` is the `#[wasm_bindgen]` entry point the demo's JS
//! loader calls from inside the worker.  It installs a message handler
//! on `DedicatedWorkerGlobalScope`, wires a [`WebTransport`] to the
//! shared inbound queue, and runs the regular [`serve_transport`] main
//! loop.  Outbound LSP messages go through `postMessage` on the same
//! global scope so the browser thread can forward them to Monaco.
//!
//! The protocol wire format is identical to stdio — JSON-RPC with LSP's
//! `lsp-types` schemas — so the demo can swap its agent-wasm mode for
//! lsp-wasm mode without a new message encoding (spec 0004 §7.2, §10.4).

#![allow(missing_docs)] // wasm-bindgen-generated shims trip the lint.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use lsp_server::{Message, Request, Response};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

use crate::transport::Transport;

// ---------------------------------------------------------------------------
// Inbound queue — buffered by the JS message handler.
// ---------------------------------------------------------------------------

/// Shared inbound queue.  JS `postMessage` handler pushes onto the back;
/// the LSP dispatch loop pops from the front.  Single-threaded by
/// construction — the worker runtime is single-threaded — so a plain
/// `Rc<RefCell<..>>` is sufficient.  No `Send + Sync` needed.
type Inbox = Rc<RefCell<VecDeque<Message>>>;

// ---------------------------------------------------------------------------
// WebTransport
// ---------------------------------------------------------------------------

/// `postMessage` bridge between the LSP server loop and the JS host.
///
/// Inbound messages are pre-parsed by the JS-side handler into JSON and
/// pushed onto [`Inbox`]; this transport drains them one at a time.
/// Outbound [`Message`]s serialize to JSON via `serde_json` and ship
/// out through `self.global.post_message(js_value)`.
///
/// The `handle_shutdown` impl mirrors `lsp_server::Connection`'s
/// two-step handshake: when the `shutdown` request arrives we ack with
/// a null response, flip the `shutdown_acked` flag, and tell the
/// caller "the shutdown handshake is done".  The caller then waits for
/// the follow-up `exit` notification before exiting the loop (the
/// dispatch loop treats any further `handle_shutdown` call after the
/// flag is set as `Ok(true)`, same as lsp-server).
pub struct WebTransport {
    global: DedicatedWorkerGlobalScope,
    inbox: Inbox,
    shutdown_acked: RefCell<bool>,
}

impl std::fmt::Debug for WebTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransport")
            .field("inbox_len", &self.inbox.borrow().len())
            .field("shutdown_acked", &*self.shutdown_acked.borrow())
            .finish_non_exhaustive()
    }
}

impl WebTransport {
    /// Construct a transport tied to the given worker global scope.
    /// `inbox` is shared with the `onmessage` closure installed in
    /// [`start_lsp`].
    pub fn new(global: DedicatedWorkerGlobalScope, inbox: Inbox) -> Self {
        Self {
            global,
            inbox,
            shutdown_acked: RefCell::new(false),
        }
    }

    /// Convert `msg` to JSON and ship it over `postMessage`.
    fn post(&self, msg: &Message) -> Result<()> {
        let json = serde_json::to_string(msg)
            .map_err(|e| anyhow!("web-lsp: serialize outbound message: {e}"))?;
        let js = JsValue::from_str(&json);
        self.global
            .post_message(&js)
            .map_err(|e| anyhow!("web-lsp: postMessage failed: {e:?}"))
    }
}

impl Transport for WebTransport {
    fn recv(&self) -> Result<Option<Message>> {
        // The worker runtime drives message delivery: there is no true
        // blocking `recv` primitive on wasm32 in a worker thread
        // (`atomics.wait` requires cross-origin isolation + a shared
        // buffer).  The `start_lsp` driver instead pumps the inbox on
        // every `onmessage` event — which means the dispatch loop only
        // runs while messages are available.  `recv` therefore pops
        // one message if present, or `Ok(None)` to signal "go back to
        // the JS event loop".
        Ok(self.inbox.borrow_mut().pop_front())
    }

    fn recv_timeout(&self, _timeout: Duration) -> Result<Option<Message>> {
        // Timeouts are a debounce-flush hint on native; on web the
        // event loop already gives us that granularity through
        // `setTimeout` on the JS side.  Return whatever is buffered.
        Ok(self.inbox.borrow_mut().pop_front())
    }

    fn send(&self, msg: Message) -> Result<()> {
        self.post(&msg)
    }

    fn handle_shutdown(&self, req: &Request) -> Result<bool> {
        if req.method != "shutdown" {
            // After a shutdown ack, any further request (other than
            // `exit`) is protocol-invalid — lsp-server returns it to
            // the client as an `InvalidRequest`.  We mirror that.
            if *self.shutdown_acked.borrow() {
                let resp = Response::new_err(
                    req.id.clone(),
                    lsp_server::ErrorCode::InvalidRequest as i32,
                    "Shutdown already requested.".to_owned(),
                );
                self.send(resp.into())?;
                return Ok(true);
            }
            return Ok(false);
        }
        // Ack the shutdown with a null result and flip the flag.
        let resp = Response::new_ok(req.id.clone(), serde_json::Value::Null);
        self.send(resp.into())?;
        *self.shutdown_acked.borrow_mut() = true;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// #[wasm_bindgen] entry point
// ---------------------------------------------------------------------------

/// Initialize the web-LSP server inside a Dedicated Worker.
///
/// Wire-up performed:
///
/// 1. Grab the `DedicatedWorkerGlobalScope` — required; otherwise we're
///    not in a worker and the caller has a bug.
/// 2. Install an `onmessage` closure that parses inbound JSON-RPC
///    messages and pushes them onto the shared [`Inbox`].  The closure
///    also kicks the dispatch loop via [`pump`] so the server processes
///    messages on the same JS event-loop tick.
/// 3. Send a single log line via `console.log` so the demo JS can tell
///    the worker is alive (no structured channel needed; the next
///    message will be an `initialize` request).
///
/// The server loop itself stays driven by `pump`: each JS message
/// delivery pumps the dispatch loop until [`Transport::recv`] returns
/// `Ok(None)`, then we yield back to the JS event loop.  This gives the
/// worker a cooperative-multitasking shape without any `await` on the
/// Rust side.
#[wasm_bindgen]
pub fn start_lsp() -> Result<(), JsValue> {
    let global: DedicatedWorkerGlobalScope = js_sys::global()
        .dyn_into()
        .map_err(|_| JsValue::from_str("start_lsp: not running in a DedicatedWorkerGlobalScope"))?;

    let inbox: Inbox = Rc::new(RefCell::new(VecDeque::new()));

    // Install the inbound postMessage handler.
    let pump_global = global.clone();
    let pump_inbox = inbox.clone();
    let server_cell: Rc<RefCell<Option<WebLoopState>>> = Rc::new(RefCell::new(None));
    let pump_server = server_cell.clone();

    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
        let data = ev.data();
        // Accept either a string (JSON text) or a JS object that
        // serializes via JSON.stringify.  Normalise to a string so
        // serde_json can parse uniformly.
        let json = if let Some(s) = data.as_string() {
            s
        } else if let Ok(jsstr) = js_sys::JSON::stringify(&data) {
            jsstr.into()
        } else {
            web_sys::console::warn_1(&JsValue::from_str(
                "web-lsp: received non-serializable message; dropped",
            ));
            return;
        };
        match serde_json::from_str::<Message>(&json) {
            Ok(msg) => {
                pump_inbox.borrow_mut().push_back(msg);
            }
            Err(e) => {
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "web-lsp: drop malformed message: {e}"
                )));
                return;
            }
        }
        pump(&pump_global, &pump_inbox, &pump_server);
    });
    global.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    // Forget the closure so it stays alive for the worker's lifetime.
    onmessage.forget();

    web_sys::console::log_1(&JsValue::from_str(
        "cypher-lsp: web transport ready (spec 0004 §7)",
    ));
    Ok(())
}

/// State carried between `pump` invocations: the transport + the
/// `Server` that owns the Salsa database.  Instantiated lazily on the
/// first `initialize` request so we don't do any analysis-crate work
/// before the client has handed us its capabilities.
struct WebLoopState {
    transport: WebTransport,
    server: crate::Server,
}

/// Drain as many messages as are currently buffered, running the
/// standard LSP dispatch for each.  Returns once [`Transport::recv`]
/// yields `Ok(None)`; the next `onmessage` event will re-enter.
///
/// On the very first call we synthesize the initialize handshake:
/// the client's `initialize` request is at the head of the inbox,
/// so we pop it, build capabilities, and reply before handing off to
/// the normal dispatch loop.
fn pump(
    global: &DedicatedWorkerGlobalScope,
    inbox: &Inbox,
    server_cell: &Rc<RefCell<Option<WebLoopState>>>,
) {
    // Drive initialization lazily.
    if server_cell.borrow().is_none() {
        let Some(Message::Request(init_req)) = peek_initialize(inbox) else {
            // Haven't seen `initialize` yet — keep waiting.
            return;
        };
        // Pop it now.
        inbox.borrow_mut().pop_front();

        let transport = WebTransport::new(global.clone(), inbox.clone());
        match send_initialize_response(&transport, init_req) {
            Ok((dialect, schema, debounce)) => {
                let server = crate::Server::with_config_and_debounce(dialect, schema, debounce);
                *server_cell.borrow_mut() = Some(WebLoopState { transport, server });
            }
            Err(e) => {
                web_sys::console::error_1(&JsValue::from_str(&format!(
                    "web-lsp: initialize failed: {e:#}"
                )));
                return;
            }
        }
    }

    let mut borrow = server_cell.borrow_mut();
    let Some(state) = borrow.as_mut() else {
        return;
    };
    loop {
        let msg = match state.transport.recv() {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                web_sys::console::error_1(&JsValue::from_str(&format!("web-lsp: recv: {e:#}")));
                break;
            }
        };
        match msg {
            Message::Request(req) => {
                match state.transport.handle_shutdown(&req) {
                    Ok(true) => {
                        crate::evict_all(&mut state.server);
                        // Leave the loop running; a follow-up `exit`
                        // notification (if any) is a no-op at this point.
                        continue;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        web_sys::console::error_1(&JsValue::from_str(&format!(
                            "web-lsp: handle_shutdown: {e:#}"
                        )));
                        continue;
                    }
                }
                if let Err(e) = crate::handle_request(&state.transport, &mut state.server, req) {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "web-lsp: handle_request: {e:#}"
                    )));
                }
            }
            Message::Notification(notif) => {
                if let Err(e) =
                    crate::handle_notification(&state.transport, &mut state.server, notif)
                {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "web-lsp: handle_notification: {e:#}"
                    )));
                }
            }
            Message::Response(_) => {}
        }
    }
}

/// Non-destructive head probe: returns a clone of the first message if
/// it is an `initialize` request, else `None`.
fn peek_initialize(inbox: &Inbox) -> Option<Message> {
    let q = inbox.borrow();
    match q.front() {
        Some(Message::Request(r)) if r.method == "initialize" => Some(Message::Request(r.clone())),
        _ => None,
    }
}

/// Resolved `initializationOptions` triple returned after the web
/// handshake: the dialect, an optional schema provider, and the
/// watched-files debounce window.  Declared as a type alias so the
/// `Arc<dyn SchemaProvider>` doesn't trip clippy's complex-type lint
/// inline.
type InitializeResolved = (
    cypher_db::DialectMode,
    Option<std::sync::Arc<dyn cypher_schema::SchemaProvider>>,
    Duration,
);

/// Parse the `initialize` params, ship the capabilities response, and
/// return the resolved dialect / schema / debounce triple.
///
/// Mirrors the logic in `serve` (lib.rs) — kept separate because we
/// can't call `lsp_server::Connection::initialize` on the web path
/// (no stdio, no threads).
fn send_initialize_response(transport: &WebTransport, req: Request) -> Result<InitializeResolved> {
    let params: lsp_types::InitializeParams = serde_json::from_value(req.params.clone())
        .map_err(|e| anyhow!("initialize: bad params: {e}"))?;

    let capabilities = crate::server_capabilities()?;
    let result = serde_json::json!({
        "capabilities": capabilities,
    });
    transport.send(Response::new_ok(req.id, result).into())?;

    let init = crate::init_options::InitOptions::from_value(params.initialization_options.as_ref());
    let dialect = init.resolved_dialect();
    let schema = crate::init_options::load_schema(&init).ok().flatten();
    Ok((dialect, schema, init.resolved_debounce()))
}
