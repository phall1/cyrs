# 0001 — `cypher-hir` should expose a single-call `parse_to_hir`

**Severity:** medium
**Discovered:** embedder legacy-parser→cyrs migration, stage 0

## Problem

Today an embedder that wants `(syntax errors, hir)` has to call:

```rust
let parse = cypher_syntax::parse(src);            // parse #1
let errs  = parse.errors();
let hir   = cypher_hir::lower::lower_statement(src); // parse #2 — re-lexes!
```

`lower_statement` takes `&str`, not the existing `Parse`/`SyntaxNode`, so
it re-runs the full lexer + parser pipeline. For a JIT-style embedder
(every query parse is hot path), that's a 2× cost.

## Proposed shape

```rust
// in cypher-hir
pub fn lower_parse(parse: &cypher_syntax::Parse) -> Statement;

// or, the convenience the caller actually wants:
pub fn parse_to_hir(src: &str) -> ParseToHir;

pub struct ParseToHir {
    pub parse: cypher_syntax::Parse,
    pub hir: Statement,                     // best-effort even on errors
    pub syntax_errors: Vec<SyntaxError>,
}
```

`lower_statement(&str)` can stay as a sugar wrapper, but the version that
takes a `Parse` needs to exist.

## Why it matters for the embedder

The embedder's hot path is `execute_query(src, store)`. Doubling parse
work on every query is unacceptable. The current `cyrs_bridge::parse`
in the embedder has a `TODO(cyrs-issue-0001)` flagging this.
