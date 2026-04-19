//! Cypher language server. Spec 0001 §14.
//!
//! Uses the `lsp-server` + `lsp-types` crates (the rust-analyzer stack).
//! Full capability registration lands as the analysis passes land; this
//! file establishes the process shape, logging, and server loop so the
//! binary can be driven by an editor today.

use anyhow::Result;
use lsp_server::{Connection, Message};
use lsp_types::{
    InitializeParams, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
};

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
    let _params: InitializeParams = serde_json::from_value(initialization_params)?;

    main_loop(&connection)?;
    io_threads.join()?;
    tracing::info!("cypher-lsp shutting down");
    Ok(())
}

fn main_loop(connection: &Connection) -> Result<()> {
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                tracing::debug!("unhandled request: {}", req.method);
            }
            Message::Response(_) | Message::Notification(_) => {}
        }
    }
    Ok(())
}
