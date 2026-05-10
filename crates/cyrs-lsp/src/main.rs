//! Cypher language server — stdio entry point. Spec 0001 §14.
//!
//! All server logic lives in the `cyrs_lsp` library module (`src/lib.rs`).
//! This binary just wires tracing + `Connection::stdio()` to
//! [`cyrs_lsp::serve`] so integration tests can reuse the same server
//! code against `Connection::memory()` in-process (bead cy-d48).

#![forbid(unsafe_code)]

use anyhow::Result;
use lsp_server::Connection;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CYPHER_LSP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("cyrs-lsp starting (spec 0001 §14)");

    let (connection, io_threads) = Connection::stdio();
    let result = cyrs_lsp::serve(&connection);
    io_threads.join()?;
    tracing::info!("cyrs-lsp shutting down");
    result
}
