//! Cypher language server — reusable library surface. Spec 0001 §14.
//!
//! The crate ships a binary (`cypher-lsp`) that wires `Connection::stdio()`
//! to [`serve`].  The library split exists so that integration tests can
//! drive the server against [`lsp_server::Connection::memory`] in-process
//! instead of spawning a child process (bead cy-d48): stdio-spawn tests
//! hang in the GitHub Actions macOS runners, and the in-process pattern
//! is also how `rust-analyzer` and other mature LSP servers test their
//! conformance suites.
//!
//! # Public entry points
//!
//! * [`server_capabilities`] — the `ServerCapabilities` JSON blob advertised
//!   during `initialize`.  Exposed for both the stdio bin and the test
//!   harness.
//! * [`serve`] — run `initialize` + the message loop on a pre-built
//!   [`Connection`].  Takes ownership of neither the transport nor the
//!   `IoThreads`; the caller is responsible for joining threads.
//!
//! # Internal items
//!
//! Everything else (server state, per-message handlers, diagnostic
//! publishing) is `pub(crate)` so tests can reach in without leaking
//! internals to downstream crates.  If a downstream tool needs these
//! handlers, promote them explicitly with a code review.
//!
//! ## `FileId` eviction (spec §15.X)
//!
//! * `textDocument/didClose`: evicts the `FileId` from `Database` so Salsa
//!   can GC the cached analysis.  Unknown URIs are logged and silently
//!   ignored.
//! * `shutdown`: evicts **all** open `FileId`s before returning so that
//!   Salsa state is cleanly torn down.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, HoverParams, OneOf, PublishDiagnosticsParams, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
};

use cypher_db::workspace::FileId;
use cypher_db::{Database, DialectMode};
use cypher_diag::to_lsp_all;
use cypher_syntax::LineIndex;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Build the `ServerCapabilities` JSON value this server advertises on
/// `initialize`.
///
/// Kept on the public surface so both the stdio bin and the integration
/// test harness stay in sync without copy-pasting the struct literal.
pub fn server_capabilities() -> Result<serde_json::Value> {
    Ok(serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_formatting_provider: Some(OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        ..Default::default()
    })?)
}

/// Run `initialize` + the message loop on a pre-built `Connection`.
///
/// Returns once the client sends `shutdown` + `exit`, after evicting all
/// open `FileId`s so the underlying Salsa database can reclaim memoised
/// state (spec §15.X).
///
/// This function does not touch stdio or tracing: the caller decides
/// whether the transport is `Connection::stdio()` (production bin) or
/// `Connection::memory()` (in-process tests).
pub fn serve(connection: &Connection) -> Result<()> {
    let capabilities = server_capabilities()?;
    let initialization_params = connection
        .initialize(capabilities)
        .map_err(|e| anyhow!("initialize failed: {e}"))?;
    let _params: lsp_types::InitializeParams = serde_json::from_value(initialization_params)?;

    main_loop(connection)
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

pub(crate) struct Server {
    pub(crate) db: Database,
    /// Map from URI string → `FileId` for open documents.
    pub(crate) open_files: HashMap<String, FileId>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("db", &"<Database>")
            .field("open_files", &self.open_files.len())
            .finish()
    }
}

impl Server {
    pub(crate) fn new() -> Self {
        Self {
            db: Database::new(),
            open_files: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main_loop(connection: &Connection) -> Result<()> {
    let mut server = Server::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    // Spec §15.X: evict all open FileIds before exiting so
                    // Salsa's memoisation cache can be cleanly reclaimed.
                    evict_all(&mut server);
                    return Ok(());
                }
                handle_request(connection, &mut server, req)?;
            }
            Message::Notification(notif) => {
                handle_notification(connection, &mut server, notif)?;
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// Evict all currently-open files from the database (spec §15.X).
///
/// Called on shutdown so that Salsa's memoisation tables can GC the cached
/// analysis before the process exits.  Silently ignores any `remove_file`
/// errors (the DB is being discarded anyway).
pub(crate) fn evict_all(server: &mut Server) {
    let ids: Vec<_> = server.open_files.values().copied().collect();
    let count = ids.len();
    server.open_files.clear();
    for id in ids {
        let _ = server.db.remove_file(id);
    }
    tracing::info!("shutdown eviction: removed {count} open file(s) from Salsa cache");
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

fn handle_request(connection: &Connection, server: &mut Server, req: Request) -> Result<()> {
    match req.method.as_str() {
        lsp_types::request::Formatting::METHOD => {
            let params: DocumentFormattingParams = serde_json::from_value(req.params)?;
            let resp = handle_formatting(server, req.id, &params);
            connection
                .sender
                .send(resp.into())
                .map_err(|e| anyhow!("{e}"))?;
        }
        lsp_types::request::HoverRequest::METHOD => {
            let _params: HoverParams = serde_json::from_value(req.params)?;
            // v1: binding-overlay lookup deferred; return null.
            let resp = Response::new_ok(req.id, serde_json::Value::Null);
            connection
                .sender
                .send(resp.into())
                .map_err(|e| anyhow!("{e}"))?;
        }
        lsp_types::request::GotoDefinition::METHOD => {
            // Stub: always returns null.
            let resp = Response::new_ok(req.id, serde_json::Value::Null);
            connection
                .sender
                .send(resp.into())
                .map_err(|e| anyhow!("{e}"))?;
        }
        _ => {
            tracing::debug!("unhandled request: {}", req.method);
        }
    }
    Ok(())
}

fn handle_formatting(
    server: &mut Server,
    id: RequestId,
    params: &DocumentFormattingParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_err(
            id,
            lsp_server::ErrorCode::InvalidParams as i32,
            format!("file not open: {uri_str}"),
        );
    };

    let source = match server.db.source_of(file_id) {
        Ok(s) => s,
        Err(e) => {
            return Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                e.to_string(),
            );
        }
    };

    let formatted = cypher_fmt::format(&source);

    if formatted == source {
        // No changes needed.
        return Response::new_ok(id, serde_json::Value::Array(vec![]));
    }

    // Replace the entire document with one TextEdit.
    // Compute the end position by counting lines and last-line length.
    let mut line_count: u32 = 0;
    let mut last_line_char_count: u32 = 0;
    for line in source.lines() {
        line_count += 1;
        last_line_char_count = u32::try_from(line.chars().count()).unwrap_or(u32::MAX);
    }
    // If the source is empty, line_count stays 0 and end stays (0,0).
    let end_line = if line_count == 0 { 0 } else { line_count - 1 };

    let edit = TextEdit {
        range: lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(end_line, last_line_char_count),
        ),
        new_text: formatted,
    };

    match serde_json::to_value(vec![edit]) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Notification dispatch
// ---------------------------------------------------------------------------

fn handle_notification(
    connection: &Connection,
    server: &mut Server,
    notif: Notification,
) -> Result<()> {
    match notif.method.as_str() {
        lsp_types::notification::Initialized::METHOD => {
            // No-op ack.
            tracing::debug!("initialized");
        }
        lsp_types::notification::DidOpenTextDocument::METHOD => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri;
            let text = params.text_document.text;
            let uri_str = uri.to_string();

            let file_id =
                server
                    .db
                    .open_file(Path::new(uri_str.as_str()), text, DialectMode::GqlAligned);
            server.open_files.insert(uri_str, file_id);

            publish_diagnostics(connection, server, &uri)?;
        }
        lsp_types::notification::DidChangeTextDocument::METHOD => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri;
            let uri_str = uri.to_string();

            // Full sync — last change is the full content.
            if let Some(change) = params.content_changes.into_iter().last()
                && let Some(&file_id) = server.open_files.get(&uri_str)
            {
                let _ = server.db.update_file(file_id, change.text);
            }

            publish_diagnostics(connection, server, &uri)?;
        }
        lsp_types::notification::DidCloseTextDocument::METHOD => {
            let params: DidCloseTextDocumentParams = serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri.clone();
            let uri_str = uri.to_string();

            // Spec §15.X: evict FileId so Salsa can GC the cached analysis.
            // Unknown URI → log and return; do not panic.
            if let Some(file_id) = server.open_files.remove(&uri_str) {
                let _ = server.db.remove_file(file_id);
                tracing::debug!("didClose: evicted {file_id} for {uri_str}");
            } else {
                tracing::warn!(
                    "didClose: unknown URI {uri_str} — no FileId to evict (already closed?)"
                );
                return Ok(());
            }

            // Clear diagnostics on close.
            let clear = PublishDiagnosticsParams {
                uri,
                diagnostics: vec![],
                version: None,
            };
            send_notification::<lsp_types::notification::PublishDiagnostics>(connection, &clear)?;
        }
        _ => {
            tracing::debug!("unhandled notification: {}", notif.method);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Diagnostics helper
// ---------------------------------------------------------------------------

fn publish_diagnostics(connection: &Connection, server: &mut Server, uri: &Uri) -> Result<()> {
    let uri_str = uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Ok(());
    };

    let source = server.db.source_of(file_id).unwrap_or_default();
    let line_index = LineIndex::new(&source);

    let lsp_diags = match server.db.all_diagnostics(file_id) {
        Ok(diags_out) => to_lsp_all(diags_out.diagnostics(), uri, &line_index),
        Err(e) => {
            tracing::warn!("diagnostics error: {e}");
            vec![]
        }
    };

    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: lsp_diags,
        version: None,
    };

    send_notification::<lsp_types::notification::PublishDiagnostics>(connection, &params)
}

fn send_notification<N: lsp_types::notification::Notification>(
    connection: &Connection,
    params: &N::Params,
) -> Result<()>
where
    N::Params: serde::Serialize,
{
    let notif = Notification::new(N::METHOD.to_owned(), params);
    connection
        .sender
        .send(notif.into())
        .map_err(|e| anyhow!("{e}"))
}

// ---------------------------------------------------------------------------
// Unit tests — spec §15.X FileId eviction
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cypher_db::{Database, DialectMode};

    use super::{Server, evict_all};

    /// Helper: open a file in the server's database and register it in
    /// `open_files` the same way the `didOpen` handler does.
    fn open(server: &mut Server, uri: &str, source: &str) -> cypher_db::workspace::FileId {
        let file_id = server
            .db
            .open_file(Path::new(uri), source.into(), DialectMode::GqlAligned);
        server.open_files.insert(uri.to_owned(), file_id);
        file_id
    }

    // -----------------------------------------------------------------------
    // didClose: eviction removes FileId from map and Database
    // -----------------------------------------------------------------------

    #[test]
    fn did_close_removes_from_map_and_db() {
        let mut server = Server::new();
        let uri = "file:///tmp/a.cyp";
        let file_id = open(&mut server, uri, "RETURN 1");

        assert!(server.open_files.contains_key(uri));
        assert!(server.db.is_open(file_id));

        // Simulate the didClose eviction path.
        let removed_id = server.open_files.remove(uri);
        assert_eq!(removed_id, Some(file_id));
        server.db.remove_file(file_id).expect("remove must succeed");

        assert!(!server.open_files.contains_key(uri));
        assert!(!server.db.is_open(file_id));

        // Subsequent DB queries must return Err, not panic.
        assert!(server.db.parse_cst(file_id).is_err());
    }

    // -----------------------------------------------------------------------
    // didClose: unknown URI → no panic, open_files unchanged
    // -----------------------------------------------------------------------

    #[test]
    fn did_close_unknown_uri_is_noop() {
        let mut server = Server::new();
        let uri = "file:///tmp/real.cyp";
        let unknown = "file:///tmp/ghost.cyp";
        let file_id = open(&mut server, uri, "RETURN 2");

        // Simulate the unknown-URI branch: remove returns None → log + return.
        let result = server.open_files.remove(unknown);
        assert!(result.is_none(), "unknown URI must not be in the map");

        // The real file is unaffected.
        assert!(server.open_files.contains_key(uri));
        assert!(server.db.is_open(file_id));
    }

    // -----------------------------------------------------------------------
    // evict_all: shutdown clears all open files
    // -----------------------------------------------------------------------

    #[test]
    fn evict_all_removes_all_files() {
        let mut server = Server::new();
        let a_id = open(&mut server, "file:///tmp/a.cyp", "RETURN 1");
        let b_id = open(&mut server, "file:///tmp/b.cyp", "RETURN 2");
        let c_id = open(&mut server, "file:///tmp/c.cyp", "RETURN 3");

        assert_eq!(server.open_files.len(), 3);

        evict_all(&mut server);

        assert!(
            server.open_files.is_empty(),
            "open_files must be empty after evict_all"
        );
        assert!(!server.db.is_open(a_id));
        assert!(!server.db.is_open(b_id));
        assert!(!server.db.is_open(c_id));
    }

    // -----------------------------------------------------------------------
    // evict_all: idempotent — second call does not panic
    // -----------------------------------------------------------------------

    #[test]
    fn evict_all_idempotent() {
        let mut server = Server::new();
        open(&mut server, "file:///tmp/x.cyp", "RETURN 42");

        evict_all(&mut server);
        // Second call: open_files is already empty; Database has no entries.
        // Must not panic.
        evict_all(&mut server);

        assert!(server.open_files.is_empty());
    }

    // -----------------------------------------------------------------------
    // Database round-trip: open → query → remove → error (spec §15.X)
    // -----------------------------------------------------------------------

    #[test]
    fn database_remove_file_makes_fileid_stale() {
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("t.cyp"),
            "MATCH (n) RETURN n".into(),
            DialectMode::GqlAligned,
        );

        // Query while open.
        assert!(db.parse_cst(id).is_ok());

        // Remove.
        db.remove_file(id).expect("remove_file must succeed");

        // All subsequent queries return Err, not panic.
        assert!(db.parse_cst(id).is_err());
        assert!(db.source_of(id).is_err());
        assert!(db.remove_file(id).is_err(), "double-remove must return Err");
    }
}
