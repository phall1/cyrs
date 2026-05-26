# Stability policy

**Status:** pre-1.0 (workspace ships at `0.0.x`). Every surface in this
workspace can still change. This document is the explicit contract for
what is intended to remain stable through 1.0 — a baseline downstream
consumers can depend on rather than reverse-engineer from the changelog.

cyrs is a compiler front-end; the SemVer contract that matters to
consumers is the public Rust API of the fifteen `cypher-*` crates,
plus two on-the-wire protocols (agent JSON + LSP) and the schema file
format. Each surface below is tagged **stable**, **unstable**, or
**internal**.

## Stable surfaces

These surfaces will not change in breaking ways across minor version
bumps once 1.0 lands. Pre-1.0 churn is avoided here and tracked in
`CHANGELOG.md`:

- **Diagnostic codes and their messages.**  The registry in
  `crates/cyrs-diag/src/codes.rs` (AGENTS.md §7, spec §10.2) is
  append-only.  Codes never change meaning; retired codes are never
  reused.  Message wording may be polished, but the code → message
  mapping must stay recognisable to existing consumers.  CI's
  `check-diag-codes` xtask enforces registry integrity.
- **Agent JSON wire protocol.**  The `op` names and required-field
  semantics documented in `crates/cyrs-agent/src/main.rs` (spec §15.2)
  are stable: `parse`, `check`, `complete`, `hover`, `format`,
  `rewrite`, `plan`, `explain`, `schema_set`, `schema_clear`,
  `shutdown`.  New optional fields on requests or responses are
  non-breaking; removing or renaming is a major version bump.
  Every request and response carries a `proto_version: u32` field
  (currently `1`); requests omitting the field are accepted as
  `proto_version: 1` for backward compatibility, and the constant
  bumps on any breaking wire change (cy-2i9).
- **Dialect-mode enum names.**  `DialectMode::GqlAligned` and
  `DialectMode::OpenCypherV9` are wire-observable values (JSON tags,
  LSP `initializationOptions`, schema defaults).  The enum is marked
  `#[non_exhaustive]` (cy-2i9.1) so new dialects can land without a
  SemVer-major break; existing variant names are permanent.
- **Schema file format** per spec 0002 (§8).  Schemas are versioned
  via the `cyrs_schema_version` field; the shapes of `properties`,
  `labels`, `relationships`, `functions`, and `procedures` entries are
  stable within a pinned `cyrs_schema_version`.  A new major schema
  version is a coordinated roll-out, not an in-place break.
- **MSRV policy.**  The workspace pins MSRV in `rust-toolchain.toml`
  (currently `1.94`).  MSRV bumps are *Cargo-minor* events — every
  release that bumps MSRV gets a minor version bump (pre-1.0: a patch
  bump); MSRV bumps do not land in patch releases.  Spec §18.

## Unstable (may change across minor versions pre-1.0)

These surfaces are public Rust types that the frontend pipeline *will*
extend during development.  Many have `#[non_exhaustive]` attributes
pinned on them to soften the blow — adding a new enum variant or
struct field is a non-breaking change, but semantic shape changes are
not.

- **HIR shape** — `cyrs_hir::{Expr, Clause, Statement, PatternElement,
  SetItem, RemoveItem, BinOp, UnaryOp, VarKind, MapProjectionItem}`
  internals.  Lowering shape evolves with each new Cypher construct
  brought in.  See "Deferred: intra-workspace `non_exhaustive`" below
  for why these are not yet attributed.
- **Plan IR shape** — `cyrs_plan::{ReadOp, WriteOp, Expr, OpId, BinOp,
  UnaryOp, Direction, RelLength, UnionKind, SortDir}`.  Attributed
  `#[non_exhaustive]` (cy-2i9.1) so new operators / expressions don't
  force SemVer-major releases, but the variant *set* is still growing.
- **Type lattice** — `cyrs_sema::ty::Type`.  The 14-ish variant type
  lattice will grow with each semantic improvement; see `Deferred`.
- **LSP extensions beyond baseline LSP spec.**  Cyrs's LSP server
  implements the standard LSP protocol; any non-standard `workspace/
  executeCommand` requests (e.g. `explainPlan`, `lowerToHir`) are
  unstable, can rename, and are version-gated by the
  `workspace.cyrs.experimental` initialization option.
- **Compiletest UI fixture format.**  `crates/*/tests/ui/**` files and
  the `cargo xtask bless` transform are evolved with the fixtures
  themselves; downstream consumers should not depend on the exact
  rendering.

## Internal (no guarantees)

- **Salsa tracked query signatures.**  The `cyrs-db` query functions
  (`parse_cst`, `ast`, `resolved_names`, `plan`, `all_diagnostics`,
  etc.) are implementation details of incrementality.  Their memoised
  return shapes can change whenever the schema / analysis layer does;
  downstream code should query the `Database` trait, not the Salsa
  internals.
- **Codegen'd AST.**  `crates/cyrs-ast/src/generated.rs` is produced
  by `cargo xtask codegen` from `cyrs-syntax/src/grammar/
  cypher.ungrammar`.  The *grammar* is a spec-governed artifact; the
  *generated Rust* is an implementation detail.  `SyntaxKind` is
  additionally marked `#[non_exhaustive]` so grammar growth is
  automatically non-breaking.
- **Private modules.**  Anything not re-exported from a crate's
  `lib.rs` top-level (or inside a `#[doc(hidden)]` module) carries no
  stability promise.

## `non_exhaustive` coverage

A downstream-consumer canary at `tests/canary/` (crate `cyrs-canary`,
bead cy-e3h) exhaustively matches every attributed enum below with a
trailing wildcard arm under `#![deny(unreachable_patterns)]`. The
canary builds today; it will refuse to compile if any of these enums
loses its `#[non_exhaustive]` attribute (the wildcard would become
unreachable). It also keeps building when new variants are added — the
wildcard absorbs them — which is the consumer-facing contract this
section documents.

cy-2i9.1 applied `#[non_exhaustive]` to the following public surface
to soften future variant / field additions:

**Enums**

- `cyrs_hir::{Direction, RelLength}`
- `cyrs_plan::{Direction, RelLength, UnionKind, SortDir, ReadOp,
  WriteOp, Expr, BinOp, UnaryOp}`
- `cyrs_sema::DialectMode`
- `cyrs_schema::{Cardinality, ProcMode}`
- `cyrs_db::DialectMode`
- `cyrs_diag::{Severity, Applicability}`
- `cyrs_lang_services::CompletionItemKind`
- `cyrs_syntax::SyntaxKind` *(pre-existing; cy-2i9.1 preserves it)*
- `cyrs_fmt::FormatError` *(pre-existing)*

**Structs**

- `cyrs_diag::Diagnostic`
- `cyrs_schema::PropertyDecl`  *(constructor: `PropertyDecl::new`)*
- `cyrs_lang_services::{CompletionItem, Hover, RewriteEdit,
  RewritePayload}`
- `cyrs_db::UnknownFileId`

### Deferred: intra-workspace `non_exhaustive`

The following enums / structs are **listed in the bead but not yet
attributed** because they are matched exhaustively across the workspace
(cross-crate, from `cyrs-sema` / `cyrs-plan` / `cyrs-db` /
`cyrs-lang-services` / `cyrs-lsp`).  Adding `#[non_exhaustive]`
would force a wildcard arm at every cross-crate match site — 28+
sites in `cyrs-sema` alone for `HirExpr` / `Clause` / `SetItem` etc.
The attribute lands in a follow-up bead that performs the mechanical
match-arm churn in one focused commit, rather than piggybacking on
the SemVer gating work:

- `cyrs_hir::{VarKind, Clause, PatternElement, Expr, SetItem,
  RemoveItem, MapProjectionItem, BinOp, UnaryOp}`
- `cyrs_sema::ty::Type`
- `cyrs_schema::PropertyType`
- `cyrs_hir::{Statement, Binding}` (structs — many cross-crate
  constructions)
- `cyrs_plan::{AggExpr, OrderKey}` (structs — integration-test
  fixtures construct via literals)
- `cyrs_schema::{EndpointDecl, FunctionSignature, ProcedureSignature,
  ParamDecl, YieldDecl, FnCategories}`

None of the deferred types are stable today; all are documented as
**unstable** above.  Downstream consumers that match on them must
already accept the churn from variant additions landing each bead; the
follow-up bead only changes the *compile-time signal* from "non-
exhaustive" errors to silent "wildcard ignored" behaviour, which is
the SemVer-safer default for 1.0.

## Known acknowledged breakers

At time of writing `cargo semver-checks` identifies no pre-existing
breaking changes on the published (0.0.x) surfaces — nothing has been
released to crates.io, so there is no prior baseline to diff against.
The CI gate below is effectively advisory until the first crates.io
release; after that, each PR's diff is checked against the baseline
branch (`main`) rather than against a published version.

A future bead requiring an un-attributable breaking change is logged
here with:

1. Date + bead id
2. The affected public type / function
3. The consumer-visible impact ("rename of field X to Y on struct Z")
4. Migration note (constructor to use, wildcard arm to add, etc.)

## CI gate

`.github/workflows/ci.yml` defines a `semver-checks` job
(cy-2i9.1) that runs
[`cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks).
It compares the PR's public API against an `obi1kenobi/cargo-semver-
checks-action@v2`-resolved baseline.

**Status: disabled (`if: false`) as of fix/ci-infra-rescue.**  The v2
action resolves the baseline by querying crates.io for each named
package; because none of the sixteen cyrs crates have shipped to the
registry yet, the action errors on every PR with `cyrs-ast not found
in registry (crates.io)`.  The job is left in the workflow so that
re-enabling it is a one-line change once any of the following holds:

1. **First crates.io publish ships** — any of the 16 crates at
   `>= 0.0.1` with a real registry baseline.
2. **Baseline-rev migration** — the action grows (or is switched to)
   an input that resolves the baseline from a git revision rather
   than the registry, verified green against `main`.

Either path swaps `if: false` back to
`if: github.event_name == 'pull_request'`.  Until then the SemVer
contract is enforced by code review against the `## Stable surfaces`
list above and the deferred-types note below.

## 1.0 cutover plan

Cyrs ships 1.0 when:

1. **Workspace semantics are locked** (spec 0003, TBD).  This mostly
   means: HIR lowering rules are frozen, type lattice shape is
   frozen, Plan IR operator set covers the spec §12.1 workload.
2. **The `non_exhaustive` pass is complete**, including the deferred
   intra-workspace types above.  No `non_exhaustive`-driven churn is
   pending.
3. **A 24-hour fuzz soak is clean** on all five fuzz targets
   (`fuzz_lexer`, `fuzz_parser`, `fuzz_formatter`, `fuzz_sema`,
   `fuzz_plan`) — spec §17.4.
4. **`cargo-semver-checks` PR gate is hard-blocking** (not
   advisory), having run for at least one release cycle with the
   workspace at `0.x`.

At that point the workspace receives a coordinated `0.99` → `1.0.0`
bump across all sixteen crates and the CHANGELOG gains a formal
"1.0 promise" section.

## Performance budgets

Cyrs enforces per-benchmark wall-clock budgets on top of the standard
10% time-regression gate (spec §17.10).  The PR-blocking `bench` workflow
runs the fast benches (parse, sema, plan, fmt, incremental, lsp-
completion) against the main-branch baseline; the nightly
`bench-nightly` workflow runs the heavy 10k-line large-file bench
against absolute p95 budgets committed to
`benches/large_file.budget.toml`.

### bench_large_file (cy-y6a.1)

The large-file gate enforces that end-to-end **parse**, **HIR-lower**,
and **full diagnostic** pipelines can process a 10,000-line synthetic
Cypher source within bounded wall-clock time.  A 10% regression on the
fast benches is noise-tolerant; a budget breach on the large-file bench
is a *policy* failure and means one of three things:

1. A legitimate algorithmic regression — fix before merge.
2. A workload-shape change (new clause templates in the generator,
   growing line count) — the operator re-baselines.
3. CI-runner noise large enough to exceed ×1.2 headroom — treat as
   flaky and investigate the runner, not the code.

Current budgets (milliseconds, p95):

| Bench                | budget |
|----------------------|--------|
| `parse_10k`          | 35 ms  |
| `hir_lower_10k`      | 70 ms  |
| `diagnose_10k`       | 65 ms  |

**On regression, page the operator.**  The nightly workflow does not
auto-open a bug; the operator reviews the run and decides between
"revert the regressing commit" and "bump the budget with
justification".

### Re-baselining after an intentional perf change

If a commit legitimately changes the expected cost of the large-file
pipeline (e.g. lowering no longer short-circuits a common pattern), the
operator re-baselines:

1. Run `cargo bench --bench large_file` from `benches/` on a quiet
   machine.  Note the three reported p95s.
2. Update `benches/large_file.budget.toml` — each field set to measured
   p95 × 1.2, rounded up.
3. Update the rustdoc table atop `benches/benches/large_file.rs` with
   the same measured numbers.
4. Update the budget table above in this document.
5. Commit all four changes together with a `cy-…: bench — re-baseline
   large_file budgets` message that names the perf change and links the
   bead.

The budget file is the authority at runtime — the rustdoc and this
document are human-facing mirrors.  CI will re-run the nightly bench
against the committed file on the next cron tick.

## See also

- `AGENTS.md` §7 — diagnostic-code registry discipline
- `docs/specs/0001-cypher-frontend.md` — spec (locked)
- `CHANGELOG.md` — per-release change log
