//! Cypher language server — stdio transport + v1 capabilities. Spec 0001 §14.
//!
//! Capabilities:
//! - textDocumentSync: Full
//! - publishDiagnostics on didOpen / didChange
//! - documentFormattingProvider
//! - hoverProvider (returns null for v1 — overlay lookup deferred)
//! - definitionProvider (stub — returns null)

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

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CYPHER_LSP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("cypher-lsp starting (spec 0001 §14)");

    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_formatting_provider: Some(OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        ..Default::default()
    })?;

    let initialization_params = match connection.initialize(server_capabilities) {
        Ok(it) => it,
        Err(e) => {
            if e.channel_is_disconnected() {
                io_threads.join()?;
            }
            return Err(e.into());
        }
    };
    let _params: lsp_types::InitializeParams = serde_json::from_value(initialization_params)?;

    main_loop(&connection)?;
    io_threads.join()?;
    tracing::info!("cypher-lsp shutting down");
    Ok(())
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

struct Server {
    db: Database,
    /// Map from URI string → `FileId` for open documents.
    open_files: HashMap<String, FileId>,
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
    fn new() -> Self {
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

            if let Some(file_id) = server.open_files.remove(&uri_str) {
                let _ = server.db.remove_file(file_id);
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
