# tree-sitter-cypher

A [tree-sitter][ts] grammar for Cypher / GQL, parity-tested against the
Rust [`cyrs`][cyrs] front-end.

This subproject lives inside the `cyrs` repository but is published as an
independent npm/tree-sitter grammar. The Rust parser in `cyrs-syntax`
is authoritative; this grammar is a parallel hand-maintained artefact
kept in lock-step by the `cargo xtask tree-sitter-parity` harness.

## Parity claim

For every scenario in `crates/cyrs-tck/tck/v1.toml` (the cyrs TCK v1
surface, 48 scenarios as of today) plus 12 supplementary scenarios
compiled into `xtask tree-sitter-parity` to cover grammar-v1 surfaces
that aren't yet in the TCK fixture (UNION, list comp with map, list
slicing) and the spec §9.3 bans (`CALL { ... }` / `EXISTS { ... }` /
`SHOW` / `CYPHER` prefix / `LOAD CSV`):

- `outcome = "ok"` queries produce no `(ERROR)` nodes in the tree-sitter
  parse.
- `outcome = "error"` queries produce at least one `(ERROR)` node.

CI runs the parity harness on every PR; regressions fail.

## Scope (v1 — bead cy-h0p)

Clauses: `MATCH`, `OPTIONAL MATCH`, `WHERE`, `WITH`, `RETURN` (with
`DISTINCT` / `ORDER BY` / `SKIP` / `LIMIT` / `ASC` / `DESC`),
`UNWIND ... AS`, `CREATE`, `MERGE` (with `ON CREATE` / `ON MATCH`),
`SET`, `REMOVE`, `DELETE` / `DETACH DELETE`, `CALL <proc> YIELD ...`
(non-subquery form). `UNION` / `UNION ALL` tails between single-query
bodies.

Patterns: node patterns `(v:Label {props})`, relationship patterns with
directional arrows (`->`, `<-`, `-`), chained patterns, variable-length
`*m..n` with elided bounds, named paths `p = (a)-[]->(b)`,
`shortestPath(...)` / `allShortestPaths(...)`.

Expressions: integer / float / string (single and double quote) /
boolean / null literals, list and map literals, function calls, binary
operators (`AND`, `OR`, `XOR`, `NOT`, `=`, `<>`, `<`, `<=`, `>`, `>=`,
`+`, `-`, `*`, `/`, `%`, `^`, `CONTAINS`, `STARTS WITH`, `ENDS WITH`,
`IN`), property access (`.`), subscript (`[i]`), slice (`[i..j]` with
elided bounds), `IS NULL` / `IS NOT NULL`, `CASE WHEN … THEN … ELSE …
END`, parenthesised expressions, list comprehensions
(`[x IN xs WHERE p | f(x)]`), list predicates
(`ALL|ANY|NONE|SINGLE(x IN xs WHERE p)`), map projection
(`n { .name, .age, *, key: v }`), pattern predicates
(`EXISTS( (a)-->(b) )`).

Identifiers: regular and backtick-escaped.

Comments: line (`//`) and block (`/* */`).

### §9.3 bans (rejected with ERROR)

`CALL { ... }` block subqueries, `EXISTS { ... }` block subqueries,
`SHOW` statements, `CYPHER` prefixes, APOC procedures, `LOAD CSV`. The
grammar contains no rule that accepts these shapes; they fall through
as `(ERROR)` nodes. The `cargo xtask tree-sitter-parity` harness pins
the rejection contract with dedicated scenarios.

## Editor highlights

`queries/highlights.scm`, `queries/locals.scm`, and
`queries/injections.scm` ship with the grammar. Highlights target:
every keyword (via named `kw_*` leaf nodes), literals, binders (node /
rel / UNWIND / aliases / YIELD / list comprehension / list predicate),
labels / types (after `:`), properties (after `.`), procedure names,
parameters, operators and punctuation, and comments. `locals.scm`
links references to nearest-scope definitions so Neovim / Helix / Zed
can highlight the same variable consistently.

## Install (grammar developers)

```sh
cd tree-sitter-cypher
npm install
npx tree-sitter generate
npx tree-sitter test
```

The generated C sources (`src/parser.c`, `src/tree_sitter/`) are not
checked in — consumers that need pre-built artefacts should run
`tree-sitter generate` as part of their build.

## Use in editors

### Neovim (with [nvim-treesitter][nts])

Add a custom parser entry in your init:

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
parser_config.cypher = {
  install_info = {
    url = "https://github.com/phallsignup/cyrs",
    location = "tree-sitter-cypher",
    files = { "src/parser.c" },
    branch = "main",
    generate_requires_npm = true,
    requires_generate_from_grammar = true,
  },
  filetype = "cypher",
}
```

Then `:TSInstall cypher` and you get syntax highlighting + folding for
`*.cyp` / `*.cypher` files.

### Helix

Add to `~/.config/helix/languages.toml`:

```toml
[[language]]
name = "cypher"
scope = "source.cypher"
file-types = ["cyp", "cypher"]
roots = []
comment-token = "//"

[[grammar]]
name = "cypher"
source = { git = "https://github.com/phallsignup/cyrs", subpath = "tree-sitter-cypher" }
```

Then `hx --grammar fetch && hx --grammar build`.

## License

Dual-licensed under Apache-2.0 OR MIT, matching the parent workspace.

[ts]: https://tree-sitter.github.io/
[cyrs]: ../README.md
[nts]: https://github.com/nvim-treesitter/nvim-treesitter
