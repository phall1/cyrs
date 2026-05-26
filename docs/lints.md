# Lints

Beyond the error-severity semantic checks, cyrs ships a
**clippy-equivalent lint pack**: warning-severity diagnostics for
queries that parse and analyse cleanly but are stylistically poor or
likely a bug. Each lint carries a `note:` fix hint. Implementation
lives in [`crates/cyrs-sema/src/lints/`](../crates/cyrs-sema/src/lints).

## Opt-in by design

Lints are off by default and never change the exit code of `cypher
check`. Three opt-in surfaces:

- **CLI** — `cypher check --lints` runs the pass and prints lints
  alongside semantic diagnostics.
- **LSP** — set `initializationOptions.lints` to `true`; lints surface
  as `Information`-severity diagnostics.
- **Project manifest** — each lint maps to a rule name in
  `cypher-project.toml`'s lint registry
  ([spec 0003](./specs/0003-project-manifest.md)).

## Catalogue

| Code | Lint | Fires when | Rule name |
| ----- | ---- | ---------- | --------- |
| `W6011` | unused pattern variable | a `MATCH` binder is never referenced downstream | `unused-pattern-var` |
| `W6012` | redundant `MATCH` | a `MATCH` exactly duplicates an earlier one | `redundant-match` |
| `W6013` | unrestricted pattern | a node / relationship pattern has no label or type (schema-aware) | `unrestricted-pattern` |
| `W6014` | implicit cartesian product | two `MATCH` clauses share no variable or join predicate | `cartesian-product` |
| `W6015` | wide `RETURN *` | `RETURN *` in a statement binding more than N variables | `wildcard-return` |
| `W6016` | `OPTIONAL MATCH` + `WHERE` on its binding | a trailing `WHERE` constrains the optional binding (defeats `OPTIONAL`) | `optional-match-where` |

`W6012` (redundant `MATCH`) and `W6014` (cartesian product) are
conservative: they fire only on unambiguous cases. False positives
cost trust faster than missed positives cost completeness.
