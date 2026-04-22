//! SCIP index emitter (spec 0001 §14, bead cy-o8c tranche 3 / cy-k2r).
//!
//! Converts `cypher-lang-services`' workspace symbol index
//! ([`cypher_lang_services::WorkspaceSymbolIndex`] — see spec §14.2,
//! bead cy-kkw) into a SCIP proto-encoded index that the Sourcegraph /
//! code-search ecosystem can consume.
//!
//! # Why SCIP?
//!
//! SCIP is the Sourcegraph Code Index Protocol — a compact, proto3-encoded
//! "index" representing every symbol, occurrence, and doc site in a
//! codebase.  The wire format is stable and consumed by Sourcegraph,
//! `VSCode`, and code-search tooling at large.  Publishing a SCIP index
//! for a Cypher workspace makes cross-repo symbol navigation possible
//! via any SCIP-aware indexer host.
//!
//! # Subcommand shape
//!
//! ```text
//! cypher index scip <path> [--output <file>] [--stdout]
//! ```
//!
//! * `<path>` is walked via [`cypher_project::discover`] to find the
//!   workspace's `cypher-project.toml`; members + schema are loaded into
//!   a fresh [`Database`] exactly like `cypher check <dir>` already
//!   does.  The SCIP emitter reuses the project discovery / schema
//!   wiring intentionally — the symbol universe it dumps is the same
//!   one the LSP surfaces for go-to-def / references, so a fresh
//!   emitter and a running LSP stay consistent.
//! * `--output <file>` writes to an explicit path; the default is
//!   `<path>/index.scip`.  `--stdout` bypasses file output and dumps
//!   the proto-encoded bytes to stdout for piping into `scip print`
//!   or similar.
//!
//! # Symbol encoding
//!
//! SCIP symbols follow a descriptor-grammar scheme:
//!
//! ```text
//! scheme          package-manager package-name package-version  descriptors
//! cypher-frontend cypher-project  <project>    <version|HEAD>   <kind>/<name>#
//! ```
//!
//! * `Label` → `<project>/<name>#` (Namespace + Type descriptor)
//! * `RelType` → `<project>/<name>#` (Interface)
//! * `Param` → `<project>/<name>.` (Term descriptor, trailing `.`)
//! * `NamedPath` → `<project>/<name>/<file-stem>.` (Term scoped per-file)
//!
//! Every declaration site emits a [`scip::types::Occurrence`] with
//! `SymbolRole::Definition`, every reference site emits a plain
//! `ReadAccess` occurrence.  `SymbolInformation` is added once per
//! symbol on the document that carries its declaration.
//!
//! # Round-tripping
//!
//! [`emit_index`] returns the fully-built [`scip::types::Index`] so
//! tests can round-trip it through `protobuf::Message::write_to_bytes`
//! + `parse_from_bytes` without touching the filesystem.  The CLI
//!   subcommand calls [`emit_index`] + [`write_index`] / [`stdout_index`].

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use cypher_db::{Database, DialectMode};
use cypher_lang_services::{Location, SymbolKind as CyKind, WorkspaceSymbolIndex, build_index};
use cypher_project::{DialectDefault, ProjectManifest};
use cypher_schema::SchemaProvider;
use protobuf::{Enum, Message};
use scip::types::{
    Document, Index, Metadata, Occurrence, ProtocolVersion, SymbolInformation, TextEncoding,
    ToolInfo, symbol_information,
};

/// SCIP scheme identifier for the cyrs Cypher / GQL front-end.
///
/// Stable: once published in a SCIP index this string must not change
/// (Sourcegraph cross-references treat it as a namespace key).  If
/// ever refined, do it via a spec-governed bump + migration note.
const SCIP_SCHEME: &str = "cypher-frontend";

/// Package-manager slug embedded in every emitted symbol.  Chosen to
/// mirror the ecosystem's real-world "cypher-project" manifest name.
const PACKAGE_MANAGER: &str = "cypher-project";

/// Tool name surfaced in the SCIP `Metadata.tool_info` field.
const TOOL_NAME: &str = "cypher-cli";

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Discover + load the project rooted at `path`, walk the workspace
/// symbol index, and return a fully-populated [`Index`].
///
/// Errors bubble out of project discovery, schema load, or member
/// reads; the index itself cannot fail to construct once the database
/// is populated.
pub fn emit_index(path: &Path) -> Result<Index> {
    let manifest_path = cypher_project::discover(path).with_context(|| {
        format!(
            "no cypher-project.toml found at or above {}",
            path.display()
        )
    })?;
    let manifest = cypher_project::load_from_toml_path(&manifest_path)
        .with_context(|| format!("loading {}", manifest_path.display()))?;

    let mut db = Database::new();
    if let Some(schema) = clone_schema(&manifest) {
        db.set_schema(Some(schema));
    }

    // Track (FileId, absolute path, raw source) so we can emit SCIP
    // occurrences keyed on absolute-path documents.  This mirrors the
    // `cypher check <dir>` loader in main.rs.
    let mut loaded: Vec<(cypher_db::FileId, PathBuf, String)> =
        Vec::with_capacity(manifest.members.len());
    for member in &manifest.members {
        let source = fs::read_to_string(member)
            .with_context(|| format!("reading member {}", member.display()))?;
        let dialect = match manifest.dialect_for(member) {
            DialectDefault::GqlAligned => DialectMode::GqlAligned,
            DialectDefault::OpenCypherV9 => DialectMode::OpenCypherV9,
        };
        let id = db.open_file(member, source.clone(), dialect);
        loaded.push((id, member.clone(), source));
    }

    let symbol_index = build_index(&db, &manifest);

    Ok(build_scip(&manifest, &symbol_index, &loaded))
}

/// Serialise `index` to bytes via protobuf and write them to `path`.
///
/// Creates parent directories if missing (matching `fs::write`
/// behaviour — the caller is responsible for `path`'s placement).
pub fn write_index(index: &Index, path: &Path) -> Result<()> {
    let bytes = index
        .write_to_bytes()
        .context("serialising SCIP index to protobuf")?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Serialise `index` to bytes and dump them on stdout.  Used by the
/// `--stdout` flag so the output can pipe into `scip print`.
pub fn stdout_index(index: &Index) -> Result<()> {
    let bytes = index
        .write_to_bytes()
        .context("serialising SCIP index to protobuf")?;
    io::stdout()
        .write_all(&bytes)
        .context("writing SCIP bytes to stdout")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build the `Index` proto from a populated symbol index + loaded
/// documents.  Split out so tests can drive it directly.
fn build_scip(
    manifest: &ProjectManifest,
    index: &WorkspaceSymbolIndex,
    loaded: &[(cypher_db::FileId, PathBuf, String)],
) -> Index {
    let mut out = Index::new();
    out.metadata = protobuf::MessageField::some(build_metadata(manifest));

    // Reverse lookup: absolute path → (line index, SCIP document slot).
    // SCIP wants one `Document` per source file; we materialise them
    // lazily as we encounter occurrences.  `loaded` drives the
    // discovery order so documents land in manifest order, which is
    // deterministic (spec §17.14).
    let mut documents: Vec<(PathBuf, Document)> = loaded
        .iter()
        .map(|(_, path, _source)| {
            let mut doc = Document::new();
            doc.language = "Cypher".to_string();
            doc.relative_path = relative_path(&manifest.manifest_dir, path);
            // UTF-8 byte offsets — we hand back byte-range-derived line/char
            // pairs, where "char" is UTF-8 byte offset from line start.
            // SCIP's PositionEncoding::UTF8CodeUnitOffsetFromLineStart is
            // the right value (byte offset = UTF-8 code unit offset).
            doc.position_encoding =
                scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into();
            (path.clone(), doc)
        })
        .collect();

    // Source-text snapshots, parallel array with `documents`, so we
    // can convert a byte offset to SCIP's line/char pair on demand.
    let sources: Vec<&str> = loaded.iter().map(|(_, _, s)| s.as_str()).collect();

    let project_version = manifest.version.as_deref().map_or("HEAD", |v| v);
    let project_name = manifest.name.as_str();

    // For schema decls (schema.toml), we may need to emit a document
    // that is not in `loaded` — schema.toml isn't opened into the
    // database.  Track it here and merge at the end.
    let schema_abs = manifest
        .schema_path
        .as_ref()
        .map(|rel| manifest.manifest_dir.join(rel));
    let schema_text = schema_abs
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    let mut schema_doc: Option<Document> = None;
    if let (Some(abs), Some(_)) = (schema_abs.as_deref(), schema_text.as_deref()) {
        let mut d = Document::new();
        d.language = "TOML".to_string();
        d.relative_path = relative_path(&manifest.manifest_dir, abs);
        d.position_encoding = scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into();
        schema_doc = Some(d);
    }

    for entry in index.entries() {
        let scip_symbol = encode_symbol(project_name, project_version, entry.kind, &entry.name);
        let sym_info = build_symbol_info(&scip_symbol, entry.kind, &entry.name);

        // Emit the declaration occurrence + attach SymbolInformation
        // to the document that carries it.  Schema decls go on
        // `schema.toml`; in-file NamedPath decls go on their home file.
        if let Some(decl) = entry.declaration.as_ref() {
            let is_schema = schema_abs
                .as_deref()
                .is_some_and(|s| s == decl.path.as_path());
            if is_schema {
                if let (Some(ref mut d), Some(text)) = (schema_doc.as_mut(), schema_text.as_deref())
                {
                    d.occurrences.push(occurrence(
                        &scip_symbol,
                        text,
                        decl,
                        scip::types::SymbolRole::Definition.value(),
                    ));
                    d.symbols.push(sym_info.clone());
                }
            } else if let Some((idx, _)) = documents
                .iter_mut()
                .enumerate()
                .find(|(_, (p, _))| *p == decl.path)
            {
                let (_, doc) = &mut documents[idx];
                doc.occurrences.push(occurrence(
                    &scip_symbol,
                    sources[idx],
                    decl,
                    scip::types::SymbolRole::Definition.value(),
                ));
                doc.symbols.push(sym_info.clone());
            }
        }

        // Emit every reference occurrence.  The declaration site will
        // also appear in `references` (workspace_nav's invariant);
        // we dedupe by not double-pushing when the ref equals the decl
        // we already emitted as a Definition.
        for reference in &entry.references {
            if entry.declaration.as_ref().is_some_and(|d| d == reference) {
                continue;
            }
            let is_schema = schema_abs
                .as_deref()
                .is_some_and(|s| s == reference.path.as_path());
            if is_schema {
                if let (Some(ref mut d), Some(text)) = (schema_doc.as_mut(), schema_text.as_deref())
                {
                    d.occurrences.push(occurrence(
                        &scip_symbol,
                        text,
                        reference,
                        scip::types::SymbolRole::ReadAccess.value(),
                    ));
                }
            } else if let Some((idx, _)) = documents
                .iter_mut()
                .enumerate()
                .find(|(_, (p, _))| *p == reference.path)
            {
                let (_, doc) = &mut documents[idx];
                doc.occurrences.push(occurrence(
                    &scip_symbol,
                    sources[idx],
                    reference,
                    scip::types::SymbolRole::ReadAccess.value(),
                ));
            }
        }
    }

    for (_, doc) in documents {
        if doc.occurrences.is_empty() && doc.symbols.is_empty() {
            // Empty document — skip, SCIP consumers don't care about
            // source files with no symbols.  Keeps the index smaller
            // and matches the scip-python / scip-rust convention.
            continue;
        }
        out.documents.push(doc);
    }
    if let Some(d) = schema_doc
        && !(d.occurrences.is_empty() && d.symbols.is_empty())
    {
        out.documents.push(d);
    }

    out
}

fn build_metadata(manifest: &ProjectManifest) -> Metadata {
    let mut tool = ToolInfo::new();
    tool.name = TOOL_NAME.to_string();
    tool.version = env!("CARGO_PKG_VERSION").to_string();
    tool.arguments = Vec::new();

    let mut meta = Metadata::new();
    meta.version = ProtocolVersion::UnspecifiedProtocolVersion.into();
    meta.tool_info = protobuf::MessageField::some(tool);
    // `project_root` must be a URI-encoded absolute path.  We use
    // `file://` + the manifest directory verbatim; `display()` on a
    // `Path` is already UTF-8 on every platform we support.
    meta.project_root = format!("file://{}", manifest.manifest_dir.display());
    meta.text_document_encoding = TextEncoding::UTF8.into();
    meta
}

fn build_symbol_info(symbol: &str, kind: CyKind, name: &str) -> SymbolInformation {
    let mut info = SymbolInformation::new();
    info.symbol = symbol.to_string();
    info.display_name = name.to_string();
    info.kind = scip_kind_for(kind).into();
    info
}

fn scip_kind_for(kind: CyKind) -> symbol_information::Kind {
    match kind {
        CyKind::Label => symbol_information::Kind::Class,
        CyKind::RelType => symbol_information::Kind::Interface,
        CyKind::Param => symbol_information::Kind::Parameter,
        CyKind::NamedPath => symbol_information::Kind::Key,
        // Future-kind fallback — the `#[non_exhaustive]` marker on
        // `cypher_lang_services::SymbolKind` means we must handle
        // additions gracefully.
        _ => symbol_information::Kind::UnspecifiedKind,
    }
}

fn occurrence(symbol: &str, source: &str, loc: &Location, role: i32) -> Occurrence {
    let mut occ = Occurrence::new();
    occ.range = range_to_scip(source, loc);
    occ.symbol = symbol.to_string();
    occ.symbol_roles = role;
    occ
}

/// Convert a byte-range [`Location`] against its source text into
/// SCIP's 4-element `[startLine, startChar, endLine, endChar]` form.
///
/// We avoid dragging `cypher_syntax::LineIndex` into this crate
/// (cypher-cli's edge list only admits `cypher-db`, `-diag`, `-fmt`,
/// `-schema`, `-project`, `-lang-services`) by reconstructing line/col
/// from the source string with a single linear scan.  Fine for
/// emitter-side work because each occurrence's offset is small and
/// contiguous with the previous one.
fn range_to_scip(source: &str, loc: &Location) -> Vec<i32> {
    let (start_line, start_col) = line_col(source, loc.range.start().into());
    let (end_line, end_col) = line_col(source, loc.range.end().into());
    vec![start_line, start_col, end_line, end_col]
}

/// Compute `(line, utf8_col)` for a byte `offset` in `source`.
///
/// Under SCIP's `UTF8CodeUnitOffsetFromLineStart` encoding the
/// "character" coordinate is the byte offset from the last newline,
/// which matches exactly what this function returns.
fn line_col(source: &str, offset: usize) -> (i32, i32) {
    let clamped = offset.min(source.len());
    let mut line: i32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in source.as_bytes().iter().enumerate().take(clamped) {
        if *b == b'\n' {
            line = line.saturating_add(1);
            line_start = i + 1;
        }
    }
    let col = i32::try_from(clamped.saturating_sub(line_start)).unwrap_or(i32::MAX);
    (line, col)
}

/// Encode a workspace symbol as a SCIP symbol string.
///
/// The SCIP symbol grammar is: `<scheme> <package-manager> <package-name>
/// <package-version> <descriptors>`, with each field space-separated
/// except when one field contains a space.  Our symbols are simple
/// namespace + name pairs so we avoid spaces in every descriptor.
///
/// Descriptor suffix:
/// * `Label` → `#` (type)
/// * `RelType` → `#` (interface / type)
/// * `Param` → `.` (term)
/// * `NamedPath` → `.` (term)
fn encode_symbol(project: &str, version: &str, kind: CyKind, name: &str) -> String {
    // The `_` arm collapses future kinds (`#[non_exhaustive]`) onto the
    // term-style `.` suffix.  Keep the explicit `Param | NamedPath`
    // arm alongside it so the known-kind mapping stays in the match
    // body rather than hiding under the wildcard.
    let suffix = match kind {
        CyKind::Label | CyKind::RelType => '#',
        _ => '.',
    };
    let kind_ns = match kind {
        CyKind::Label => "label",
        CyKind::RelType => "relType",
        CyKind::Param => "param",
        CyKind::NamedPath => "namedPath",
        _ => "other",
    };
    // `cypher-frontend cypher-project <project> <version> <kind>/<name><suffix>`
    format!("{SCIP_SCHEME} {PACKAGE_MANAGER} {project} {version} {kind_ns}/{name}{suffix}")
}

fn clone_schema(manifest: &ProjectManifest) -> Option<Arc<dyn SchemaProvider>> {
    manifest
        .schema
        .as_ref()
        .map(|s| Arc::new(s.clone()) as Arc<dyn SchemaProvider>)
}

/// Return `abs` as a path relative to `root`, or the absolute path if
/// `abs` is not a descendant of `root`.  SCIP's `Document.relative_path`
/// is relative to `Metadata.project_root`.
fn relative_path(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .map_or_else(|_| abs.display().to_string(), |r| r.display().to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Fixture helper: build a tiny 3-file workspace on disk, return the
    /// root directory (so `TempDir` stays alive).
    fn mk_workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();

        fs::write(
            root.join("cypher-project.toml"),
            "[project]\nname = \"scip-fixture\"\n\n\
             [project.schema]\npath = \"schema.toml\"\n",
        )
        .unwrap();

        fs::write(
            root.join("schema.toml"),
            "[meta]\ncyrs_schema_version = \"0.1.0\"\n\n\
             [[label]]\nname = \"Person\"\n\n\
             [[rel_type]]\nname = \"KNOWS\"\nstart_labels = []\nend_labels = []\n",
        )
        .unwrap();

        fs::write(root.join("a.cyp"), "MATCH (p:Person) RETURN p\n").unwrap();
        fs::write(
            root.join("b.cyp"),
            "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a, b\n",
        )
        .unwrap();
        (dir, root)
    }

    #[test]
    fn emit_index_produces_at_least_one_symbol() {
        let (_dir, root) = mk_workspace();
        let idx = emit_index(&root).expect("index build");
        // Person + KNOWS → at least 2 distinct SymbolInformation entries
        // across documents.
        let total_symbols: usize = idx.documents.iter().map(|d| d.symbols.len()).sum();
        assert!(
            total_symbols >= 2,
            "expected >= 2 SymbolInformation rows, got {total_symbols}"
        );

        // Occurrences: schema decl + uses in a.cyp + uses in b.cyp.
        let total_occs: usize = idx.documents.iter().map(|d| d.occurrences.len()).sum();
        assert!(
            total_occs >= 4,
            "expected >= 4 occurrences, got {total_occs}"
        );
    }

    #[test]
    fn emit_index_roundtrips_through_scip_reader() {
        let (_dir, root) = mk_workspace();
        let idx = emit_index(&root).expect("index build");
        let bytes = idx.write_to_bytes().expect("serialise");

        let parsed = Index::parse_from_bytes(&bytes).expect("round-trip");
        assert_eq!(parsed.documents.len(), idx.documents.len());

        // Every symbol we wrote shows up in the parsed index, unchanged.
        let mut original_syms: Vec<String> = idx
            .documents
            .iter()
            .flat_map(|d| d.symbols.iter().map(|s| s.symbol.clone()))
            .collect();
        let mut parsed_syms: Vec<String> = parsed
            .documents
            .iter()
            .flat_map(|d| d.symbols.iter().map(|s| s.symbol.clone()))
            .collect();
        original_syms.sort();
        parsed_syms.sort();
        assert_eq!(original_syms, parsed_syms);

        // Metadata survives the round-trip.
        assert_eq!(
            parsed.metadata.project_root,
            format!("file://{}", root.display())
        );
    }

    #[test]
    fn emit_index_encodes_label_kind_as_class() {
        let (_dir, root) = mk_workspace();
        let idx = emit_index(&root).expect("index build");
        let person = idx
            .documents
            .iter()
            .flat_map(|d| d.symbols.iter())
            .find(|s| s.display_name == "Person")
            .expect("Person must appear in the index");
        assert_eq!(
            person.kind.enum_value(),
            Ok(symbol_information::Kind::Class)
        );
    }

    #[test]
    fn write_index_produces_readable_file() {
        let (_dir, root) = mk_workspace();
        let idx = emit_index(&root).expect("index build");
        let out = root.join("index.scip");
        write_index(&idx, &out).expect("write");
        let bytes = fs::read(&out).expect("read scip");
        let parsed = Index::parse_from_bytes(&bytes).expect("round-trip");
        assert!(!parsed.documents.is_empty());
    }
}
