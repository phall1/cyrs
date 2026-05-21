# cyrs feature requests — from the pgGraph embedder

> **Embedder:** [pgGraph](https://github.com/) — a PostgreSQL extension that
> exposes graph queries over ordinary registered tables. It is adding a
> `graph.cypher(text, jsonb)` SQL function that parses openCypher v9
> through cyrs and dispatches the resulting plan to pgGraph's existing
> in-memory engine (reads) and SPI-issued DML (writes).
>
> **Companion document:** pgGraph's integration spec lives in
> `pgGraph/docs/contributor_guide/cypher-frontend/` (000–080). Its
> `080-open-questions.md` `Q-UP-*` items each cite a section number
> below; **the `§N.M` numbering here is a stable contract** — pgGraph
> docs link to it. Retitle freely, but do not renumber.

This is distinct from `docs/embedder-issues/` (0001–0010), which is
feedback from a *different* embedder migrating off a bespoke parser.
A few asks here overlap those; cross-links are noted inline.

---

## ✅ Resolution status — all 13 items shipped

Every request in this document landed in **cyrs 0.1.0** (19 crates
published to crates.io, 2026-05-10; embedder PRs #56 and #58). Two
items — §1.3 and §4.2 — were over-delivered beyond the original ask.

The per-section **Status** lines and the summary table record what
shipped; the *Problem* / *Proposed shape* bodies are kept as the
historical record of the ask. pgGraph's `080-open-questions.md`
Upstream section is updated to match — no `Q-UP-*` item blocks
pgGraph any longer.

## How to read this

Each request carries:

- **Blocks** — the pgGraph milestone (M0–M5) that needs it.
- **Severity** — from pgGraph's perspective; cyrs may prioritise
  differently.
- **Status** — what shipped (all resolved as of cyrs 0.1.0).

---

## §1 — `cyrs-plan` additions

### §1.1 — `ShortestPath` read operator

- **Blocks:** pgGraph M5. **Severity:** medium (deferrable).
- **Status:** ✅ **Resolved** (cyrs 0.1.0) — `cyrs_plan::ReadOp`
  gained a `ShortestPath { input, from, to, rel, kind, bind_path }`
  variant; sema lowers `shortestPath` / `allShortestPaths` to it.

**Problem.** `cyrs_plan::ReadOp` has `Source`, `Expand`, `Filter`,
`Project`, `Aggregate`, `OrderBy`, `Skip`, `Limit`, `Distinct`,
`Unwind`, `Union`, `With`, `OptionalJoin` — but no shortest-path
operator. `MATCH p = shortestPath((a)-[*]-(b))` either fails to lower
or degrades to a generic var-length `Expand`, losing the
shortest-path semantics. pgGraph has a native `path_finder` module
and wants a dedicated op to dispatch to it instead of post-filtering
an exhaustive expansion.

**Proposed shape.** A new `ReadOp` variant:

```rust
ShortestPath {
    input: OpId,
    from: VarId,
    to: VarId,
    rel: RelSpec,          // the var-length pattern between the endpoints
    kind: ShortestPathKind, // Single | All
    bind_path: VarId,       // receives the path value (see §1.2)
}
```

sema validates the `shortestPath` / `allShortestPaths` call shape and
lowers to it.

### §1.2 — Path-variable surface in the plan IR

- **Blocks:** pgGraph M2 (`RETURN p` for a matched path).
- **Severity:** medium.
- **Status:** ✅ **Resolved** — documented contract: the plan IR has
  no `Path` type; the embedder owns path materialisation;
  `cyrs_hir::VarKind::Path` carries the contract and `ShortestPath`
  is the only path-producing read operator.

**Problem.** For `MATCH p = (a)-[*1..3]->(b) RETURN p`, the embedder
must materialise `p` as a value. It is unclear from the IR docs how a
path-bound variable is represented: is `p` a `VarId` carrying a
`Path` type, and what is the structural contract — an ordered
node/relationship element sequence, or an opaque value the embedder
shapes itself?

**Proposed shape.** Either:

1. document a `PlanType::Path` and state that a path-bound variable
   yields a value with `nodes()` / `relationships()` / `length()`
   accessor semantics, **or**
2. state explicitly that cyrs guarantees only the ordered element
   sequence and the embedder owns materialisation.

Either is fine — pgGraph needs the contract pinned, not a specific
representation. *(cyrs took option 2.)*

### §1.3 — Builtin function enumeration *(resolved — exceeded)*

- **Blocks:** pgGraph M2 (`RETURN` with function calls).
- **Severity:** low.
- **Status:** ✅ **Resolved, exceeded** — beyond the existing
  `builtin_names()` / `builtin_count()`, cyrs 0.1.0 added
  `StandardLibrary::builtin_signature()` exposing the full
  `FunctionSignature` per name, including `deterministic` and
  `null_propagating` flags. Builtin enumeration is now normative and
  embedder-facing.

**Context.** pgGraph dispatches each Cypher function to either a SQL
fragment or a Rust evaluator, and wants a CI test that fails when
cyrs adds a function pgGraph does not handle.

`cyrs_schema::StandardLibrary` already exposed
`builtin_names() -> Vec<&'static str>` (documented "stable across
releases") and `builtin_count() -> usize`. That was already enough
for the drift test; the request was to confirm it and, if cheap,
expose per-function arity / signature metadata.

---

## §2 — `SchemaProvider` and write-plan surface

### §2.1 — MERGE key surface on `WriteOp::MergeNode` / `MergeRel`

- **Blocks:** pgGraph M4. **Severity:** high.
- **Status:** ✅ **Resolved** — `WriteOp::MergeNode` and `MergeRel`
  gained `key_props: Vec<SmolStr>`, populated by HIR→Plan lowering
  whenever the pattern's `props` is a literal `Expr::Map`. When it is
  not, `key_props` is empty and the embedder falls back to inspecting
  `props`.
- **Related:** `docs/embedder-issues/0007` (write-side coverage).

**Problem.** `WriteOp::MergeNode { labels, props: Expr, on_create,
on_match, bind }` exposes the merge pattern's properties as a single
opaque `props` map expression. pgGraph compiles MERGE to
`INSERT ... ON CONFLICT (<key columns>) DO UPDATE ...`, which needs
the **list of key property names** as structured data. Today pgGraph
must crack open the `props` `Expr`, assume it is a literal map, and
re-derive the keys — duplicating analysis lowering already did.

**Proposed shape.** Surface the pattern key properties as structured
data, e.g.:

```rust
MergeNode {
    labels: Vec<SmolStr>,
    key_props: Vec<(SmolStr, Expr)>, // the {k: ...} in the MERGE pattern
    on_create: Vec<WriteOp>,
    on_match: Vec<WriteOp>,
    bind: Option<VarId>,
}
```

If keeping `props: Expr` is preferred, instead **guarantee** it is
always a literal map expression and document that the embedder may
destructure it. *(cyrs kept `props` and added a `key_props:
Vec<SmolStr>` mirror.)*

### §2.2 — `label_unique_props` / `rel_type_unique_props` on `SchemaProvider`

- **Blocks:** pgGraph M4. **Severity:** high.
- **Status:** ✅ **Resolved** — both methods added to the
  `SchemaProvider` trait (default-empty); `cyrs-sema`'s schema-aware
  pass now proves MERGE determinism upstream.
- **Related:** `docs/embedder-issues/0004` (SchemaProvider adapter).

**Problem.** `SchemaProvider` today has `labels`,
`relationship_types`, `node_properties`, `relationship_properties`,
`relationship_endpoints`, `inverse_of`, `function`, `procedure`,
`schema_digest` — but nothing exposing **uniqueness constraints**.
sema therefore cannot prove a MERGE key is backed by a declared
uniqueness; the determinism check falls to the embedder at execution
time.

**Proposed shape.** Add to the trait:

```rust
/// Ordered property tuples that are guaranteed unique for this label.
fn label_unique_props(&self, label: &str) -> Vec<Vec<SmolStr>> { Vec::new() }
fn rel_type_unique_props(&self, rel_type: &str) -> Vec<Vec<SmolStr>> { Vec::new() }
```

Default-empty so existing impls compile unchanged. sema uses them to
diagnose a MERGE whose key is not a registered uniqueness tuple.
*(Shipped as proposed.)*

### §2.3 — `labels_compatible` on `SchemaProvider`

- **Blocks:** pgGraph M3 (multi-label `CREATE`). **Severity:** high.
- **Status:** ✅ **Resolved** — added as
  `labels_compatible(&self, labels: &[SmolStr]) -> Option<bool>`. The
  `Option` (vs the proposed bare `bool`) lets a provider answer
  "no opinion" with `None`.
- **Related:** `docs/embedder-issues/0004`.

**Problem.** `CREATE (n:A:B)` requires knowing whether labels `A` and
`B` can co-exist on one node. For a relational embedder that is
whether they map to the same (or a compatible) table. No
`SchemaProvider` method asks this.

**Proposed shape.**

```rust
fn labels_compatible(&self, labels: &[SmolStr]) -> bool { labels.len() <= 1 }
```

Default rejects multi-label so existing impls keep current behaviour.
sema rejects incompatible label sets with a schema-aware diagnostic.
*(Shipped with an `Option<bool>` return — `None` = no opinion.)*

### §2.4 — Typed parameter surface

- **Blocks:** pgGraph M1. **Severity:** high.
- **Status:** ✅ **Resolved** (cy-7it) — `lower::PlanStatement`
  carries a `params` list of `ParamRef`, each with a best-effort
  `ParamType` (with an `Unknown` variant for unconstrained
  parameters).

**Problem.** Cypher `$param` references must be bound by the embedder
from its own value domain (for pgGraph: JSONB / Postgres scalar
types). pgGraph needs (a) a way to **enumerate** every parameter a
statement references and (b), ideally, a declared or inferred **type**
per parameter so the binding can be type-checked before execution.

**Proposed shape.** A `parameters()` accessor on the lowered
statement / plan returning:

```rust
pub struct ParamRef {
    pub name: SmolStr,
    pub inferred_type: Option<PlanType>, // None when unconstrained
}
```

plus a param-map input to the execution-facing API so substitution is
consistent across the read and write phases. *(Shipped as
`PlanStatement::params` with `ParamRef` + `ParamType::Unknown`.)*

---

## §3 — Diagnostics contract

### §3.1 — Reserve `E45xx` as an embedder-owned diagnostic range

- **Blocks:** pgGraph M1 (any schema-rejection diagnostic).
- **Severity:** medium.
- **Status:** ✅ **Resolved** — `E4500..=E4999` is now formally
  reserved as an embedder-owned range. A `DiagCode::ALL` test asserts
  no cyrs-defined code lands in it; cyrs deliberately defines no
  `E45xx`–`E49xx` variant of its own.
- **Related:** `docs/embedder-issues/0003` (typed diagnostic codes).

**Problem.** cyrs's `cyrs-diag` code ranges (spec §10.2) are `E0xxx`
syntax, `E1xxx` name resolution, `E2xxx` schema-free sema, `E3xxx`
schema-aware sema, `E4xxx` dialect/compat, `E5xxx` type system,
`W6xxx`/`W7xxx` lints, `N8xxx` notes. pgGraph mints its own codes for
rejections cyrs cannot diagnose because they concern pgGraph's
storage model (label not registered, MERGE key not backed by a
constraint, label-set arithmetic unsupported, etc.). It currently
uses the `E45xx` block for these.

**Ask.** Formally reserve `E45xx` (or a clearly-named host/embedder
sub-range) as embedder-owned, and guarantee `cyrs-diag`'s own
`DiagCode` enum never mints codes there. Without the guarantee a
future cyrs release could collide with pgGraph's `E4500`–`E4560`.

**Proposed shape.** A note in spec §10.2 and a `DiagCode` test
asserting the range stays empty. Pairs well with embedder-issue 0003
(typed `DiagCode` enum) — a reserved range is easier to police with
a typed enum than with `u16` discriminants. *(Shipped: the reserved
range is `E4500..=E4999`, comfortably covering pgGraph's
`E4500`–`E4560`.)*

---

## §4 — `cyrs-hir` API

### §4.1 — `lower_statement` / `lower_parse` should return `Result`

- **Blocks:** pgGraph M1. **Severity:** high.
- **Status:** ✅ **Resolved** — both `lower_statement(&str)` and
  `lower_parse(&Parse)` now return `Result<Statement, HirLowerError>`;
  the new `cyrs-hir` `error` module defines `HirLowerError` with
  `ParseFailed` and `Invariant` variants. No panic boundary needed.
- **Related:** `docs/embedder-issues/0001`, `0010`.

**Problem.** `cyrs_hir::lower::lower_statement(&str) -> Statement` and
`lower_parse(&Parse) -> Statement` both return `Statement`
unconditionally. (Good news: `lower_parse` already exists, so the
re-parse cost in embedder-issue 0001/0010 is addressed for pgGraph.)
If lowering can fail — input that parsed but cannot lower, or an
internal invariant violation — the embedder has no typed failure
channel. pgGraph would have to wrap the call in `catch_unwind`; a
panic inside a Postgres backend caught and re-raised is fragile and
yields a generic `42601` instead of a precise diagnostic.

**Proposed shape.**

```rust
pub fn lower_parse(parse: &Parse) -> Result<Statement, LowerError>;
pub fn lower_statement(src: &str) -> Result<Statement, LowerError>;
```

A best-effort partial `Statement` alongside the error is welcome but
not required — pgGraph only needs a non-panicking failure path.
*(Shipped as proposed; the error type is named `HirLowerError`.)*

### §4.2 — `HirId → byte span` accessor *(resolved — exceeded)*

- **Blocks:** pgGraph M1 (`errposition()` carets in diagnostics).
- **Severity:** low.
- **Status:** ✅ **Resolved, exceeded** — `Statement::span_of(HirId)`
  returns `Option<Range<usize>>`, i.e. a **byte range** directly,
  rather than the `TextRange` originally asked for. This folds in
  embedder-issue 0008 (the `TextRange → Range<usize>` conversion):
  pgGraph gets a Postgres-ready byte offset in one call.
- **Related:** `docs/embedder-issues/0008` (span ↔ byte-range).

**Context.** Postgres `errposition()` takes a byte offset into the
query string; pgGraph wants to render carets under offending tokens.

The capability already existed: the HIR statement carries a
`node_map: IndexMap<HirId, SyntaxNode>` and a
`syntax_for(HirId) -> Option<&SyntaxNode>` accessor, and a
`SyntaxNode` yields its `TextRange`. So
`stmt.syntax_for(id).map(|n| n.text_range())` already gave the span.

**Ask.** Add a one-line convenience and document it as the supported
path:

```rust
impl Statement {
    pub fn span(&self, id: HirId) -> Option<TextRange> {
        self.syntax_for(id).map(|n| n.text_range())
    }
}
```

Pairs with embedder-issue 0008's `TextRange → Range<usize>` helper so
the embedder gets a byte offset in one hop. *(cyrs shipped
`span_of(HirId) -> Option<Range<usize>>` — the byte range directly.)*

---

## §5 — Plan semantics pgGraph depends on

These were not API changes — they asked cyrs to treat
already-documented behaviour as a stable contract pgGraph can build
on. Both contracts are documented and held stable.

### §5.1 — `Filter` drops both `false` and `null` rows

- **Blocks:** pgGraph M1. **Severity:** low.
- **Status:** ✅ **Resolved** — `ReadOp::Filter`'s rustdoc contract
  is documented and treated as stable behaviour.

`ReadOp::Filter`'s rustdoc states rows where the predicate evaluates
to `false` *or* `null` are dropped. pgGraph relies on this to align
Cypher 3-valued logic with SQL `WHERE` (which also drops `null`) when
it pushes a predicate to SQL.

### §5.2 — `Aggregate` with empty `keys` emits one row on empty input

- **Blocks:** pgGraph M2. **Severity:** low.
- **Status:** ✅ **Resolved** — `ReadOp::Aggregate`'s empty-`keys`
  contract is documented and treated as stable behaviour.

`ReadOp::Aggregate`'s rustdoc states an empty `keys` vec aggregates
the whole input into a single row. Cypher requires that single row to
appear *even when the input is empty* (e.g. `MATCH (n) RETURN
count(n)` returns `0`, not zero rows). pgGraph's row-evaluator and
its SQL `GROUP BY` emission diverge on this, so it special-cases
empty-key aggregates.

---

## §6 — Packaging

### §6.1 — Stable release channel: crates.io or signed git tags

- **Blocks:** pgGraph *release* (not development). **Severity:**
  medium.
- **Status:** ✅ **Resolved** — cyrs 0.1.0 published 19 crates to
  crates.io (2026-05-10). pgGraph can pin a crates.io version for
  release; it keeps the `../../cyrs` path dependency for M1–M5
  co-development and flips to `0.1.0` before shipping.
- **Related:** `docs/embedder-issues/0005` (crate naming).

**Problem.** pgGraph depends on cyrs via a path dependency
(`../../cyrs`). That is fine for co-development but cannot ship: a
released pgGraph needs a reproducible, pinnable cyrs version. cyrs is
not on crates.io and has no stable release tags.

**Proposed shape.** Either:

1. publish the consumed crates (`cyrs-hir`, `cyrs-plan`,
   `cyrs-schema`, `cyrs-sema`, `cyrs-diag`) to crates.io with a
   SemVer minor, **or**
2. cut signed git tags (`v0.1.0`, …) pgGraph can pin a rev against.

Resolve embedder-issue 0005 (the `cyrs-*` package / `cypher_*` lib
naming split) first if publishing. cyrs already has `release.toml`
and `docs/release-playbook.md`, so the tooling exists. *(cyrs took
option 1 — full crates.io publication at 0.1.0.)*

---

## Summary

All 13 items resolved in cyrs 0.1.0 (PRs #56, #58).

| §    | Ask                                   | pgGraph milestone | Severity | Status                          |
|------|---------------------------------------|-------------------|----------|---------------------------------|
| 1.1  | `ShortestPath` ReadOp                 | M5                | medium   | ✅ `ReadOp::ShortestPath`        |
| 1.2  | Path-variable contract                | M2                | medium   | ✅ documented (embedder-owned)   |
| 1.3  | Builtin function enumeration          | M2                | low      | ✅ `builtin_signature()` (exceeded) |
| 2.1  | MERGE key surface on `WriteOp`        | M4                | high     | ✅ `MergeNode.key_props`         |
| 2.2  | `*_unique_props` on `SchemaProvider`  | M4                | high     | ✅ shipped on the trait          |
| 2.3  | `labels_compatible` on `SchemaProvider`| M3               | high     | ✅ `-> Option<bool>`             |
| 2.4  | Typed parameter surface               | M1                | high     | ✅ `PlanStatement::params`       |
| 3.1  | Reserve `E45xx` embedder code range   | M1                | medium   | ✅ `E4500..=E4999` reserved      |
| 4.1  | `lower_*` return `Result`             | M1                | high     | ✅ `Result<_, HirLowerError>`    |
| 4.2  | `HirId → span` convenience            | M1                | low      | ✅ `span_of() -> Range<usize>` (exceeded) |
| 5.1  | `Filter` 3VL semantics stable         | M1                | low      | ✅ documented contract           |
| 5.2  | Empty-key `Aggregate` emits one row   | M2                | low      | ✅ documented contract           |
| 6.1  | Stable release channel                | release           | medium   | ✅ crates.io 0.1.0               |

**Outcome.** cyrs cleared every pgGraph blocker in two PRs. §1.3 and
§4.2 were over-delivered (full signature metadata; a byte-range span
accessor). pgGraph's M1–M5 plan now builds straight against the
shipped 0.1.0 API with no embedder-side workarounds — see
`pgGraph/docs/contributor_guide/cypher-frontend/080-open-questions.md`.
