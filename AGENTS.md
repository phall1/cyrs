# AGENTS.md

Operating manual for the `cypher/` workspace. Read at session start and
after context compaction.

Audience: agents — human or model — working inside the Rust workspace
without breaking its invariants. This is the operations manual, not the
design doc. The design doc is `docs/specs/0001-cypher-frontend.md`,
**locked**; twenty-three numbered sections, referenced throughout as
`§N`. The spec wins on architectural questions; rust-analyzer / Biome /
ruff house style wins where the spec is silent (§23).

---

## 0. Operator overrides everything

Explicit instructions from the human operator in the current session
override this file, AGENTS.md conventions, and the spec. An operator
instruction that conflicts with the spec is followed after the conflict
is surfaced. Ambiguity is resolved by asking.

---

## 1. What this workspace is, and is not

**Is:** a standalone, domain-free Rust front-end for Cypher / GQL. Lexer,
recovering parser, lossless CST, typed AST, HIR with name resolution,
schema-aware semantic analysis, diagnostics, formatter, Salsa-based
incremental DB, LSP server, agent JSON API, CLI. Fifteen crates under
`crates/`, one `xtask/` for developer tasks. Rust-compiler-grade testing
bar (§17).

**Is not:**

- An executor. No storage, no runtime, no plan execution. Consumers own
  that (§1.3 N1, §12.5).
- Coupled to any other project.
- A place for domain concepts. No reserved downstream vocabulary; CI
  greps for the denylist (§2.C2).
- An "overlay crate" host. Domain extensions live in consumer
  repositories and plug in via the traits in §8. Overlay crates are
  not permitted in this workspace (§2.C3).

Tasks asking for a domain concept, an executor, or an unrelated
dependency are almost certainly out of scope. Stop and ask the operator
before proceeding.

---

## 2. Non-coupling contract (§2, load-bearing)

Hard invariants enforced by CI. A violation is a blocking bug even with
green tests.

- **C2.2 — no domain names.** The denylist is enforced by CI. Greps run
  across all `.rs` files minus `tests/` fixtures. One-off fixture strings
  in corpus files are allowed; source code names are not.
- **C2.3 — no overlay crates.** Every crate in `crates/` is either a
  layer from §3.1 or the meta-crate `cypher`. Nothing else lands here.
- **C2.4 — published-shaped.** `README.md`, `LICENSE-APACHE`,
  `LICENSE-MIT`, crate-level docs, `docs.rs`-clean. 
- **C2.5 — own toolchain.** `rust-toolchain.toml` lives here. MSRV
  `1.94`. Do not reach into parent workspace state.

---

## 3. Crate graph (§3, authoritative)

The edges below are the only allowed dependencies. No "convenience"
exceptions.

```
cyrs-syntax        → (external only: rowan, logos, smol_str, text-size, drop_bomb)
cyrs-ast           → cyrs-syntax
cyrs-hir           → cyrs-ast, cyrs-syntax
cyrs-schema        → cyrs-syntax (types only)
cyrs-project       → cyrs-schema, smol_str, thiserror, serde, toml,
                       globset, walkdir
cyrs-sema          → cyrs-hir, cyrs-schema
cyrs-diag          → cyrs-syntax
cyrs-plan          → cyrs-hir
cyrs-fmt           → cyrs-syntax
cyrs-db            → cyrs-syntax, cyrs-hir, cyrs-sema, cyrs-plan,
                       cyrs-schema, cyrs-diag, salsa
cyrs-lang-services → cyrs-db, cyrs-hir, cyrs-schema, cyrs-sema,
                       cyrs-syntax, cyrs-ast, cyrs-fmt
cyrs-lsp           → cyrs-lang-services, cyrs-db, cyrs-diag,
                       cyrs-fmt, lsp-server, lsp-types
cyrs-agent         → cyrs-lang-services, cyrs-db, cyrs-diag,
                       cyrs-fmt, serde_json
cyrs-cli           → cyrs-db, cyrs-diag, cyrs-fmt, cyrs-schema,
                       cyrs-project
cyrs-tck           → cyrs-db
cyrs-testkit       → any (dev only, not published)
cypher               → all non-binary crates above
```

- **`cyrs-lang-services` is the shared home for LSP/agent engines.**
  The completion, hover, and rewrite engines both binaries expose live
  here as pure functions keyed on `(db, file_id, byte-offset)`.  The
  LSP and agent crates are thin adapters: position ↔ byte-offset and
  wire-format conversion on the edges; zero logic duplication.

- **No crate above `cyrs-db` may depend on `salsa`.** Incrementality
  is an integration concern.
- **Binaries are thin shells.** No analysis logic in `cyrs-lsp`,
  `cyrs-agent`, `cyrs-cli`. If you catch yourself writing a parser
  call inside a binary crate, move it into the relevant library crate.
- **`cyrs-testkit` is dev-only.** Never re-exported from `cypher`.

**Pointing embedders at the right layer.** The crate graph above is
internal — who may depend on whom *inside* the workspace. The
external-consumer question ("which layer should I depend on?") is
answered by [`docs/integration-depth.md`](docs/integration-depth.md):
decision table by embedder kind, per-layer entry-point snippets,
stability-promise matrix per surface. Beads, PR reviews, and issues
triaging "which layer should X consume?" link that doc rather than
restating the spec.

---

## 4. Development workflow

### 4.1 Branch & commit policy

- Parallel-bead dispatch goes through git worktrees: one worktree per
  bead, branch named `bead/<id>-<slug>` off `main`. The implementing
  agent commits inside its worktree; the orchestrator fast-forwards /
  merges branches into `main` serially. Solo work goes directly on
  `main`.
- Small, frequent commits. Every commit compiles and passes
  `cargo check -p <touched crate>`.
- Push after every commit when collaborating — unpushed commits are
  invisible to other agents.
- Forbidden without explicit operator instruction: `git reset --hard`,
  `git clean -fd`, `git checkout -- <file>`, `git push --force`. To drop
  work, `git stash` and report the situation.
- Another agent's uncommitted work is never stashed, reverted, or
  overwritten. Changes encountered in the tree are treated as belonging
  to whoever was working there (pull, rebase gently, or leave alone).

### 4.2 Multi-agent coordination

- **Beads state is orchestrator-owned.** Implementing agents do not run
  `br update` / `br close` / `br sync`. The orchestrator claims beads
  with `br update --status=in_progress` before dispatch and closes them
  with `br close <id>` after the branch lands on `main`. This avoids
  SQLite write contention across worktrees (worktrees share `.git` but
  each carries its own `.beads/` working copy).
- **Crate-scoped parallelism.** Dispatch only beads whose `crate:*`
  label is disjoint from other in-flight beads. Root `Cargo.toml`
  `[workspace.members]` edits (e.g. adding a new crate) serialize —
  run those beads alone.
- **Merge order.** Merge finished branches into `main` one at a time,
  in dependency order where declared. Re-run `cargo xtask gate` on
  `main` after each merge; if it fails, revert the merge rather than
  patching on top.
- **File reservations** (single-tree multi-agent fallback): when more
  than one agent shares a tree, claim crates or files via Agent Mail
  (or the equivalent reservation channel). Release on commit.
  Advisory, not rigid — a stale reservation after 1h is auto-expired.
  See Flywheel AGENTS.md guidance for the macro shape.

#### 4.2.1 Worktree isolation rules (load-bearing — read before fanning out)

Parallel-agent fan-out has destroyed work before. The cy-0hj GQL
bootstrap post-mortem (2026-05-19) lost three of six agents' commits
to cross-contamination. The rules below codify the fix.

**Where worktrees live.** Always under `/tmp/cyrs-<bead-id>/`. Not
under `.claude/worktrees/`, `crates/.../.claude/`, or anywhere inside
the main checkout. The Claude Code Agent tool's `isolation: "worktree"`
flag defaults to `.claude/worktrees/<id>/`; that path is wrong. Two
options:

- Pre-create the worktree before invoking the agent:
  ```sh
  git worktree add -b cy-<bead> /tmp/cyrs-cy-<bead> main
  ```
  and pass `/tmp/cyrs-cy-<bead>` as the agent's working directory; **or**
- Use `isolation: "worktree"` only for single-agent tasks under
  direct supervision.

**Why `.claude/worktrees/` fails.** Git worktrees share `.git/`. Two
worktrees under the same parent (the main checkout) cause agents to
`cd /Users/<you>/workspace/cyrs` expecting "their" tree, and write to
`main`'s working copy. The shared index also turns one agent's
`git stash` into every agent's `git stash` — the mechanism by which
unrelated stashes land in main mid-session.

**Hard rules for every implementing agent prompt.** Include all five
verbatim when fanning out:

1. "Stay in `$worktree_path`. Do not `cd` anywhere else. Do not touch
   `/Users/<you>/workspace/cyrs/` or any other worktree's path."
2. "Do not run `git stash`, `git stash pop`, `git stash drop`, or
   `git checkout <branch>`. These operations touch the shared `.git/`
   and corrupt parallel agents' state."
3. "If a flaky test blocks your commit, re-run the single test in
   isolation; do not stash + retry. If still flaky, report it as a
   blocker rather than papering over it."
4. "Use `git commit --no-verify` only when the orchestrator has told
   you the pre-commit gate is broken for an unrelated reason
   (typically a `cargo deny` lockfile issue). Otherwise `--no-verify`
   is forbidden."
5. "Operate only on the files listed in your prompt. A shared file
   (`crates/cyrs-syntax/src/kind.rs`, `lexer.rs`, `parser.rs`,
   `cypher.ungrammar`, `diag/codes.rs`, `diag/tests/registry.rs`)
   touched by another in-flight bead is a stop-and-report case. The
   orchestrator pre-allocates SyntaxKind raw u16 values and `DiagCode`
   slots; reservations are followed exactly."

**Pre-allocation discipline for shared-file work.** When fanning out
beads that all add to `kind.rs` / `lexer.rs` / `cypher.ungrammar`, the
orchestrator pre-allocates in the dispatch prompt:

- `SyntaxKind` raw `u16` values (e.g. `INSERT_KW = 181`, `FILTER_KW =
  182` — leave explicit gaps for reservations).
- `DiagCode` slots (`E0083`, `E0084`, …) — also with explicit gaps.

Kinds are declared with `= <pinned>` discriminants, never bare
appends. This gives parallel branches deterministic merge ordering and
zero numeric drift.

**Parallel fan-out sanity check.** Six agents touching the same five
files is integration hell regardless of isolation. Work shaped as
"every bead edits kind.rs + lexer.rs + parser.rs" runs faster
sequentially than parallel-plus-merge. Parallel fan-out is reserved
for genuinely disjoint surfaces (one bead per crate, one bead per
parser subsystem with no shared SyntaxKind additions).

### 4.3 Pre-commit gate (§17)

Every commit runs (the hook will do it):

```
cargo xtask gate
```

which invokes, in order, on the touched crates:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test` (unit + snapshot)
4. `cargo deny check`
5. Non-coupling greps (§2 denylist)
6. Diagnostic-code registry lint (§10.2, codes are stable, no dupes)

A failing gate blocks the commit. `--no-verify` is not the answer.
A genuinely broken gate gets fixed in a dedicated commit, not bypassed.

Heavier gates run nightly in CI, not pre-commit: fuzz (5-min PR, 24h
nightly), `cargo mutants`, Miri, coverage. See §17.4–17.12.

---

## 5. Task intake — work off beads, not chat

Work is tracked in `.beads/` via the `br` CLI (installed from
[`beads_rust`][beads]). One bead = one self-contained unit of work with
description, rationale, acceptance criteria, and explicit dependencies.
Every bead references the spec section it implements.

[beads]: https://github.com/Dicklesworthstone/beads_rust

### 5.1 Picking a bead

```
br ready --json        # unblocked open beads
br show <id>           # full bead detail
br update <id> --status in_progress    # claim before editing
```

With `bv` (beads viewer) installed, `bv --robot-triage` gives
dependency-aware routing. The bare `bv` TUI blocks in an agent session
and is not used.

### 5.2 Completing a bead

- All acceptance criteria are met.
- Every test gate named in the bead is green.
- The commit message cites the spec section the bead implements
  (e.g. `spec §7.3: aggregation scope`).
- `br close <id>` runs only after the gate passes and the commit lands.

### 5.3 Creating a bead

- One responsibility per bead. A title with "and" in it splits into two
  beads.
- Dependencies are explicit (`br dep add <child> <parent>`). No
  implicit ordering through priority.
- Acceptance criteria cite spec numbers. "Implements §7.3" is
  sufficient; the spec text is not restated.
- Tests live in the same bead as the code they exercise — no "tests
  later" beads.

---

## 6. Spec discipline

- Every non-trivial design decision lives in a numbered spec under
  `docs/specs/`. Spec 0001 is locked; further specs (0002…) are the
  only mechanism for evolving v1 scope.
- Open questions live in the spec's §21, not in scattered comments.
- Deferred work lives in §20 of the relevant spec, not in TODOs.
- An implementation that contradicts the spec is a stop-and-amend
  situation: raise it with the operator and amend the spec, not
  silently diverge.

---

## 7. Diagnostic codes (§10, load-bearing)

- Codes are **stable**. Once assigned, never change meaning. Retired
  codes are never reused.
- Registry lives at `crates/cyrs-diag/src/codes.rs`. Every emit
  site references a registered constant; raw strings are forbidden by
  CI lint.
- Ranges:

| Range           | Meaning                                   |
| --------------- | ----------------------------------------- |
| `E0001–E0999`   | Syntax (lexer + parser)                   |
| `E1000–E1999`   | Name resolution                           |
| `E2000–E2999`   | Semantic — schema-free                    |
| `E3000–E3999`   | Semantic — schema-aware                   |
| `E4000–E4999`   | Dialect / compatibility                   |
| `E5000–E5999`   | Type system                               |
| `W6000–W6999`   | Style / lint warnings                     |
| `W7000–W7999`   | Performance warnings                      |
| `N8000–N8999`   | Informational notes                       |

- New check → new code, pulled from the next free slot in the relevant
  range and added to the registry with a message + docs page stub.
- CI fails on: duplicate codes, codes emitted but not registered, codes
  registered but not emitted (dead code), code message mutations
  without a registry bump.

---

## 8. Testing expectations (§17)

The bar is rust-compiler-grade. A bead lowering any of these does not
ship.

- **Unit tests** per crate, covering the public API and internal
  modules that have branching logic.
- **Snapshot tests** (`insta`) for anything shaped like output: CST,
  AST, HIR, diagnostics, plans, formatter output. Regenerate with
  `cargo insta review`; CI rejects unreviewed diffs.
- **Property tests** (`proptest`): the seven named properties in §17.3
  are non-negotiable. Adding a new public type that emits output
  typically implies a new property.
- **Compiletest corpus** (`tests/ui/**` per crate) for anything that
  produces diagnostics. `cargo xtask bless` regenerates; CI rejects
  unblessed diffs.
- **Fuzz targets** exist for lexer, parser, formatter, sema, plan
  (§17.4). PR gate: 5 minutes per target. Any panic is a blocker.
- **TCK conformance** (`cyrs-tck`): the v1 tags in §17.5 must be
  green on every PR.
- **Benchmarks** (`criterion`) with a 10% regression gate per §17.10.
- **Miri** nightly with `-Zmiri-strict-provenance`. No UB, including
  in dependencies.
- **Determinism** (§17.14): no `HashMap` iteration order in outputs.
  Use `BTreeMap` or `IndexMap` for anything that crosses the public
  API.

Coverage minimums per crate are in §17.9. Beads that drop a crate
below its floor do not land.

---

## 9. Dialect gates (§9)

Every construct that differs between `GqlAligned` and `OpenCypherV9`
goes through `DialectGate`. `if dialect == …` checks scattered across
the codebase are not the pattern; every gate is a named constant with
its own diagnostic code in the `E4000–E4999` range.

`Neo4jCurrent` is not in v1 (§9.3). Beads asking for APOC, `EXISTS {}`
subqueries, `CALL { ... }`, `LOAD CSV`, `SHOW`, or `CYPHER` prefixes
are out of scope — rejected with a pointer to §19 / §20.

---

## 10. Files that are off-limits without explicit permission

- `docs/specs/*.md` — locked. Amend only via operator-approved spec
  revision.
- `rust-toolchain.toml` — MSRV bumps are spec-governed.
- `Cargo.lock` — commit updates that come from legitimate dep changes;
  never regenerate to "refresh".
- `LICENSE-*` — never edit.
- Root `Cargo.toml` `[workspace.dependencies]` version pins — bump
  only for a documented reason.

---

## 11. Tools on hand

- **`cargo`** with stable/nightly as pinned. No manual toolchain
  switching inside a bead — tasks needing nightly (fuzz, mutants,
  Miri) run in CI or via an `xtask` subcommand.
- **`br`** — beads CLI for task state.
  `cargo install --git https://github.com/Dicklesworthstone/beads_rust.git`.
- **`cargo xtask`** — the project's developer-task hub. Subcommands:
  `gate` (pre-commit), `bless` (regenerate UI tests), `codegen` (regen
  AST from ungrammar), `release` (gated release), `fuzz <target>`.
- **`cargo fuzz`, `cargo mutants`, `cargo llvm-cov`** — installed on
  CI runners; optional locally.
- **`cargo insta`** — required locally for snapshot review.
- **`cargo deny`** — runs in gate; install locally with
  `cargo install cargo-deny --locked`.

No network access at runtime in any library or binary crate. CI tests
run with network disabled where the harness allows.

---

## 12. Destructive-command policy

Forbidden without explicit operator instruction in this session:

- `rm -rf` of any path inside the workspace
- `git reset --hard`, `git clean -fd`, `git checkout -- <file>`
- `git push --force` (`--force-with-lease` is preferred when truly
  necessary)
- Deleting files another agent recently touched, without checking
  Agent Mail / the recent commit log
- Overwriting `Cargo.lock`, `rust-toolchain.toml`, `LICENSE-*`, or
  spec files

Backing out a change uses `git revert` rather than destructive
rewriting.

---

## 13. When stuck

Order of attack:

1. **The bead.** Acceptance criteria answer most questions.
2. **The relevant spec section.** Every bead cites one.
3. **rust-analyzer / Biome / ruff** for the same problem in neighbouring
   code. The spec defers to their house style (§23).
4. **The operator.** Architecture is locked — no improvising, no
   scope-creep "while you're there".

---

## 14. Reminders

- This file is re-read after every context compaction.
- Commit messages cite spec sections.
- Bead IDs appear in commit messages (`br-0042: implement §7.3`).
- Output text is for humans who cannot see tool calls — a brief
  one-line update before each action is enough.

---

*End of AGENTS.md.*

<!-- br-agent-instructions-v1 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`/`bd`) for issue tracking. Issues are stored in `.beads/` and tracked in git.

### Essential Commands

```bash
# View ready issues (open, unblocked, not deferred)
br ready              # or: bd ready

# List and search
br list --status=open # All open issues
br show <id>          # Full issue details with dependencies
br search "keyword"   # Full-text search

# Create and update
br create --title="..." --description="..." --type=task --priority=2
br update <id> --status=in_progress
br close <id> --reason="Completed"
br close <id1> <id2>  # Close multiple issues at once

# Sync with git
br sync --flush-only  # Export DB to JSONL
br sync --status      # Check sync status
```

### Workflow Pattern

1. **Start**: Run `br ready` to find actionable work
2. **Claim**: Use `br update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id>`
5. **Sync**: Always run `br sync --flush-only` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `br ready` shows only open, unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers 0-4, not words)
- **Types**: task, bug, feature, epic, chore, docs, question
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
br sync --flush-only    # Export beads changes to JSONL
git commit -m "..."     # Commit everything
git push                # Push to remote
```

### Best Practices

- Check `br ready` at session start to find available work
- Update status as you work (in_progress → closed)
- Create new issues with `br create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always sync before ending session

<!-- end-br-agent-instructions -->
