//! Cross-file navigation engines — spec §14.2 / §15, bead cy-kkw.
//!
//! Three pure functions keyed on `(db, project, file_id, byte-offset)`
//! (or `(db, project, query)` for `workspace_symbols`) that power the
//! LSP `textDocument/references`, `textDocument/definition`, and
//! `workspace/symbol` requests.  The LSP adapter layer in `cyrs-lsp`
//! maps LSP positions ↔ byte offsets and shapes the JSON; this module
//! owns the cross-file logic and has zero LSP dependencies.
//!
//! ## Symbol universe
//!
//! A workspace symbol in v1 is one of four kinds (`SymbolKind`):
//!
//! * [`SymbolKind::Label`] — declared in `schema.toml` via `[[label]]`.
//!   Declaration location points at the `schema.toml` entry; references
//!   are every `:Person` use in any `.cyp` file.
//! * [`SymbolKind::RelType`] — declared in `schema.toml` via
//!   `[[rel_type]]`.
//! * [`SymbolKind::Param`] — declared in `schema.toml` via
//!   `[[parameter]]`.  Reference sites are `$name` parameter tokens.
//! * [`SymbolKind::NamedPath`] — declared in a `.cyp` file via
//!   `MATCH path = (a)-->(b)`.  Declaration is the first binding site
//!   inside the file; references are other identifier uses with that
//!   name in the same file.
//!
//! ## Incrementality note
//!
//! The engines re-walk every member file on every call.  `parse_cst`
//! remains cached by Salsa, so the cost per call is just the token
//! scan.  A future bead (tracked `workspace_symbols` query in
//! `cyrs-db`) can cache the index itself once the tracked-query
//! shape is stable across schema + manifest mutations.

use std::path::{Path, PathBuf};

use cyrs_db::{Database, FileId};
use cyrs_hir::VarKind;
use cyrs_project::ProjectManifest;
use cyrs_schema::SchemaProvider;
use cyrs_syntax::{LineIndex, SyntaxKind, SyntaxToken, TextRange, TextSize};
use rowan::TokenAtOffset;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Public DTOs
// ---------------------------------------------------------------------------

/// The kind of a workspace symbol.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new kinds (e.g. function
/// declarations from the schema's `[[function]]` table) can land
/// without forcing a SemVer-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SymbolKind {
    /// A node label, declared in `schema.toml`.
    Label,
    /// A relationship type, declared in `schema.toml`.
    RelType,
    /// A named parameter, declared in `schema.toml`.
    Param,
    /// A named path alias, declared in a `.cyp` file.
    NamedPath,
}

/// A location in one of the workspace's source files (`.cyp` or
/// `schema.toml`).
///
/// Paths are absolute, matching the paths registered via
/// [`Database::open_file`] (spec §11.4).  `range` is a byte range into
/// that file's source; adapters translate to LSP `Range` via
/// [`LineIndex`].
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so a `uri` field (or similar)
/// can land without a SemVer-major release.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Location {
    /// Absolute file path.
    pub path: PathBuf,
    /// Byte range inside the file.
    pub range: TextRange,
}

/// One entry in the workspace symbol index.
///
/// `declaration` is `None` for symbols whose declaration is in a file
/// not currently tracked by the database (e.g. a schema reference
/// used in a `.cyp` but the `schema.toml` path is unknown).
///
/// Marked `#[non_exhaustive]` (cy-2i9.1).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SymbolInfo {
    /// Canonical name (e.g. `Person`, `KNOWS`, `userId`, `path`).
    pub name: SmolStr,
    /// Symbol kind — drives how reference searches interpret tokens.
    pub kind: SymbolKind,
    /// Declaration site.  `None` if declaration location is unknown
    /// (e.g. schema symbol with no schema file path registered).
    pub declaration: Option<Location>,
    /// All reference sites (declaration included when in-file).
    pub references: Vec<Location>,
}

/// Full workspace symbol index.
///
/// Built once per request by [`build_index`].  Iteration order is
/// deterministic (`Vec`-backed, sorted by `(kind, name)`).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceSymbolIndex {
    entries: Vec<SymbolInfo>,
}

impl WorkspaceSymbolIndex {
    /// All symbols, sorted by `(kind, name)`.
    #[must_use]
    pub fn entries(&self) -> &[SymbolInfo] {
        &self.entries
    }

    /// Look up a single symbol by name.  Returns the first match
    /// across kinds; callers that need kind disambiguation should
    /// filter [`entries`] themselves.
    ///
    /// [`entries`]: Self::entries
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&SymbolInfo> {
        self.entries.iter().find(|s| s.name.as_str() == name)
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Compute the list of reference locations for the identifier at
/// `(file_id, offset)` in the given workspace.
///
/// Returns an empty vector when the cursor is not on a resolvable
/// token.  The declaration site is included in the result (LSP
/// `includeDeclaration` gating is the adapter's job).
///
/// # Workspace scope
///
/// * If the cursor names a label / rel-type / parameter declared in
///   the project's `schema.toml`, references are every matching
///   use across every member `.cyp` file.
/// * If the cursor names a named path alias inside a `.cyp` file,
///   references stay file-local (v1 — cross-file path aliases are
///   not a thing in Cypher).
#[must_use]
pub fn find_references(
    db: &Database,
    project: &ProjectManifest,
    file_id: FileId,
    offset: TextSize,
) -> Vec<Location> {
    let Some(query) = classify_cursor(db, file_id, offset) else {
        return Vec::new();
    };
    let index = build_index(db, project);
    let Some(entry) = index
        .entries
        .iter()
        .find(|e| e.kind == query.kind && e.name.as_str() == query.name.as_str())
    else {
        // Not in the index (e.g. a label referenced but schema empty):
        // fall back to token-matching so the user still sees their
        // own uses.
        return collect_token_matches(db, project, &query);
    };
    entry.references.clone()
}

/// Compute the definition location of the identifier at
/// `(file_id, offset)`.
///
/// Returns `None` when the cursor is not on a resolvable token or
/// the symbol has no declaration site (schema-less workspace, for
/// instance).
#[must_use]
pub fn goto_definition(
    db: &Database,
    project: &ProjectManifest,
    file_id: FileId,
    offset: TextSize,
) -> Option<Location> {
    let query = classify_cursor(db, file_id, offset)?;
    let index = build_index(db, project);
    let entry = index
        .entries
        .iter()
        .find(|e| e.kind == query.kind && e.name.as_str() == query.name.as_str())?;
    entry.declaration.clone()
}

/// Compute the `workspace/symbol` result for a substring query.
///
/// Empty query returns every symbol.  Matching is case-insensitive
/// substring, mirroring the LSP convention rust-analyzer documents.
#[must_use]
pub fn workspace_symbols(db: &Database, project: &ProjectManifest, query: &str) -> Vec<SymbolInfo> {
    let index = build_index(db, project);
    if query.is_empty() {
        return index.entries;
    }
    let needle = query.to_ascii_lowercase();
    index
        .entries
        .into_iter()
        .filter(|s| s.name.to_ascii_lowercase().contains(&needle))
        .collect()
}

/// Build the workspace symbol index.
///
/// Exposed so callers that want to stream multiple queries off the
/// same snapshot can amortise the walk.  Each of [`find_references`],
/// [`goto_definition`], and [`workspace_symbols`] calls this
/// internally when given a fresh project handle.
#[must_use]
pub fn build_index(db: &Database, project: &ProjectManifest) -> WorkspaceSymbolIndex {
    let mut builder = IndexBuilder::default();

    // 1. Seed the index with declarations from the schema.
    if let Some(schema) = db.schema() {
        seed_schema_symbols(&mut builder, &*schema, project);
    }

    // 2. Walk every open file; gather reference sites and per-file
    //    named-path declarations.
    for file_id in db.file_ids() {
        collect_file(db, file_id, &mut builder);
    }

    builder.finish()
}

// ---------------------------------------------------------------------------
// Index builder
// ---------------------------------------------------------------------------

#[derive(Default)]
struct IndexBuilder {
    // Keyed by (kind, name) → entry.  We build as Vec-of-entries at
    // the end to preserve deterministic ordering.
    entries: Vec<SymbolInfo>,
}

impl IndexBuilder {
    fn declare(&mut self, kind: SymbolKind, name: SmolStr, decl: Option<Location>) -> usize {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|e| e.kind == kind && e.name == name)
        {
            if let Some(d) = decl.clone() {
                if self.entries[idx].declaration.is_none() {
                    self.entries[idx].declaration = Some(d.clone());
                }
                // Include the declaration itself in the reference list.
                if !self.entries[idx].references.iter().any(|r| r == &d) {
                    self.entries[idx].references.push(d);
                }
            }
            idx
        } else {
            let mut references = Vec::new();
            if let Some(d) = decl.clone() {
                references.push(d);
            }
            self.entries.push(SymbolInfo {
                name,
                kind,
                declaration: decl,
                references,
            });
            self.entries.len() - 1
        }
    }

    fn add_reference(&mut self, kind: SymbolKind, name: &str, loc: Location) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.kind == kind && e.name.as_str() == name)
        {
            if !entry.references.iter().any(|r| r == &loc) {
                entry.references.push(loc);
            }
        } else {
            // Use-site before declaration: create a declaration-less
            // entry so subsequent lookups still find it.
            self.entries.push(SymbolInfo {
                name: SmolStr::new(name),
                kind,
                declaration: None,
                references: vec![loc],
            });
        }
    }

    fn finish(mut self) -> WorkspaceSymbolIndex {
        // Deterministic order: (kind, name) ascending.
        self.entries.sort_by(|a, b| {
            symbol_kind_order(a.kind)
                .cmp(&symbol_kind_order(b.kind))
                .then_with(|| a.name.cmp(&b.name))
        });
        WorkspaceSymbolIndex {
            entries: self.entries,
        }
    }
}

fn symbol_kind_order(k: SymbolKind) -> u8 {
    match k {
        SymbolKind::Label => 0,
        SymbolKind::RelType => 1,
        SymbolKind::Param => 2,
        SymbolKind::NamedPath => 3,
    }
}

// ---------------------------------------------------------------------------
// Schema seeding
// ---------------------------------------------------------------------------

/// Seed the builder with label / rel-type / parameter declarations
/// from the schema.
///
/// When the project manifest carries a `schema_path`, declaration
/// locations point at `schema.toml`; we scan the file's text for each
/// declared name and return its first match range.  This keeps us
/// honest under `goto_definition`: clicking `:Person` jumps into the
/// schema.toml file, not "location unknown".  If the schema file is
/// unreadable or a name is absent from the text (shouldn't happen in
/// well-formed schemas) we still declare the symbol with a `None`
/// location so `workspace_symbols` can list it.
fn seed_schema_symbols(
    builder: &mut IndexBuilder,
    schema: &dyn SchemaProvider,
    project: &ProjectManifest,
) {
    let schema_abs = project
        .schema_path
        .as_ref()
        .map(|rel| project.manifest_dir.join(rel));
    let schema_text = schema_abs
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    for name in schema.labels() {
        let decl = schema_decl_location(
            schema_abs.as_deref(),
            schema_text.as_deref(),
            "label",
            name.as_str(),
        );
        builder.declare(SymbolKind::Label, name, decl);
    }
    for name in schema.relationship_types() {
        let decl = schema_decl_location(
            schema_abs.as_deref(),
            schema_text.as_deref(),
            "rel_type",
            name.as_str(),
        );
        builder.declare(SymbolKind::RelType, name, decl);
    }
    // Parameters aren't exposed via the SchemaProvider trait — the
    // `InMemorySchema` concrete type owns them.  Expose via
    // downcast-through-SchemaProvider is not possible cleanly; we
    // seed parameter names from the project's underlying schema only
    // when it is the `InMemorySchema` we loaded ourselves.
    if let Some(ref im) = project.schema {
        for decl in im.parameters() {
            let loc = schema_decl_location(
                schema_abs.as_deref(),
                schema_text.as_deref(),
                "parameter",
                decl.name.as_str(),
            );
            builder.declare(SymbolKind::Param, decl.name.clone(), loc);
        }
    }
}

/// Find the source byte range of `name` inside a schema entry of the
/// given `section` (`"label"` / `"rel_type"` / `"parameter"`).  Matches the
/// first occurrence of `name = "NAME"` inside a `[[section]]` block;
/// this is coarse but robust for the hand-written TOML schemas the
/// loader accepts and keeps us free of a second TOML parser inside
/// this crate.
fn schema_decl_location(
    path: Option<&Path>,
    text: Option<&str>,
    section: &str,
    name: &str,
) -> Option<Location> {
    let path = path?;
    let text = text?;
    let header = format!("[[{section}]]");
    let needle = format!("name = \"{name}\"");
    let mut cursor = 0usize;
    while let Some(hdr_off) = text[cursor..].find(&header) {
        let hdr_start = cursor + hdr_off;
        let body_start = hdr_start + header.len();
        // Body of this section ends at the next `[[` or EOF.
        let next_section = text[body_start..]
            .find("[[")
            .map_or(text.len(), |n| body_start + n);
        let body = &text[body_start..next_section];
        if let Some(name_off) = body.find(&needle) {
            let abs_start = body_start + name_off + "name = \"".len();
            let abs_end = abs_start + name.len();
            let range = TextRange::new(
                TextSize::from(u32::try_from(abs_start).ok()?),
                TextSize::from(u32::try_from(abs_end).ok()?),
            );
            return Some(Location {
                path: path.to_path_buf(),
                range,
            });
        }
        cursor = next_section;
    }
    None
}

// ---------------------------------------------------------------------------
// Per-file walk
// ---------------------------------------------------------------------------

fn collect_file(db: &Database, file_id: FileId, builder: &mut IndexBuilder) {
    let Ok(source) = db.source_of(file_id) else {
        return;
    };
    let Ok(path) = db.path_of(file_id) else {
        return;
    };
    let path = path.to_path_buf();
    let Ok(parse) = db.parse_cst(file_id) else {
        return;
    };
    let root = parse.parse().syntax();

    // Label / rel-type references: walk every token of the tree and
    // tag each IDENT with what the grammar places it under.
    for element in root.descendants_with_tokens() {
        let Some(token) = element.as_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }
        if let Some(kind) = classify_ident_token(token) {
            builder.add_reference(
                kind,
                token.text(),
                Location {
                    path: path.clone(),
                    range: token.text_range(),
                },
            );
        }
    }

    // Parameter references: tokens of kind `PARAM` in the CST carry a
    // leading `$` — strip it for the symbol name.
    for element in root.descendants_with_tokens() {
        let Some(token) = element.as_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::PARAM {
            continue;
        }
        let text = token.text();
        let Some(name) = text.strip_prefix('$') else {
            continue;
        };
        // Only report identifier-shaped params; `$0` is a positional.
        if !is_ident_name(name) {
            continue;
        }
        // The reference span points at the full `$name` token so
        // rename / goto work naturally.
        builder.add_reference(
            SymbolKind::Param,
            name,
            Location {
                path: path.clone(),
                range: token.text_range(),
            },
        );
    }

    // Named path aliases: harvest from the file's HIR bindings.
    // Lower from the parsed tree directly (cy-cfi): editor buffers may
    // contain syntax errors, so this keeps the best-effort contract that
    // `lower_statement`'s typed `Err` channel would otherwise reject.
    let stmt = cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(&source))
        .expect("lower_parse is infallible");
    for b in stmt.bindings.values() {
        if b.kind != VarKind::Path {
            continue;
        }
        let loc = Location {
            path: path.clone(),
            range: b.defined_at,
        };
        // The path binding's `defined_at` points at the name itself,
        // so we treat that as the declaration.
        builder.declare(SymbolKind::NamedPath, b.name.clone(), Some(loc.clone()));
        // Also fold in any bare-identifier uses in the same file that
        // match the path name (e.g. `RETURN path`).
        for element in root.descendants_with_tokens() {
            let Some(token) = element.as_token() else {
                continue;
            };
            if token.kind() != SyntaxKind::IDENT {
                continue;
            }
            if token.text() != b.name.as_str() {
                continue;
            }
            // Skip the declaration site (already added via declare).
            if token.text_range() == b.defined_at {
                continue;
            }
            // A conservative filter: we include every identifier
            // occurrence that matches the name.  Name collisions
            // between a path alias and a node/relationship alias
            // inside the same file are handled by the existing
            // single-file `references::compute`; this cross-file
            // path is only reached when the symbol is known to be a
            // path name.
            builder.add_reference(
                SymbolKind::NamedPath,
                b.name.as_str(),
                Location {
                    path: path.clone(),
                    range: token.text_range(),
                },
            );
        }
    }
}

/// Decide whether `token` is playing the role of a label reference
/// (under `NODE_LABEL` / `LABEL_EXPR_ATOM`) or a rel-type reference
/// (under `REL_TYPE` / similar).
///
/// The grammar wraps both under specialised nodes whose kind we can
/// read off the parent chain; v1 keeps the mapping conservative — we
/// only tag tokens whose immediate parent is one of the label-/
/// reltype-typed nodes.
fn classify_ident_token(token: &SyntaxToken) -> Option<SymbolKind> {
    // The grammar wraps `:Foo` label atoms under `LABEL_EXPR` and
    // `:Foo` rel-type atoms under `REL_TYPE_EXPR`.  A single ident
    // sits directly beneath either; we walk one step up to classify.
    // (Any nesting that appears in future — e.g. label-expr
    // disjunction — still places the ident ancestor-wise under a
    // `LABEL_EXPR`, so the walk is safe.)
    let mut anc = token.parent();
    while let Some(p) = anc {
        match p.kind() {
            SyntaxKind::LABEL_EXPR => return Some(SymbolKind::Label),
            SyntaxKind::REL_TYPE_EXPR => return Some(SymbolKind::RelType),
            _ => anc = p.parent(),
        }
    }
    None
}

fn is_ident_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------
// Cursor classification
// ---------------------------------------------------------------------------

struct CursorQuery {
    name: SmolStr,
    kind: SymbolKind,
}

/// Figure out which workspace symbol the cursor is pointing at.
///
/// Returns `None` when the cursor is on trivia or a non-identifier
/// token that does not play a navigation role.
fn classify_cursor(db: &Database, file_id: FileId, offset: TextSize) -> Option<CursorQuery> {
    let parse = db.parse_cst(file_id).ok()?;
    let root = parse.parse().syntax();
    let clamped = u32::min(u32::from(offset), u32::from(root.text_range().end()));
    let token = pick_token(root.token_at_offset(TextSize::from(clamped)))?;

    if token.kind() == SyntaxKind::IDENT {
        if let Some(kind) = classify_ident_token(&token) {
            return Some(CursorQuery {
                name: SmolStr::new(token.text()),
                kind,
            });
        }
        // Named-path cursor: check if the token text matches any
        // `VarKind::Path` binding in this file's HIR. Lower best-effort
        // from the parsed tree (cy-cfi) — the buffer may not parse cleanly.
        let source = db.source_of(file_id).ok()?;
        let stmt = cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(&source))
            .expect("lower_parse is infallible");
        let name = token.text();
        for b in stmt.bindings.values() {
            if b.kind == VarKind::Path && b.name.as_str() == name {
                return Some(CursorQuery {
                    name: b.name.clone(),
                    kind: SymbolKind::NamedPath,
                });
            }
        }
        return None;
    }

    if token.kind() == SyntaxKind::PARAM {
        let name = token.text().strip_prefix('$')?;
        if !is_ident_name(name) {
            return None;
        }
        return Some(CursorQuery {
            name: SmolStr::new(name),
            kind: SymbolKind::Param,
        });
    }

    None
}

fn pick_token(at: TokenAtOffset<SyntaxToken>) -> Option<SyntaxToken> {
    match at {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(left, right) => {
            if left.kind().is_trivia() {
                Some(right)
            } else {
                Some(left)
            }
        }
    }
}

/// Fallback: collect every token match when the index has no entry
/// for this cursor.  Used for symbols whose declaration is absent
/// (e.g. a label appears in a `.cyp` but the schema does not declare
/// it); this way `find_references` still returns the user's own use
/// sites rather than `[]`.
fn collect_token_matches(
    db: &Database,
    _project: &ProjectManifest,
    query: &CursorQuery,
) -> Vec<Location> {
    let mut out = Vec::new();
    for file_id in db.file_ids() {
        let Ok(path) = db.path_of(file_id) else {
            continue;
        };
        let path = path.to_path_buf();
        let Ok(parse) = db.parse_cst(file_id) else {
            continue;
        };
        let root = parse.parse().syntax();
        for element in root.descendants_with_tokens() {
            let Some(token) = element.as_token() else {
                continue;
            };
            match query.kind {
                SymbolKind::Param => {
                    if token.kind() == SyntaxKind::PARAM
                        && token
                            .text()
                            .strip_prefix('$')
                            .is_some_and(|n| n == query.name.as_str())
                    {
                        out.push(Location {
                            path: path.clone(),
                            range: token.text_range(),
                        });
                    }
                }
                _ => {
                    if token.kind() == SyntaxKind::IDENT
                        && token.text() == query.name.as_str()
                        && classify_ident_token(token) == Some(query.kind)
                    {
                        out.push(Location {
                            path: path.clone(),
                            range: token.text_range(),
                        });
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Utilities re-used by adapters
// ---------------------------------------------------------------------------

/// Convert a byte [`TextRange`] into a line-indexed form.  Exposed so
/// the LSP adapter doesn't have to re-implement the `LineIndex` dance.
#[must_use]
pub fn line_index_for(source: &str) -> LineIndex {
    LineIndex::new(source)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cyrs_db::{Database as Db, DialectMode};
    use cyrs_project::{DialectConfig, DialectDefault, ProjectManifest};
    use cyrs_schema::InMemorySchema;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn mk_project(manifest_dir: PathBuf, schema: Option<InMemorySchema>) -> ProjectManifest {
        ProjectManifest {
            name: SmolStr::new("test"),
            version: None,
            description: None,
            dialect: DialectConfig {
                default: DialectDefault::GqlAligned,
                per_file: Vec::new(),
            },
            members: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            schema: schema.clone(),
            schema_path: if schema.is_some() {
                Some(PathBuf::from("schema.toml"))
            } else {
                None
            },
            lint_levels: std::collections::BTreeMap::new(),
            manifest_dir,
        }
    }

    #[test]
    fn workspace_symbols_empty_workspace() {
        let db = Db::new();
        let project = mk_project(PathBuf::from("/tmp"), None);
        let index = build_index(&db, &project);
        assert!(index.entries().is_empty());
    }

    #[test]
    fn named_path_alias_is_indexed() {
        let mut db = Db::new();
        let id = db.open_file(
            Path::new("/tmp/a.cyp"),
            "MATCH path = (a)-->(b) RETURN path".into(),
            DialectMode::GqlAligned,
        );
        let project = mk_project(PathBuf::from("/tmp"), None);
        let index = build_index(&db, &project);
        let entry = index.find("path").expect("path alias must be in the index");
        assert_eq!(entry.kind, SymbolKind::NamedPath);
        assert!(
            entry.declaration.is_some(),
            "named path declaration must be recorded"
        );
        // At least one reference (the declaration itself).
        assert!(!entry.references.is_empty());
        let _ = id;
    }

    #[test]
    fn label_refs_cross_files() {
        let mut db = Db::new();
        let schema = InMemorySchema::builder()
            .add_label(SmolStr::new("Person"), Vec::new())
            .build()
            .expect("schema builds");
        db.set_schema(Some(Arc::new(schema.clone())));

        let _a = db.open_file(
            Path::new("/tmp/a.cyp"),
            "MATCH (a:Person) RETURN a".into(),
            DialectMode::GqlAligned,
        );
        let _b = db.open_file(
            Path::new("/tmp/b.cyp"),
            "MATCH (p:Person) RETURN p".into(),
            DialectMode::GqlAligned,
        );

        // Manifest dir has no real schema.toml so declaration is
        // None but workspace_symbols / references still work.
        let project = mk_project(PathBuf::from("/tmp/does-not-exist"), Some(schema));
        let hits = workspace_symbols(&db, &project, "Perso");
        assert!(
            hits.iter().any(|s| s.name == "Person"),
            "workspace_symbols must return Person for query 'Perso'"
        );
    }
}
