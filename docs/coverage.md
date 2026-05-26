# Coverage

cyrs is a Cypher compiler front-end with first-class GQL ISO/IEC
39075:2024 parser support. The numbers below are rolling measurements
produced by the TCK harness ([spec 0001 §17.5](./specs/0001-cypher-frontend.md)),
not aspirational targets.

| Surface | Corpus | Result | Source |
| ------- | ------ | ------ | ------ |
| openCypher v9 | upstream openCypher TCK `2024.3` (220 feature files, 3 897 expanded scenarios) | **3 632 / 3 897 accepted (93.2 %)** | [`crates/cyrs-tck/tck/full-baseline.md`](../crates/cyrs-tck/tck/full-baseline.md) |
| GQL ISO/IEC 39075:2024 | hand-authored §-cited bootstrap (28 feature files, 140 expanded scenarios) | **140 / 140 accepted (100 %); 153 / 574 grammar productions reached (26.7 %)** | [`crates/cyrs-tck/tck/gql-iso-39075/baseline.md`](../crates/cyrs-tck/tck/gql-iso-39075/baseline.md), [`coverage.md`](../crates/cyrs-tck/tck/gql-iso-39075/coverage.md) |
| GQL ISO/IEC 39075:2024 (upstream samples) | OpenGQL `opengql/grammar` samples (14 files) | **14 / 14 accepted (100 %)** | [`crates/cyrs-tck/tck/opengql-samples/baseline.md`](../crates/cyrs-tck/tck/opengql-samples/baseline.md) |

## What the numbers mean

Both numbers measure **parser acceptance** — the parser emits zero
syntax errors for the `When executing query:` step. They do not assert
runtime semantics: the front-end performs no execution
([spec §1.3 N1](./specs/0001-cypher-frontend.md)).

- **openCypher 93.2 %.** Measured against the full 3 897-scenario
  upstream TCK. `Expected::Error` scenarios are still untriaged and
  surface as `Expected::Ignored`; the baseline file's preamble lists
  the open items.
- **GQL 100 %.** Measured against a hand-authored bootstrap that
  pins the GQL-distinct surface — feature files grouped by area, each
  scenario citing its ISO/IEC 39075:2024 section. ISO does not publish
  a public conformance test corpus for GQL, so the bootstrap *is* the
  corpus. The companion file
  [`coverage.md`](../crates/cyrs-tck/tck/gql-iso-39075/coverage.md)
  reports how many of the 574 GQL.g4 parser productions are reached
  by at least one passing scenario, plus the uncovered-production
  worklist that drives further corpus growth.

## GQL bootstrap coverage

Each row maps a GQL-distinct construct to its ISO section and the bead
that landed it. Per-feature files live under
[`crates/cyrs-tck/tck/gql-iso-39075/features/`](../crates/cyrs-tck/tck/gql-iso-39075/features/).

| Construct | ISO § | Feature file | Bead |
| --------- | ----- | ------------ | ---- |
| `INSERT NODE` / `INSERT EDGE` (vs Cypher `CREATE`) | §13.4 | `clauses/Insert1.feature` | cy-8z3 |
| `OPTIONAL CALL` (empty-multiset on failure) | §14.11.3 | `clauses/Optional1.feature` | cy-tdl |
| `RETURN ALL` / `RETURN ... EXCLUDE <field>` | §14.13 | `clauses/Return1.feature` | cy-auh |
| `FILTER` (post-projection row filter) | §14.10 | `clauses/Filter1.feature` | cy-r50 |
| `IS TYPED <T>` predicate + `::` cast | §6.5.2 / §6.2 | `types/SchemaTypes1.feature` | cy-pnp |
| `ANY SHORTEST` / `ALL SHORTEST` / `SHORTEST k` + `->+` quantifier | §10.4.2 / §10.5 | `paths/PathSelector1.feature` | cy-3mq |
| `REPEATABLE ELEMENTS` / `DIFFERENT EDGES` + `->{m,n}` quantifier | §10.6.3 | `values/Repeatable1.feature` | cy-q2g |

## Out of scope for the bootstrap

Parser acceptance only; the following are corpus-growth follow-ups, not
parser bugs:

- Full ISO 39075 surface coverage.
- Schema DDL (`CREATE GRAPH TYPE`, etc.).
- Transaction-control statements.
- Catalog and authorisation statements (§14.14, §14.15).
- Procedure result-set introspection.
- `Neo4jCurrent`-only constructs (spec 0001 §9.3).

## Dialect routing

The parser emits the same CST for both dialects. The
[`DialectMode`](../crates/cyrs-db/src/lib.rs) selector at the analysis
layer gates GQL-only and Cypher-only constructs through `E4xxx`
diagnostics; the mapping lives in
[`crates/cyrs-sema/src/dialect.rs`](../crates/cyrs-sema/src/dialect.rs).
