# Vendored: opengql/grammar samples

This directory vendors the official ISO/IEC 39075:2024 GQL sample query
corpus from the OpenGQL grammar repository:

- **Upstream**: https://github.com/opengql/grammar
- **License**: Apache-2.0 (see `LICENSE-APACHE` at the workspace root for the
  identical license text; upstream `LICENSE` is at
  https://github.com/opengql/grammar/blob/main/LICENSE)
- **Pinned commit**: `16ea71bd320ad07fd2c46a3066afbaef7d226922`
- **Source path**: `samples/` at the pinned commit
- **Vendored on**: 2026-05-19
- **Bead**: cy-qsze

## What

14 hand-authored GQL queries published alongside the ISO/IEC 39075:2024
ANTLR4 grammar (`GQL.g4`) as conformance smoke samples. They cover:

- `CREATE GRAPH` / `CREATE SCHEMA` / `CREATE …GRAPH TYPE` (both
  double-colon and lexical forms, including nested graph types)
- `INSERT` (node + edge with temporal literals)
- `MATCH … INSERT` combinations
- `MATCH` with `EXISTS { … }` predicates (braces, parentheses, nested)
- `SESSION SET` statements (`GRAPH`, `PROPERTY GRAPH`, value, time zone)

## Why

These samples come straight from the body that publishes the GQL grammar —
they are the closest thing to an upstream-blessed conformance smoke test.
The hand-authored `tck/gql-iso-39075/` bootstrap covers a wider feature
matrix but is authored by cyrs maintainers; this corpus is independent.

The two corpora are kept as separate harnesses so a regression in either
is attributable to its source.

## Refresh procedure

To re-sync against a newer upstream commit:

```sh
SHA=<new-commit-sha>
cd crates/cyrs-tck/tck/opengql-samples
gh api "repos/opengql/grammar/contents/samples?ref=$SHA" --jq '.[] | .download_url' \
  | while IFS= read -r url; do
      name=$(basename "$url" | python3 -c "import sys,urllib.parse; print(urllib.parse.unquote(sys.stdin.read().strip()))")
      curl -fsSL "$url" -o "$name"
    done
```

Then update the **Pinned commit** + **Vendored on** lines above, re-run
`cargo test -p cyrs-tck --features opengql-samples --test opengql_samples`,
and commit the new `baseline.md`.
