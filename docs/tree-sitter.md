# Tree-sitter grammar

A parallel [tree-sitter](https://tree-sitter.github.io/) grammar for
Cypher / GQL lives at
[`tree-sitter-cypher/`](../tree-sitter-cypher), shipped for editor
integrations (Neovim, Helix, GitHub highlighter).

The Rust parser in `cyrs-syntax` is **authoritative**. The tree-sitter
grammar is a hand-maintained artefact kept in lock-step via the
`cargo xtask tree-sitter-parity` gate.

## Parity contract

The grammar parses the same TCK v1 surface as the Rust parser: every
`outcome = "ok"` scenario in `crates/cyrs-tck/tck/v1.toml` parses
without `(ERROR)` nodes, and every `outcome = "error"` scenario
produces at least one. Regressions fail CI.

## Neovim (nvim-treesitter)

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
parser_config.cypher = {
  install_info = {
    url = "https://github.com/phall1/cyrs",
    location = "tree-sitter-cypher",
    files = { "src/parser.c" },
    branch = "main",
    generate_requires_npm = true,
    requires_generate_from_grammar = true,
  },
  filetype = "cypher",
}
```

Then `:TSInstall cypher`.

## Helix

`~/.config/helix/languages.toml`:

```toml
[[language]]
name = "cypher"
scope = "source.cypher"
file-types = ["cyp", "cypher"]
roots = []
comment-token = "//"

[[grammar]]
name = "cypher"
source = { git = "https://github.com/phall1/cyrs", subpath = "tree-sitter-cypher" }
```

Then `hx --grammar fetch && hx --grammar build`.

## VS Code / VSCodium

The full language client (LSP + grammar) lives at
[`editors/vscode/`](../editors/vscode). The bundled tree-sitter grammar
backs the highlighter; the LSP backs everything else. Dev-install
instructions are in the editor's
[README](../editors/vscode/README.md). Marketplace publishing is a
manual maintainer step.

## Developer workflow

Grammar source, parity tests, and the scope list live at
[`tree-sitter-cypher/README.md`](../tree-sitter-cypher/README.md).
Regenerate after grammar edits with the workflow described there;
parity is verified by `cargo xtask tree-sitter-parity`.
