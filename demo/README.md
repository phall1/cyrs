# `cypher` demo — Neovim LSP

A 5-minute walkthrough that wires the `cyrs-lsp` binary into Neovim
so you can watch the frontend produce live diagnostics, formatting,
hover, goto-definition, rename, completion, and more. No plugins
required.

![cyrs-lsp demo: diagnostics + format-on-save](demo.gif)

The recording above is regenerated from [`demo.tape`](demo.tape) with
[charmbracelet/vhs](https://github.com/charmbracelet/vhs):

```sh
vhs demo/demo.tape     # produces demo/demo.gif
```

Run from the workspace root; the tape builds `cyrs-lsp` first and
then drives Neovim through the parser-recovery diagnostic on
`unclosed_paren.cyp` and the format-on-save hook on `needs_fmt.cyp`.

## What you'll see

- **Diagnostics** — syntax errors (`E0xxx`) from the recovering
  parser and name-resolution errors (`E1xxx`) from the sema
  pipeline.
- **Format-on-save** via `cyrs-fmt`, same formatter `cypher fmt`
  uses.
- **Hover** — keyword descriptions + bound-variable kind lookups.
- **Goto-definition** — jump from `RETURN n` to the `MATCH (n)`
  that defined it.
- **References + rename** — `gr` for references, `<leader>rn` for
  rename (via the defaults in Neovim 0.11).
- **Completion** — keyword + parameter suggestions; label
  suggestions when a schema is loaded (see below).
- **Signature help** — types/positions for a function call's
  parameters (requires a schema with a `functions` entry).
- **Code actions** — fix-its surfaced by the diagnostics pipeline.
- **Folding + semantic tokens + inlay hints** — all live in v1.

All of this runs against the same incremental Salsa database the
language server uses in production — not a scripted demo.

## Prerequisites

- Rust toolchain (edition 2024, see `rust-toolchain.toml`).
- Neovim 0.8 or newer. Tested on 0.11.

## Build

From the workspace root:

```sh
cargo build --release -p cyrs-lsp
```

That produces `target/release/cypher-lsp`. The demo's `init.lua`
finds it automatically — no need to add it to `$PATH`.

## Run

```sh
nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
```

You should see a red `error[E0011]: expected ')' to close node
pattern` virtual-text diagnostic on the `MATCH (n:Person` line
within a second or two of the buffer opening. Hover the cursor and
wait 250 ms for the floating diagnostic popup.

Cycle through the other samples with `:e demo/samples/unknown_var.cyp`
etc. and watch the diagnostic set change.

## Features to try

Once a `.cyp` file is open and `cyrs-lsp` has attached:

| Action | Keybinding (nvim 0.11 default) | What it hits |
|---|---|---|
| Hover | `K` | `textDocument/hover` |
| Goto definition | `grd` / `<C-]>` | `textDocument/definition` |
| References | `grr` | `textDocument/references` |
| Rename | `grn` | `textDocument/rename` + `prepareRename` |
| Completion | `<C-x><C-o>` (omnifunc) | `textDocument/completion` |
| Signature help | `<C-s>` in insert mode | `textDocument/signatureHelp` |
| Code actions | `gra` | `textDocument/codeAction` |
| Format | `:lua vim.lsp.buf.format()` | `textDocument/formatting` |

`init.lua` also registers a `BufWritePre` autocommand that formats
the buffer on every `:w`.

## Custom commands (`workspace/executeCommand`)

The server advertises two custom commands you can invoke from Neovim:

```vim
:lua vim.lsp.buf_request(0, 'workspace/executeCommand',
     { command = 'cypher.explainPlan',
       arguments = { { uri = vim.uri_from_bufnr(0) } } },
     function(_, result) print(result) end)
```

Commands available:

- `cypher.explainPlan` — returns the pretty-printed logical plan
  for the active buffer.
- `cypher.lowerToHir` — returns the HIR overlay (useful for
  debugging name resolution).

## Format-on-save demo

```sh
nvim -u demo/nvim/init.lua demo/samples/needs_fmt.cyp
```

The file opens unformatted. Hit `:w`. The `BufWritePre` autocommand
runs `vim.lsp.buf.format()`, which sends a
`textDocument/formatting` request to `cyrs-lsp`, which returns a
`TextEdit` produced by `cyrs-fmt`. The buffer is rewritten in
place before the save completes.

To compare against the CLI:

```sh
cargo run -p cyrs-cli -- fmt demo/samples/needs_fmt.cyp
```

Same bytes out.

## Loading a schema (unlocks label + function completion)

Drop a `schema.json` next to your queries:

```json
{
  "labels": ["Person", "Movie"],
  "rel_types": ["ACTED_IN", "KNOWS"],
  "node_properties": {
    "Person": [
      { "name": "name", "type": "String" },
      { "name": "age",  "type": "Int"    }
    ]
  },
  "functions": [
    { "name": "toUpper", "params": ["s"], "return_type": "String" }
  ]
}
```

Launch Neovim with the init and the schema path in
`$CYPHER_SCHEMA_PATH`:

```sh
CYPHER_SCHEMA_PATH=$PWD/schema.json \
  nvim -u demo/nvim/init.lua demo/samples/good.cyp
```

Label completion after `:` and function signatureHelp after `(`
will now surface schema entries. See spec §14.3 for the full
init-option shape.

## Sample files

| File | What it demonstrates |
|---|---|
| `samples/good.cyp` | Clean query. No diagnostics; formatter output is idempotent. |
| `samples/unclosed_paren.cyp` | Parser recovery: `E0011` on the unclosed `(`. |
| `samples/unknown_var.cyp` | Name resolution: `E1001` on `undefined_var`. |
| `samples/needs_fmt.cyp` | Formatter: valid input with bad whitespace; format-on-save fixes it. |

## Troubleshooting

**"cypher-lsp binary not found"** — the init walks up from
`demo/nvim/` to look for `target/{release,debug}/cypher-lsp`. If you
built somewhere else, point `$CYPHER_LSP` at the absolute path:

```sh
CYPHER_LSP=/custom/path/cypher-lsp \
  nvim -u demo/nvim/init.lua samples/good.cyp
```

**No diagnostics appear** — check `:LspLog` (Neovim 0.11 provides
it by default) or run with `CYPHER_LSP_LOG=debug` to stream tracing
output to stderr. The LSP prints to stderr, so run in a terminal
that will show it.

**Hover returns empty** — v1 hover only fires on keywords and
IDENT tokens that match a binding in the current statement.  On
punctuation or whitespace the server returns `null` and Neovim
shows no popup.

## What's inside

```
parser (cyrs-syntax)  ──►  AST (cyrs-ast)  ──►  HIR + resolver (cyrs-hir)
                                                            │
                                                            ▼
                                                        sema (cyrs-sema)
                                                            │
                                                            ▼
                                                   diagnostics (cyrs-diag)
```

The formatter (`cyrs-fmt`) rides on top of the CST directly.
`cyrs-lsp` exposes the whole stack through the LSP protocol.
`cyrs-agent` exposes the same pipeline over a JSON-per-line
stdio protocol for sandboxed tool-using agents (spec §15).
