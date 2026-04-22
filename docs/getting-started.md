# Getting started

Five-minute quickstart. Install the CLI, run it against a real query,
and wire `cypher-lsp` into your editor. No prior Cypher / GQL tooling
required.

---

## 1. Install

Pick one.

### Cargo (Rust users)

```sh
cargo install --locked cypher-cli
```

This installs the `cypher` binary on `$PATH`. `--locked` respects the
crate's pinned `Cargo.lock` so your install matches the CI-tested build.

### From source

```sh
git clone https://github.com/phall1/cyrs
cd cyrs
cargo build --release -p cypher-cli
cp target/release/cypher ~/.local/bin/
```

The release binary has no runtime dependencies. MSRV is `1.94` (see
[`rust-toolchain.toml`](../rust-toolchain.toml)).

Verify:

```sh
cypher --version
```

---

## 2. Your first query

Create a `.cyp` file with a deliberate syntax error:

```sh
cat > bad.cyp <<'EOF'
MATCH (n RETURN n
EOF
```

Run `check`:

```sh
cypher check bad.cyp
```

Output:

```
error[E0011]: expected ')' to close node pattern
  ┌─ bad.cyp:1:10
  │
1 │ MATCH (n RETURN n
  │          ^
```

Exit code `1` — `cypher check` exits non-zero whenever any
error-severity diagnostic fires (spec 0001 §16). `E0011` is a stable
diagnostic code; once assigned it never changes meaning.

---

## 3. Format a query

Create an unformatted file:

```sh
cat > messy.cyp <<'EOF'
match (n) return n
EOF
```

Format it:

```sh
cypher fmt messy.cyp
```

Output:

```
MATCH (n)
RETURN n
```

`cypher fmt` writes to stdout by default. Use `--in-place` / `-i` to
rewrite the file. `cypher fmt --check` exits `1` if the file is not
already formatted — useful as a pre-commit gate.

The formatter is idempotent (`fmt(fmt(x)) == fmt(x)`) and round-trips
through the parser.

---

## 4. Project mode (cross-file analysis)

For anything bigger than a single file, declare a project manifest.
The CLI ships with a worked fixture at
[`crates/cypher-cli/tests/workspace/`](../crates/cypher-cli/tests/workspace/).

Layout:

```
my-project/
├── cypher-project.toml
├── schema.toml
└── samples/
    ├── people.cyp
    └── movies.cyp
```

`cypher-project.toml` (spec 0003):

```toml
[project]
name = "demo"

[project.dialect]
default = "GqlAligned"

[project.members]
include = ["samples/*.cyp"]

[project.schema]
path = "schema.toml"
```

`schema.toml` (spec 0002):

```toml
[[label]]
name = "Person"
properties = [{ name = "name", type = "STRING", required = true }]
```

`samples/people.cyp`:

```cypher
MATCH (p:Person) RETURN p.name
```

`samples/movies.cyp`:

```cypher
MATCH (m:Movie) RETURN m.title
```

Run at the project root:

```sh
cypher check .
```

Output:

```
error[E3001]: unknown label `:Movie`
  ┌─ samples/movies.cyp:1:6
  │
1 │ MATCH (m:Movie) RETURN m.title
  │      ^^^^^^^^^^

checked 2 files in project 'demo': 1 diagnostic
```

The workspace loader discovers `cypher-project.toml` at or above the
given directory, installs the shared schema, and runs analysis against
every member file. `Movie` is missing from `schema.toml`, so sema
raises `E3001` (unknown label).

---

## 5. LSP — Neovim

Build the language server and open Neovim with the demo init:

```sh
cargo build --release -p cypher-lsp
nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
```

Within a second or two you'll see a red virtual-text diagnostic on the
unclosed paren. Hover (`K`), goto-definition (`grd`), references
(`grr`), rename (`grn`), completion (`<C-x><C-o>`), and format-on-save
are all wired. No plugins required; the init uses Neovim 0.11's
built-in LSP client.

Full walkthrough: [`demo/README.md`](../demo/README.md). Demo
recording: [`demo/demo.gif`](../demo/demo.gif).

---

## 6. LSP — Helix / VS Code

`cypher-lsp` is a vanilla LSP server — every client that speaks LSP
over stdio works. The exact binary path below assumes
`cargo build --release -p cypher-lsp`; substitute `~/.cargo/bin/cypher-lsp`
when installing via `cargo install`.

### Helix (`~/.config/helix/languages.toml`)

```toml
[language-server.cypher-lsp]
command = "cypher-lsp"

[[language]]
name = "cypher"
scope = "source.cypher"
file-types = ["cyp", "cypher"]
roots = ["cypher-project.toml"]
comment-token = "//"
language-servers = ["cypher-lsp"]
auto-format = true
```

### VS Code

Add a minimal client extension or use
[`vscode-languageclient`](https://github.com/microsoft/vscode-languageclient-node).
The server command and transport:

```json
{
  "command": "cypher-lsp",
  "transport": "stdio",
  "documentSelector": [{ "scheme": "file", "language": "cypher" }]
}
```

No custom capabilities required; declare `cypher` as a language in
`package.json` (`"aliases": ["Cypher"], "extensions": [".cyp", ".cypher"]`).

---

## 7. Agent JSON — for tools and scripts

`cypher-agent` exposes the same analysis pipeline over line-delimited
JSON on stdio. One request per line, one response per line, no network,
no filesystem writes (spec 0001 §15).

```sh
echo '{"op":"check","text":"MATCH (n) RETURN n"}' | cypher-agent
```

Output:

```json
{"op":"check","diagnostics":[]}
```

Error path:

```sh
echo '{"op":"check","text":"MATCH (n RETURN n"}' | cypher-agent
```

Output:

```json
{"op":"check","diagnostics":[{"code":"E0011","fixes":[],"labels":[],"message":"expected ')' to close node pattern","notes":[],"primary":{"caption":"","range":[9,9]},"related":[],"severity":"error"}]}
```

Ops: `parse`, `check`, `complete`, `hover`, `format`, `rewrite`,
`plan`, `explain`, `schema_set`, `schema_clear`, `shutdown`. The
request field is `text` (not `source`); see
[`crates/cypher-agent/src/main.rs`](../crates/cypher-agent/src/main.rs)
for the full wire table.

---

## 8. What's next

- [Interop bindings](interop.md) — WASM, C FFI, Python, tree-sitter,
  LSP-Web.
- [Architecture](architecture.md) — crate graph and pipeline overview.
- [Diagnostic-code reference](diagnostics.md) — every stable code.
- [Performance and benchmarks](performance.md) — criterion results,
  size budgets.
- [Contributing](../CONTRIBUTING.md) — commit policy, bead workflow,
  pre-commit gate.
- [Spec 0001](specs/0001-cypher-frontend.md) — the locked architectural
  specification.
