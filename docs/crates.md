# Crates

Workspace layout. The crate graph and allowed-edges list is normative
in [spec 0001 §3](./specs/0001-cypher-frontend.md); the table below is
the human-readable index.

| Crate            | Purpose                                                      |
| ---------------- | ------------------------------------------------------------ |
| `cyrs-syntax`        | Lexer, recovering parser, lossless CST, `SyntaxKind`     |
| `cyrs-ast`           | Typed AST wrappers over the CST                          |
| `cyrs-hir`           | Lowered HIR, name resolution, scope graph, desugaring    |
| `cyrs-sema`          | Semantic analysis + type system                          |
| `cyrs-schema`        | `SchemaProvider` trait + supporting types                |
| `cyrs-diag`          | Diagnostic type, stable code registry, rendering         |
| `cyrs-plan`          | Logical read / write plan IR                             |
| `cyrs-fmt`           | CST-driven formatter                                     |
| `cyrs-db`            | Salsa-based incremental analysis database                |
| `cyrs-lang-services` | Shared completion / hover / rewrite engines              |
| `cyrs-lsp`           | Language server binary                                   |
| `cyrs-agent`         | JSON-over-stdio agent API binary                         |
| `cyrs-cli`           | `cypher {parse,check,fmt,explain,plan,schema …}`         |
| `cyrs-project`       | `cypher-project.toml` workspace manifest reader          |
| `cyrs-tck`           | openCypher / GQL TCK harness                             |
| `cyrs-testkit`       | Shared test fixtures, compiletest runner (dev only)      |
| `cyrs-wasm`          | WASM + Monaco bindings                                   |
| `cyrs-ffi`           | Stable C ABI + cbindgen                                  |
| `cyrs-py`            | PyO3 wheel                                               |
| `cyrs-lang`          | Meta-crate re-exporting the library surface              |

Conceptual groupings:

- **Pipeline layers**: `cyrs-syntax`, `cyrs-ast`, `cyrs-hir`,
  `cyrs-sema`, `cyrs-plan` — see [`overview.md`](./overview.md) and
  per-layer pages in [`concepts/`](./concepts).
- **Cross-cutting**: `cyrs-schema`, `cyrs-diag`, `cyrs-fmt`,
  `cyrs-project`.
- **Services**: `cyrs-db`, `cyrs-lang-services`, `cyrs-lsp`,
  `cyrs-agent`, `cyrs-cli` — see
  [`concepts/services.md`](./concepts/services.md).
- **Interop**: `cyrs-wasm`, `cyrs-ffi`, `cyrs-py` — see
  [spec 0004](./specs/0004-interop-surfaces.md).
- **Testing**: `cyrs-tck`, `cyrs-testkit`.
- **Top-level surface**: `cyrs-lang`.
