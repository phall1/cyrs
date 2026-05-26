# Integration depth

cyrs is a layered compiler front-end: the same input passes through six
plausible consumption surfaces (CST, AST, HIR, sema, Plan, agent JSON),
each carrying progressively more semantic information and progressively
less of the original text.

The spec ([0001 §3]) is exhaustive about *what* each layer is. This
document is normative about *which* layer an embedder should depend on.
The rule of thumb: depend on the shallowest layer that answers the
question. Every layer above the chosen one is paid for whether it is
used or not, and every layer below carries detail that cannot be
reconstructed.

[0001 §3]: ./specs/0001-cypher-frontend.md

## Decision table

| Embedder kind         | Recommended layer | Rationale                                                          |
| --------------------- | ----------------- | ------------------------------------------------------------------ |
| Graph database        | HIR + Plan        | Skip parser internals; reuse the resolved Plan IR.                 |
| IDE / LSP-host        | CST + AST         | Lossless tree for refactors, trivia preservation.                  |
| Static analysis tool  | AST + sema        | Diagnostic spans are the product.                                  |
| Query rewriter        | HIR               | Resolved names; emit fresh HIR after rewrite.                      |
| Just a parser bench   | Parse only        | Don't pay for HIR if you don't need it.                            |
| Out-of-process agent  | Agent JSON        | Cross-language, sandboxed; one stdin-line per request.             |

Use cases missing from the table or sitting between two rows resolve
to the shallower layer: adding a layer above is cheaper than peeling
one off.

## Layer reference

Six surfaces, ordered cheapest-to-richest. Each entry lists the
consumed type signature, what is preserved, what is lost, the public
type to import, the `cargo add` line, and a five-line entry-point
snippet.

### 1. Parse (`cyrs-syntax`) — the lossless tree

- **Consume:** [`cyrs_syntax::Parse`] (root) and [`SyntaxNode`] for
  walks. The tree is a `rowan` green/red graph parameterised by
  [`Lang`]; every byte of input — whitespace, comments, and fragments
  the parser could not make sense of — is present
  ([spec §4.4][s44]).
- **Preserved:** trivia, exact byte spans, recovery `ERROR` nodes,
  round-trip identity (`parse(src).syntax().to_string() == src`).
- **Lost:** nothing structural, but **no** name resolution, no types,
  no desugar. A `MATCH (a {name: $n})` is still a shorthand-property
  pattern, not a `WHERE`.
- **Public type:** `cyrs_syntax::Parse`, `cyrs_syntax::SyntaxNode`,
  `cyrs_syntax::SyntaxKind`.

```sh
cargo add cyrs-syntax
```

```rust
use cyrs_syntax::parse;

let parse = parse("MATCH (a:Person) RETURN a");
let root = parse.syntax();
let errors = parse.errors();           // Vec<SyntaxError>
assert_eq!(root.to_string(), "MATCH (a:Person) RETURN a");
```

[s44]: ./specs/0001-cypher-frontend.md#44-cst

### 2. AST (`cyrs-ast`) — typed wrappers over the CST

- **Consume:** generated wrapper structs (`Statement`, `Clause`,
  `Expression`, …) that hold a [`SyntaxNode`] and expose typed
  accessors. Wrappers are zero-cost ([spec §5.1][s51]); navigation
  re-walks the underlying rowan tree.
- **Preserved:** everything Parse preserves, plus typed grammar shape
  (`Match::pattern() -> Option<Pattern>`). Missing children are
  `Option`, so the AST keeps working over partial input
  ([spec §5.3][s53]).
- **Lost:** still no name resolution; still no types; still no
  desugar. `WITH ... AS x` introduces no scope yet at this layer.
- **Public type:** `cyrs_ast::Statement`, `cyrs_ast::Clause`,
  `cyrs_ast::Expression`, plus the rest of the generated catalogue
  in `cyrs_ast::generated`.

```sh
cargo add cyrs-syntax cyrs-ast
```

```rust
use cyrs_ast::{AstNode, SourceFile};
use cyrs_syntax::parse;

let parse = parse("MATCH (a:Person) RETURN a");
let file = SourceFile::cast(parse.syntax()).unwrap();
for stmt in file.statements() { /* typed walk */ }
```

[s51]: ./specs/0001-cypher-frontend.md#51-wrappers-not-owned-values
[s53]: ./specs/0001-cypher-frontend.md#53-missing-fields-are-option

### 3. HIR (`cyrs-hir`) — resolved, desugared, owned

- **Consume:** [`cyrs_hir::Statement`], an owned tree of `Clause`,
  `Expr`, `PatternElement`, etc. Every variable reference carries its
  defining `VarId`; sugar (list comprehensions, pattern predicates,
  shorthand property matching, map projection) has been expanded
  ([spec §6.1][s61]).
- **Preserved:** AST↔HIR map via [`HirId`] for span-accurate
  diagnostics. Scope graph and `ResolvedNames`. Variable kinds
  (`Node`, `Relationship`, `Path`, `Value`, [spec §6.3][s63]).
- **Lost:** trivia (HIR is owned, not a tree of `SyntaxNode`s).
  Spans survive only via the `HirId → SyntaxNode` map. Source
  formatting is unreachable from HIR alone; the AST and CST hold it.
- **Public type:** `cyrs_hir::Statement`, `cyrs_hir::Clause`,
  `cyrs_hir::Expr`, `cyrs_hir::VarId`, `cyrs_hir::HirId`.

```sh
cargo add cyrs-hir
```

```rust
use cyrs_hir::lower::lower_statement;

let stmt = lower_statement("MATCH (a:Person {name: $n}) RETURN a");
// Pattern shorthand has been desugared to MATCH + WHERE;
// every Expr::VarRef carries a resolved VarId.
for clause in &stmt.clauses { /* walk owned HIR */ }
```

[s61]: ./specs/0001-cypher-frontend.md#61-hir-shape
[s63]: ./specs/0001-cypher-frontend.md#63-variable-kinds

### 4. Sema (`cyrs-sema`) — type system on top of HIR

- **Consume:** the diagnostic stream + `Type` annotations produced by
  running sema over a HIR statement. Two modes share a single pipeline
  (schema-free always; schema-aware when a `SchemaProvider` is
  supplied, [spec §7.1][s71]).
- **Preserved:** every diagnostic carries a stable code (`E0001…`,
  `W6000…`, `N8000…` per [spec §10.2][s102]) and a span anchored back
  to the CST through HIR.
- **Lost:** sema does not retain the HIR; it consumes one and emits
  diagnostics + a type map. Re-typing after a rewrite requires
  re-running sema against the new HIR.
- **Public type:** `cyrs_sema::ty::Type`, `cyrs_sema::DialectMode`,
  the analyses in `cyrs_sema` (currently coupled with `cyrs-diag`).

```sh
cargo add cyrs-hir cyrs-sema cyrs-schema cyrs-diag
```

```rust
use cyrs_hir::lower::lower_statement;
// A `SchemaProvider` impl feeds the sema analyses in `cyrs-sema`.
// The public entry point depends on the in-progress sema surface
// (see crate docs).
let stmt = lower_statement("MATCH (a) RETURN a.unknown");
// pass `stmt` and the schema to the sema analyses…
```

[s71]: ./specs/0001-cypher-frontend.md#71-two-modes-one-pipeline
[s102]: ./specs/0001-cypher-frontend.md#102-code-scheme

### 5. Plan (`cyrs-plan`) — logical operator graph

- **Consume:** [`cyrs_plan::lower::PlanStatement`], a directed
  acyclic graph of [`ReadOp`] / [`WriteOp`] nodes with typed columns.
  Logical only: no cost model, no cardinality, no physical operator
  selection ([spec §12.1][s121]). Variable identities are
  plan-scoped (`VarId`), not HIR-scoped, so a plan outlives the HIR
  it was lowered from ([spec §12.3][s123]).
- **Preserved:** fully resolved expression IR, parameter discipline,
  read/write split.
- **Lost:** spans (a `VarMap` carries the back-reference to source),
  trivia, any source formatting. Sema diagnostics are *upstream* of
  plan; input that fails sema must not be lowered.
- **Public type:** `cyrs_plan::ReadOp`, `cyrs_plan::WriteOp`,
  `cyrs_plan::Expr`, `cyrs_plan::OpId`, `cyrs_plan::VarId`,
  `cyrs_plan::lower::PlanStatement`.

```sh
cargo add cyrs-hir cyrs-plan
```

```rust
use cyrs_hir::lower::lower_statement as hir_lower;
use cyrs_plan::lower::lower_statement as plan_lower;

let hir = hir_lower("MATCH (a:Person) RETURN a");
let plan = plan_lower(&hir).expect("HIR was sema-clean");
// `plan.read` is a ReadOp tree your executor consumes.
```

[s121]: ./specs/0001-cypher-frontend.md#121-shape
[s123]: ./specs/0001-cypher-frontend.md#123-ownership-of-identifiers

### 6. Agent JSON (`cyrs-agent`) — out-of-process protocol

- **Consume:** stdin/stdout JSON Lines, one request per line. Ten
  ops: `parse`, `check`, `complete`, `hover`, `format`, `rewrite`,
  `plan`, `explain`, `schema_set`, `schema_clear`, `shutdown`
  ([spec §15.2][s152]). Sandbox-safe: the binary makes no network
  calls, spawns no subprocesses, performs no filesystem writes.
- **Preserved:** wire-stable op names + required-field semantics
  (see [`docs/stability.md`][stab]); diagnostic codes match the
  in-process Rust API.
- **Lost:** structural typing — the wire format is JSON, not Rust
  types. Cross-language overhead per request. No interactive `&Parse`
  borrowing; text is re-sent on every call. The LSP server is the
  stateful equivalent where per-call cost matters.
- **Public type:** the binary itself; speak JSON Lines on its
  stdin/stdout.

```sh
cargo install cyrs-cli         # ships the `cyrs-agent` binary too
cyrs-agent < requests.jsonl  # one JSON request per line
```

```jsonc
{"v": 1, "op": "check", "text": "MATCH (a) RETURN a.x"}
{"v": 1, "op": "plan",  "text": "MATCH (a:Person) RETURN a"}
{"v": 1, "op": "shutdown"}
```

[s152]: ./specs/0001-cypher-frontend.md#152-requests
[stab]: ./stability.md

## Stability promise per layer

The contract below is the per-layer mapping of [`docs/stability.md`][stab]
into the layer vocabulary used here. "Stable today" means the surface
will not break in minor versions once 1.0 lands and is already
treated as load-bearing pre-1.0. "`#[non_exhaustive]` planned" means
the cy-2i9 follow-up will attribute the surface so adding variants /
fields is non-breaking. "Still moving" means the shape itself is
expected to change before 1.0.

| Layer       | Stable today                                                                                  | `#[non_exhaustive]` planned (cy-2i9)                                          | Still moving                                       |
| ----------- | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | -------------------------------------------------- |
| Parse / CST | `parse()` entry point; round-trip identity; `SyntaxKind` *(already non-exhaustive)*           | —                                                                              | Internal node hierarchy as grammar grows           |
| AST         | Typed wrapper pattern (zero-cost, `Option`-valued accessors); regen via `cargo xtask codegen` | —                                                                              | Generated catalogue grows with grammar             |
| HIR         | Lowering invariants (desugar list, sugar set per [spec §6.1][s61]); `HirId` map               | `Statement`, `Binding`, `VarKind`, `Clause`, `PatternElement`, `Expr`, …       | Variant set still expanding (each clause feature)  |
| Sema        | Diagnostic codes (`E…/W…/N…`) and message stability per [spec §10.2][s102]                    | `Type` lattice                                                                 | Type lattice variants, parameter inference rules   |
| Plan        | Logical-only contract (no cost / cardinality, [spec §12.5][s125])                             | `ReadOp`, `WriteOp`, `Expr`, `BinOp`, `UnaryOp`, `Direction`, `RelLength`, …   | Operator coverage as write-side lands              |
| Agent JSON  | Op names + required-field semantics ([spec §15.2][s152])                                      | n/a (wire protocol; new optional fields are non-breaking)                      | Optional response fields, streaming envelope shape |

[s125]: ./specs/0001-cypher-frontend.md#125-consumer-contract

## Where to go next

- Crate graph and allowed import edges: normative in
  [`docs/specs/0001-cypher-frontend.md`][0001 §3] §3.
- Per-surface stability (diagnostic codes, agent wire protocol,
  schema file format, HIR / Plan IR shape, 1.0 cutover plan):
  [`docs/stability.md`][stab].
- Context for agents working *on* cyrs (not on top of it): `AGENTS.md`,
  §3 onward.

[//]: # (Bead: cy-emb9 — document the AST-vs-HIR consumption contract.)
