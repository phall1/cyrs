# `cypher` demo — Neovim LSP

A 5-minute walkthrough that wires the `cypher-lsp` binary into Neovim so
you can watch the frontend produce live diagnostics and format queries
as you type. No plugins required.

## What you'll see

- **Syntax diagnostics** from the recovering parser (stable `E0xxx` codes).
- **Name-resolution diagnostics** from the sema pipeline (`E1xxx`).
- **Format-on-save** via `cypher-fmt`, same formatter `cypher fmt` uses.

All of this runs against the same incremental Salsa database the
language server uses in production — not a scripted demo.

## Prerequisites

- Rust toolchain (edition 2024, see `rust-toolchain.toml`).
- Neovim 0.8 or newer. Tested on 0.11.

## Build

From the workspace root:

```sh
cargo build --release -p cypher-lsp
```

That produces `target/release/cypher-lsp`. The demo's `init.lua` finds
it automatically — no need to add it to `$PATH`.

## Run

```sh
nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
```

You should see a red `E0011: expected ')' to close node pattern`
virtual-text diagnostic on the `MATCH (n RETURN n` line within a second
or two of the buffer opening. Hover the cursor and wait 250 ms for the
floating diagnostic popup.

Cycle through the other samples with `:e demo/samples/unknown_var.cyp`
etc. and watch the diagnostic set change.

## Format-on-save demo

```sh
nvim -u demo/nvim/init.lua demo/samples/needs_fmt.cyp
```

The file opens unformatted. Hit `:w`. The `BufWritePre` autocommand runs
`vim.lsp.buf.format()`, which sends a `textDocument/formatting` request
to `cypher-lsp`, which returns a `TextEdit` produced by `cypher-fmt`.
The buffer is rewritten in place before the save completes.

To compare against the CLI:

```sh
cargo run -p cypher-cli -- fmt demo/samples/needs_fmt.cyp
```

Same bytes out.

## Sample files

| File                      | What it demonstrates                                          |
| ------------------------- | ------------------------------------------------------------- |
| `samples/good.cyp`        | Clean query. No diagnostics; formatter output is idempotent.  |
| `samples/unclosed_paren.cyp` | Parser recovery: `E0011` on the unclosed `(`.             |
| `samples/unknown_var.cyp` | Name resolution: `E1001` on `undefined_var`.                  |
| `samples/needs_fmt.cyp`   | Formatter: valid input with bad whitespace; format-on-save fixes it. |

## Troubleshooting

**"cypher-lsp binary not found"** — the init walks up from `demo/nvim/`
to look for `target/{release,debug}/cypher-lsp`. If you built somewhere
else, point `$CYPHER_LSP` at the absolute path:

```sh
CYPHER_LSP=/custom/path/cypher-lsp nvim -u demo/nvim/init.lua samples/good.cyp
```

**No diagnostics appear** — check `:LspLog` (nvim 0.11 provides it by
default) or run with `CYPHER_LSP_LOG=debug` to stream tracing output to
stderr. The LSP prints to stderr, so run in a terminal that will show
it.

**Hover returns nothing** — by design. Hover and goto-definition are
v1-deferred (spec §14, bead cy-gc4). The LSP advertises the capability
but returns `null` on every request. Diagnostics and formatting are the
only live features in v1.

## Why only these features?

This is a pre-0.1 front-end. The working layers right now:

```
parser (cypher-syntax)  ──►  AST (cypher-ast)  ──►  HIR + resolver (cypher-hir)
                                                            │
                                                            ▼
                                                        sema (cypher-sema)
                                                            │
                                                            ▼
                                                   diagnostics (cypher-diag)
```

The formatter (`cypher-fmt`) rides on top of the CST directly.
The LSP plugs both into `lsp-server`.

The plan IR and codegen are spec'd but not implemented, so `hover`,
`goto-definition`, `completion`, and `rewrite` are all intentionally
wired as stubs. When they land, the LSP picks them up with no config
change on the client side — the capabilities are already advertised.
