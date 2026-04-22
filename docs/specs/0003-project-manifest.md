# Spec 0003 — Project Manifest (`cypher-project.toml` v0)

| Field          | Value                                                                 |
| -------------- | --------------------------------------------------------------------- |
| Status         | Draft                                                                 |
| Owner          | phall                                                                 |
| Authors        | phall                                                                 |
| Depends on     | 0001-cypher-frontend, 0002-schema-file-format                         |
| Supersedes     | —                                                                     |
| Superseded by  | —                                                                     |

---

## 0. TL;DR

A small, human-authored TOML file that declares a Cypher / GQL **project**:
the set of `.cyp` files that compose it, the schema they share, the default
dialect, and project-local lint levels. Analysis and IDE tooling discover the
manifest by walking up from the current working directory, exactly as Cargo
discovers `Cargo.toml`.

Spec 0001 is locked. This spec evolves v1 scope along the workspace-semantics
axis called out in epic **cy-o8c**, and directly resolves spec 0002 §21 Q1
("where does `schema.toml` live inside a `cypher-project.toml`?") by giving
the schema a single, named entry under `[project.schema]`.

This spec does **not** implement cross-file analysis. It defines the manifest
shape, a loader, and a discovery API. Cross-file analysis, incremental
workspace watching, and SCIP / LSIF export are deferred to follow-up beads
under cy-o8c.

---

## 1. Motivation

Today the LSP, CLI, and agent each accept a single file per invocation. A
multi-file Cypher project has three practical needs that a single-file view
cannot satisfy:

1. **Shared schema.** A project typically has one `schema.toml` (spec 0002)
   that applies to every query in the tree. Today each consumer re-wires
   that path through invocation flags or `initializationOptions`.
2. **Project-local lint config.** Projects want to set a rule to `deny`
   (enforced in CI) in one repo and `warn` (helpful but not blocking) in
   another. A file-scoped config at the project root is the rustc / ruff
   house style; every consumer expects to find it.
3. **A boundary for multi-file analysis.** Forward work under cy-o8c (the
   parent epic) needs to know *what files are in the project* before it
   can run cross-file name resolution or whole-workspace symbol search.
   The manifest is the artefact that answers that question.

A shared, spec-governed manifest is the minimum common surface that unblocks
those three needs without committing the workspace to any particular
multi-file analysis implementation.

This spec defines the v0 manifest: project identity, members (glob-expanded
file list), dialect default, optional schema path, lint levels. Inheritance,
per-file dialect overrides beyond a simple glob map, and `.gitignore`
integration are deferred (§20).

---

## 2. File location

The manifest file is named **`cypher-project.toml`** and lives at the project
root. The loader discovers it by walking up the directory tree from a
starting path (typically the current working directory), exactly as Cargo
discovers `Cargo.toml`. The first ancestor containing a `cypher-project.toml`
is the project root.

No manifest is found ⇒ the tool operates in single-file mode; no project
context is implied. A consumer (LSP / CLI) that wants a project MUST call
[`discover`](§12) explicitly; the loader never walks up without being asked.

Exactly one `cypher-project.toml` per project is supported at v0. Nested
projects (a `cypher-project.toml` inside another) are not meaningful and are
not rejected; the inner file wins when walking up from a path below it.

---

## 3. Format choice: TOML

The format is TOML, for the same reasons spec 0002 §2 documents: diff-friendly
array-of-tables for repeated entries, first-class comments, mature parsers
(`toml` + `taplo`), alignment with Cargo's own manifest, and round-trip
serialisation. JSON, YAML, and custom DSLs were considered and rejected on
the same axes as in spec 0002.

If later spec work needs richer expressivity than TOML supports (e.g., union
types for member selection), a successor spec can introduce a new format.
v0 stays minimal.

---

## 4. Top-level shape

A manifest is a single TOML document with a `[project]` table and a small,
closed set of sub-tables under it. Unknown top-level keys and unknown keys
inside `[project]` are rejected by the loader — forward compatibility
happens through new spec revisions, not silent extensibility.

```toml
[project]
name        = "my-graph"             # required
version     = "0.1.0"                # optional, semver
description = "Graph model for X"    # optional

[project.dialect]
default     = "GqlAligned"           # or "OpenCypherV9"

[project.dialect.per_file]           # optional
"legacy/*.cyp" = "OpenCypherV9"

[project.members]
include = ["**/*.cyp"]               # defaults to ["**/*.cyp"]
exclude = ["**/target/**", "**/.git/**"]

[project.schema]
path = "schema.toml"                 # relative to manifest dir; optional

[project.lint]
"dead-pattern-var"     = "warn"
"unused-import-schema" = "deny"
"wildcard-return"      = "allow"
```

### 4.1 `[project]`

- `name` (string, required) — the project's human-readable identity. Used in
  CLI messages and LSP window titles. Characters are not restricted at v0;
  future work may align with Cargo's package-name rules.
- `version` (string, optional) — semver; advisory at v0, not enforced.
- `description` (string, optional) — free-form prose.

### 4.2 `[project.dialect]`

- `default` (string) — the dialect applied to any member file not matched
  by `per_file`. Accepted values: `"GqlAligned"` (default if the whole
  `[project.dialect]` table is absent) and `"OpenCypherV9"`. Any other
  value is a load error (`UnknownDialect`).
- `per_file` (map of glob → dialect-string, optional) — per-glob dialect
  overrides. Globs are evaluated in declaration order; the first match
  wins. Unmatched files fall back to `default`.

### 4.3 `[project.members]`

- `include` (array of glob strings, optional, default `["**/*.cyp"]`) —
  files to include. Globs are relative to the manifest directory.
- `exclude` (array of glob strings, optional, default
  `["**/target/**", "**/.git/**"]`) — files to exclude. Evaluated after
  `include`; a file must match at least one `include` and no `exclude`
  to appear in the resolved member list.

### 4.4 `[project.schema]`

- `path` (string, optional) — path to a `schema.toml` (spec 0002), relative
  to the manifest directory. When present, the loader passes the file to
  `cypher_schema::file::load_from_toml_path` and stores the resulting
  `InMemorySchema` on the manifest.
- When the whole `[project.schema]` table is absent, the project runs in
  schema-free mode: the semantic pass skips schema-aware checks exactly
  as it does today when no `SchemaProvider` is supplied.

### 4.5 `[project.lint]`

A map of rule-name (string) → level. Levels are one of `"allow"`, `"warn"`,
`"deny"`. Unknown level strings are rejected (`UnknownLevel`, surfaced as
`TomlParse` at v0). Unknown rule names are rejected (`UnknownLintRule`): the
loader consults a small registry of known rule names (see §6) and fails on
any string outside that set.

---

## 5. Resolution order

When loading a manifest from a TOML source, the loader performs these steps
in order:

1. **Manifest discovery** (only when called via [`discover`](§12)). Walk
   up the directory tree from the start path; return the first ancestor
   containing `cypher-project.toml`, or `None`.
2. **TOML parse.** Parse the file with `toml::from_str`. Unknown keys at
   any level are rejected (`deny_unknown_fields` in serde).
3. **Glob expansion.** Walk the manifest directory with the `include` /
   `exclude` globs, producing a sorted `Vec<PathBuf>` of absolute paths
   to member `.cyp` files.
4. **Schema loading.** If `[project.schema].path` is set, resolve the
   path against the manifest directory and call
   `cypher_schema::file::load_from_toml_path`. Propagate any
   `SchemaLoadError` as `ProjectLoadError::Schema`.
5. **Lint level resolution.** Validate every rule name against the
   registered lint set (§6). Unknown names produce
   `ProjectLoadError::UnknownLintRule`. The result is stored as a
   `BTreeMap<String, LintLevel>` so iteration order is deterministic.

The loader is pure: it performs no writes and no network I/O. I/O errors
during discovery, file reads, or glob walking surface as
`ProjectLoadError::Io`.

---

## 6. Lint rule registry

v0 ships a small, closed set of placeholder rule names so the manifest
format can validate lint config end-to-end before the real lints exist:

- `dead-pattern-var` — pattern variable bound but never used.
- `unused-import-schema` — a schema type declared but never referenced.
- `wildcard-return` — a `RETURN *` in a position where explicit projection
  would aid readability.

These are **placeholders**. The real rules will land in `cypher-sema`
alongside the cross-file analysis work under cy-o8c; the registry here
exists solely so a manifest that misspells a rule name fails fast at load
time. A successor spec (or a bead under cy-o8c) will move the registry to
the sema crate and grow the set.

Unknown rule names are a load error at v0 (no silent acceptance). This is
the same policy spec 0002 adopts for unknown top-level keys.

---

## 7. Error taxonomy

The public error type is `ProjectLoadError`:

| Variant              | When                                                       |
| -------------------- | ---------------------------------------------------------- |
| `TomlParse`          | `toml::de::Error` from malformed TOML or unknown keys.     |
| `Io`                 | `std::io::Error` reading the manifest or walking members.  |
| `Schema`             | The referenced schema file failed to load (§4.4 / §5.4).   |
| `GlobError`          | A glob string in `include` / `exclude` / `per_file` is malformed. |
| `UnknownDialect`     | A dialect string outside `{GqlAligned, OpenCypherV9}`.     |
| `UnknownLintRule`    | A rule name outside the §6 registry.                       |
| `ManifestNotFound`   | `load_from_toml_path` called on a missing file, or `discover` returned `None` and the caller expected a manifest. |

The variant set is closed at v0.

---

## 12. Public API surface

Three functions plus the manifest type live in `cypher_project`:

```rust
pub fn load_from_toml_str(input: &str) -> Result<ProjectManifest, ProjectLoadError>;
pub fn load_from_toml_path(path: &Path) -> Result<ProjectManifest, ProjectLoadError>;
pub fn discover(start: &Path) -> Option<PathBuf>;
```

`ProjectManifest` is the public struct carrying the resolved manifest:

```rust
pub struct ProjectManifest {
    pub name: SmolStr,
    pub version: Option<SmolStr>,
    pub description: Option<String>,
    pub dialect: DialectConfig,
    pub members: Vec<PathBuf>,        // resolved, absolute paths
    pub exclude: Vec<String>,         // raw glob strings (diagnostics)
    pub schema: Option<cypher_schema::InMemorySchema>,
    pub lint_levels: BTreeMap<String, LintLevel>,
    pub manifest_dir: PathBuf,
}
```

The CLI surfaces the loader through `cypher project load <path>`, which
prints a one-line human-readable summary and exits 0/1. No JSON output at
v0.

---

## 20. Deferred

- **Workspace analysis.** Running name resolution, diagnostics, and
  symbol search across every member of the project. v0 resolves the
  member list only; it does not load or analyse them.
- **Cross-file references.** `textDocument/references` that spans files
  within the project.
- **`cypher-workspace-symbol` search.** Project-wide fuzzy search over
  labels, rel types, parameter defs, named paths.
- **`.gitignore` integration.** A project will typically want to honour
  `.gitignore` by default. At v0 exclusion is explicit. A later spec
  will integrate the `ignore` crate's gitignore engine.
- **Manifest inheritance.** Nested workspaces that inherit lint levels
  from an outer project.
- **Workspace output overrides.** Project-local overrides for the
  agent / LSP output format (e.g., JSON vs text rendering).
- **Lint rule registry in `cypher-sema`.** The v0 placeholder registry
  moves to `cypher-sema` once the real rules exist.

## 21. Open questions

1. **Cargo.toml interop.** Should a `cypher-project.toml` beside a
   `Cargo.toml` optionally consume `[package.metadata.cypher]` from the
   Cargo manifest, so Rust consumers of `cypher-lsp` / `cypher-agent`
   can pin their project config from `Cargo.toml` alone? Pros: one
   manifest for Rust users. Cons: couples the project format to
   Cargo's evolution, and non-Rust consumers (web, editor
   integrations) would not benefit. Leave open; decide when the first
   Rust-consumer bead asks for it.
