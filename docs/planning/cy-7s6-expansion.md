# cy-7s6 Expansion — Language Coverage Child Beads

*Parent: cy-7s6 (openCypher TCK + GQL ISO 39075 long tail).*
*Sibling done: cy-3xz (40/40 v1 TCK), cy-5gh, cy-8x5, cy-zo9, cy-7s6.1.*

Spec 0001 is locked (§6). Anything the spec does not permit becomes
spec-0003 follow-up (§20), NOT a child of cy-7s6. Spec §9.3 and §19
forbid: `CALL { }`, `EXISTS { }`, `COUNT { }`, `LOAD CSV`, `SHOW`,
`CYPHER` prefix, Neo4jCurrent. Those must be filed as spec-amendment
requests, not implemented.

## 1. Child Bead Catalog

### In-spec beads (actionable under 0001)

---

**cy-7s6.2 — stdlib gap: `reduce(acc = init, x IN xs | expr)`**
- Scope:
  - Parser: add `reduce` as reserved contextual ident; grammar
    `REDUCE_EXPR` with accumulator binding
  - HIR lower: `Expr::Reduce { acc, init, var, list, body }`
  - Sema: scope binding for `acc` + `x`; type = `init ∪ body`
- Labels: `crate:cypher-syntax`, `crate:cypher-ast`, `crate:cypher-hir`, `crate:cypher-sema`
- Deps: none (parallel with other sema-only beads after syntax lands)
- Size: M
- Spec §: §6.1 (HIR sugar desugaring), §7.4, §19 (list comprehensions row)
- Priority: 2
- Accept: Implements §6.1 list-comprehension desugaring extended with `REDUCE`; TCK `@LISTS/reduce` scenarios green

---

**cy-7s6.3 — stdlib gap: `keys(map)` + `values(map)` on map typed args**
- Scope:
  - `StandardLibrary` already has `keys()` for nodes/rels — extend to `Map`
  - Add `values(map)` returning `LIST<ANY>`
  - Sema `infer` map-kind check; E3xxx for non-map arg
- Labels: `crate:cypher-schema`, `crate:cypher-sema`
- Deps: none
- Size: S
- Spec §: §8.3 (standard-library), §7.2 (Map type)
- Priority: 1
- Accept: Implements §8.3 map accessors; unit + ui tests for unknown-arg kind

---

**cy-7s6.4 — null-safe property chain + `CASE WHEN n.x IS NULL`**
- Scope:
  - Confirm infer handles propagation of `null` through `.prop` chains
    on `OPTIONAL MATCH` bindings (Type::union with Null)
  - Compiletest UI: `CASE WHEN n.x IS NULL THEN 'missing' ELSE n.x`
  - Snapshot: null-flow diagnostics stable
- Labels: `crate:cypher-sema`
- Deps: cy-7s6.7 (CASE expr wiring)
- Size: S
- Spec §: §7.2 (nullability), §7.4
- Priority: 2
- Accept: Implements §7.2 null-propagation on OPTIONAL-bound vars

---

**cy-7s6.5 — stdlib: numeric `rand()`**
- Scope:
  - Already in `StandardLibrary` — audit; confirm `FnCategories { pure: false, deterministic: false }`
  - Add `W7xxx` warning when used in `ORDER BY` / equality (non-deterministic red flag per diag §10.2 W7 range)
  - Sema: flag `rand` ≈ pure-rewrite barrier
- Labels: `crate:cypher-schema`, `crate:cypher-sema`, `crate:cypher-diag`
- Deps: none
- Size: S
- Spec §: §8.3, §10.2 (W7000 range)
- Priority: 3
- Accept: New registered diagnostic code in W7xxx range for `rand()` in `ORDER BY`

---

**cy-7s6.6 — Pattern comprehensions `[(a)-[r]->(b) | r.weight]`**
- Scope:
  - Lexer/parser: disambiguate `[ (` as start-of-pattern-comprehension in expr position
  - Grammar `PATTERN_COMPREHENSION` already enumerated in `kind.rs:227` — wire parser
  - HIR lower: desugar to existential pattern scope + projection expression (§6.1)
  - Sema: scope the pattern-local variables `a, r, b` in a nested scope; result type `LIST<T>` of projection
- Labels: `crate:cypher-syntax`, `crate:cypher-ast`, `crate:cypher-hir`, `crate:cypher-sema`
- Deps: none (SyntaxKind exists)
- Size: L
- Spec §: §6.1 (desugaring), §7.5 (pattern-level validation)
- Priority: 2
- Accept: Implements §6.1 pattern-comprehension desugar; TCK `@PATTERNS` comprehensions green

---

**cy-7s6.7 — `CASE` expression — generic + simple-when**
- Scope:
  - Parser: wire `CASE_EXPR`, `CASE_WHEN_ARM`, `CASE_ELSE_ARM` (tokens already in `kind.rs`)
  - AST codegen: regenerate
  - HIR: `Expr::Case` already exists in infer.rs — add lowering
  - Sema: union of arm types already implemented (`infer.rs:302`) — verify simple-when scrutinee equality
- Labels: `crate:cypher-syntax`, `crate:cypher-ast`, `crate:cypher-hir`, `crate:cypher-sema`, `crate:cypher-plan`
- Deps: none
- Size: M
- Spec §: §6.1, §7.2 (union type), §19 (CASE row)
- Priority: 1 — unblocks many other scenarios
- Accept: Implements §19 CASE row; TCK `@EXPRESSIONS/case` scenarios green

---

**cy-7s6.8 — Pattern predicates in WHERE: `EXISTS((a)-->(b))`**
- Scope:
  - Parser: allow pattern as expression only inside EXISTS(...) call form (NOT `EXISTS { }` — that's deferred per §19)
  - HIR: `ExprKind::PatternPredicate` (already contemplated in §6.1 desugaring)
  - Sema: patterns use existential semantics; all-vars must resolve
- Labels: `crate:cypher-syntax`, `crate:cypher-hir`, `crate:cypher-sema`
- Deps: none
- Size: M
- Spec §: §6.1 (pattern predicate desugar), §19 (row "Pattern predicates in expressions: ✅")
- Priority: 1
- Accept: Implements §6.1 pattern-predicate desugar

---

**cy-7s6.9 — Full openCypher TCK vendor-in + harness upgrade**
- Scope:
  - Vendor complete openCypher TCK (not just v1 subset) into `crates/cypher-tck/tck/full/`
  - Retire "v1 label" in favor of per-scenario `expected: supported|error|ignored` per epic acceptance
  - Harness emits rolling pass-rate snapshot into `tck/full-baseline.md`
- Labels: `crate:cypher-tck`
- Deps: none (isolated harness work)
- Size: M
- Spec §: §17.5 (TCK conformance)
- Priority: 1 — gates measurement for everything after
- Accept: Implements §17.5; `cargo test -p cypher-tck` reports full-TCK pass count; v1 harness still green

---

**cy-7s6.10 — Type predicates: `n IS :: STRING | INTEGER | ...`**
- Scope:
  - Lexer/parser: `IS ::` operator
  - HIR: `Expr::TypePredicate { operand, ty: Type }`
  - Sema: result `Bool`; no constraint on operand type (like `IS NULL`)
- Labels: `crate:cypher-syntax`, `crate:cypher-ast`, `crate:cypher-hir`, `crate:cypher-sema`
- Deps: none
- Size: S
- Spec §: §7.2 (type system) — marked `new-spec` if §19 doesn't cover; candidate for spec 0001 §19 row addition rather than 0003 (GQL-aligned)
- Priority: 2
- Accept: Implements §7.2 TypePredicate; UI tests for each predicate type

---

**cy-7s6.11 — Temporal: `time`, `localdatetime`, `duration` + arithmetic**
- Scope:
  - `cypher-sema::Type` add `Time`, `LocalDatetime`, `Duration`
  - `cypher-schema::PropertyType` add the same
  - `StandardLibrary` functions: `time()`, `localdatetime()`, `duration()`, duration arithmetic `date + duration`
  - NOTE: §19 marks these `❌ Deferred` → this child bead **requires a spec 0001 amendment or spec 0003**; file the amendment request first, then implement
- Labels: `crate:cypher-schema`, `crate:cypher-sema`, `crate:cypher-hir`
- Deps: spec-amendment (blocking); cy-0ek PropertyType extension
- Size: L
- Spec §: §19 Deferred row (`new-spec` — propose spec 0003)
- Priority: 3
- Accept: Implements spec-0003 §X (TBD); propose as part of 0003 scoping

---

**cy-7s6.12 — Spatial: `point()` + `distance()`**
- Scope: `Type::Point`, `point({x, y})`, `distance(p1, p2)`; same spec-amendment caveat as cy-7s6.11
- Labels: `crate:cypher-schema`, `crate:cypher-sema`
- Deps: spec-amendment (blocking)
- Size: M
- Spec §: §19 `❌ Deferred` row (`new-spec` — candidate for spec 0003)
- Priority: 3
- Accept: Implements spec-0003 §X (TBD)

---

**cy-7s6.13 — GQL quantified path patterns `(a)-->{1,5}(b)`**
- Scope:
  - Parser: GQL quantified edge syntax — dialect-gated to `GqlAligned`
  - Lower to existing variable-length-path HIR nodes
  - Dialect gate diagnostic in E4000–E4999 range (§9, §10.2)
- Labels: `crate:cypher-syntax`, `crate:cypher-sema`, `crate:cypher-diag`
- Deps: none
- Size: M
- Spec §: §9 (DialectGate) — `new-spec` for GQL quant-path syntax, candidate spec 0003
- Priority: 2
- Accept: Implements spec 0003 §X on GQL quantified paths (pending)

---

### Out-of-scope under spec 0001 (SHOULD NOT become cy-7s6 children)

| Epic item                          | Reason                                | Recommended action                          |
| ---------------------------------- | ------------------------------------- | ------------------------------------------- |
| `CALL { <subquery> }`              | §19 `❌ Deferred`, §9.3 Neo4jCurrent  | Drop from cy-7s6; open spec-0003 proposal   |
| `EXISTS { <subquery> }`            | §19 `❌ Deferred`                     | Drop; spec-0003                             |
| `COUNT { <subquery> }` expr        | §19 `❌ Deferred`                     | Drop; spec-0003                             |
| `SHOW` commands                    | §9.3 Neo4jCurrent, §19 `❌`           | **Drop entirely**                           |
| `CYPHER` prefix directives         | §9.3 Neo4jCurrent, §19 `❌`           | **Drop entirely**                           |
| `IMPORTING WITH`                   | tied to `CALL { }` — Neo4jCurrent     | **Drop entirely**                           |
| `APOC`                             | §9.3 bans                             | **Drop entirely**                           |
| `LOAD CSV`                         | §19 `❌`                              | **Drop entirely**                           |
| GQL `REPEATABLE` path mode         | not in 0001                           | Defer to spec 0003 (GQL extensions)         |
| GQL `FILTER` clause                | not in 0001                           | Defer to spec 0003                          |
| GQL `EXCLUDE` projection           | not in 0001                           | Defer to spec 0003                          |
| GQL `REPEATABLE ELEMENTS`          | not in 0001                           | Defer to spec 0003                          |
| GQL graph schema catalog objects   | overlaps cy-0ek schema.toml           | Defer; coordinate with cy-0ek               |

---

## 2. Parallelisation Matrix

Crate labels by bead. Two beads are parallel-safe if label sets are
disjoint per AGENTS.md §4.2.

| Bead       | syntax | ast | hir | sema | schema | plan | diag | tck |
| ---------- | :----: | :-: | :-: | :--: | :----: | :--: | :--: | :-: |
| cy-7s6.2   |   ✓    |  ✓  |  ✓  |  ✓   |        |      |      |     |
| cy-7s6.3   |        |     |     |  ✓   |   ✓    |      |      |     |
| cy-7s6.4   |        |     |     |  ✓   |        |      |      |     |
| cy-7s6.5   |        |     |     |  ✓   |   ✓    |      |  ✓   |     |
| cy-7s6.6   |   ✓    |  ✓  |  ✓  |  ✓   |        |      |      |     |
| cy-7s6.7   |   ✓    |  ✓  |  ✓  |  ✓   |        |  ✓   |      |     |
| cy-7s6.8   |   ✓    |     |  ✓  |  ✓   |        |      |      |     |
| cy-7s6.9   |        |     |     |      |        |      |      |  ✓  |
| cy-7s6.10  |   ✓    |  ✓  |  ✓  |  ✓   |        |      |      |     |
| cy-7s6.11  |        |     |  ✓  |  ✓   |   ✓    |      |      |     |
| cy-7s6.12  |        |     |     |  ✓   |   ✓    |      |      |     |
| cy-7s6.13  |   ✓    |     |     |  ✓   |        |      |  ✓   |     |

### Disjointness notes

- cy-7s6.3 / cy-7s6.4 / cy-7s6.9 all disjoint: `schema+sema`, `sema-only`, `tck-only`.
- cy-7s6.9 is disjoint from everything else — can always run.
- Any two beads touching `cypher-syntax` must serialize (parser is a
  single hand-written file; conflicts are likely).
- Similarly `cypher-sema::infer` edits often conflict even across
  disjoint label sets — orchestrator should be cautious with >2 sema-
  touching beads in-flight.

---

## 3. Dispatch Order Recommendation

### Round 1 — unblock measurement + isolated work (3 parallel)

| Bead       | Rationale                                                      |
| ---------- | -------------------------------------------------------------- |
| cy-7s6.9   | TCK vendoring — disjoint, gives everyone measurement surface   |
| cy-7s6.3   | `keys/values` on Map — schema+sema, isolated                   |
| cy-7s6.5   | `rand()` — diag-only add, W7xxx registry entry                 |

### Round 2 — grammar unblock + parallel stdlib

Syntax-touching beads should serialize. Safer round 2:

| Bead       | Crates               | Notes                                    |
| ---------- | -------------------- | ---------------------------------------- |
| cy-7s6.7   | syntax+ast+hir+sema  | CASE — highest blocking factor           |
| cy-7s6.4   | sema only            | follows .7 but can start stub concurrently |

### Round 3 — post-CASE broad expansion

| Bead       | Reason                                                        |
| ---------- | ------------------------------------------------------------- |
| cy-7s6.2   | `reduce` — syntax                                             |
| cy-7s6.10  | Type predicate — sema mostly, depends on syntax landing       |

### Round 4 — pattern work (serialize — both heavy on syntax+hir)

| Bead       | Reason                                                        |
| ---------- | ------------------------------------------------------------- |
| cy-7s6.6   | Pattern comprehensions                                        |
| cy-7s6.8   | Pattern predicates — follows .6 (shared HIR lowering path)    |

### Round 5+ — spec-gated (BLOCKED on spec 0003)

| Bead       | Reason                                                        |
| ---------- | ------------------------------------------------------------- |
| cy-7s6.11  | Temporal expansion — requires spec amendment                  |
| cy-7s6.12  | Spatial — requires spec amendment                             |
| cy-7s6.13  | GQL quant-path — requires spec amendment                      |

---

## 4. Out-of-Scope / Deferred

### Needs spec amendment (file as spec 0003 proposal)

- cy-7s6.11 Temporal (time, localdatetime, duration + arithmetic)
- cy-7s6.12 Spatial (point, distance)
- cy-7s6.13 GQL quantified path patterns
- GQL `REPEATABLE`, `FILTER`, `EXCLUDE`, `REPEATABLE ELEMENTS`
- GQL graph schema catalog

### Blocked by in-progress epics

- cy-7s6.3 (`keys/values` map) — soft-blocks on cy-0ek if PropertyType::Map expands
- cy-7s6.11 Temporal — PropertyType additions overlap cy-0ek schema.toml format
- Workspace-level TCK integration — cy-o8c (cross-file project model) is an upstream concern for spec-0003-scale temporal/spatial

### Dropped entirely (not even deferred)

These conflict with AGENTS.md §9 and spec §19/§20; they are Neo4jCurrent
features that are **never** valid v1 targets and should not be filed as
children:

- `CALL { ... }` subqueries + `IMPORTING WITH`
- `EXISTS { ... }` subqueries (block form; `EXISTS(pattern)` call form is OK)
- `COUNT { ... }` subqueries
- `SHOW` commands
- `CYPHER` prefix directives
- `APOC` and `LOAD CSV`

---

## 5. Exclusions — AGENTS.md §9 Re-check

| Epic feature                         | §9 / §19 / §20 ruling         | Disposition    |
| ------------------------------------ | ----------------------------- | -------------- |
| CALL { } subqueries                  | §9.3 ❌, §19 ❌, §20 D1      | **Reject**     |
| IMPORTING WITH                       | §9.3 ❌ (ties to CALL { })   | **Reject**     |
| EXISTS { } block subquery            | §9.3 ❌, §19 ❌, §20 D1      | **Reject**     |
| COUNT { } expressions                | §19 ❌                        | **Reject**     |
| SHOW commands                        | §9.3 ❌, §19 ❌              | **Reject**     |
| CYPHER prefix directives             | §9.3 ❌, §19 ❌              | **Reject**     |
| APOC / LOAD CSV                      | §9.3 ❌, §19 ❌              | **Reject**     |
| Pattern comprehensions               | §6.1 sugar, §19 allows        | Implement      |
| Pattern predicate `EXISTS(patt)`     | §6.1 sugar, §19 allows        | Implement      |
| CASE                                 | §19 ✅                        | Implement      |
| List indexing / slicing              | (cy-7s6.1 closed)             | Done           |
| String stdlib                        | §8.3                          | Already landed |
| List stdlib (head/tail/size)         | cy-5gh closed                 | Done           |
| Map keys/values/properties           | §8.3                          | Implement      |
| Numeric stdlib (abs/ceil/floor/...)  | §8.3                          | Already landed |
| Temporal (time/localdatetime/dur)    | §19 ❌ Deferred, §20 D3      | Spec 0003      |
| Spatial (point/distance)             | §19 ❌ Deferred, §20 D4      | Spec 0003      |
| Type predicates `n IS :: T`          | §7.2 adjacent — no row        | Candidate 0003 |
| GQL quantified paths                 | not in 0001                   | Spec 0003      |
| GQL REPEATABLE / FILTER / EXCLUDE    | not in 0001                   | Spec 0003      |
| GQL schema catalog                   | overlaps cy-0ek               | Spec 0003, coordinate with cy-0ek |

---

## 6. Summary

- Proposed child beads: 12 (cy-7s6.2 through cy-7s6.13)
- Actionable without spec amendment: 9 (cy-7s6.2–cy-7s6.10)
- Blocked on spec-0003 amendment: 3 (cy-7s6.11, .12, .13)
- Drop entirely (conflict with §9.3 / §19): 7 items from epic description
- Best-round parallelism: 3 concurrent beads (round 1: .9, .3, .5)

*End of plan.*
