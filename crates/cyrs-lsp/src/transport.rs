//! Pluggable LSP transport abstraction (spec 0004 §7).
//!
//! The server's request/notification loop speaks exclusively to a
//! [`Transport`] trait so the same handler code runs over two wire
//! adapters:
//!
//! * [`StdioTransport`] — the default native path, wraps an
//!   `lsp_server::Connection` (stdio or in-memory for tests).  Zero
//!   behaviour change from the pre-m0d world.
//! * `WebTransport` — feature-gated under `web-lsp`, bridges JS
//!   `postMessage` in/out inside a `DedicatedWorkerGlobalScope`.  Lives in
//!   the sibling `web` module so this file stays target-agnostic.
//!
//! Messages are the same `lsp_server::Message` shape both ways: JSON-RPC
//! per spec 0004 §7.2.  Under `web-lsp` we inherit the type via the
//! `lsp-server` dep — that dep is only pulled in on the native build per
//! the Cargo feature gate on `lsp-server` itself; see `Cargo.toml`.

#![allow(dead_code)] // `recv_timeout` unused on web path (no debounce yet).

use std::time::Duration;

use anyhow::Result;
use lsp_server::{Connection, Message};

/// Generic send/receive surface for an LSP transport.
///
/// Implementations own the wire plumbing (stdio pipe, in-memory channel,
/// `postMessage` port).  The server loop is transport-agnostic: it calls
/// [`recv`] / [`recv_timeout`] / [`send`] and lets the adapter handle
/// framing.
///
/// # Error model
///
/// A return of `Err(RecvError::Disconnected)` signals that the peer has
/// hung up and the loop should exit cleanly; `Err(RecvError::Io(_))`
/// propagates as a fatal error.  `send` errors are fatal — on stdio a
/// broken pipe ends the session, on `postMessage` a failed post means
/// the worker is torn down.
///
/// [`recv`]: Transport::recv
/// [`recv_timeout`]: Transport::recv_timeout
/// [`send`]: Transport::send
pub trait Transport {
    /// Block until the next message arrives, or return `Ok(None)` on a
    /// clean disconnect (EOF on stdio, port closed on web).
    fn recv(&self) -> Result<Option<Message>>;

    /// Like [`recv`], but return `Ok(None)` once `timeout` elapses so
    /// the caller can run side-effects (e.g. flush the watched-files
    /// debounce buffer) without blocking forever.
    ///
    /// [`recv`]: Transport::recv
    fn recv_timeout(&self, timeout: Duration) -> Result<Option<Message>>;

    /// Push `msg` onto the outbound wire.
    fn send(&self, msg: Message) -> Result<()>;

    /// Return `Ok(true)` if the given request is the shutdown request
    /// and has been acknowledged on the wire; `Ok(false)` otherwise.
    ///
    /// The stdio path delegates to `lsp_server::Connection::handle_shutdown`
    /// which already tracks the shutdown/exit handshake.  The web path
    /// implements the same trivial state machine inline.
    fn handle_shutdown(&self, req: &lsp_server::Request) -> Result<bool>;
}

// ---------------------------------------------------------------------------
// StdioTransport — native default, wraps lsp_server::Connection.
// ---------------------------------------------------------------------------

/// Native transport backed by an `lsp_server::Connection`.
///
/// The wrapped connection may be `Connection::stdio()` (production
/// binary) or `Connection::memory()` (integration tests).  This adapter
/// does nothing beyond forwarding to the crossbeam channels — all the
/// stdio framing, JSON-RPC parsing, and shutdown bookkeeping lives in
/// `lsp-server` itself.
pub struct StdioTransport<'conn> {
    connection: &'conn Connection,
}

impl std::fmt::Debug for StdioTransport<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `lsp_server::Connection` doesn't implement Debug; print a
        // placeholder so workspace-wide `missing_debug_implementations`
        // stays green without leaking channel internals.
        f.debug_struct("StdioTransport")
            .field("connection", &"<lsp_server::Connection>")
            .finish()
    }
}

impl<'conn> StdioTransport<'conn> {
    /// Wrap a borrowed `Connection` as a [`Transport`].  The caller
    /// retains ownership; dropping the transport leaves the connection
    /// untouched so `IoThreads::join` on the calling binary still works.
    pub fn new(connection: &'conn Connection) -> Self {
        Self { connection }
    }
}

impl Transport for StdioTransport<'_> {
    fn recv(&self) -> Result<Option<Message>> {
        match self.connection.receiver.recv() {
            Ok(msg) => Ok(Some(msg)),
            // crossbeam `RecvError` only fires on disconnect — treat as
            // clean EOF so the loop exits cleanly.
            Err(_) => Ok(None),
        }
    }

    fn recv_timeout(&self, timeout: Duration) -> Result<Option<Message>> {
        match self.connection.receiver.recv_timeout(timeout) {
            Ok(msg) => Ok(Some(msg)),
            Err(e) if e.is_timeout() => Ok(None),
            // Disconnected — same clean-EOF treatment as `recv`.
            Err(_) => Ok(None),
        }
    }

    fn send(&self, msg: Message) -> Result<()> {
        self.connection
            .sender
            .send(msg)
            .map_err(|e| anyhow::anyhow!("send failed: {e}"))
    }

    fn handle_shutdown(&self, req: &lsp_server::Request) -> Result<bool> {
        self.connection
            .handle_shutdown(req)
            .map_err(|e| anyhow::anyhow!("handle_shutdown failed: {e}"))
    }
}
