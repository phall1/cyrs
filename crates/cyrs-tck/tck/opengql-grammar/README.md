# opengql `GQL.g4` rule manifest

Vendored ISO/IEC 39075:2024 (GQL) ANTLR4 reference grammar plus the
derived rule manifest. Lives next to the existing `opengql-samples/`
corpus so the conformance-tooling crates can index both off a single
relative path. Bead: **cy-7hn0**.

## Contents

| File          | Purpose                                                        |
| ------------- | -------------------------------------------------------------- |
| `GQL.g4`      | Verbatim upstream grammar; pinned in `VENDORED.md`.            |
| `rules.json`  | Machine-readable rule manifest. Source of truth.               |
| `rules.md`    | Human-readable view of `rules.json` with `Implemented?` boxes. |
| `VENDORED.md` | Upstream provenance, license, and refresh procedure.           |
| `README.md`   | This file.                                                     |

## Regenerating

```
cargo xtask gql-rules
```

The xtask:

1. Reads `GQL.g4` from this directory.
2. Extracts every parser rule (`lowercaseName`), lexer rule
   (`UPPER_CASE`), and `fragment` definition.
3. For each rule, records: name, kind, 1-based line range, top-level
   alternative count, and the sorted/deduplicated set of identifiers
   referenced from the body.
4. Writes `rules.json` and `rules.md` deterministically — running the
   xtask twice in a row must yield a zero diff.

The implementation lives at `xtask/src/gql_rules.rs`. It is pure Rust:
**no JVM, no ANTLR runtime, no `antlr-rust`.** ANTLR4 `.g4` syntax is
regular enough for our needs once we ignore embedded actions and
semantic predicates (`{ … }`), which the scanner skips as balanced
brace blocks.

## `rules.json` schema

```json
{
  "total": 1018,
  "parser": 574,
  "lexer": 390,
  "fragment": 54,
  "rules": [
    {
      "name": "gqlProgram",
      "kind": "parser",
      "line_start": 7,
      "line_end": 10,
      "alts": 2,
      "refs": ["programActivity", "sessionCloseCommand"]
    }
  ]
}
```

* `kind` ∈ `"parser" | "lexer" | "fragment"`.
* `line_start` / `line_end` are 1-based inclusive over the source
  position of the rule header and its terminating `;`.
* `alts` counts top-level `|` alternatives at paren-depth 0 outside
  strings and `[…]` char classes; `(A | B) | C` is **2**, not 3.
* `refs` is sorted lexically and deduplicated; ANTLR meta-tokens
  (`EOF`, `HIDDEN`, `channel`, `skip`, `mode`, …) are stripped.

## `rules.md` contract

One table row per rule, in **source order**. The final column —
`Implemented?` — is `[ ]` for every row in this initial pass. Downstream
beads that grow parser coverage will tick boxes as productions land.
The xtask does **not** read implementation state today; the manifest is
write-only from the grammar side.

## Known gaps

The scanner is intentionally narrow:

* Per-rule `options { … }` blocks are stripped before the `:`.
* Embedded actions and semantic predicates (`{ … }` inside bodies) are
  treated as opaque and contribute no refs / no alts.
* Lexer commands after `->` (e.g. `-> channel(HIDDEN)`, `-> skip`) are
  picked up by the identifier scanner; meta-tokens are filtered, but
  any future custom action name will leak into `refs`. Add to
  `is_meta_keyword` in `xtask/src/gql_rules.rs` if that happens.
* Mode declarations (`mode FOO;`) and `tokens { … }` blocks would be
  skipped, but they do not appear in opengql `GQL.g4`.

If a future upstream revision introduces constructs the scanner cannot
handle, document the gap here and in the rules.md preamble; partial
coverage is acceptable as long as the rule count is honest.

## Related

* `crates/cyrs-tck/tck/opengql-samples/` — the matching sample corpus
  (also vendored from upstream).
* Bead `cy-7hn0` — this manifest, plus the xtask that emits it.
