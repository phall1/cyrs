//! `cypher` — meta-crate re-exporting the full front-end library surface.
//!
//! Consumers that want "everything for parsing Cypher and turning it into
//! a typed plan" depend on this crate. Consumers with tighter needs
//! (parse-only, schema-only, etc.) depend on the specific member crate.
//!
//! Feature flags (spec 0001 §17.15):
//! - `default`            — full library surface: core + fmt + schema
//! - `lsp-only`           — library deps consumed by the LSP binary (core + fmt)
//! - `fmt-only`           — formatter slice (core + fmt)
//! - `schema-only`        — schema / type-system slice (core + schema)
//! - `no-default-features` — empty; downstream picks slices explicitly
//!
//! Note: `cyrs-lsp` is a pure binary crate (no lib target), so it is
//! not re-exported here. Use `cargo check -p cyrs-lsp` to verify the
//! LSP binary compiles.
//!
//! Spec 0001 §3.1.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher/0.0.1")]

#[cfg(feature = "core")]
pub use cyrs_ast as ast;
#[cfg(feature = "core")]
pub use cyrs_db as db;
#[cfg(feature = "core")]
pub use cyrs_diag as diag;
#[cfg(feature = "core")]
pub use cyrs_hir as hir;
#[cfg(feature = "core")]
pub use cyrs_plan as plan;
#[cfg(feature = "core")]
pub use cyrs_sema as sema;
#[cfg(feature = "core")]
pub use cyrs_syntax as syntax;

#[cfg(feature = "fmt")]
pub use cyrs_fmt as fmt;

#[cfg(feature = "schema")]
pub use cyrs_project as project;
#[cfg(feature = "schema")]
pub use cyrs_schema as schema;

// ---------------------------------------------------------------------------
// Convenience re-exports at the crate root.
// ---------------------------------------------------------------------------

#[cfg(feature = "core")]
pub use cyrs_db::{
    CypherDatabase, CypherDb, Database, DialectMode, FileId, ParseOutput, SourceFile,
};
#[cfg(feature = "core")]
pub use cyrs_diag::{DiagCode, Diagnostic, Severity};
#[cfg(feature = "core")]
pub use cyrs_syntax::{Parse, SyntaxKind, parse};

#[cfg(feature = "schema")]
pub use cyrs_schema::{EmptySchema, SchemaProvider};
