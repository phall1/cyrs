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

## How to read this

Each request carries:

- **Blocks** — the pgGraph milestone (M0–M5) that needs it.
- **Severity** — from pgGraph's perspective; cyrs may prioritise
  differently.
- **Workaround** — what pgGraph does until the ask lands, so nothing
  here is a hard blocker before its milestone.

Three items (§1.3, §4.2, and partly §4.1) turned out to be mostly or
fully satisfied by the current cyrs API — they are kept as
*confirmation* asks so the numbering stays stable and pgGraph can
depend on the surface not regressing.

---

## §1 — `cyrs-plan` additions

### §1.1 — `ShortestPath` read operator

- **Blocks:** pgGraph M5. **Severity:** medium (deferrable).
- **Status:** missing.

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

**Workaround.** pgGraph emits `E4530` (feature_not_supported) for any
`shortestPath` query. M0–M4 ship fully without it.

### §1.2 — Path-variable surface in the plan IR

- **Blocks:** pgGraph M2 (`RETURN p` for a matched path).
- **Severity:** medium. **Status:** needs a documented contract.

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
representation.

**Workaround.** pgGraph JSONB-shapes the path like its existing
`traverse()` `path` column. If cyrs surfaces a structured form,
pgGraph adopts it to avoid divergence.

### §1.3 — Builtin function enumeration *(largely satisfied)*

- **Blocks:** pgGraph M2 (`RETURN` with function calls).
- **Severity:** low. **Status:** **already provided** — confirmation
  ask only.

**Context.** pgGraph dispatches each Cypher function to either a SQL
fragment or a Rust evaluator, and wants a CI test that fails when
cyrs adds a function pgGraph does not handle.

`cyrs_schema::StandardLibrary` already exposes
`builtin_names() -> Vec<&'static str>` (documented "stable across
releases") and `builtin_count() -> usize`. That is exactly the
surface pgGraph needs.

**Ask.** Confirm `builtin_names()` is the SemVer-stable surface to
snapshot for a drift test, and — if cheap — expose per-function
arity / signature metadata (pgGraph would use it to pick push-to-SQL
vs row-eval). Not a blocker either way.

---

## §2 — `SchemaProvider` and write-plan surface

### §2.1 — MERGE key surface on `WriteOp::MergeNode` / `MergeRel`

- **Blocks:** pgGraph M4. **Severity:** high. **Status:** missing.
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
destructure it.

**Workaround.** pgGraph M4 does embedder-side analysis of `props`,
rejecting non-literal-map MERGE patterns. Removed when this lands.
Pairs with §2.2.

### §2.2 — `label_unique_props` / `rel_type_unique_props` on `SchemaProvider`

- **Blocks:** pgGraph M4. **Severity:** high. **Status:** missing.
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

**Workaround.** pgGraph runtime-checks against its own catalog and
raises `E4504` when the constraint is not registered. Pairs
with §2.1.

### §2.3 — `labels_compatible` on `SchemaProvider`

- **Blocks:** pgGraph M3 (multi-label `CREATE`). **Severity:** high.
- **Status:** missing.
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

**Workaround.** pgGraph rejects *every* multi-label `CREATE` with
`E4503` until this lands. Single-label `CREATE` is unaffected, so M3
ships partially.

### §2.4 — Typed parameter surface

- **Blocks:** pgGraph M1. **Severity:** high. **Status:** missing /
  needs documenting.

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
consistent across the read and write phases.

**Workaround.** pgGraph treats every parameter as untyped JSONB,
losing pg-side type checking, and raises `E4550` at runtime on a
type mismatch. Re-binds properly once a typed surface exists.

---

## §3 — Diagnostics contract

### §3.1 — Reserve `E45xx` as an embedder-owned diagnostic range

- **Blocks:** pgGraph M1 (any schema-rejection diagnostic).
- **Severity:** medium. **Status:** needs a formal guarantee.
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
a typed enum than with `u16` discriminants.

---

## §4 — `cyrs-hir` API

### §4.1 — `lower_statement` / `lower_parse` should return `Result`

- **Blocks:** pgGraph M1. **Severity:** high. **Status:** missing.
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

**Workaround.** pgGraph wraps lowering in a panic boundary and maps
any panic to SQLSTATE `42601`. Functional, poor UX.

### §4.2 — `HirId → byte span` accessor *(largely satisfied)*

- **Blocks:** pgGraph M1 (`errposition()` carets in diagnostics).
- **Severity:** low. **Status:** **derivable today** — confirmation /
  ergonomics ask.
- **Related:** `docs/embedder-issues/0008` (span ↔ byte-range).

**Context.** Postgres `errposition()` takes a byte offset into the
query string; pgGraph wants to render carets under offending tokens.

The capability already exists: the HIR statement carries a
`node_map: IndexMap<HirId, SyntaxNode>` and a
`syntax_for(HirId) -> Option<&SyntaxNode>` accessor, and a
`SyntaxNode` yields its `TextRange`. So
`stmt.syntax_for(id).map(|n| n.text_range())` already gives the span.

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
the embedder gets a byte offset in one hop.

**Workaround.** None needed — pgGraph uses `syntax_for` directly. The
ask is purely ergonomic.

---

## §5 — Plan semantics pgGraph depends on

These are not API changes — they ask cyrs to treat already-documented
behaviour as a stable contract pgGraph can build on.

### §5.1 — `Filter` drops both `false` and `null` rows

- **Blocks:** pgGraph M1. **Severity:** low. **Status:** documented;
  asking for it to stay so.

`ReadOp::Filter`'s rustdoc states rows where the predicate evaluates
to `false` *or* `null` are dropped. pgGraph relies on this to align
Cypher 3-valued logic with SQL `WHERE` (which also drops `null`) when
it pushes a predicate to SQL. **Ask:** keep this documented and
treat it as SemVer-stable behaviour.

### §5.2 — `Aggregate` with empty `keys` emits one row on empty input

- **Blocks:** pgGraph M2. **Severity:** low. **Status:** documented;
  asking for it to stay so.

`ReadOp::Aggregate`'s rustdoc states an empty `keys` vec aggregates
the whole input into a single row. Cypher requires that single row to
appear *even when the input is empty* (e.g. `MATCH (n) RETURN
count(n)` returns `0`, not zero rows). pgGraph's row-evaluator and
its SQL `GROUP BY` emission diverge on this, so it special-cases
empty-key aggregates. **Ask:** confirm this is the intended contract
and keep it documented.

---

## §6 — Packaging

### §6.1 — Stable release channel: crates.io or signed git tags

- **Blocks:** pgGraph *release* (not development). **Severity:**
  medium. **Status:** open.
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
and `docs/release-playbook.md`, so the tooling exists.

**Workaround.** pgGraph develops on the path dependency. Must flip to
a tagged rev or published version before any pgGraph release — not
before M1.

---

## Summary

| §    | Ask                                   | pgGraph milestone | Severity | Status                         |
|------|---------------------------------------|-------------------|----------|--------------------------------|
| 1.1  | `ShortestPath` ReadOp                 | M5                | medium   | missing                        |
| 1.2  | Path-variable contract                | M2                | medium   | needs documented contract      |
| 1.3  | Builtin function enumeration          | M2                | low      | **already provided** — confirm |
| 2.1  | MERGE key surface on `WriteOp`        | M4                | high     | missing                        |
| 2.2  | `*_unique_props` on `SchemaProvider`  | M4                | high     | missing                        |
| 2.3  | `labels_compatible` on `SchemaProvider`| M3               | high     | missing                        |
| 2.4  | Typed parameter surface               | M1                | high     | missing                        |
| 3.1  | Reserve `E45xx` embedder code range   | M1                | medium   | needs formal guarantee         |
| 4.1  | `lower_*` return `Result`             | M1                | high     | missing                        |
| 4.2  | `HirId → span` convenience            | M1                | low      | **derivable today** — confirm  |
| 5.1  | `Filter` 3VL semantics stable         | M1                | low      | documented — hold stable       |
| 5.2  | Empty-key `Aggregate` emits one row   | M2                | low      | documented — hold stable       |
| 6.1  | Stable release channel                | release           | medium   | open                           |

**Suggested upstream order:** §4.1 (frees M1 from the panic boundary)
→ §2.4 (typed params, M1) → §3.1 (code-range guarantee, cheap) →
§1.2 (path contract, M2) → §2.1 + §2.2 + §2.3 (write/schema surface,
land together — all touch `WriteOp` / `SchemaProvider`) → §1.1
(`ShortestPath`, larger, deferrable) → §6.1 (whenever the API
settles). §1.3, §4.2, §5.1, §5.2 need no code change beyond a
confirming note / test.
