# tree-sitter-cypher

A [tree-sitter][ts] grammar for Cypher / GQL, parity-tested against the
Rust [`cyrs`][cyrs] front-end.

This subproject lives inside the `cyrs` repository but is published as an
independent npm/tree-sitter grammar. The Rust parser in `cypher-syntax`
is authoritative; this grammar is a parallel hand-maintained artefact
kept in lock-step by the `cargo xtask tree-sitter-parity` harness.

## Parity claim

For every scenario in `crates/cypher-tck/tck/v1.toml` (the cyrs TCK v1
surface, 41 scenarios as of today):

- `outcome = "ok"` queries produce no `(ERROR)` nodes in the tree-sitter
  parse.
- `outcome = "error"` queries produce at least one `(ERROR)` node.

CI runs the parity harness on every PR; regressions fail.

## Scope (v0)

Clauses: `MATCH`, `OPTIONAL MATCH`, `WHERE`, `WITH`, `RETURN` (with
`DISTINCT` / `ORDER BY` / `SKIP` / `LIMIT` / `ASC` / `DESC`),
`UNWIND ... AS`, `CREATE`, `MERGE` (with `ON CREATE` / `ON MATCH`),
`SET`, `REMOVE`, `DELETE` / `DETACH DELETE`.

Patterns: node patterns `(v:Label {props})`, relationship patterns with
directional arrows (`->`, `<-`, `-`), chained patterns, variable-length
`*m..n` with elided bounds, named paths `p = (a)-[]->(b)`.

Expressions: integer / float / string (single and double quote) /
boolean / null literals, list and map literals, function calls, binary
operators (`AND`, `OR`, `XOR`, `NOT`, `=`, `<>`, `<`, `<=`, `>`, `>=`,
`+`, `-`, `*`, `/`, `%`, `^`, `CONTAINS`, `STARTS WITH`, `ENDS WITH`,
`IN`), property access (`.`), subscript (`[i]`), slice (`[i..j]` with
elided bounds), `IS NULL` / `IS NOT NULL`, `CASE WHEN … THEN … ELSE …
END`, parenthesised expressions.

Identifiers: regular and backtick-escaped.

Comments: line (`//`) and block (`/* */`).

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
