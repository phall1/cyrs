//! Neutral language-services engines shared by `cypher-lsp` and
//! `cypher-agent`. Spec 0001 §14 (LSP) / §15 (agent).
//!
//! The LSP and agent crates both need to answer "what are the
//! completions / hover markdown / rewrite edits at this cursor?"
//! questions.  Rather than duplicate the engines across both binaries
//! (the original v1 cut, because `cypher-lsp` pulls in
//! `lsp-server`/`lsp-types` and the agent binary wants to stay small),
//! this crate hosts the shared business logic in three neutral
//! free functions:
//!
//! * [`complete`] — keyword / label / parameter completion.
//! * [`hover`] — keyword + variable-binding hover.
//! * [`rewrite`] — apply `Diagnostic::fixes` edits by id.
//!
//! # Public API shape
//!
//! Every engine is a **pure function** keyed on
//! `(db, file_id, byte_offset)` — no `lsp_types::Position`, no
//! `lsp_types::Range`.  Position ↔ byte-offset conversion lives in the
//! LSP adapters; JSON DTO shaping lives in the agent.  The engines
//! return *neutral* DTOs ([`CompletionItem`], [`CompletionItemKind`],
//! [`Hover`], [`RewritePayload`], [`RewriteEdit`]) which adapters
//! translate to their wire shapes.
//!
//! # Crate graph (authoritative per AGENTS.md §3.1)
//!
//! ```text
//! cypher-lang-services → cypher-db, cypher-hir, cypher-schema,
//!                        cypher-sema, cypher-syntax, cypher-ast,
//!                        cypher-fmt
//! cypher-lsp           → cypher-lang-services, cypher-db, cypher-diag,
//!                        cypher-fmt, lsp-server, lsp-types
//! cypher-agent         → cypher-lang-services, cypher-db, cypher-diag,
//!                        cypher-fmt, serde_json
//! ```
//!
//! No `lsp-server` / `lsp-types` dependency here — by design.  Any
//! adapter that needs LSP types wraps this crate.

#![forbid(unsafe_code)]

mod complete;
mod hover;
mod rewrite;
mod workspace_nav;

pub use complete::{CompletionItem, CompletionItemKind, complete};
pub use hover::{Hover, hover};
pub use rewrite::{RewriteEdit, RewritePayload, rewrite};
pub use workspace_nav::{
    Location, SymbolInfo, SymbolKind, WorkspaceSymbolIndex, build_index, find_references,
    goto_definition, line_index_for, workspace_symbols,
};
