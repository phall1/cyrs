# Bead Seed — cypher v1

> Phase-5 artifact of the Agent Flywheel: the markdown decomposition of
> spec 0001 into self-contained work units, ready to be loaded into
> `.beads/` via `scripts/seed-beads.sh`. Review and edit this file
> before running the seed script.

Every bead cites the spec section(s) it implements. Acceptance criteria
= "make those sections true". Tests live in the same bead as the code.
Deps are explicit; `bv --robot-triage` will route on the resulting DAG.

Tiers are topological layers, not priorities. A bead in a higher tier
cannot be started until every bead in the tiers below with a direct
edge into it is closed.

---

## Tier 0 — Foundation (no internal deps)

| ID    | Title                                   | Spec refs                 | Deps |
| ----- | --------------------------------------- | ------------------------- | ---- |
| B001  | Workspace hygiene + forbid(unsafe)      | §2.C4, §17.13, §17.16     | —    |
| B002  | xtask skeleton (gate/bless/codegen/release stubs) | §11             | —    |
| B003  | Pre-commit hook → `cargo xtask gate`    | §4.3                      | B002 |
| B004  | Non-coupling CI lint (denylist)         | §2.C1, §2.C2              | —    |
| B005  | CI matrix bootstrap (stable/beta/nightly × 3 OS) | §17.15           | B001 |
| B006  | Diagnostic code registry scaffolding    | §10.2                     | —    |

## Tier 1 — Syntax layer (deps mostly Tier 0)

| ID    | Title                                              | Spec refs        | Deps        |
| ----- | -------------------------------------------------- | ---------------- | ----------- |
| B010  | `SyntaxKind` enum (hand-enumerated, non-exhaustive) | §4.1, §4.4      | B001        |
| B011  | Logos lexer — all token classes                    | §4.1             | B010        |
| B012  | Parser event stream + Pratt precedence             | §4.2             | B010        |
| B013  | Rowan green/red tree builder                       | §4.4             | B010, B012  |
| B014  | Error recovery table + `recovery.md`               | §4.3             | B012        |
| B015  | `LineIndex` + UTF-8 `TextRange` helpers            | §4.5             | B001        |
| B016  | Statement boundary splitting (`;`)                 | §4.6             | B013        |
| B017  | Syntax snapshot corpus bootstrap                   | §17.2            | B013        |

## Tier 2 — AST

| ID    | Title                                    | Spec refs | Deps       |
| ----- | ---------------------------------------- | --------- | ---------- |
| B020  | `cypher.ungrammar` grammar description   | §5.2      | B010       |
| B021  | `xtask codegen` for AST wrappers         | §5.2      | B002, B020 |
| B022  | Typed AST enum wrappers for sum types    | §5.4      | B021       |
| B023  | AST snapshot corpus                      | §17.2     | B022       |

## Tier 3 — HIR

| ID    | Title                                      | Spec refs         | Deps       |
| ----- | ------------------------------------------ | ----------------- | ---------- |
| B030  | HIR node types + `HirId`                   | §6.1              | B022       |
| B031  | AST → HIR lowering pass                    | §6.1              | B030       |
| B032  | Desugaring (list comp / map proj / pat pred / shorthand) | §6.1  | B031       |
| B033  | `ScopeGraph` + `ResolvedNames` + WITH barrier | §6.2           | B031       |
| B034  | Variable kinds + kind-consistency check    | §6.3              | B033       |
| B035  | HIR snapshot corpus with resolved-name overlay | §17.2         | B032, B034 |

## Tier 4 — Schema (parallel to syntax)

| ID    | Title                                  | Spec refs | Deps |
| ----- | -------------------------------------- | --------- | ---- |
| B040  | `SchemaProvider` trait                 | §8.1      | B001 |
| B041  | Supporting types (PropertyDecl, EndpointDecl, signatures) | §8.2 | B040 |
| B042  | `StandardLibrary` catalog impl          | §8.3      | B041 |
| B043  | `schema_digest` stability + identity tests | §8.5   | B040 |

## Tier 5 — Sema

| ID    | Title                                          | Spec refs                     | Deps                    |
| ----- | ---------------------------------------------- | ----------------------------- | ----------------------- |
| B050  | `cypher-sema` skeleton + `Type` enum           | §7.2                          | B035                    |
| B051  | Unification engine + `Num` unification         | §7.2                          | B050                    |
| B052  | Schema-free pass (aggregation / ordering / params / kinds) | §7.1, §7.3, §7.4, §7.6 | B051, B006              |
| B053  | Schema-aware pass (labels/types/props/fn arity) | §7.1                         | B052, B040, B041, B042  |
| B054  | `DialectGate` plumbing + GQL / openCypher branch | §9                          | B052                    |
| B055  | Sema snapshot corpus + compiletest             | §17.2, §17.6                  | B053, B054              |

## Tier 6 — Diagnostics

| ID    | Title                                      | Spec refs | Deps       |
| ----- | ------------------------------------------ | --------- | ---------- |
| B060  | `Diagnostic` + `Severity` + `Label` + `FixIt` | §10.1  | B006       |
| B061  | Codes registered for E0001–E0999 (syntax)  | §10.2     | B060, B017 |
| B062  | Codes registered for E1000–E5999 (res / sema / dialect / type) | §10.2 | B060, B055 |
| B063  | Codes registered for W6000–N8999 (lint / perf / note) | §10.2 | B060 |
| B064  | Plain-text renderer via `codespan-reporting` | §10.3   | B060       |
| B065  | JSON renderer (for agent API)              | §10.3     | B060       |
| B066  | LSP diagnostic converter                   | §10.3     | B060       |

## Tier 7 — Plan IR

| ID    | Title                               | Spec refs | Deps       |
| ----- | ----------------------------------- | --------- | ---------- |
| B070  | `ReadOp` + `WriteOp` + `Expr` enums | §12.1     | B035       |
| B071  | HIR → Plan lowering                 | §12.1     | B070       |
| B072  | Plan serde JSON + pretty-print      | §12       | B071       |
| B073  | Plan snapshot corpus                | §17.2     | B072       |

## Tier 8 — Formatter

| ID    | Title                                        | Spec refs     | Deps       |
| ----- | -------------------------------------------- | ------------- | ---------- |
| B080  | CST-driven formatter core                    | §13.1         | B013       |
| B081  | Idempotence + semantic-preservation property tests | §13.2, §17.3 | B080  |
| B082  | Options + `cypher-fmt: off/on` magic comment | §13.3, §13.4  | B080       |
| B083  | Formatter snapshot corpus                    | §17.2         | B080       |

## Tier 9 — Incremental DB

| ID    | Title                                     | Spec refs      | Deps                         |
| ----- | ----------------------------------------- | -------------- | ---------------------------- |
| B090  | Salsa skeleton + `Database`               | §11.1          | B055, B073, B066, B083       |
| B091  | Input queries (source / dialect / digest / opts) | §11.2    | B090                         |
| B092  | Derived queries (parse/ast/hir/sema/plan/diagnostics/formatted) | §11.3 | B091 |
| B093  | Snapshot / concurrency / `FileId` model   | §11.4, §11.5   | B092                         |

## Tier 10 — Binaries

| ID    | Title                                    | Spec refs | Deps        |
| ----- | ---------------------------------------- | --------- | ----------- |
| B100  | `cypher-cli` subcommands (parse/check/fmt/plan/explain) | §16 | B093    |
| B101  | `cypher-lsp` stdio/TCP + LSP features | §14      | B093        |
| B102  | `cypher-agent` JSON protocol (11 ops)    | §15       | B093, B065  |
| B103  | `cypher-tck` harness + v1 green tags     | §17.5     | B093        |

## Tier 11 — Big-picture quality (cross-cutting)

| ID    | Title                                          | Spec refs | Deps                                    |
| ----- | ---------------------------------------------- | --------- | --------------------------------------- |
| B110  | Property suite P17.3.1–P17.3.7                 | §17.3     | B017, B023, B035, B055, B073, B083       |
| B111  | Fuzz targets (lexer/parser/fmt/sema/plan) + ASan/UBSan + 5-min PR gate + 24h nightly | §17.4 | B011, B013, B080, B053, B071 |
| B112  | Compiletest corpus + `cargo xtask bless`       | §17.6     | B002, B055                              |
| B113  | Criterion benches + 10% regression gate        | §17.10    | B017, B055, B073, B083, B100, B101      |
| B114  | `cargo-mutants` config + weekly CI + kill rates | §17.8    | B055, B035, B017, B073                  |
| B115  | Full CI matrix (stable/beta/nightly × 3 OS × feature matrix) | §17.15 | B005 |
| B116  | Miri CI (`-Zmiri-strict-provenance`)           | §17.12    | B115                                    |
| B117  | `cargo deny` allowlist + `cargo audit` PR gate | §17.16    | B001                                    |
| B118  | `cargo llvm-cov` coverage gates per crate      | §17.9     | B115                                    |
| B119  | `cypher-testkit` dev crate (fixtures, compiletest runner, mock executor) | §3.1, §17.6 | B002 |

---

## Stats

66 seed beads across 12 tiers. 15 library/binary crates × 3–5 beads
each + cross-cutting infra. Each bead is mechanical relative to its
spec section: implementation is coasting because §17 already says what
"done" looks like.

## Labels

The seed script applies these labels so `bv` can scope triage:

- `tier-0` … `tier-11`
- `layer:syntax` / `layer:ast` / `layer:hir` / `layer:sema` /
  `layer:schema` / `layer:diag` / `layer:plan` / `layer:fmt` /
  `layer:db` / `layer:bin` / `layer:infra`
- `crate:<name>` for the primary crate a bead lives in

## What this seed does **not** cover (deferred per §19 / §20)

`CALL { ... }` subqueries, `EXISTS { ... }`, `COUNT { ... }`, `LOAD CSV`,
`SHOW` / `USE`, Neo4j-current dialect, `time` / `localdatetime` / spatial
types, SARIF backend, streaming agent-API. Each becomes a separate
spec (0002…) when scheduled.

---

*Review, prune, merge, then run `scripts/seed-beads.sh` to populate
`.beads/`.*
