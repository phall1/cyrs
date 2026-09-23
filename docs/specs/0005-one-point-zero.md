# 0005 — One frontend, two dialects

**Status:** accepted by the operator, 2026-09-23.
**Amends:** spec 0001 §§1.3 (N1, N3, N4), 9.3, 12.1, 19, 20, and open question 21.1, where they conflict with this document. Spec 0001 remains the description of the pipeline. This document is the 1.0 contract.

## 1. What 1.0 is

cyrs 1.0 is the Rust frontend a database or an editor can depend on for both of these, through one pipeline:

- **Cypher as it is shipped.** openCypher v9, plus the constructs real queries use and spec 0001 §19 / §20 left out: procedure `CALL`, `CALL { }` subqueries, `EXISTS { }`, `FOREACH`, map `SET`, and the rest of the Neo4j-current surface an embedder actually receives.
- **GQL as specified.** ISO/IEC 39075:2024 is a first-class dialect, not a side corpus. Coverage counts a production when it lowers to a plan operator or a diagnostic, not when a hand-written scenario parses cleanly.

The parser, the lossless CST, the HIR, and the plan IR stay. They are the asset. 1.0 does not replace them with a new stack.

## 2. The interface

The documented entry point is one function:

```rust
cyrs::analyze(source, &AnalyzeOptions) -> Analysis
```

`Analysis` carries the diagnostics, the resolved HIR, and the plan (absent when the query is too broken to lower). `AnalyzeOptions` carries the dialect and an optional schema.

Layer crates remain available for a caller who wants only a CST or only a formatter. The README example, the integration guide, and new embedder code go through `analyze`. Salsa stays inside the incremental database used by the language server. A single query does not have to open a database to be checked and lowered.

## 3. Honesty rule

A construct the parser accepts does one of two things:

- it lowers to a plan operator whose fields mean what the construct means, or
- it produces a diagnostic, and no plan is returned.

Silent omission and placeholder operators are defects. The 1.0 blocker list starts here:

| Construct | State at the time of this spec | Required end state |
| --- | --- | --- |
| `SET n = map` / `SET n += map` | `+=` did not parse. `=` lowered to `SetLabels { labels: [] }` | `WriteOp::SetMap { replace }` |
| `CALL proc() YIELD …` | Parsed, then dropped. The HIR lowerer returned `None` for `CALL_CLAUSE` | `ReadOp::ProcedureCall` |
| `FOREACH` | Not an HIR clause | A write operator, or a diagnostic |
| `CALL { subquery }` / `EXISTS { }` | Parser accepts `EXISTS`; semantics deferred | Real subquery plans |
| Unique-key `MERGE` | Property map is kept; which keys identify the node is a side channel | The key list is part of the operator (already started for literal maps) |

## 4. Dialects

Three modes:

- `OpenCypherV9` — the frozen openCypher surface.
- `Neo4jCurrent` — Cypher as databases ship it. This amends spec 0001 §9.3, which excluded the mode from v1.
- `GqlAligned` — ISO/IEC 39075:2024.

A construct that is legal in one mode and not another is a dialect diagnostic (`E4xxx`), not a missing plan node and not a parse error shared by every mode.

## 5. Conformance

There is still no storage engine and no cost-based optimizer. Consumers execute the plan.

Spec 0001 N1 is amended only this far: a conformance evaluator over an in-memory graph may live in the test suite, so a TCK `Then` clause can fail. It is not a supported execution API and it is not a database.

Parser-acceptance percentages stay published, and they are labeled as parser acceptance. 1.0 additionally reports:

- openCypher: expected-error scenarios triaged, and the read subset the evaluator implements matched against `Then` results.
- GQL: productions covered, where covered means lowered or explicitly diagnosed. The 1.0 GQL bar is the query language (patterns, expressions, clauses). Catalog DDL and authorisation stay on the uncovered list until a later spec takes them up. A green score against a corpus we wrote is not the bar.

## 6. Sequence

1. Make the plan tell the truth for constructs the parser already accepts. Map `SET` and procedure `CALL` are the first two.
2. Land `analyze` and point the embedder docs at it.
3. Cypher lane: `FOREACH`, subqueries, and the remaining rows in `docs/plan-write-coverage.md`.
4. GQL lane: walk the uncovered-production list under the same honesty rule. Do not add a production that parses and then disappears.
5. Conformance evaluator for the read subset of both dialects.
6. Cut 1.0 when sections 3 and 5 hold. The process checklist in `docs/stability.md` (fuzz soak, semver-checks, `non_exhaustive` sweep) follows that, and does not define it.

## 7. Unchanged

The crate layering in spec 0001 §3, the ban on domain vocabulary, diagnostic-code stability, and the rule that this workspace does not ship a database.
