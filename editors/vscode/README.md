# cyrs — Cypher / GQL for VS Code

VS Code language support for [cyrs](https://github.com/phall1/cyrs), a
compiler front-end for Cypher / GQL written in Rust. Powered by the
`cypher-lsp` language server (spec §14).

> Status: preview. Marketplace publishing is the maintainer's manual step;
> for now, dev-install via the steps below.

## Features

The extension is a thin client over `cypher-lsp`. Everything the server
advertises in
[`server_capabilities`](https://github.com/phall1/cyrs/blob/main/crates/cypher-lsp/src/lib.rs)
is available out of the box:

- Diagnostics with stable codes (parser, name-resolution, sema).
- Hover (signature + docs).
- Go-to-definition / find-references.
- Workspace symbol search.
- Completion with trigger characters `:` (labels), `.` (properties), `$`
  (parameters), plus `resolveProvider`.
- Rename with `prepareProvider`.
- Semantic tokens (full + range).
- Inlay hints.
- Document and range formatting (powered by `cypher-fmt`).
- Code actions.
- Folding ranges.
- Signature help (triggers on `(` and `,`).
- File watchers (`workspace/didChangeWatchedFiles` debounced per
  `cyrs.watchedFilesDebounceMs`).
- Workspace commands: `cypher.explainPlan`, `cypher.lowerToHir`.

## Install

### From source (until the marketplace listing exists)

Build the language server:

```sh
cargo build --release -p cyrs-lsp
```

Install the extension into your local VS Code:

```sh
cd editors/vscode
npm install
npm run compile
# Optional: package as a .vsix and install
npx --yes @vscode/vsce package
code --install-extension cyrs-vscode-0.0.1.vsix
```

Or develop it: open `editors/vscode/` in VS Code and press `F5` to
launch an Extension Development Host.

## Configuration

| Setting | Default | Notes |
| ------- | ------- | ----- |
| `cyrs.server.path` | `""` | Absolute path to `cypher-lsp`. Falls back to `$CYPHER_LSP`, then `$PATH`. |
| `cyrs.server.extraEnv` | `{}` | Extra env vars for the server process. |
| `cyrs.trace.server` | `"off"` | LSP trace level (`off` / `messages` / `verbose`). |
| `cyrs.schema.source` | `"none"` | Forwarded as `initializationOptions.schemaSource`. |
| `cyrs.schema.path` | `""` | Path to schema JSON when `schema.source = file`. |
| `cyrs.schema.command` | `""` | Shell command emitting schema JSON when `schema.source = command`. |
| `cyrs.dialect` | `"GqlAligned"` | `GqlAligned` or `OpenCypherV9`. |
| `cyrs.formatting.width` | `100` | Soft column limit. |
| `cyrs.formatting.keywordCasing` | `"Upper"` | `Upper` / `Lower` / `Preserve`. |
| `cyrs.formatting.trailingCommas` | `"AsNeeded"` | `Always` / `AsNeeded` / `Never`. |
| `cyrs.formatting.indentStyle` | `"Spaces"` | `Spaces` / `Tabs`. |
| `cyrs.formatting.indentWidth` | `2` | Spaces per level. |
| `cyrs.watchedFilesDebounceMs` | `250` | Clamped to `[0, 5000]`. |

Schema/dialect/formatter changes restart the server automatically.

## Commands

| Command | ID |
| ------- | -- |
| cyrs: Restart Language Server | `cyrs.restartServer` |

## Screenshots

_TODO once captured by the maintainer._

```
![hover](docs/hover.png)
![diagnostics](docs/diagnostics.png)
![rename](docs/rename.png)
```

## Development

```sh
cd editors/vscode
npm install
npm run watch        # incremental tsc
# In another terminal: open the `editors/vscode` folder in VS Code,
# press F5 to launch an Extension Development Host with the extension loaded.
```

Open any `.cyp` / `.cypher` / `.gql` file in the dev host. With the
language server on `$PATH`, hover/diagnostics/completion light up
immediately.

## License

Dual-licensed under Apache-2.0 OR MIT, matching the cyrs workspace.
