//! `cypher-db` — incremental analysis database (spec 0001 §11).
//!
//! Thin facade today; Salsa integration lands in a follow-up change
//! (spec §11.1 pins Salsa 2022-style API). The facade commits to a
//! stable call surface so binary crates can depend on it:
//!
//! - inputs: `source_text`, `dialect`, `schema`, `sema_options`
//! - derived: `parse`, `hir`, `diagnostics`, `plan`, `formatted`
//!
//! The facade currently re-runs each query on every call. Replacing the
//! internals with Salsa is an invariant-preserving change.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-db/0.0.1")]

use std::sync::{Arc, Mutex};

use cypher_diag::{Diagnostic, DiagnosticsSink};
use cypher_fmt::{FmtOptions, format as fmt_format};
use cypher_schema::{EmptySchema, SchemaProvider};
use cypher_sema::SemaOptions;
use cypher_syntax::{Parse, parse};
use indexmap::IndexMap;
use smol_str::SmolStr;

/// File identity within a `Database`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// Dialect mode selected at parse time. Spec §9.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DialectMode {
    #[default]
    GqlAligned,
    OpenCypherV9,
}

#[derive(Default)]
struct Inner {
    sources: IndexMap<FileId, Arc<str>>,
    dialects: IndexMap<FileId, DialectMode>,
    #[allow(dead_code)] // consumed when sema passes are wired in
    sema_opts: SemaOptions,
    next_file_id: u32,
}

/// The analysis database. `Send + Sync`; snapshotting will arrive with
/// Salsa integration.
pub struct Database {
    inner: Mutex<Inner>,
    schema: Mutex<Arc<dyn SchemaProvider>>,
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            schema: Mutex::new(Arc::new(EmptySchema)),
        }
    }

    pub fn set_schema(&self, schema: Arc<dyn SchemaProvider>) {
        *self.schema.lock().expect("db mutex") = schema;
    }

    pub fn allocate_file(&self) -> FileId {
        let mut i = self.inner.lock().expect("db mutex");
        let id = FileId(i.next_file_id);
        i.next_file_id += 1;
        id
    }

    pub fn set_source(&self, file: FileId, src: impl Into<Arc<str>>) {
        let mut i = self.inner.lock().expect("db mutex");
        i.sources.insert(file, src.into());
    }

    pub fn set_dialect(&self, file: FileId, d: DialectMode) {
        let mut i = self.inner.lock().expect("db mutex");
        i.dialects.insert(file, d);
    }

    fn source_of(&self, file: FileId) -> Arc<str> {
        let i = self.inner.lock().expect("db mutex");
        i.sources
            .get(&file)
            .cloned()
            .unwrap_or_else(|| Arc::from(""))
    }

    #[must_use]
    pub fn parse(&self, file: FileId) -> Parse {
        let src = self.source_of(file);
        parse(&src)
    }

    #[must_use]
    pub fn diagnostics(&self, file: FileId) -> Vec<Diagnostic> {
        let _parse = self.parse(file);
        let sink = DiagnosticsSink::new();
        sink.into_sorted()
    }

    #[must_use]
    pub fn formatted(&self, file: FileId, opts: &FmtOptions) -> SmolStr {
        let src = self.source_of(file);
        fmt_format(&src, opts).into()
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_through_db() {
        let db = Database::new();
        let f = db.allocate_file();
        db.set_source(f, "MATCH (n) RETURN n");
        let p = db.parse(f);
        assert_eq!(p.syntax().to_string(), "MATCH (n) RETURN n");
    }

    #[test]
    fn empty_source_is_ok() {
        let db = Database::new();
        let f = db.allocate_file();
        assert_eq!(db.parse(f).syntax().to_string(), "");
        assert!(db.diagnostics(f).is_empty());
    }
}
