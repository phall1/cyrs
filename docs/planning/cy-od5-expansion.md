# cy-od5 Expansion Plan — Interop (WASM + C FFI + PyO3 + tree-sitter + LSP-WASM)

**Discovery note:** `cy-od5.1` (tree-sitter grammar v0) is already CLOSED — `tree-sitter-cypher/` exists at repo root with grammar.js, package.json, test/corpus, and a parity harness against `cypher-tck/tck/v1.toml`. `cy-2i9.1` (non_exhaustive + semver-checks CI + `docs/stability.md`) is also CLOSED. The remaining four interop surfaces (WASM, FFI, PyO3, LSP-WASM) are what this expansion covers, plus one tree-sitter follow-up bead.

---

## 1. Child bead catalog

### cy-od5.2 — `cypher-wasm` crate + Monaco demo page
- **Title:** Agent JSON protocol compiled to `wasm32-unknown-unknown`, with a minimal JS wrapper and a Monaco demo.
- **Scope:**
  - New `crates/cypher-wasm` binding `cypher-lang-services` + `cypher-db` + `cypher-diag` + `cypher-fmt` via `wasm-bindgen`.
  - Exposes the agent v1 op surface (parse/check/complete/hover/format/rewrite/plan/explain/schema_set/schema_clear) as JS callables, reusing the agent's serde types — no new logic.
  - `demo/web/` page loads Monaco + `cypher-wasm`, wires `onDidChangeModelContent` → `check`, renders diagnostics as markers. Target p95 < 100 ms on a 1k-line buffer.
  - Brotli-compressed artifact size ≤ 2 MB. Size gate in CI.
- **Labels:** `crate:cypher-wasm`, `interop`, `tier-17`. **Adds new crate → serialises on root Cargo.toml.**
- **Depends on:** spec amendment bead (0004 see §3), `cy-2i9` closure, agent crate (exists). No dep on FFI or Py.
- **Size:** L (wasm-bindgen setup + size discipline + demo page + CI wasm toolchain).
- **Spec §:** new spec 0004 §§ WASM, Agent-Wire-Stability. `needs-spec-amendment`.
- **Priority:** 2.
- **Acceptance:**
  - `cargo build -p cypher-wasm --target wasm32-unknown-unknown --release` green.
  - `wasm-opt -Os` + brotli artifact ≤ 2 MB; size check in CI fails if breached.
  - Demo page serves locally; puppeteer harness verifies round-trip `check` latency < 100 ms on the reference corpus.
  - No `unsafe_code` in first-party crate (workspace invariant §17.13).

### cy-od5.3 — `cypher-ffi` crate + `cbindgen` header
- **Title:** Stable C ABI exporting parse/hover/complete/rewrite from `cypher-lang-services`.
- **Scope:**
  - New `crates/cypher-ffi` with `cdylib` + `staticlib` crate types. One `unsafe` boundary; body of every export is a thin adapter that calls `cypher-lang-services` and returns opaque `CypherDiagnostic*`, `CypherCompletion*`, etc.
  - `xtask cbindgen` regenerates `include/cypher.h`; check-mode asserts committed header matches regen (rust-analyzer/Biome pattern).
  - Errors surface via `cypher_last_error()` (thread-local); ABI never panics (panics are caught and converted).
  - Memory ownership rule: all outputs are allocated Rust-side, freed via `cypher_free_*`. Documented in header.
  - `#![forbid(unsafe_code)]` is waived only for this crate; justified in `cypher-ffi/README.md` and noted in spec 0004.
- **Labels:** `crate:cypher-ffi`, `interop`, `tier-17`, `abi-commitment`. **Adds new crate → serialises on root Cargo.toml.**
- **Depends on:** 0004 spec amendment, `cy-2i9` (hard — ABI must not churn after v1 consumers depend on it).
- **Size:** L.
- **Spec §:** new spec 0004 §§ FFI, ABI-Stability-Policy, Memory-Contract. `needs-spec-amendment`.
- **Priority:** 3 (blocks on stability).
- **Acceptance:**
  - `cargo build -p cypher-ffi` produces `libcypher.{so,dylib,a}` and `cypher.dll` matrix on CI.
  - `cargo xtask cbindgen --check` green.
  - Integration smoke: one Go and one Node example in `demo/ffi/` link the dylib and call `cypher_check` on a sample query.
  - No panic escapes the ABI: catch_unwind harness test covers every exported fn.

### cy-od5.4 — `cypher-py` PyO3 wheel via maturin
- **Title:** Pythonic bindings built on `cypher-lang-services` directly (not via FFI).
- **Scope:**
  - New `crates/cypher-py` (extension module), built with `maturin`. `pyproject.toml` targets CPython 3.10–3.13, PyPy optional.
  - Python API mirrors the agent ops as methods on a `CypherDatabase` class: `.parse(text)`, `.check(text)`, `.complete(text, offset)`, etc. Diagnostics are dataclasses with `.code`, `.severity`, `.range`.
  - CI uses `maturin build --release` on linux/macos/windows × x86_64/aarch64; wheels uploaded as artifacts (no publish yet — gated by `cy-zgz`).
  - `pytest` suite in `crates/cypher-py/tests/` exercises every op.
  - Package name placeholder: `cypher` on PyPI (verify availability before publish bead).
- **Labels:** `crate:cypher-py`, `interop`, `tier-17`. **Adds new crate → serialises.**
- **Depends on:** 0004 spec amendment. Does **not** depend on cy-od5.3; goes direct to `cypher-lang-services`. Depends on `cy-2i9` for API stability.
- **Size:** M.
- **Spec §:** new spec 0004 §§ Python-Bindings, Wheel-Matrix. `needs-spec-amendment`.
- **Priority:** 3.
- **Acceptance:**
  - `maturin develop` in a fresh venv yields a working `import cypher`.
  - pytest green across 3.10–3.13.
  - Wheel matrix artifact < 5 MB per platform.
  - Stub file (`cypher.pyi`) shipped; mypy clean against it.

### cy-od5.5 — `cypher-lsp` WASM build (`lsp-web-server`)
- **Title:** Swap `lsp-server` for a `wasm32-unknown-unknown`-compatible web-server transport so `cypher-lsp` runs in-browser.
- **Scope:**
  - Feature flag `web-lsp` on `cypher-lsp` crate. Under the flag, swap the `lsp-server` stdio transport for a message-channel transport (postMessage or similar). **No new crate**: stays as a feature inside existing `cypher-lsp` (thin-shell rule §3.1 preserved). If the transport code is too fat, extract into a sibling `cypher-lsp-transport` — but the default path is feature-gated within `cypher-lsp`.
  - Hook the Monaco demo (cy-od5.2) up to consume LSP over the wasm channel, as an alternative mode to the direct agent-JSON call path.
- **Labels:** `crate:cypher-lsp`, `interop`, `tier-17`. **No new crate.** Still touches root Cargo.toml only if we add optional deps for `web-lsp` — likely yes, so **treat as serialising**.
- **Depends on:** cy-od5.2 (wasm toolchain + demo page exist), 0004 spec amendment for LSP-web transport naming.
- **Size:** M–L.
- **Spec §:** 0004 §§ LSP-Web-Transport.
- **Priority:** 3.
- **Acceptance:**
  - `cargo build -p cypher-lsp --features web-lsp --target wasm32-unknown-unknown` green.
  - Demo page has a toggle: "agent-wasm" vs "lsp-wasm"; both produce identical diagnostics for the corpus.
  - Artifact size budget: combined wasm ≤ 3 MB brotli (LSP adds types).

### cy-od5.6 — tree-sitter grammar v1 hardening (follow-up to cy-od5.1)
- **Title:** Close known gaps in `tree-sitter-cypher/grammar.js` vs the full spec §19 matrix; publish to the tree-sitter org.
- **Scope:**
  - Expand grammar to cover the full openCypher v9 + GQL-aligned surfaces passed by cypher-syntax that aren't in the v0 scope listed in tree-sitter-cypher/README.md (e.g., `CALL <proc> YIELD`, `UNION`/`UNION ALL`, `shortestPath`/`allShortestPaths`, map projection, list comprehension inner syntax, pattern predicates).
  - Consider a one-shot `xtask tree-sitter-gen` that lowers `cypher-ast/cypher.ungrammar` to a seed `grammar.js` for future regeneration (stretch; not required).
  - Add semantic-tokens-grade highlight queries (`queries/highlights.scm`, `locals.scm`, `injections.scm`) so Neovim/Helix/Zed get usable colouring.
  - Publish: repo move to `tree-sitter/tree-sitter-cypher` with appropriate README + license.
- **Labels:** `crate:tree-sitter` (no Rust crate — tree-sitter subtree), `interop`, `tier-17`. **No root Cargo.toml edit.**
- **Depends on:** nothing blocking; can run in parallel with WASM/FFI work (different file tree, no workspace churn).
- **Size:** M.
- **Spec §:** 0004 §§ Tree-Sitter-Parity (document parity contract, doesn't change crate graph).
- **Priority:** 2.
- **Acceptance:**
  - Parity harness green for the full v1 TCK, not just v0 scope.
  - `queries/highlights.scm` validates in Neovim + Helix.
  - PR opened to tree-sitter org (acceptance is the PR, not the merge).

### cy-od5.7 — Spec 0004: Interop Surfaces (prerequisite)
- **Title:** Draft spec 0004 unlocking the three new crates and defining ABI / wire-stability commitments.
- **Scope:** Write `docs/specs/0004-interop-surfaces.md` with sections listed in §3 below. Gate on operator approval.
- **Labels:** `docs:spec`, `tier-17`. **No code / no Cargo.toml edit.**
- **Depends on:** cy-2i9 (stability policy must exist before spec pins ABI).
- **Size:** S–M.
- **Priority:** 1 — blocks cy-od5.2, .3, .4, .5.
- **Acceptance:** spec 0004 landed, status "Accepted", AGENTS.md §3.1 crate graph amendment cross-referenced.

---

## 2. Parallelisation matrix

Root `Cargo.toml` `[workspace.members]` edits **serialise** (AGENTS.md §4.2). Adding each new crate is one such edit.

| Bead      | New crate? | Root `Cargo.toml` edit? | Can run in parallel with                                                 |
|-----------|------------|-------------------------|--------------------------------------------------------------------------|
| cy-od5.7 (spec) | no      | no                      | anything                                                                 |
| cy-od5.2 (wasm)   | yes  | **yes — SERIALISES**    | cy-od5.6 (tree-sitter), cy-od5.7 (spec)                                 |
| cy-od5.3 (ffi)    | yes  | **yes — SERIALISES**    | cy-od5.6, cy-od5.7; NOT with .2, .4                                     |
| cy-od5.4 (py)     | yes  | **yes — SERIALISES**    | cy-od5.6, cy-od5.7; NOT with .2, .3                                     |
| cy-od5.5 (lsp-wasm) | no† | optional dep add (light)| cy-od5.6; scheduled after .2 so demo page is already up                 |
| cy-od5.6 (ts hardening) | no | no                   | anything (lives outside workspace tree)                                 |
| cy-2i9 stability  | n/a  | no (codeaudit)          | cy-od5.6, cy-od5.7                                                      |

† assumes feature-gated transport inside `cypher-lsp`; if extracted to `cypher-lsp-transport` later, that's a SERIALISE edit.

**Round-1 achievable parallelism: 2 threads.** Example: one agent drafts cy-od5.7 (spec), another hardens tree-sitter in cy-od5.6. Both avoid the root-Cargo.toml contention lane and do not touch the same crate.

**Round-2 parallelism: 1 thread at a time on the crate-add beads** (cy-od5.2, .3, .4 serialise on root manifest). Optionally a second agent can work on cy-od5.6 polish or early demo-page wiring in parallel.

---

## 3. Spec amendment plan — `docs/specs/0004-interop-surfaces.md`

Draft headings only (body deferred to cy-od5.7):

- **0. TL;DR** — interop widens the crate graph; ABI + wire surfaces get stability commitments.
- **1. Motivation** — per the epic description; browser consoles, tree-sitter editors, non-Rust embedders.
- **2. Non-Coupling Contract — amendments**
  - 2.1 Confirms §2.C3 "no overlay crate" still holds; the new crates are genuine interop layers, not overlays.
  - 2.2 Confirms §2.C2 denylist still applies to the new crates (no domain names).
- **3. Crate Graph — amendment to spec 0001 §3.1**
  - Adds `cypher-wasm`, `cypher-ffi`, `cypher-py` to the authoritative table.
  - Declares each as a pure **adapter** on `cypher-lang-services` + `cypher-db` + `cypher-diag` + `cypher-fmt`; no analysis logic permitted (mirrors §3.1 thin-shell rule for binaries).
  - `tree-sitter-cypher/` is not a crate; called out as a sibling subproject.
- **4. WASM surface (`cypher-wasm`)**
  - 4.1 Bindgen strategy (wasm-bindgen + serde-wasm-bindgen).
  - 4.2 Size budget (≤ 2 MB brotli) and CI enforcement.
  - 4.3 JS API shape and versioning.
- **5. C FFI surface (`cypher-ffi`)**
  - 5.1 ABI scope (parse/hover/complete/rewrite/check/format/plan).
  - 5.2 Memory-ownership contract (Rust-allocated, caller-freed via `cypher_free_*`).
  - 5.3 Panic policy (catch_unwind at every boundary; `cypher_last_error`).
  - 5.4 `unsafe_code` waiver (scoped exception to §17.13, crate-local).
  - 5.5 ABI-stability commitment (tied to `cy-2i9`'s `proto_version` analogue for C).
  - 5.6 Header-gen check (`xtask cbindgen --check`).
- **6. Python surface (`cypher-py`)**
  - 6.1 Binding choice (PyO3 direct to `cypher-lang-services`, not via FFI).
  - 6.2 Wheel matrix (OS × arch × Python version).
  - 6.3 Type-stub (`cypher.pyi`) policy.
  - 6.4 Package-name policy (placeholder, reserved, not published v1).
- **7. LSP-Web transport**
  - 7.1 Feature gate `web-lsp` on `cypher-lsp`.
  - 7.2 Message-channel transport contract (postMessage-compatible).
  - 7.3 No analysis-logic regression; LSP capabilities identical to stdio mode.
- **8. Tree-Sitter parity contract**
  - 8.1 Scope: v1 TCK surface; parity assertions "ok → no ERROR nodes" / "error → ≥1 ERROR nodes".
  - 8.2 CI gate cross-reference to `xtask tree-sitter-parity`.
  - 8.3 Highlight-query ownership (`tree-sitter-cypher/queries/`).
- **9. Stability commitments**
  - 9.1 Wire protocols (agent JSON, LSP) — stability inherited from `cy-2i9`.
  - 9.2 C ABI — "once a symbol ships in a tagged release, it is stable for the life of the major; new symbols may be added."
  - 9.3 Python API — PEP 440 SemVer; stubs track Rust types marked `#[non_exhaustive]`.
  - 9.4 tree-sitter grammar versioning — independent SemVer on the tree-sitter side; parity harness holds the invariant.
- **10. Testing**
  - 10.1 WASM: puppeteer round-trip test in CI.
  - 10.2 FFI: Go + Node integration smoke in `demo/ffi/`.
  - 10.3 Py: pytest suite + wheel-install smoke.
  - 10.4 LSP-Web: same lsp-types conformance harness, web-transport-backed.
- **11. CI matrix additions** — wasm32 target, cbindgen, maturin per OS, node for tree-sitter, puppeteer headless.
- **12. Deferred** — publish to crates.io / PyPI / npm / tree-sitter-org (handled in `cy-zgz`).
- **13. Open questions** — PyPI name availability (`cypher` vs `cypher-frontend`); whether LSP-Web needs its own crate; whether to generate `grammar.js` from ungrammar.

Body of these sections is **not** drafted here — that is cy-od5.7's deliverable.

---

## 4. Dispatch order (recommended)

1. **cy-od5.7 (spec 0004)** — prerequisite; unlocks the three new crates. Without this, cy-od5.2/.3/.4 cannot land (AGENTS.md §6: "further specs 0002+ are the only way to evolve scope"). **Priority 1.**
2. **Wait on `cy-2i9`** for sealing — interop ABIs lock in whatever the current public Rust API looks like; shipping FFI before `non_exhaustive` is applied will manufacture breaking changes later. `cy-2i9.1` is already CLOSED so the Rust side is sealed; `docs/stability.md` is published. **Unblocked now.**
3. **cy-od5.6 (tree-sitter hardening)** — dispatch in parallel with cy-od5.7 since it touches a separate subtree and no Cargo.toml. Low blast radius, wide adoption (Neovim/Helix/Zed), no published crate. **Priority 2.**
4. **cy-od5.2 (cypher-wasm + Monaco demo)** — first crate-add. Demo-friendly (the most visible proof of life for interop), small binary artefact, no ABI commitment (JS is dynamic). **Priority 2.**
5. **cy-od5.5 (LSP-Web)** — light, rides on the wasm toolchain set up by .2. **Priority 3.**
6. **cy-od5.3 (cypher-ffi)** — hardest ABI commitment, widest reach. Intentionally ships last among code beads so WASM/Py have already stressed the adapter-layer design.
7. **cy-od5.4 (cypher-py)** — last because (a) wheel-matrix CI is expensive, (b) Py can lean on lessons from FFI's panic/error-surface design, (c) goes direct to `cypher-lang-services` so waiting does not cost it blocking deps.

**Rationale for tree-sitter-first, then WASM, then FFI, then Py** matches the epic's own hint and the blast-radius argument: the earliest surfaces with the least commitment go first so ABI mistakes are cheap; the surfaces with the tightest commitment go last after `cy-2i9` has stabilised the Rust side that all of them wrap.

---

## 5. Dependencies on cy-2i9 (stability sealing)

**Hard-block relationship:**
- **cy-od5.3 (FFI) — HARD BLOCK.** A C ABI that leaks `cypher-hir::Expr` variants without `non_exhaustive` manufactures a SONAME bump on every new expression kind. The `cargo-semver-checks` gate landed by `cy-2i9.1` is exactly the check FFI depends on; ship FFI only after that gate has one green cycle on `main`.
- **cy-od5.4 (Py) — HARD BLOCK.** Same reason. PyO3 re-exports Rust enums to Python; `#[non_exhaustive]` on the Rust side is what makes the `cypher.pyi` stub safe to grow.
- **cy-od5.2 (WASM) — SOFT BLOCK.** JS is dynamic; new fields silently appear. But the agent wire protocol's `proto_version` (added by `cy-2i9`) is what lets the JS wrapper refuse to talk to an incompatible runtime, so cy-2i9 still gates it — just more softly.
- **cy-od5.5 (LSP-Web) — SOFT BLOCK.** LSP is already versioned by `lsp-types`; extensions are the risk surface. The stability doc's "unstable: LSP extensions" clause (already in `docs/stability.md`) covers this. Can ship before full `cy-2i9`.
- **cy-od5.6 (tree-sitter) — NO BLOCK.** Grammar lives in its own SemVer. Independent.

Since `cy-2i9.1` is already CLOSED, the hard blocks are effectively cleared. One outstanding concern: the `proto_version: u32` field on agent requests/responses. Verify it exists before starting cy-od5.2; if missing, it's a pre-req bead.

---

## 6. Critical Files for Implementation

- `/Users/phall/workspace/cyrs/Cargo.toml` (root workspace members list — serialising edits for each new crate)
- `/Users/phall/workspace/cyrs/docs/specs/0004-interop-surfaces.md` (to be authored in cy-od5.7 — amends §3.1 crate graph)
- `/Users/phall/workspace/cyrs/crates/cypher-lang-services/src/lib.rs` (shared engine every interop adapter wraps; FFI/WASM/Py all bind to its public API)
- `/Users/phall/workspace/cyrs/crates/cypher-agent/src/main.rs` (wire-protocol source-of-truth the WASM shell reuses; `proto_version` scheme from `cy-2i9` lives here)
- `/Users/phall/workspace/cyrs/tree-sitter-cypher/grammar.js` (v0 grammar — cy-od5.6 expands it to full v1 surface)
- `/Users/phall/workspace/cyrs/docs/stability.md` (stable-surface enumeration the new spec 0004 cross-references for ABI and wire commitments)
