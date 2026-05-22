# GQL ISO/IEC 39075:2024 Conformance Bootstrap

This directory holds a hand-authored bootstrap corpus of scenarios that
exercise the **GQL-distinct surface** of ISO/IEC 39075:2024 — i.e. the
constructs where GQL diverges from openCypher v9. It is the seed for the
conformance harness called out by bead **cy-0hj** (acceptance bullet
under cy-7s6: *"GQL ISO 39075 conformance harness"*).

## What's in scope (v0 bootstrap)

The corpus is intentionally small: ≤20 hand-authored scenarios across a
handful of areas. Each scenario carries an inline ISO/IEC 39075:2024
section citation. The scope:

| Area                | File(s)                                  | ISO §           | What it pins                                  |
| ------------------- | ---------------------------------------- | --------------- | --------------------------------------------- |
| `clauses/insert`    | `clauses/Insert1.feature`                | §13.4           | `INSERT NODE` / `INSERT EDGE` (vs `CREATE`)   |
| `clauses/filter`    | `clauses/Filter1.feature`                | §14.10          | `FILTER` clause (post-projection)             |
| `clauses/return`    | `clauses/Return1.feature`                | §14.13          | `RETURN ALL`, `EXCLUDE`                       |
| `clauses/optional`  | `clauses/Optional1.feature`              | §14.7, §14.11.3 | `OPTIONAL CALL`, `OPTIONAL MATCH` parity      |
| `values/repeatable` | `values/Repeatable1.feature`             | §10.6.3         | `REPEATABLE ELEMENTS`, `DIFFERENT EDGES`      |
| `types/schema`      | `types/SchemaTypes1.feature`             | §6.5.2, §6.2    | `IS TYPED` / `::` casts, named GQL types      |
| `paths/selector`    | `paths/PathSelector1.feature`            | §10.4.2         | `ANY SHORTEST`, `ALL SHORTEST`, `SHORTEST k`  |

## What's *not* in scope

- Full ISO 39075 surface coverage — there is no public, exhaustive
  ISO conformance test corpus for GQL. Reaching full compliance is
  follow-up work tracked under future beads.
- Schema DDL (`CREATE GRAPH TYPE`, `CREATE NODE TYPE`, etc.).
- Transaction-control / session-management statements.
- Catalog / authorisation statements (§14.14, §14.15).
- Procedure-result-set introspection, `LIST` / `DESCRIBE`.
- Anything `Neo4jCurrent`-only (not part of either v1 dialect — see
  spec 0001 §9.3).

## Source citations

Every scenario file opens with a comment naming the ISO/IEC 39075:2024
section it derives from. If a scenario covers a construct that is **not**
in the standard but is a deliberate cyrs extension, the comment reads
`# Cyrs-extension:` instead, so the deviation is loud and grep-able.

The current bootstrap contains zero `Cyrs-extension:` markers — every
scenario derives directly from the standard.

## The `@covers:` convention

Every scenario carries a Gherkin tag line naming the **GQL.g4 parser
productions** it exercises:

```gherkin
  @covers:matchStatement,returnStatement,filterStatement
  Scenario: [1] FILTER after RETURN-style projection
```

Production names are the parser rules of the vendored ISO grammar —
see `../opengql-grammar/rules.json` (regenerate with `cargo xtask
gql-rules`). The coverage harness uses these tags to compute, against
the 574 parser productions, how much of the grammar a *passing*
scenario reaches. This is what makes the corpus a measurable
conformance suite rather than an unanchored pile of queries.

Rules for `@covers:`:

- **Mandatory.** Every scenario must carry at least one `@covers:`
  production. The harness *fails* otherwise — an untagged scenario
  contributes nothing to coverage and is silent rot.
- **Real names only.** Each name must be a `kind: parser` rule in
  `rules.json`. The harness *fails* on a typo — there is no silent
  drift.
- **Honest, not exhaustive.** List the productions the scenario
  meaningfully exercises. You need not enumerate every leaf token; do
  name the construct(s) the scenario exists to pin.

## Compliance badges

Two independent measurements come off this corpus, both auto-generated
(do not hand-edit) and both independent of the openCypher TCK badge:

- **`baseline.md`** — parser-acceptance rate (how many scenarios parse
  with zero syntax errors).
- **`coverage.md`** — grammar coverage (how many of the 574 GQL.g4
  parser productions are reached by a passing scenario), plus the
  uncovered-production worklist for growing the corpus.

## Running

```sh
cargo xtask gql-coverage
# or, directly:
cargo test -p cyrs-tck --features gql-iso --test gql_iso
```

The runner walks `features/`, extracts the `When executing query:`
block of every scenario, runs it through `cyrs_db::Database` in
`DialectMode::GqlAligned`, and writes `baseline.md` + `coverage.md`.
The acceptance baseline never fails (it is a *measurement*); the
coverage test *does* fail on a bad or missing `@covers:` tag.

## Initial pass-rate

The initial pass-rate is expected to be **low**. The cyrs parser does
not yet implement most GQL-only constructs (e.g. `INSERT NODE`,
`FILTER`, `REPEATABLE ELEMENTS`, `IS TYPED`, `ANY SHORTEST`, etc.).
That is by design: this PR establishes the *harness* and a body of
scenarios so that follow-up beads can land parser changes and watch
the baseline tick upward without first re-discovering the surface.

## Layout

```
gql-iso-39075/
├── README.md            # this file
├── baseline.md          # auto-generated: parser-acceptance rate (do not edit)
├── coverage.md          # auto-generated: grammar coverage + worklist (do not edit)
└── features/
    ├── clauses/
    │   ├── Filter1.feature
    │   ├── Insert1.feature
    │   ├── Optional1.feature
    │   └── Return1.feature
    ├── paths/
    │   └── PathSelector1.feature
    ├── types/
    │   └── SchemaTypes1.feature
    └── values/
        └── Repeatable1.feature
```
