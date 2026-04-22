# Spec 0002 — Schema File Format (`schema.toml` v0)

| Field          | Value                                                                 |
| -------------- | --------------------------------------------------------------------- |
| Status         | Draft                                                                 |
| Owner          | phall                                                                 |
| Authors        | phall                                                                 |
| Depends on     | 0001-cypher-frontend                                                  |
| Supersedes     | —                                                                     |
| Superseded by  | —                                                                     |

---

## 0. TL;DR

A small, human-authored, human-reviewable file that declares the labels,
relationship types, and query parameters a Cypher workspace expects. The
file is pure data; the front-end loads it into an in-memory
[`SchemaProvider`] and stops. Execution, migrations, and storage layout
are out of scope.

Spec 0001 is locked. This spec evolves v1 scope along exactly the axis
0001 §8 anticipated: "consumers implement [`SchemaProvider`] against
their own storage (graph database catalog, TOML spec, JSON document,
etc.)." The TOML spec track is this one.

---

## 1. Motivation

Spec 0001 §8 defines [`SchemaProvider`] as the only way schema enters
the front-end, but leaves the origin of schema data to consumers. In
practice, three concrete consumers exist in the near term:

1. the CLI (`cypher check`, `cypher plan`) run against a local file
   tree with no database attached;
2. the LSP server started from a VS Code workspace;
3. the agent JSON API invoked by an LLM that has a repository checkout
   but no live graph.

Each of them needs a schema that is:

- **Committed to the repository.** Checked into git alongside the
  queries it types.
- **Diff-friendly.** Property-level changes produce property-level
  diffs; reordering labels does not rewrite the whole file.
- **Human-editable.** An operator scans the file and understands the
  graph's shape in under a minute. Comments are supported; whitespace
  is tolerant.
- **Round-trippable.** The front-end can serialise an in-memory schema
  back to a byte-for-byte equivalent file modulo comments and
  whitespace.

A shared, spec-governed file format is the minimum common surface that
unblocks all three consumers without coupling the workspace to any one
graph database's catalog shape.

This spec defines that format at v0: "enough to declare labels, rel
types, parameters, and a small meta block." Schema inheritance,
sub-typing, and migration versioning are deferred (§20).

---

## 2. Format choice: TOML

The format is TOML. The alternatives considered:

- **TOML** — chosen. Diff-friendly (`[[label]]` table arrays produce
  clean section boundaries); comments are first-class; the Rust
  ecosystem's tools (`toml`, `taplo`, `cargo`'s own parser) are
  rustc-grade; Cargo itself uses TOML, so every contributor already
  reads it; round-trip serialisation is supported.
- **JSON** — rejected. No comments, no trailing commas, fragile diffs
  on deeply nested arrays of objects.
- **YAML** — rejected. Indentation-significant parsing introduces a
  class of bugs the workspace does not want; the spec corpus (`serde`,
  `serde_yaml`) is less maintained than the TOML corpus; type coercion
  surprises (`country: NO` parsing as `false`) are a known footgun.
- **RON / S-expressions / custom DSL** — rejected. Tooling cost
  dominates any expressiveness gain at v0.

TOML wins on every axis that matters for v0. If a later spec needs
richer expressivity than TOML supports (e.g. union types as first-class
syntax), a spec revision can introduce a successor format; v0 stays
minimal.

---

## 3. Top-level shape

A schema file is a TOML document with four kinds of top-level entries:

```toml
# Optional: metadata about the schema file itself.
[meta]
cyrs_schema_version = "0.1.0"
schema_name = "example"
description = "illustrative schema for the cyrs workspace."

# Zero or more label declarations.
[[label]]
name = "Person"
properties = [
    { name = "name", type = "STRING", required = true },
    { name = "age",  type = "INTEGER" },
]

# Zero or more relationship-type declarations.
[[rel_type]]
name = "KNOWS"
start_labels = ["Person"]
end_labels   = ["Person"]
properties   = []

# Zero or more query parameter declarations.
[[parameter]]
name = "min_age"
type = "INTEGER"
default = 18
```

`[[label]]`, `[[rel_type]]`, and `[[parameter]]` are TOML *array of
tables*. Their order within the file is not semantic; the loader
canonicalises on the way in (§10). `[meta]` is a single table and is
optional.

Unknown top-level keys are rejected by the loader — forward
compatibility happens through new spec revisions, not silent
extensibility.

---

## 4. Types

The `type` field of a property or parameter is a string drawn from
this closed grammar:

```
scalar   ::= "STRING" | "INTEGER" | "FLOAT" | "BOOLEAN"
           | "DATE"   | "DATETIME" | "DURATION"
           | "POINT"  | "MAP"     | "NULL"
list     ::= "LIST<" type ">"
modified ::= "NULLABLE " type
type     ::= scalar | list | modified
```

Notes:

- `NULLABLE` is a type modifier, not a variant — `NULLABLE STRING` and
  `STRING` unify with the same value kinds, but the former permits
  `null` inhabitants.
- `LIST<T>` nests: `LIST<STRING>`, `LIST<NULLABLE STRING>`,
  `LIST<LIST<INTEGER>>` are all well-formed.
- `MAP` is intentionally untyped at v0; adding structural map typing
  (record-shaped maps) is deferred to §20.
- Any type string outside this grammar is a load error
  (`SchemaLoadError::BadType`).
- v0 maps onto spec 0001 §8.2's [`PropertyType`] surface. `DURATION`
  and `POINT` have no 0001 equivalent; at load time they fall back to
  `PropertyType::Opaque("DURATION")` / `Opaque("POINT")` so the
  semantic pass still unifies them with matching opaques. A later
  spec will lift them into first-class variants.

---

## 5. Labels

A label declaration:

```toml
[[label]]
name = "Person"                                # required, string
properties = [                                 # optional, array
    { name = "name", type = "STRING", required = true },
    { name = "age",  type = "INTEGER" },
]
```

Each element of `properties` is a table with:

- `name` (string, required) — property name.
- `type` (string, required) — a well-formed type per §4.
- `required` (bool, optional, default `false`) — whether the property
  is required on every instance of the label.

Property order within a label is preserved on round-trip but is not
semantic for lookup purposes.

---

## 6. Relationship types

```toml
[[rel_type]]
name = "ACTED_IN"
start_labels = ["Person"]                      # may be empty = any
end_labels   = ["Movie"]
properties   = [
    { name = "role", type = "STRING" },
]
```

- `name` (string, required) — relationship type.
- `start_labels` (array of string, required) — labels allowed on the
  source endpoint. Empty array means endpoint-polymorphic (the
  semantic pass skips endpoint checks).
- `end_labels` (array of string, required) — same for the target.
- `properties` (array of property-table, optional) — same shape as
  labels.

A rel type has no cardinality field at v0; all endpoints are
many-to-many. First-class cardinality is deferred (§20).

---

## 7. Parameters

```toml
[[parameter]]
name    = "since_year"
type    = "INTEGER"
default = 1990
```

- `name` (string, required) — parameter name as used in Cypher
  (`$since_year`). The leading `$` is not part of the stored name.
- `type` (string, required) — a well-formed type per §4.
- `default` (TOML scalar, optional) — a literal TOML value. Allowed
  shapes: string, integer, float, boolean. Arrays / tables / dates as
  defaults are rejected at v0 (deferred to §20).

The loader stores the default as a Cypher-source literal (`SmolStr`)
for direct reuse in diagnostics.

---

## 8. `[meta]` block

```toml
[meta]
cyrs_schema_version = "0.1.0"           # required when [meta] present
schema_name         = "example"         # optional
description         = "one-liner."      # optional
```

- `cyrs_schema_version` (string, required inside `[meta]`) — the
  format version this file was authored against. v0 accepts exactly
  `"0.1.0"`. Newer loaders MAY accept older versions; older loaders
  reject newer ones (`SchemaLoadError::BadType` with a version-shaped
  message).
- `schema_name` (string, optional) — a short human identifier. Used
  only for CLI messages and LSP window titles.
- `description` (string, optional) — free-form prose. Preserved on
  round-trip.

`[meta]` is optional at v0 — a file with only `[[label]]`, `[[rel_type]]`,
and `[[parameter]]` tables loads. Emitting a `[meta]` block on serialise
is encouraged but not required.

---

## 9. Invariants

Enforced at load time:

- All `label.name` values are unique (`SchemaLoadError::DuplicateLabel`).
- All `rel_type.name` values are unique (`SchemaLoadError::DuplicateRelType`).
- Every `rel_type.start_labels` and `rel_type.end_labels` entry either
  is empty or references a declared label
  (`SchemaLoadError::UnknownLabelRef`).
- Every `type` field is well-formed per §4
  (`SchemaLoadError::BadType`).

Not enforced at v0 (property names within a label may repeat; that is
a future lint candidate).

---

## 10. Round-trip property

For any schema `s` loadable by the v0 loader:

```
load(serialise(s)) ≡ s
```

where `≡` is semantic equality — ordering within collections does not
matter, comments and whitespace do not matter, but every declaration
round-trips byte-for-byte in its scalar fields.

The test harness (`crates/cypher-schema/tests/file.rs`) expresses this
property on a representative fixture at v0; a proptest suite over
schema shape is deferred to a later bead.

Internally the loader uses `BTreeMap` for anything that crosses a
public boundary, per spec 0001 §17.14 (no `HashMap` iteration order in
outputs).

---

## 11. Error taxonomy

The public error type is `SchemaLoadError`:

| Variant               | When                                                      |
| --------------------- | --------------------------------------------------------- |
| `TomlParse`           | `toml::de::Error` from malformed TOML.                    |
| `Io`                  | `std::io::Error` from `load_from_toml_path`.              |
| `UnknownLabelRef(n)`  | Rel type endpoint references an undeclared label.         |
| `DuplicateLabel(n)`   | Same label name appears twice.                            |
| `DuplicateRelType(n)` | Same rel type name appears twice.                         |
| `BadType(s)`          | Type string outside the §4 grammar, or bad version tag.   |

The variant set is closed at v0; adding a variant is a minor breaking
change and a spec revision.

---

## 12. Public API surface

Three functions live in `cypher_schema::file`:

```rust
pub fn load_from_toml_str(input: &str) -> Result<InMemorySchema, SchemaLoadError>;
pub fn load_from_toml_path(path: &Path) -> Result<InMemorySchema, SchemaLoadError>;
pub fn serialise_to_toml(schema: &InMemorySchema) -> String;
```

[`InMemorySchema`] is the concrete `SchemaProvider` implementation in
`cypher-schema`; see spec 0001 §8.1 for the trait contract.

The CLI surfaces the loader through `cypher schema load <path>`, which
prints a one-line human-readable summary and exits 0/1. No JSON output
at v0.

---

## 20. Deferred

- **Schema inheritance.** Labels extending other labels ("Person is-a
  Entity") and property set inheritance.
- **Sub-typing.** First-class cardinality on relationship endpoints
  (spec 0001 §8.2 [`Cardinality`] exists but is not surfaced in the
  file format).
- **Typed maps.** Record-shaped map types with named fields.
- **Schema versioning for migrations.** A migration plan between two
  `schema.toml` revisions as a distinct artefact.
- **First-class `DURATION` and `POINT` types** (currently opaque).
- **Default values richer than scalars.** Array and table defaults for
  parameters.
- **File-format version negotiation.** Today a loader accepts
  exactly `cyrs_schema_version = "0.1.0"`; a successor spec will
  define compatibility ranges.
- **Proptest round-trip** over arbitrary schema shapes.

## 21. Open questions

1. **Workspace integration (cy-o8c).** Where does `schema.toml` live
   inside a `cypher-project.toml`? Candidates: a `schema_path = "..."`
   key at the workspace root; a `[[schema]]` array of tables for
   multi-schema projects; or inline `[schema]` for single-file
   workspaces. This spec does not commit to one; the cy-o8c epic
   resolves it.
2. **File-format self-versioning.** Today v0 pins `cyrs_schema_version
   = "0.1.0"`. Should minor bumps be additive-only (new optional
   fields) with the loader silently accepting unknown keys behind a
   `strict` / `lenient` flag, or should every bump be a new spec? The
   workspace leans toward the latter (strict-by-default, spec-governed
   evolution) but this is not yet decided.
