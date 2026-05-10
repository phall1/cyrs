# Spec 0004 — Interop Surfaces

| Field          | Value                                                                 |
| -------------- | --------------------------------------------------------------------- |
| Status         | Accepted                                                              |
| Owner          | phall                                                                 |
| Last locked    | 2026-04-22                                                            |
| Supersedes     | —                                                                     |
| Superseded by  | —                                                                     |

---

## 0. TL;DR

Cyrs exposes a single Rust library surface plus a single wire protocol today.
This spec widens the crate graph to carry the library into three foreign
runtimes — WebAssembly (`cyrs-wasm`), a stable C ABI (`cyrs-ffi`), and
Python (`cyrs-py`) — and pins stability commitments for each so downstream
consumers can depend on them without waiting for a 1.0. The `tree-sitter-cypher`
grammar lives alongside as a sibling subproject, held to a parity contract
against the core lexer/parser. LSP is extended to run in-browser via a
`web-lsp` feature on `cyrs-lsp`. Every new surface is a **thin adapter**:
zero analysis logic lives outside the Rust crates enumerated in spec 0001 §3.1.

---

## 1. Motivation

The epic `cy-od5` exists because Cyrs's reach is bounded by what Rust alone can
serve. A browser-based editor (Monaco console, a hosted query playground, a
GitHub-style web IDE) cannot link a Rust `cdylib`; an embedder written in C,
Go, Zig, or Swift will not pick up a Rust crate; a data-science notebook
reaches first for `pip install`. The three new crates close those gaps with
thin adapters that expose the existing `cyrs-lang-services` engine — no
functionality is reimplemented, only re-surfaced.

Tree-sitter is the second motivation. Editor integrations (Neovim, Helix,
Zed) have standardised on tree-sitter for syntax highlighting and structural
selection; the `tree-sitter-cypher` grammar gives every such editor usable
Cypher support for free. Keeping it inside this repository lets the parity
harness assert tree-sitter and `cyrs-syntax` agree on well-formedness.

LSP-in-browser (`cyrs-lsp` with `web-lsp`) is the smallest addition: it
reuses the WASM toolchain set up for `cyrs-wasm` and lets the demo page
swap between the agent wire protocol and full LSP without a native binary.

---

## 2. Non-Coupling Contract — amendments

Spec 0001 §2 (Non-Coupling Contract) is unchanged; this spec adds concrete
confirmations that the new surfaces do not breach it.

**2.1. §2.C3 ("no overlay crates") still holds.** `cyrs-wasm`, `cyrs-ffi`,
and `cyrs-py` are genuine interop layers — each wraps the existing
`cyrs-lang-services` engine and exposes it in a foreign runtime's idiom.
None of them carries domain knowledge, none adds analysis logic, and none
re-exports a consumer-specific variant of a Cypher type. The thin-shell rule
spec 0001 §3.1 applies to binaries is lifted verbatim to these three new
adapter crates: if you find yourself writing a parse, semantic, or format
pass inside them, the logic belongs upstream in a library crate.

**2.2. §2.C2 (denylist) applies unchanged.** The new crates are included in
the CI grep sweep. Foreign-language wrappers and test fixtures inherit the
rule; the denylist is language-agnostic and covers `.rs`, `.ts`, `.py`, and C
header strings produced by cbindgen.

---

## 3. Crate Graph — amendment to spec 0001 §3.1

Three new crates join the authoritative table:

| Crate          | Purpose                                                                   | Depends on                                                                      |
| -------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `cyrs-wasm`  | `wasm32-unknown-unknown` binding: agent wire protocol as JS callables     | `cyrs-lang-services`, `cyrs-db`, `cyrs-diag`, `cyrs-fmt`, `wasm-bindgen`, `serde-wasm-bindgen` |
| `cyrs-ffi`   | `cdylib` + `staticlib` exporting a stable C ABI + cbindgen-generated header | `cyrs-lang-services`, `cyrs-db`, `cyrs-diag`, `cyrs-fmt`                |
| `cyrs-py`    | PyO3 extension module shipped as a `maturin`-built wheel                  | `cyrs-lang-services`, `cyrs-db`, `cyrs-diag`, `cyrs-fmt`, `pyo3`        |

**Thin-adapter rule.** Each of the three new crates is a pure binding layer.
No parser call, no HIR construction, no sema pass, no format routine runs
inside them — they translate foreign-runtime request shapes into calls on
`cyrs-lang-services` and translate results back. Spec 0001 §3.1's
thin-shell clause on binary crates applies to these three in full. A clippy
lint on `unused_crate_dependencies` plus a CI grep for `cyrs_ast::` and
`cyrs_syntax::` inside these three crates keeps the rule honest.

**Tree-sitter is not a crate.** `tree-sitter-cypher/` is a sibling subproject
rooted next to `crates/`, with its own `package.json` and JavaScript grammar.
It does not appear in the workspace member list, is not published to
crates.io, and has no Rust dependencies on Cyrs. The parity harness
(§8 below) lives in `xtask` and exercises the grammar from the Rust side.

Cross-reference: every dep listed above is already reachable via spec 0001
§3.1's allowed edges (`cyrs-lang-services` depends on the library trunk),
so adding these three adapters does not widen the core graph.

---

## 4. WASM surface (`cyrs-wasm`)

**4.1. Bindgen strategy.** `wasm-bindgen` generates the JS↔Rust boundary;
`serde-wasm-bindgen` carries the agent wire types across as idiomatic
JavaScript objects rather than JSON strings. Every public export is a
`pub fn` on a single `CypherDatabase` JS class, mirroring the agent v1 op
surface (`parse`, `check`, `complete`, `hover`, `format`, `rewrite`, `plan`,
`explain`, `schema_set`, `schema_clear`). Types flow through the existing
serde derives on the agent crate's request/response structs — no bespoke JS
shapes.

**4.2. Size budget.** The brotli-compressed artifact (after `wasm-opt -Os`)
must be ≤ 2 MB. A CI size gate runs `brotli -q 11` against the optimised
`.wasm` and fails the build if the threshold is breached. Budget rationale:
2 MB is the envelope a hosted playground can fetch on a warm CDN without a
loading spinner; going above it shifts the library from "drop-in" to
"opt-in".

**4.3. JS API versioning.** The agent wire protocol's `proto_version: u32`
field (sealed by `cy-2i9`) is the sole versioning axis. `cyrs-wasm`
exports a `protoVersion()` function; the JS wrapper refuses to talk to a
runtime whose proto version does not match its own expected value. JS API
shape churn inside a single proto version is permitted (additive only);
breaking shape changes require a proto version bump, which is itself a
spec-governed action. See spec 0001 §15 for the agent wire contract and
`docs/stability.md` for the cross-surface stability table.

---

## 5. C FFI surface (`cyrs-ffi`)

**5.1. ABI scope.** Exports cover the agent v1 op surface reduced to the
seven C-representable entrypoints: `cypher_parse`, `cypher_check`,
`cypher_complete`, `cypher_hover`, `cypher_format`, `cypher_rewrite`,
`cyrs_plan`. Each takes a `CypherDatabase*` handle and a UTF-8 input;
each returns a Rust-allocated result struct plus a status code. Diagnostics
are surfaced as opaque `CypherDiagnostic*` pointers with getters for code,
severity, span, message. No `EXISTS { ... }`, `CALL { ... }`, or other §9.3
Neo4j-current features appear in the ABI.

**5.2. Memory-ownership contract.** All outputs are allocated Rust-side and
must be freed by the caller via the matching `cypher_free_*` function.
The header (§5.6) documents each pairing; mixing allocators is undefined.
Input buffers are borrowed — the caller retains ownership for the duration
of the call. Strings cross the boundary as NUL-terminated UTF-8 in
Rust-owned storage; callers copy before freeing.

**5.3. Panic policy.** Every exported `extern "C"` function wraps its body
in `std::panic::catch_unwind`. On panic, the function returns a sentinel
error code and stashes a thread-local string readable via
`cypher_last_error()`. No panic crosses the FFI boundary; a unit test
exercises every exported fn under an induced panic path to hold the
invariant.

**5.4. `unsafe_code` waiver.** Spec 0001 §17.13 sets
`#![forbid(unsafe_code)]` workspace-wide. `cyrs-ffi` is granted a
crate-local exception — `unsafe` is required for raw-pointer handling at
the ABI. The waiver is justified in `cyrs-ffi/README.md`, enforced with
a crate-level `#![deny(unsafe_op_in_unsafe_fn)]`, and every `unsafe` block
carries a `// SAFETY:` comment (rust-analyzer house style). The waiver
does not extend to any other crate.

**5.5. ABI stability.** Once a symbol ships in a tagged release, its
signature and behaviour are stable for the life of the major version. New
symbols may be added between minors; removal or signature change requires a
major bump and is called out in `CHANGELOG.md`. `cargo-semver-checks`
catches Rust-side churn; a cbindgen diff check (§5.6) catches C-side churn.

**5.6. Header check.** `cargo xtask cbindgen` regenerates
`cyrs-ffi/include/cypher.h`; `--check` mode asserts the committed header
matches regeneration byte-for-byte and fails the gate otherwise. The
mechanic mirrors the AST codegen check (spec 0001 §5).

---

## 6. Python surface (`cyrs-py`)

**6.1. Binding choice.** `cyrs-py` binds `cyrs-lang-services` directly
via PyO3; it does **not** go through `cyrs-ffi`. Rationale: PyO3 handles
error conversion, GIL management, and type coercion natively, and routing
through the C ABI would force a double serialisation and lose Rust-level
type fidelity. The two adapter crates can ship independently.

**6.2. Wheel matrix.** `maturin build --release` produces wheels across
`{linux, macos, windows} × {x86_64, aarch64} × CPython {3.10, 3.11, 3.12, 3.13}`.
That is 24 wheels per release; CI caches `maturin` builds per OS and runs the
matrix in parallel. PyPy is opportunistic — if a build succeeds it ships,
but PyPy failure does not gate the release.

**6.3. Type stubs.** `cyrs-py` ships a hand-written `cypher.pyi` alongside
the wheel. Every PyO3-exported class, method, and enum appears in the stub;
a `pytest` gate imports the stub against the built wheel via `mypy
--strict` and fails CI on drift. Enums that correspond to
`#[non_exhaustive]` Rust types carry a `# non-exhaustive` comment so
downstream users do not exhaustively match on them. Stub churn follows the
Rust API's SemVer (§9.3).

**6.4. Package name.** Placeholder is `cypher` on PyPI; v1 publish is
deferred to `cy-zgz` (release playbook) — see §12 below. The open question
in §13 covers fallback name (`cypher-frontend`) if `cypher` is taken.

---

## 7. LSP-Web transport

**7.1. Feature gate.** `cyrs-lsp` gains a `web-lsp` Cargo feature. When
enabled with `--target wasm32-unknown-unknown`, the stdio transport is
replaced by a message-channel transport compatible with `MessageChannel`
and `Worker.postMessage`. The default (native) build is unchanged: stdio +
TCP as today.

**7.2. Message-channel transport.** The transport layer is a pair of
`postMessage`-compatible ports: one for LSP requests inbound to the server
worker, one for notifications and responses outbound. Messages are JSON —
the same shape the stdio transport carries — so the LSP protocol wire
contract is untouched. Clients built for stdio `cyrs-lsp` can reuse their
`lsp-types` integration against the web build by swapping the transport
adapter only.

**7.3. Feature parity.** The web build exposes the identical LSP feature
set as the stdio build: completion, hover, go-to-definition, find-references,
diagnostics, formatting, rename, document symbols. Feature-gating hides the
`lsp-server` stdio wiring but does not gate any analysis path; logic parity
falls out of the thin-shell rule (spec 0001 §3.1).

---

## 8. Tree-Sitter parity contract

**8.1. Scope.** The parity contract covers the v1 TCK surface (spec 0001
§19). For every TCK scenario classed `ok`, the tree-sitter grammar must
produce a tree with zero `ERROR` nodes. For every scenario classed
`error`, the grammar must produce a tree with at least one `ERROR` node
in the spans the parser rejects. Broader semantic equivalence — node
labels, tree shape — is explicitly not asserted; tree-sitter and
`cyrs-syntax` use different grammar models, and locking node shapes
would freeze the grammar against upstream tree-sitter improvements.

**8.2. Harness.** `cargo xtask tree-sitter-parity` runs the harness across
every TCK scenario in scope. The gate is green when 100% of scenarios
satisfy the ok/error polarity above. Failures print the offending scenario
ID, the expected polarity, and the observed ERROR-node count. A CI job
runs this on every PR that touches either `cyrs-syntax/` or
`tree-sitter-cypher/`.

**8.3. Highlight queries.** `tree-sitter-cypher/queries/highlights.scm`,
`locals.scm`, and `injections.scm` are held to the tree-sitter org's
canonical capture names so Neovim, Helix, and Zed get usable colouring
out of the box. Queries are hand-authored; a `tree-sitter test`
round-trip in CI exercises them against the corpus.

---

## 9. Stability commitments

**9.1. Wire protocols.** The agent JSON wire protocol and the LSP wire
protocol inherit the stability policy from `cy-2i9` and
`docs/stability.md`. `proto_version` gates breaking changes to the agent
wire; LSP extensions are flagged "unstable" in the stability doc until
promoted. `cyrs-wasm` and LSP-Web (§7) reuse these rails verbatim.

**9.2. C ABI.** Once a symbol ships in a tagged release, it is stable for
the life of the major version: signature frozen, semantics frozen,
return-code meanings frozen. New symbols may be added in minor releases.
Symbol removal or signature change requires a major bump and a
`CHANGELOG.md` entry in `cyrs-ffi/`.

**9.3. Python API.** `cyrs-py` follows PEP 440 SemVer. Public classes
and methods exported from PyO3 are frozen within a major. Enums over
`#[non_exhaustive]` Rust types (spec 0001 §10 diagnostic codes,
`cyrs-hir::Expr`, etc.) may grow variants inside a minor; the `cypher.pyi`
stub tracks those with non-exhaustive markers so downstream `match`
statements stay forward-compatible. Type-stub changes follow the same
SemVer the wheel does.

**9.4. Tree-sitter grammar.** `tree-sitter-cypher` carries its own SemVer
independent of the Rust workspace. The parity harness (§8) holds the
"ok/error polarity" invariant; everything else — node labels, tree shape,
highlight queries — may churn at the grammar's own pace. A grammar
release that breaks parity is blocked by the CI gate, not by a spec
amendment.

---

## 10. Testing

**10.1. WASM.** A puppeteer-headless round-trip test loads the compiled
`.wasm` in a headless Chromium, instantiates the `CypherDatabase` JS
class, and exercises every agent op against a fixture query. The test
asserts (a) the wire round-trip succeeds, (b) `proto_version` matches,
(c) p95 latency on a 1k-line buffer is under 100 ms. Runs on every PR
that touches `cyrs-wasm/` or its upstream deps.

**10.2. FFI.** One Go program and one Node.js program live under
`demo/ffi/` and dlopen the produced `libcypher.{so,dylib}` (or
`cypher.dll` on Windows). A CI smoke test builds the dylib, builds the
Go and Node examples, runs each against a fixture, and asserts
diagnostics match a checked-in golden. A catch_unwind unit test suite
hits every `extern "C"` fn with an induced panic path to hold §5.3.

**10.3. Python.** `pytest` under `crates/cyrs-py/tests/` exercises
every exported class and method against fixtures. A wheel-install smoke
test builds the current-OS wheel, `pip install`s it into a fresh venv,
imports `cypher`, and runs a tiny script — catches packaging regressions
the in-tree tests miss. `mypy --strict` validates `cypher.pyi` against
the built wheel.

**10.4. LSP-Web.** The existing `lsp-types` conformance harness runs a
second time with the web-transport adapter in front of it. Protocol
messages are identical, so the harness is reused verbatim; only the
transport injection changes.

---

## 11. CI matrix additions

- **wasm32 target.** Rust toolchain on every CI runner gains
  `wasm32-unknown-unknown`. `wasm-bindgen-cli`, `wasm-opt`, and `brotli`
  are cached as pre-built binaries.
- **cbindgen.** `cargo install cbindgen --locked` on the Linux runner; the
  `xtask cbindgen --check` job runs on every PR that touches
  `cyrs-ffi/`.
- **maturin per OS.** Separate runners for Linux (manylinux2014),
  macOS (universal2), and Windows. Each builds the wheel matrix for that
  OS × CPython 3.10–3.13 in parallel. Wheels are uploaded as CI artifacts
  (publishing is deferred to §12).
- **Node + puppeteer.** A single Linux runner hosts the WASM
  round-trip test with `node`, `puppeteer-headless`, and a pinned
  Chromium.
- **tree-sitter CLI.** Node + `tree-sitter-cli` on the Linux runner for
  the parity harness and highlight query smoke tests.

---

## 12. Deferred

- **crates.io publishing** of `cyrs-wasm`, `cyrs-ffi`, `cyrs-py` is
  deferred to the release playbook in `cy-zgz` (closed; see
  `docs/release-playbook.md`). Until publish, the wheels and `.wasm`
  artifacts ship as GitHub release assets only.
- **PyPI publishing** of `cyrs-py` is deferred to `cy-zgz`. Package
  name reservation is covered by §13 below.
- **npm publishing** of the JS wrapper around `cyrs-wasm` is deferred —
  the demo page consumes the wrapper in-tree until a user asks for it.
- **tree-sitter-org repo move.** The grammar lives in this repo until
  `tree-sitter-cypher` graduates to the tree-sitter org; move is a
  `cy-zgz` follow-up.
- **`grammar.js` generation from `cypher.ungrammar`** (cf. §13) is a
  stretch goal; v1 holds the hand-authored grammar and the parity harness
  as the invariant.

---

## 13. Open questions

- **PyPI package name.** Is `cypher` available on PyPI? If not, fall
  back to `cypher-frontend` or `cypher-lang`? Decision required before
  `cy-zgz` publish.
- **LSP-Web as its own crate.** If the `web-lsp` feature's transport
  shim grows past ~200 LOC, promote it to a sibling `cyrs-lsp-transport`
  crate (see spec 0001 §3.1 thin-shell precedent). Revisit after
  `cy-od5.5` has landed an initial implementation.
- **Grammar generation from ungrammar.** Can `cyrs-ast/cypher.ungrammar`
  be lowered to a seed `tree-sitter-cypher/grammar.js` via an
  `xtask tree-sitter-gen` step? Would reduce hand-maintenance cost but
  requires an ungrammar→EBNF lowering that preserves recovery points.
  Investigate during `cy-od5.6` follow-up.
