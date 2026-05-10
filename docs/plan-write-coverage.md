# `cyrs-plan` write-side coverage matrix

**Status:** living document. Updated alongside `crates/cyrs-plan` lowering changes.
**Companion to:** `docs/specs/0001-cypher-frontend.md` §12.1 (locked).
**Tracking bead:** cy-emb7. **Source issue:** `docs/embedder-issues/0007-cyrs-plan-write-coverage.md`.

## Why this doc exists

Spec §12.1 lists the `WriteOp` variants the Plan IR exposes, but it doesn't say
which Cypher write constructs *currently lower into them* in
`cyrs-plan`. Stage 2 of the embedder migration needs that information to
decide what to keep in its hand-rolled write planner. This file is the
authoritative coverage matrix.

The reference surface is the embedder's `PlanBuilder` (see issue 0007),
which covers a representative spectrum of openCypher write clauses.

## Status legend

- **full** — clause lowers end-to-end into the listed `WriteOp`(s);
  golden tests in `crates/cyrs-plan/tests/` lock the pretty / JSON shape.
- **partial** — clause lowers, but the IR drops or normalises some
  semantic distinction the consumer must reconstruct.
- **placeholder** — clause is recognised at the HIR layer and reaches the
  lowering pass, but no faithful `WriteOp` exists; emitted IR is a
  documented no-op the consumer must pattern-match and reject or
  re-handle out-of-band.
- **not lowered** — clause is not represented in HIR / lowering at all;
  using it surfaces as a parser, AST, or HIR error rather than a Plan.

## Matrix

| # | Cypher construct | `WriteOp` variant(s) | Status | Notes |
|---|------------------|----------------------|--------|-------|
| 1  | `CREATE (n:L)` / `CREATE (n:L {p: $v})` | `CreateNode` | full | Multi-label nodes, anonymous nodes, parameterised property maps all lower. Covered by `corpus_pretty_create_node_chain` and `serde_pretty__json_write_create_node`. Lowering: `lower::lower_create_pattern` (`crates/cyrs-plan/src/lower.rs:822`). |
| 2  | `CREATE (a)-[r:T {p: $v}]->(b)` | `CreateNode` (×N for new endpoints) + `CreateRel` | full | Endpoints already bound by a preceding `MATCH` reuse their `VarId`; new endpoints emit fresh `CreateNode` ops. Pairing handled by `create_pattern_pairs`. Tests: `corpus_pretty_create_rel_chain`, `corpus_pretty_create_with_rel_props`, `corpus_json_write_create_rel_chain`. |
| 3  | `MERGE (n:L {k: $k})` | `MergeNode` | partial | Lowers with `on_create=[]`, `on_match=[]`. **No unique-key / natural-key handling** — the IR records the property map but does not flag which property is the merge key. Consumers must compute uniqueness from schema (`cyrs-schema`) themselves. Test: `corpus_pretty_merge_node_on_create_match`. |
| 4  | `MERGE … ON CREATE SET …` | `MergeNode { on_create }` / `MergeRel { on_create }` | full | `on_create` carries a `Vec<WriteOp>` of the lowered `SET` items. Tests: `corpus_pretty_merge_node_on_create_match`, `corpus_json_merge_rel_with_on_create`. |
| 5  | `MERGE … ON MATCH SET …` | `MergeNode { on_match }` / `MergeRel { on_match }` | full | Symmetric to row 4; same lowering path (`lower_merge_pattern`, `lower.rs:875`). Test: `corpus_pretty_merge_node_on_create_match` exercises both branches. |
| 6  | `MERGE (a)-[r:T]->(b)` | `MergeRel` (+ leading `MergeNode`s for unbound endpoints) | full | Tests: `corpus_pretty_merge_rel`, `corpus_json_merge_rel_with_on_create`. |
| 7  | `SET n.p = expr` | `SetProperty` | full | Test: `corpus_pretty_set_multiple_props`. |
| 8  | `SET n:L1:L2` | `SetLabels` | full | Test: `corpus_pretty_set_labels`. |
| 9  | `SET n = {…}` (whole-map replace) | *placeholder* — emits `SetLabels { labels: [] }` | placeholder | `lower::lower_set_item` for `SetItem::AssignMap` deliberately emits an empty-label `SetLabels` as a documented no-op (see `lower.rs:976`); the IR cannot today represent "replace every property of `n`". Consumers needing whole-map assignment must intercept it at the cyrs-db layer. **Embedder gap.** |
| 10 | `SET n += {…}` (map merge / `+=`) | *placeholder* — same as row 9 | placeholder | Same lowering arm; the `replace` flag on `SetItem::AssignMap` is dropped. **Embedder gap.** |
| 11 | `REMOVE n.p` | `RemoveProperty` | full | Test: `corpus_pretty_remove_prop_and_label`. |
| 12 | `REMOVE n:L` | `RemoveLabels` | full | Test: `corpus_pretty_remove_prop_and_label`. |
| 13 | `DELETE n` / `DELETE r` | `Delete { detach: false }` | full | Multi-target supported (`targets: Vec<Expr>`). Tests: `corpus_pretty_delete_multiple`, `corpus_json_full_delete_detach`. |
| 14 | `DETACH DELETE n` | `Delete { detach: true }` | full | Tests: `corpus_pretty_detach_delete_node`, `serde_pretty__pretty_delete_detach`. |
| 15 | `UNWIND $xs AS x` | `ReadOp::Unwind` (read-side) | full (read-side) | UNWIND is a *read* operator in cyrs (rows 16/17 cover its write composition). Tests: `corpus_pretty_unwind_list_literal`, `corpus_pretty_unwind_param`, `corpus_pretty_unwind_then_match`. |
| 16 | `UNWIND $xs AS x CREATE (n {p: x})` | `ReadOp::Unwind` → `WriteOp::CreateNode` | full | Driving CREATE off UNWIND falls out naturally — `Unwind` lands as a `ReadOp`, then the trailing CREATE lowers via the row-1 path. No new lowering arm needed. |
| 17 | `UNWIND $xs AS x MERGE (n {k: x})` | `ReadOp::Unwind` → `WriteOp::MergeNode` | full | Same composition as row 16, terminating in the row-3 path (with the same unique-key caveat). |
| 18 | `FOREACH (x IN $xs \| CREATE …)` | — | not lowered | `Clause::Foreach` does **not** exist in `cyrs-hir::Clause`. The parser surfaces `FOREACH` as a syntactic structure, but it never reaches the Plan layer — there is no `WriteOp` for it and no lowering arm. **Embedder gap; biggest single hole.** Workaround: rewrite as `UNWIND … CREATE/MERGE/SET` (rows 16/17), which the plan covers. |
| 19 | Unique-key MERGE for natural-keyed nodes | — | not lowered | The IR carries no notion of which property in `MergeNode { props }` is the unique key. Consumers must consult `cyrs-schema` to compute the lookup key themselves. See row 3. |
| 20 | `CALL { … }` subquery writes | — | not lowered | Spec §19/§20 explicitly defers `CALL` subqueries; `Clause::Call` is parsed but skipped during lowering (`lower.rs:536`). Out of v1 scope. |

## Tally

- 20 rows total.
- **Full coverage:** 13 rows (1, 2, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 16, 17 — i.e. CREATE node, CREATE rel, MERGE ON CREATE/MATCH on both nodes and rels, SET property, SET labels, REMOVE property, REMOVE labels, DELETE, DETACH DELETE, UNWIND read-side, UNWIND→CREATE, UNWIND→MERGE).
- **Partial:** 1 row (3 — MERGE node without unique-key flagging).
- **Placeholder (recognised but not faithful):** 2 rows (9, 10 — whole-map and `+=` assignment).
- **Not lowered:** 3 rows (18, 19, 20 — FOREACH, unique-key MERGE, CALL subquery writes).

So the headline number for the embedder migration: **~14 of 20 representative
constructs (≈70%) are losslessly covered today**, plus 1 partial; the
remaining ~25% (rows 9, 10, 18, 19, 20) are the candidates for the
embedder's hand-rolled write planner to keep owning during stage 2.

## Where each row lowers

For maintainers extending this matrix, the relevant lowering entry points
in `crates/cyrs-plan/src/lower.rs` are:

- `lower_create_pattern` — rows 1, 2, 16 trailer.
- `lower_merge_pattern` — rows 3, 4, 5, 6, 17 trailer.
- `lower_set_items` / `lower_set_item` — rows 4, 5, 7, 8, 9, 10.
- `lower_remove_items` — rows 11, 12.
- `Clause::Delete` arm in the main `lower_clauses` loop — rows 13, 14.
- `Clause::Unwind` arm — rows 15, 16, 17 (driver).
- `Clause::Call` arm — row 20 (intentional no-op).

When you change any of those, update both the matrix row(s) and the
referenced golden test name(s).
