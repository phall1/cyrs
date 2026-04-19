//! `cypher` — meta-crate re-exporting the full front-end library surface.
//!
//! Consumers that want "everything for parsing Cypher and turning it into
//! a typed plan" depend on this crate. Consumers with tighter needs
//! (parse-only, schema-only, etc.) depend on the specific member crate.
//!
//! Spec 0001 §3.1.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher/0.0.1")]

pub use cypher_ast as ast;
pub use cypher_db as db;
pub use cypher_diag as diag;
pub use cypher_fmt as fmt;
pub use cypher_hir as hir;
pub use cypher_plan as plan;
pub use cypher_schema as schema;
pub use cypher_sema as sema;
pub use cypher_syntax as syntax;

pub use cypher_db::{Database, DialectMode, FileId};
pub use cypher_diag::{DiagCode, Diagnostic, Severity};
pub use cypher_schema::{EmptySchema, SchemaProvider};
pub use cypher_syntax::{Parse, SyntaxKind, parse};
