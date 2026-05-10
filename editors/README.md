# editors/

Editor integrations for cyrs. Each subdirectory is a standalone client
that consumes the `cypher-lsp` binary published by `crates/cypher-lsp/`.
Layout mirrors the `editors/code/` convention used by `rust-analyzer`.

| Path | Editor | Notes |
| ---- | ------ | ----- |
| [`vscode/`](./vscode) | VS Code / VSCodium | TypeScript language client + TextMate grammar. Bead cy-9w5. |

The Neovim demo lives at [`demo/nvim/`](../demo/nvim/) instead of here
because it is intentionally a documentation artefact, not a packaged
plugin. Helix is configured by users directly via
[`tree-sitter-cypher/`](../tree-sitter-cypher/).

## Adding a new editor

1. Build a thin client; do not duplicate language-server logic.
2. Reuse settings names where reasonable so users moving between
   editors do not relearn the surface (`cyrs.server.path`, schema /
   dialect knobs, formatter knobs).
3. Mirror server discovery from `demo/nvim/init.lua` (settings →
   `$CYPHER_LSP` → `$PATH`).
