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

mod code_action;
mod completion;
mod definition;
mod folding;
mod hover;
mod init_options;
mod inlay_hint;
mod references;
mod rename;
mod semantic_tokens;
mod signature_help;
pub mod transport;
mod watchers;

#[cfg(feature = "web-lsp")]
pub mod web;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};

use crate::transport::{StdioTransport, Transport};
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
        references_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Simple(true)),
        folding_range_provider: Some(lsp_types::FoldingRangeProviderCapability::Simple(true)),
        semantic_tokens_provider: Some(
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                lsp_types::SemanticTokensOptions {
                    work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
                    legend: semantic_tokens::legend(),
                    range: Some(true),
                    full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                },
            ),
        ),
        inlay_hint_provider: Some(lsp_types::OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(lsp_types::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        })),
        signature_help_provider: Some(lsp_types::SignatureHelpOptions {
            // Spec §14.2: signatureHelp triggers when the user types
            // `(` to open a call or `,` to advance to the next
            // parameter (cy-f2e).
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        }),
        execute_command_provider: Some(lsp_types::ExecuteCommandOptions {
            // Spec §14.2: custom workspace commands (cy-dsi).  The
            // client invokes these via `workspace/executeCommand`
            // with `arguments: [{ "uri": "…" }]`.  See
            // handle_execute_command for the per-command contract.
            commands: vec!["cypher.explainPlan".into(), "cypher.lowerToHir".into()],
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        }),
        completion_provider: Some(lsp_types::CompletionOptions {
            resolve_provider: Some(true),
            // Spec §14.2: trigger chars per the v1 completion engine
            // (cy-zod) — `:` for labels / rel-types, `.` reserved for
            // future property-key completion (handler returns the
            // generic keyword set today), `$` for parameters.
            trigger_characters: Some(vec![":".into(), ".".into(), "$".into()]),
            ..Default::default()
        }),
        // cy-k2r (spec §14): advertise workspace-folder change
        // notifications + file-operations hook-up so a spec-compliant
        // client actually sends us `workspace/didChangeWatchedFiles`
        // and `workspace/didChangeWorkspaceFolders`.  Clients with
        // dynamic-registration support for watched files will also
        // wire the glob pattern at `initialized` time (we emit the
        // registration there).
        workspace: Some(lsp_types::WorkspaceServerCapabilities {
            workspace_folders: Some(lsp_types::WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: Some(lsp_types::WorkspaceFileOperationsServerCapabilities::default()),
        }),
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
    let params: lsp_types::InitializeParams = serde_json::from_value(initialization_params)?;

    // Spec §14.3: the client may pass `initializationOptions` that
    // select the schema source + dialect.  Failure to load the
    // schema does not fail `initialize` — we log and run with no
    // schema so the server stays usable even with a misconfigured
    // client.
    let init = init_options::InitOptions::from_value(params.initialization_options.as_ref());
    let dialect = init.resolved_dialect();
    let schema = match init_options::load_schema(&init) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("initializationOptions: load_schema failed: {e:#}");
            None
        }
    };
    let debounce = init.resolved_debounce();

    // Adapter: wrap the `lsp_server::Connection` as a `Transport` so the
    // dispatch loop runs over the same trait both native and web builds
    // use (spec 0004 §7.2).  Zero behavioural change on stdio — the
    // adapter forwards every call to the crossbeam channels.
    let transport = StdioTransport::new(connection);
    main_loop(&transport, dialect, schema, debounce)
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

pub(crate) struct Server {
    pub(crate) db: Database,
    /// Map from URI string → `FileId` for open documents.
    pub(crate) open_files: HashMap<String, FileId>,
    /// Dialect applied on every `didOpen` — set from the client's
    /// `initializationOptions.dialect` (spec §14.3, bead cy-0ls).
    pub(crate) dialect: DialectMode,
    /// Workspace project loaded lazily on the first `didOpen` whose
    /// URI is inside a directory tree containing a
    /// `cypher-project.toml` (spec 0003, bead cy-kkw).  When present,
    /// cross-file navigation engines use it to scope reference /
    /// goto-definition / workspace-symbol queries.  `None` means
    /// single-file mode — navigation falls back to in-file token
    /// matching, exactly as before cy-kkw.
    pub(crate) project: Option<cypher_project::ProjectManifest>,
    /// Directories we've already probed for a project manifest, so we
    /// don't walk `discover` up every single tree on every `didOpen`.
    pub(crate) project_probed: std::collections::HashSet<std::path::PathBuf>,
    /// Pending `didChangeWatchedFiles` events (cy-k2r, spec §14).
    /// Drained + applied to the database once the debounce window
    /// expires — see [`watchers::flush_watched_files`].
    pub(crate) watched_files: watchers::WatchedFilesBuffer,
    /// Debounce window for [`watched_files`] flushes.  Defaults to
    /// 250 ms; configurable via
    /// `initializationOptions.watchedFilesDebounceMs` (spec §14).
    ///
    /// [`watched_files`]: Self::watched_files
    pub(crate) watched_debounce: std::time::Duration,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("db", &"<Database>")
            .field("open_files", &self.open_files.len())
            .field("dialect", &self.dialect)
            .field("project", &self.project.as_ref().map(|p| p.name.as_str()))
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Default test constructor: `GqlAligned`, no schema.  Used by the
    /// unit tests under `#[cfg(test)]`; `serve` always goes through
    /// `with_config` with values parsed from `initializationOptions`.
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_config(DialectMode::GqlAligned, None)
    }

    /// Construct a server with a specific dialect and optional
    /// pre-loaded schema.  Called from `serve` after parsing
    /// `initializationOptions`.
    #[cfg(test)]
    pub(crate) fn with_config(
        dialect: DialectMode,
        schema: Option<std::sync::Arc<dyn cypher_schema::SchemaProvider>>,
    ) -> Self {
        Self::with_config_and_debounce(
            dialect,
            schema,
            std::time::Duration::from_millis(watchers::DEFAULT_DEBOUNCE_MS),
        )
    }

    /// Like [`with_config`] but takes an explicit watched-file debounce
    /// window.  Useful for tests that want to assert flush timing
    /// without blocking a full 250 ms.
    ///
    /// [`with_config`]: Self::with_config
    pub(crate) fn with_config_and_debounce(
        dialect: DialectMode,
        schema: Option<std::sync::Arc<dyn cypher_schema::SchemaProvider>>,
        watched_debounce: std::time::Duration,
    ) -> Self {
        let mut db = Database::new();
        if let Some(s) = schema {
            db.set_schema(Some(s));
        }
        Self {
            db,
            open_files: HashMap::new(),
            dialect,
            project: None,
            project_probed: std::collections::HashSet::new(),
            watched_files: watchers::WatchedFilesBuffer::default(),
            watched_debounce,
        }
    }
}

/// Try to discover + load a `cypher-project.toml` starting from the
/// directory containing the given path.  Caches the result (or its
/// absence) in `server.project_probed` so repeated calls are O(1).
///
/// When a manifest is found the server's `project` is set and, if the
/// manifest carries a schema, the database's schema is too.  Member
/// `.cyp` files that are not yet open are opened eagerly so
/// cross-file references work even before the user visits them.
pub(crate) fn ensure_project_loaded(server: &mut Server, hint: &std::path::Path) {
    use std::path::PathBuf;

    // Determine the directory to probe.
    let dir: PathBuf = if hint.is_dir() {
        hint.to_path_buf()
    } else {
        hint.parent().map_or_else(PathBuf::new, Path::to_path_buf)
    };
    if dir.as_os_str().is_empty() {
        return;
    }
    if server.project_probed.contains(&dir) {
        return;
    }
    server.project_probed.insert(dir.clone());

    if server.project.is_some() {
        return;
    }

    let Some(manifest_path) = cypher_project::discover(&dir) else {
        return;
    };
    let manifest = match cypher_project::load_from_toml_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("failed to load {manifest_path:?}: {e}");
            return;
        }
    };

    // Wire the manifest's schema into the database (if any).  The
    // init_options schema wins if already loaded — we only set when
    // the DB has no schema yet.
    if server.db.schema().is_none()
        && let Some(ref schema) = manifest.schema
    {
        use std::sync::Arc;
        server.db.set_schema(Some(
            Arc::new(schema.clone()) as Arc<dyn cypher_schema::SchemaProvider>
        ));
    }

    // Eagerly open every member file that isn't already open, so
    // cross-file navigation finds references in unvisited files.
    // Files opened via `didOpen` later will map to the same on-disk
    // path but a different URI key; we dedupe by absolute path.
    for member in &manifest.members {
        let uri_str = format!("file://{}", member.display());
        if server.open_files.contains_key(&uri_str) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(member) else {
            continue;
        };
        let dialect = match manifest.dialect_for(member) {
            cypher_project::DialectDefault::GqlAligned => DialectMode::GqlAligned,
            cypher_project::DialectDefault::OpenCypherV9 => DialectMode::OpenCypherV9,
        };
        let file_id = server.db.open_file(member, contents, dialect);
        server.open_files.insert(uri_str, file_id);
    }

    server.project = Some(manifest);
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main_loop(
    transport: &dyn Transport,
    dialect: DialectMode,
    schema: Option<std::sync::Arc<dyn cypher_schema::SchemaProvider>>,
    watched_debounce: std::time::Duration,
) -> Result<()> {
    let mut server = Server::with_config_and_debounce(dialect, schema, watched_debounce);
    dispatch(transport, &mut server)
}

/// Poll the client transport, dispatch requests / notifications, and
/// flush the watched-files debounce buffer when its window expires.
///
/// Factored out of [`main_loop`] so `Server` constructors that want a
/// non-default debounce window (tests) can re-use the same loop body.
pub(crate) fn dispatch(transport: &dyn Transport, server: &mut Server) -> Result<()> {
    loop {
        // Compute the recv deadline.  When the debounce buffer is
        // empty we block on recv; otherwise we wait at most the
        // remaining debounce so a flush can run even if the client
        // stops sending messages.
        let timeout = server.watched_files.remaining(server.watched_debounce);

        let msg = match timeout {
            None => transport.recv()?,
            Some(remaining) => transport.recv_timeout(remaining)?,
        };

        // Check the debounce first so flushes happen before we process
        // the next incoming message — a tight loop of watched-file
        // events can't starve the buffer.
        if let Some(remaining) = server.watched_files.remaining(server.watched_debounce)
            && remaining.is_zero()
        {
            watchers::flush_watched_files(transport, server);
        }

        let Some(msg) = msg else {
            // recv returned None without timeout → peer disconnected
            // (clean EOF on stdio, port closed on web).  Exit cleanly
            // so IoThreads::join on the binary sees a graceful close.
            if timeout.is_none() {
                return Ok(());
            }
            continue;
        };

        match msg {
            Message::Request(req) => {
                if transport.handle_shutdown(&req)? {
                    // Final debounce flush so any trailing events make
                    // it to the client's last diagnostics snapshot
                    // before we evict.
                    watchers::flush_watched_files(transport, server);
                    evict_all(server);
                    return Ok(());
                }
                handle_request(transport, server, req)?;
            }
            Message::Notification(notif) => {
                handle_notification(transport, server, notif)?;
            }
            Message::Response(_) => {}
        }
    }
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

pub(crate) fn handle_request(
    transport: &dyn Transport,
    server: &mut Server,
    req: Request,
) -> Result<()> {
    match req.method.as_str() {
        lsp_types::request::Formatting::METHOD => {
            let params: DocumentFormattingParams = serde_json::from_value(req.params)?;
            let resp = handle_formatting(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::HoverRequest::METHOD => {
            let params: HoverParams = serde_json::from_value(req.params)?;
            let resp = handle_hover(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::Completion::METHOD => {
            let params: lsp_types::CompletionParams = serde_json::from_value(req.params)?;
            let resp = handle_completion(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::ResolveCompletionItem::METHOD => {
            let item: lsp_types::CompletionItem = serde_json::from_value(req.params)?;
            let resolved = completion::resolve(item);
            let resp = match serde_json::to_value(resolved) {
                Ok(v) => Response::new_ok(req.id, v),
                Err(e) => Response::new_err(
                    req.id,
                    lsp_server::ErrorCode::InternalError as i32,
                    e.to_string(),
                ),
            };
            transport.send(resp.into())?;
        }
        lsp_types::request::GotoDefinition::METHOD => {
            let params: lsp_types::GotoDefinitionParams = serde_json::from_value(req.params)?;
            let resp = handle_definition(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::SignatureHelpRequest::METHOD => {
            let params: lsp_types::SignatureHelpParams = serde_json::from_value(req.params)?;
            let resp = handle_signature_help(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::References::METHOD => {
            let params: lsp_types::ReferenceParams = serde_json::from_value(req.params)?;
            let resp = handle_references(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::WorkspaceSymbolRequest::METHOD => {
            let params: lsp_types::WorkspaceSymbolParams = serde_json::from_value(req.params)?;
            let resp = handle_workspace_symbol(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::PrepareRenameRequest::METHOD => {
            let params: lsp_types::TextDocumentPositionParams = serde_json::from_value(req.params)?;
            let resp = handle_prepare_rename(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::Rename::METHOD => {
            let params: lsp_types::RenameParams = serde_json::from_value(req.params)?;
            let resp = handle_rename(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::CodeActionRequest::METHOD => {
            let params: lsp_types::CodeActionParams = serde_json::from_value(req.params)?;
            let resp = handle_code_action(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::FoldingRangeRequest::METHOD => {
            let params: lsp_types::FoldingRangeParams = serde_json::from_value(req.params)?;
            let resp = handle_folding_range(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::SemanticTokensFullRequest::METHOD => {
            let params: lsp_types::SemanticTokensParams = serde_json::from_value(req.params)?;
            let resp = handle_semantic_tokens_full(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::SemanticTokensRangeRequest::METHOD => {
            let params: lsp_types::SemanticTokensRangeParams = serde_json::from_value(req.params)?;
            let resp = handle_semantic_tokens_range(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::InlayHintRequest::METHOD => {
            let params: lsp_types::InlayHintParams = serde_json::from_value(req.params)?;
            let resp = handle_inlay_hint(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::ExecuteCommand::METHOD => {
            let params: lsp_types::ExecuteCommandParams = serde_json::from_value(req.params)?;
            let resp = handle_execute_command(server, req.id, &params);
            transport.send(resp.into())?;
        }
        lsp_types::request::RangeFormatting::METHOD => {
            let params: lsp_types::DocumentRangeFormattingParams =
                serde_json::from_value(req.params)?;
            let resp = handle_range_formatting(server, req.id, &params);
            transport.send(resp.into())?;
        }
        _ => {
            tracing::debug!("unhandled request: {}", req.method);
        }
    }
    Ok(())
}

fn handle_semantic_tokens_full(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::SemanticTokensParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let tokens = semantic_tokens::compute_full(&server.db, file_id);
    let result = lsp_types::SemanticTokensResult::Tokens(tokens);
    match serde_json::to_value(result) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

fn handle_semantic_tokens_range(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::SemanticTokensRangeParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let tokens = semantic_tokens::compute_range(&server.db, file_id, params.range);
    let result = lsp_types::SemanticTokensRangeResult::Tokens(tokens);
    match serde_json::to_value(result) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

fn handle_inlay_hint(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::InlayHintParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let hints = inlay_hint::compute(&server.db, file_id, params.range);
    match serde_json::to_value(hints) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

/// v1 `workspace/executeCommand` dispatch (spec §14.2, bead cy-dsi).
///
/// Each command takes an argument object `{ "uri": "file:///…" }`
/// referring to an open document; the response is a JSON string
/// carrying the pretty-printed output the client can display in a
/// scratch buffer.  Unknown commands return `MethodNotFound`;
/// missing / malformed `arguments` return `InvalidParams`.
fn handle_execute_command(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::ExecuteCommandParams,
) -> Response {
    // Extract the first argument's `uri` field.  Both commands share
    // this shape so we parse it once up front.
    let first_arg = params.arguments.first();
    let uri_str = first_arg
        .and_then(|v| v.get("uri"))
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string);
    let Some(uri_str) = uri_str else {
        return Response::new_err(
            id,
            lsp_server::ErrorCode::InvalidParams as i32,
            "executeCommand requires `arguments: [{ uri }]`".to_string(),
        );
    };
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_err(
            id,
            lsp_server::ErrorCode::InvalidParams as i32,
            format!("file not open: {uri_str}"),
        );
    };

    match params.command.as_str() {
        "cypher.explainPlan" => {
            // Pretty plan via the existing plan_of + pretty pipeline.
            match server.db.plan_of(file_id) {
                Ok(plan) => Response::new_ok(
                    id,
                    serde_json::Value::String(cypher_plan::pretty::pretty(plan.plan())),
                ),
                Err(e) => Response::new_err(
                    id,
                    lsp_server::ErrorCode::InternalError as i32,
                    e.to_string(),
                ),
            }
        }
        "cypher.lowerToHir" => {
            // HIR overlay via the lowering pipeline.  Re-lowers the
            // source rather than reaching into the Salsa query for
            // `Statement` (not exposed on the public workspace API).
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
            let stmt =
                cypher_hir::desugar::desugar_statement(cypher_hir::lower::lower_statement(&source));
            let mut sink = cypher_diag::DiagnosticsSink::new();
            let resolved = cypher_sema::resolve::resolve(&stmt, false, &mut sink).resolved_names;
            Response::new_ok(
                id,
                serde_json::Value::String(cypher_hir::pretty::print_overlay(&stmt, &resolved)),
            )
        }
        other => Response::new_err(
            id,
            lsp_server::ErrorCode::MethodNotFound as i32,
            format!("unknown command: {other}"),
        ),
    }
}

fn handle_folding_range(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::FoldingRangeParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let ranges = folding::compute(&server.db, file_id);
    match serde_json::to_value(ranges) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

/// Range-formatting v1: formats the whole document and returns a
/// single `TextEdit` covering the entire file when the request range
/// covers the whole file; otherwise formats the whole doc and
/// returns no edits.  This is the simplest correct behaviour (spec
/// §13 is a whole-document formatter today) and keeps us honest
/// about the limitation — a follow-up can produce clause-bounded
/// partial edits.
fn handle_range_formatting(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::DocumentRangeFormattingParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
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
        return Response::new_ok(id, serde_json::Value::Array(vec![]));
    }

    // Determine whether the requested range covers the whole doc.
    // v1 only honours whole-doc requests — partial range formatting
    // would need a clause-aware slicer the formatter doesn't yet
    // expose.  Partial requests return no edits with an info log.
    let line_index = LineIndex::new(&source);
    let total_lines = line_index.line_count();
    let covers_whole = params.range.start.line == 0
        && params.range.start.character == 0
        && params.range.end.line >= total_lines.saturating_sub(1);
    if !covers_whole {
        tracing::info!(
            "rangeFormatting: partial-range formatting not implemented; returning 0 edits"
        );
        return Response::new_ok(id, serde_json::Value::Array(vec![]));
    }

    // Whole-doc edit.  Compute the end position from the trailing
    // source; end_line is the last-line index and end_character is
    // the last-line character count.
    let mut last_line_char_count: u32 = 0;
    let mut last_line: u32 = 0;
    for (i, line) in source.lines().enumerate() {
        last_line = u32::try_from(i).unwrap_or(u32::MAX);
        last_line_char_count = u32::try_from(line.chars().count()).unwrap_or(u32::MAX);
    }
    let edit = TextEdit {
        range: lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(last_line, last_line_char_count),
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

fn handle_code_action(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::CodeActionParams,
) -> Response {
    let uri = &params.text_document.uri;
    let uri_str = uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    match code_action::compute(&server.db, file_id, uri, params.range) {
        Some(actions) => match serde_json::to_value(actions) {
            Ok(v) => Response::new_ok(id, v),
            Err(e) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                e.to_string(),
            ),
        },
        None => Response::new_ok(id, serde_json::Value::Null),
    }
}

fn handle_prepare_rename(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::TextDocumentPositionParams,
) -> Response {
    let uri_str = params.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    match rename::prepare_rename(&server.db, file_id, params.position) {
        Some(r) => match serde_json::to_value(r) {
            Ok(v) => Response::new_ok(id, v),
            Err(e) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                e.to_string(),
            ),
        },
        // Spec §14.2: returning null (not an error) tells the client
        // "the cursor is not on something you can rename", which
        // suppresses the rename UI without a scary popup.
        None => Response::new_ok(id, serde_json::Value::Null),
    }
}

fn handle_rename(server: &mut Server, id: RequestId, params: &lsp_types::RenameParams) -> Response {
    let uri = &params.text_document_position.text_document.uri;
    let uri_str = uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let position = params.text_document_position.position;
    match rename::compute(&server.db, file_id, uri, position, &params.new_name) {
        Some(edit) => match serde_json::to_value(edit) {
            Ok(v) => Response::new_ok(id, v),
            Err(e) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                e.to_string(),
            ),
        },
        None => Response::new_err(
            id,
            lsp_server::ErrorCode::InvalidParams as i32,
            "cannot rename at this location or the new name is not a valid identifier".into(),
        ),
    }
}

fn handle_references(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::ReferenceParams,
) -> Response {
    let uri = &params.text_document_position.text_document.uri;
    let uri_str = uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let position = params.text_document_position.position;
    let include_declaration = params.context.include_declaration;

    // Workspace-aware path (spec §14.2, bead cy-kkw): when a project
    // manifest is loaded, ask the cross-file engine.  It returns
    // references inside every member file including the schema.toml
    // decl site.  Falls back to the single-file engine when the
    // cursor does not resolve to a workspace symbol — preserves the
    // cy-vhe behaviour on variable / binding refs.
    if let Some(project) = server.project.as_ref() {
        let locs = references_workspace(
            &server.db,
            project,
            &server.open_files,
            file_id,
            position,
            include_declaration,
        );
        if let Some(list) = locs {
            return to_json_or_err(id, list);
        }
    }

    match references::compute(&server.db, file_id, uri, position, include_declaration) {
        Some(locations) => to_json_or_err(id, locations),
        None => Response::new_ok(id, serde_json::Value::Null),
    }
}

/// Workspace-aware references lookup.
///
/// Returns `None` when the cursor does not resolve to a workspace
/// symbol (label / rel-type / param / named-path) so the caller can
/// fall back to the single-file engine.  An empty vector means "the
/// symbol is known but has no references anywhere" — still a valid
/// answer the client should render as "no results".
fn references_workspace(
    db: &Database,
    project: &cypher_project::ProjectManifest,
    open_files: &HashMap<String, FileId>,
    file_id: FileId,
    position: lsp_types::Position,
    include_declaration: bool,
) -> Option<Vec<lsp_types::Location>> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;
    let locs = cypher_lang_services::find_references(db, project, file_id, offset);
    if locs.is_empty() {
        return None;
    }

    // Optional declaration drop.
    let decl = cypher_lang_services::goto_definition(db, project, file_id, offset);
    let mut out = Vec::with_capacity(locs.len());
    for loc in locs {
        if !include_declaration
            && let Some(ref d) = decl
            && d == &loc
        {
            continue;
        }
        let Some(uri) = file_path_to_uri(&loc.path) else {
            continue;
        };
        // Use the open-file cache when possible to avoid re-reading
        // the disk; schema files we read directly.
        let line_index = line_index_for_path(db, open_files, &loc.path)?;
        let range = text_range_to_lsp(&line_index, loc.range);
        out.push(lsp_types::Location { uri, range });
    }
    Some(out)
}

fn position_to_offset(
    line_index: &LineIndex,
    pos: lsp_types::Position,
) -> Option<cypher_syntax::TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + cypher_syntax::TextSize::from(utf8.col))
}

fn line_index_for_path(
    db: &Database,
    open_files: &HashMap<String, FileId>,
    path: &std::path::Path,
) -> Option<LineIndex> {
    for (uri, file_id) in open_files {
        let Some(disk) = uri_to_file_path(uri) else {
            continue;
        };
        if disk == path
            && let Ok(src) = db.source_of(*file_id)
        {
            return Some(LineIndex::new(&src));
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some(LineIndex::new(&text))
}

fn to_json_or_err<T: serde::Serialize>(id: RequestId, v: T) -> Response {
    match serde_json::to_value(v) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

fn handle_signature_help(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::SignatureHelpParams,
) -> Response {
    let uri_str = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let position = params.text_document_position_params.position;
    match signature_help::compute(&server.db, file_id, position) {
        Some(sig) => match serde_json::to_value(sig) {
            Ok(v) => Response::new_ok(id, v),
            Err(e) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                e.to_string(),
            ),
        },
        None => Response::new_ok(id, serde_json::Value::Null),
    }
}

fn handle_definition(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::GotoDefinitionParams,
) -> Response {
    let uri = &params.text_document_position_params.text_document.uri;
    let uri_str = uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let position = params.text_document_position_params.position;

    // Workspace-aware path (spec §14.2, bead cy-kkw): when a project
    // manifest is loaded, ask the cross-file engine for a decl
    // location.  Jumping to `:Person` in a .cyp file lands in
    // schema.toml.  Falls back to the single-file engine for
    // variable / binding jumps.
    if let Some(project) = server.project.as_ref()
        && let Some(resp) =
            definition_workspace(&server.db, project, &server.open_files, file_id, position)
    {
        return to_json_or_err(id, resp);
    }

    match definition::compute(&server.db, file_id, uri, position) {
        Some(loc) => to_json_or_err(id, loc),
        None => Response::new_ok(id, serde_json::Value::Null),
    }
}

fn definition_workspace(
    db: &Database,
    project: &cypher_project::ProjectManifest,
    open_files: &HashMap<String, FileId>,
    file_id: FileId,
    position: lsp_types::Position,
) -> Option<lsp_types::GotoDefinitionResponse> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;
    let loc = cypher_lang_services::goto_definition(db, project, file_id, offset)?;
    let uri = file_path_to_uri(&loc.path)?;
    let target_index = line_index_for_path(db, open_files, &loc.path)?;
    let range = text_range_to_lsp(&target_index, loc.range);
    Some(lsp_types::GotoDefinitionResponse::Scalar(
        lsp_types::Location { uri, range },
    ))
}

fn handle_completion(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::CompletionParams,
) -> Response {
    let uri_str = params.text_document_position.text_document.uri.to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let response = completion::compute(&server.db, file_id, params);
    match serde_json::to_value(response) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

fn handle_hover(server: &mut Server, id: RequestId, params: &HoverParams) -> Response {
    let uri_str = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let Some(&file_id) = server.open_files.get(&uri_str) else {
        // LSP allows null for "no info"; the client treats it as
        // "nothing to show".  Returning an error here would surface as
        // a popup in most clients, which is much louder than warranted
        // for a stale URI.
        return Response::new_ok(id, serde_json::Value::Null);
    };
    let position = params.text_document_position_params.position;
    match hover::compute(&server.db, file_id, position) {
        Some(h) => match serde_json::to_value(h) {
            Ok(v) => Response::new_ok(id, v),
            Err(e) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                e.to_string(),
            ),
        },
        None => Response::new_ok(id, serde_json::Value::Null),
    }
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

pub(crate) fn handle_notification(
    transport: &dyn Transport,
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

            // Resolve the disk path when the URI is `file://…` — we
            // use it both as the on-disk path handed to the database
            // and as the hint for project discovery (spec 0003
            // §2, bead cy-kkw).  Non-file URIs fall back to the
            // URI string as a synthetic path; no project is loaded.
            let on_disk = uri_to_file_path(&uri_str);
            let path_for_db: std::path::PathBuf = on_disk
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from(uri_str.as_str()));

            let file_id = server.db.open_file(&path_for_db, text, server.dialect);
            server.open_files.insert(uri_str, file_id);

            if let Some(ref p) = on_disk {
                ensure_project_loaded(server, p);
            }

            publish_diagnostics(transport, server, &uri)?;
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

            publish_diagnostics(transport, server, &uri)?;
        }
        lsp_types::notification::DidSaveTextDocument::METHOD => {
            // cy-1yb: under `TextDocumentSyncKind::FULL` the server
            // already has the latest buffer via didChange, so didSave
            // is a no-op unless the client opted in to
            // `includeText: true`.  In that case we treat the
            // delivered text as an implicit didChange so a
            // reformat-on-save that didn't already push the new text
            // via didChange still gets fresh diagnostics.
            let params: lsp_types::DidSaveTextDocumentParams =
                serde_json::from_value(notif.params)?;
            let uri = params.text_document.uri.clone();
            let uri_str = uri.to_string();
            if let Some(text) = params.text
                && let Some(&file_id) = server.open_files.get(&uri_str)
            {
                let _ = server.db.update_file(file_id, text);
                publish_diagnostics(transport, server, &uri)?;
            } else {
                tracing::debug!("didSave: no-op for {uri_str}");
            }
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
            send_notification::<lsp_types::notification::PublishDiagnostics>(transport, &clear)?;
        }
        lsp_types::notification::DidChangeWatchedFiles::METHOD => {
            // Spec §14 (cy-k2r): buffer the events; flushing happens
            // once the debounce window expires (see `main_loop`).
            let params: lsp_types::DidChangeWatchedFilesParams =
                serde_json::from_value(notif.params)?;
            watchers::handle_did_change_watched_files(server, params);
        }
        lsp_types::notification::DidChangeWorkspaceFolders::METHOD => {
            // Spec §14 (cy-k2r): user-initiated; apply synchronously.
            let params: lsp_types::DidChangeWorkspaceFoldersParams =
                serde_json::from_value(notif.params)?;
            watchers::handle_did_change_workspace_folders(transport, server, params);
        }
        _ => {
            tracing::debug!("unhandled notification: {}", notif.method);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// workspace/symbol — cross-file navigation (spec §14.2, bead cy-kkw)
// ---------------------------------------------------------------------------

fn handle_workspace_symbol(
    server: &mut Server,
    id: RequestId,
    params: &lsp_types::WorkspaceSymbolParams,
) -> Response {
    let Some(project) = server.project.as_ref() else {
        // No workspace manifest loaded — no cross-file scope exists.
        // Return an empty list rather than null so clients that
        // iterate results don't treat the response as an error.
        return Response::new_ok(id, serde_json::Value::Array(vec![]));
    };
    let results =
        cypher_lang_services::workspace_symbols(&server.db, project, params.query.as_str());
    let mut infos: Vec<lsp_types::SymbolInformation> = Vec::with_capacity(results.len());
    for symbol in results {
        let Some(loc) = symbol
            .declaration
            .clone()
            .or_else(|| symbol.references.first().cloned())
        else {
            continue;
        };
        let Some(line_index) = read_line_index(server, &loc.path) else {
            continue;
        };
        let range = text_range_to_lsp(&line_index, loc.range);
        let Some(uri) = file_path_to_uri(&loc.path) else {
            continue;
        };
        #[allow(deprecated)] // `deprecated` is a required public field
        infos.push(lsp_types::SymbolInformation {
            name: symbol.name.to_string(),
            kind: symbol_kind_to_lsp(symbol.kind),
            tags: None,
            deprecated: None,
            location: lsp_types::Location { uri, range },
            container_name: None,
        });
    }
    match serde_json::to_value(infos) {
        Ok(v) => Response::new_ok(id, v),
        Err(e) => Response::new_err(
            id,
            lsp_server::ErrorCode::InternalError as i32,
            e.to_string(),
        ),
    }
}

fn symbol_kind_to_lsp(kind: cypher_lang_services::SymbolKind) -> lsp_types::SymbolKind {
    match kind {
        cypher_lang_services::SymbolKind::Label => lsp_types::SymbolKind::CLASS,
        cypher_lang_services::SymbolKind::RelType => lsp_types::SymbolKind::INTERFACE,
        cypher_lang_services::SymbolKind::Param => lsp_types::SymbolKind::VARIABLE,
        cypher_lang_services::SymbolKind::NamedPath => lsp_types::SymbolKind::KEY,
        // Exhaustive today; any future kind surfaces as OBJECT while
        // still advertising the symbol.
        _ => lsp_types::SymbolKind::OBJECT,
    }
}

fn text_range_to_lsp(line_index: &LineIndex, range: cypher_syntax::TextRange) -> lsp_types::Range {
    lsp_types::Range {
        start: offset_to_position(line_index, range.start()),
        end: offset_to_position(line_index, range.end()),
    }
}

fn offset_to_position(
    line_index: &LineIndex,
    offset: cypher_syntax::TextSize,
) -> lsp_types::Position {
    let utf8 = line_index.line_col(offset);
    let utf16 = line_index.to_utf16(utf8);
    lsp_types::Position {
        line: utf16.line,
        character: utf16.col,
    }
}

/// Best-effort `file://…` URI → `PathBuf`.  Accepts the "narrow"
/// form the stdlib crate normalises everything into; no
/// percent-decoding beyond what `lsp_types::Uri::to_string` round-trips.
pub(crate) fn uri_to_file_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Drop an optional `localhost` authority.
    let rest = rest
        .strip_prefix("localhost/")
        .map_or_else(|| rest.to_string(), |r| format!("/{r}"));
    Some(std::path::PathBuf::from(rest))
}

fn file_path_to_uri(path: &std::path::Path) -> Option<Uri> {
    let uri_str = format!("file://{}", path.display());
    uri_str.parse().ok()
}

/// Load the source of `path` and build a `LineIndex`.  Prefers the
/// database-cached source when the path matches an open file; falls
/// back to a filesystem read otherwise so schema-file ranges (which
/// aren't tracked by the database) still resolve.
pub(crate) fn read_line_index(server: &Server, path: &std::path::Path) -> Option<LineIndex> {
    // If any open file has this on-disk path, reuse the cached source.
    for (uri, file_id) in &server.open_files {
        let Some(disk) = uri_to_file_path(uri) else {
            continue;
        };
        if disk == path
            && let Ok(src) = server.db.source_of(*file_id)
        {
            return Some(LineIndex::new(&src));
        }
    }
    // Fallback: read the file directly.  Schema files are the
    // expected case here.
    let text = std::fs::read_to_string(path).ok()?;
    Some(LineIndex::new(&text))
}

// ---------------------------------------------------------------------------
// Diagnostics helper
// ---------------------------------------------------------------------------

/// Re-publish diagnostics for a watched-file URI (cy-k2r).  The
/// `watchers` module invokes this after draining the debounce buffer
/// so the client sees fresh diagnostics within one debounce window of
/// the last external file event.
pub(crate) fn publish_diagnostics_for_watched(
    transport: &dyn Transport,
    server: &mut Server,
    uri: &Uri,
) -> Result<()> {
    publish_diagnostics(transport, server, uri)
}

fn publish_diagnostics(transport: &dyn Transport, server: &mut Server, uri: &Uri) -> Result<()> {
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

    send_notification::<lsp_types::notification::PublishDiagnostics>(transport, &params)
}

pub(crate) fn send_notification<N: lsp_types::notification::Notification>(
    transport: &dyn Transport,
    params: &N::Params,
) -> Result<()>
where
    N::Params: serde::Serialize,
{
    let notif = Notification::new(N::METHOD.to_owned(), params);
    transport.send(notif.into())
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

    // -----------------------------------------------------------------------
    // Salsa-memo GC proof (spec §11.6 / §15.X)
    //
    // After `didClose` + a revision bump, memoised derived values keyed on
    // the evicted FileId must be reclaimed.  We observe reclamation by
    // watching the `Arc<Parse>` strong count drop once Salsa replaces the
    // memo entry on the next `parse_cst` call against the recycled
    // SourceFile handle.
    //
    // `Database::remove_file` (spec §11.6, bead cy-bh5) pools freed Salsa
    // input handles in `free_slots`; the next `open_file` reuses a pooled
    // handle, resets its fields (which bumps the Salsa revision), and then
    // the next `parse_cst` replaces the stale memo entry with the new
    // ParseOutput.  The ParseOutput that was cached for the evicted FileId
    // is then unreachable from Salsa — its Arc strong count drops to 1
    // (only the test holds a clone).
    // -----------------------------------------------------------------------

    #[test]
    #[allow(non_snake_case)]
    fn memoized_values_reclaimed_after_didClose_and_revision_bump() {
        use cypher_db::ParseOutput;

        let mut server = Server::new();

        // Open file A, populate the parse_cst memo, and capture the
        // ParseOutput so we can observe its Arc strong count.
        let a_uri = "file:///tmp/a.cyp";
        let a_id = open(&mut server, a_uri, "RETURN 1");
        let a_parse: ParseOutput = server
            .db
            .parse_cst(a_id)
            .expect("file A must parse while open");

        // Salsa memo table + our local both hold the Arc → strong_count >= 2.
        assert!(
            a_parse.strong_count() >= 2,
            "before eviction: Salsa memo must retain an Arc alongside our clone, got {}",
            a_parse.strong_count()
        );

        // Simulate the didClose eviction path.
        let removed = server.open_files.remove(a_uri);
        assert_eq!(removed, Some(a_id));
        server
            .db
            .remove_file(a_id)
            .expect("remove_file must succeed");
        assert!(!server.db.is_open(a_id));

        // Bump the Salsa revision with an unrelated mutation — opening
        // file B reuses A's pooled Salsa input handle (spec §11.6) and
        // resets its fields, which bumps the revision for that input.
        let b_id = open(&mut server, "file:///tmp/b.cyp", "RETURN 2");

        // Trigger the next read on the recycled SourceFile handle.
        // Salsa re-executes parse_cst; the old memo entry (ParseOutput for
        // the evicted FileId) is swapped out of the memo table and parked
        // in Salsa's `deleted_entries` buffer, to be dropped on the start
        // of the next revision (Salsa 0.26 `function::insert_memo`).
        let b_parse = server
            .db
            .parse_cst(b_id)
            .expect("file B must parse while open");
        assert_eq!(b_parse.parse().syntax().to_string(), "RETURN 2");

        // Bump the revision one more time so Salsa's `reset_for_new_revision`
        // runs and frees the entries parked in `deleted_entries`.  Any
        // no-op mutation suffices; re-setting B's source to its current
        // value still increments the revision.
        server
            .db
            .update_file(b_id, "RETURN 2".into())
            .expect("update must succeed");

        // The old ParseOutput (for A) must no longer be held by Salsa:
        // only our local clone remains, so strong_count drops to 1.
        // This proves the memo keyed on the evicted FileId's SourceFile
        // has been reclaimed on the next revision.
        assert_eq!(
            a_parse.strong_count(),
            1,
            "after didClose + revision bump + next query, Salsa must no \
             longer retain the ParseOutput memoised for the evicted FileId \
             (expected strong_count = 1, got {})",
            a_parse.strong_count()
        );

        // And the previously-cached Arc is not identity-equal to any
        // currently-cached ParseOutput for the recycled handle.
        assert_ne!(
            a_parse, b_parse,
            "recycled SourceFile must produce a fresh ParseOutput after eviction"
        );
    }
}
