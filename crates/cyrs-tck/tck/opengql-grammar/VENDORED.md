# Vendored upstream — `opengql/grammar`

This directory vendors the ISO/IEC 39075:2024 (GQL) ANTLR4 reference
grammar from <https://github.com/opengql/grammar>.

| Field        | Value                                                                 |
| ------------ | --------------------------------------------------------------------- |
| Upstream     | https://github.com/opengql/grammar                                    |
| Commit       | `16ea71bd320ad07fd2c46a3066afbaef7d226922`                            |
| Path         | `GQL.g4`                                                              |
| Fetched      | 2026-05-19                                                            |
| License      | Apache-2.0 (see upstream `LICENSE`)                                   |
| Bead         | cy-7hn0                                                               |

## Why this is here

cyrs tracks ISO 39075 production-rule coverage so we can report exactly
which spec-defined productions our parser accepts (bead cy-7hn0). The
manifest is generated from this vendored `.g4` by:

```
cargo xtask gql-rules
```

The xtask writes a deterministic `rules.json` + `rules.md` next to this
file. Both files are committed; CI re-runs the xtask to catch drift if
the grammar is ever re-vendored.

## How to refresh

1. Update the commit hash in the table above.
2. `curl -sSL https://raw.githubusercontent.com/opengql/grammar/<sha>/GQL.g4 -o GQL.g4`
3. `cargo xtask gql-rules` and commit `GQL.g4`, `rules.json`, `rules.md`,
   and this file together.

## Constraints

- **No JVM, no ANTLR runtime.** The xtask parses `.g4` with a small
  pure-Rust scanner (`xtask/src/gql_rules.rs`). We do not link
  antlr4rust and we do not shell out to the `antlr4` Java tool.
- **No grammar mutation.** `GQL.g4` is verbatim from upstream. If we
  ever need to patch it, do so in a sibling file and document the
  rationale here.
