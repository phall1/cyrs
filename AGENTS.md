# AGENTS.md — operating manual for the `cypher/` workspace

> Re-read this file at the start of every session and after every context
> compaction. It is short on purpose.

This file tells an agent — human or model — how to operate inside the
`cypher/` Rust workspace without breaking its invariants. It is not the
design doc. The design doc is `docs/specs/0001-cypher-frontend.md` and is
**locked**; twenty-three numbered sections, referenced throughout this
file as `§N`. When in doubt, the spec wins. When the spec is silent,
rust-analyzer / Biome / ruff house style wins (§23).

---

## 0. Rule 0: the operator overrides everything

Explicit instructions from the human operator in the current session
override this file, AGENTS.md conventions, and even the spec. If an
operator instruction conflicts with the spec, surface the conflict, then
follow the operator. If you are unsure, ask.

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
- Coupled to anything in `../` (no imports from `trench/`, `intel/`, or
  any sibling workspace). Treat `cypher/` as if it lived in its own git
  repo — because it eventually will (§0 TL;DR, §2.C4).
- A place for domain concepts. No `Actor`, `Event`, `Operation`,
  `Capability`, `provenance`, `branch`, `bitemporal`, `expertise`, or any
  other trench-ontology word. CI greps for these (§2.C2).
- An "overlay crate" host. Domain extensions live in consumer
  repositories and plug in via the traits in §8. No overlay crate is
  permitted in this workspace, ever (§2.C3).

If a task asks you to add a domain concept, an executor, or a
trench/intel dependency, **stop** and ask the operator — it is almost
certainly out of scope.

---

## 2. Non-coupling contract (§2, load-bearing)

Hard invariants. CI enforces them. Violating any of these is a blocking
bug even if tests pass.

- **C2.1 — no intel/trench deps.** No `Cargo.toml` in this workspace
  lists any crate from outside `cypher/`. Grep: `^(intel-|trench-)` in
  any `Cargo.toml` dep table → fail.
- **C2.2 — no domain names.** The denylist: `Actor`, `Event`,
  `Operation`, `Capability`, `provenance`, `branch`, `bitemporal`,
  `expertise`. CI greps all `.rs` files minus `tests/` fixtures. One-off
  fixture strings in corpus files are allowed; source code names are
  not.
- **C2.3 — no overlay crates.** Every crate in `crates/` is either a
  layer from §3.1 or the meta-crate `cypher`. Nothing else lands here.
- **C2.4 — published-shaped.** `README.md`, `LICENSE-APACHE`,
  `LICENSE-MIT`, crate-level docs, `docs.rs`-clean. If trench disappeared
  tomorrow, `cargo publish` on each library crate would still work.
- **C2.5 — own toolchain.** `rust-toolchain.toml` lives here. MSRV
  `1.94`. Do not reach into parent workspace state.

---

## 3. Crate graph (§3, authoritative)

Dependency edges below are **allowed**. Anything else is forbidden —
there is no "it's convenient" exception.

```
cypher-syntax  → (external only: rowan, logos, smol_str, text-size, drop_bomb)
cypher-ast     → cypher-syntax
cypher-hir     → cypher-ast, cypher-syntax
cypher-schema  → cypher-syntax (types only)
cypher-sema    → cypher-hir, cypher-schema
cypher-diag    → cypher-syntax
cypher-plan    → cypher-hir
cypher-fmt     → cypher-syntax
cypher-db      → cypher-syntax, cypher-hir, cypher-sema, cypher-plan,
                 cypher-schema, cypher-diag, salsa
cypher-lsp     → cypher-db, cypher-diag, cypher-fmt, lsp-server, lsp-types
cypher-agent   → cypher-db, cypher-diag, cypher-fmt, serde_json
cypher-cli     → cypher-db, cypher-diag, cypher-fmt
cypher-tck     → cypher-db
cypher-testkit → any (dev only, not published)
cypher         → all non-binary crates above
```

- **No crate above `cypher-db` may depend on `salsa`.** Incrementality
  is an integration concern.
- **Binaries are thin shells.** No analysis logic in `cypher-lsp`,
  `cypher-agent`, `cypher-cli`. If you catch yourself writing a parser
  call inside a binary crate, move it into the relevant library crate.
- **`cypher-testkit` is dev-only.** Never re-exported from `cypher`.

---

## 4. Development workflow

### 4.1 Branch & commit policy

- Work directly on the current feature branch. No feature branches per
  bead; no worktrees. Treat the swarm as committing to a single branch.
- Small, frequent commits. Each commit compiles and passes
  `cargo check -p <touched crate>`.
- Push after every commit when collaborating with other agents —
  unpushed commits are invisible.
- Never: `git reset --hard`, `git clean -fd`, `git checkout -- <file>`,
  `git push --force`. If you need to drop work, `git stash` and surface
  the situation.
- Never stash, revert, or overwrite another agent's uncommitted work. If
  you find someone else's changes in the tree, treat them as your own
  (pull, rebase gently, or leave them alone).

### 4.2 File reservations (when multi-agent)

When more than one agent is active, claim the crates or files you plan
to edit via Agent Mail (or the equivalent reservation channel the
operator has configured). Release on commit. Advisory, not rigid — a
stale reservation after 1h is auto-expired. See Flywheel AGENTS.md
guidance for the exact macro shape; this workspace does not mandate a
specific reservation backend.

### 4.3 Pre-commit gate (§17)

Before every commit, run — or let the hook run:

```
cargo xtask gate
```

which invokes, in order, on the crates you touched:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test` (unit + snapshot)
4. `cargo deny check`
5. Non-coupling greps (§2 denylist)
6. Diagnostic-code registry lint (§10.2, codes are stable, no dupes)

A failing gate blocks the commit. Never `--no-verify`. If a gate is
genuinely broken, fix the gate in a dedicated commit; do not bypass it.

Heavier gates run in CI nightly, not pre-commit: fuzz (5-min PR, 24h
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

If you have `bv` (beads viewer) installed, prefer `bv --robot-triage`
for dependency-aware routing. Never launch the bare `bv` TUI in an
agent session — it blocks.

### 5.2 Completing a bead

- Every acceptance criterion in the bead must be met.
- The test gates named in the bead must be green.
- The bead must cite the spec section it implements in its commit
  message (e.g. `spec §7.3: aggregation scope`).
- Close with `br close <id>` only after the gate passes and the commit
  lands.

### 5.3 Creating a bead

- One responsibility per bead. If the title has "and" in it, split.
- Dependencies are explicit (`br dep add <child> <parent>`). No
  implicit ordering via priority.
- Acceptance criteria cite spec numbers. "Implements §7.3" is
  sufficient; do not restate the spec.
- Tests live in the same bead as the code they exercise — no "tests
  later" beads.

---

## 6. Spec discipline

- Every non-trivial design decision lives in a numbered spec under
  `docs/specs/`. Spec 0001 is locked; further specs (0002…) are the
  only way to evolve v1 scope.
- Open questions go in the spec's §21, not in scattered comments.
- Deferred work goes in §20 of the relevant spec, not in TODOs.
- If implementation reveals that the spec is wrong, stop, raise it with
  the operator, and amend the spec. Do not silently diverge.

---

## 7. Diagnostic codes (§10, load-bearing)

- Codes are **stable**. Once assigned, never change meaning. Retired
  codes are never reused.
- Registry lives at `crates/cypher-diag/src/codes.rs`. Every emit
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

Rust-compiler-grade is the bar. Do not ship a bead that lowers any of
these.

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
- **TCK conformance** (`cypher-tck`): the v1 tags in §17.5 must be
  green on every PR.
- **Benchmarks** (`criterion`) with a 10% regression gate per §17.10.
- **Miri** nightly with `-Zmiri-strict-provenance`. No UB, including
  in dependencies.
- **Determinism** (§17.14): no `HashMap` iteration order in outputs.
  Use `BTreeMap` or `IndexMap` for anything that crosses the public
  API.

Coverage minimums per crate are in §17.9. Do not ship a bead that
drops a crate below its floor.

---

## 9. Dialect gates (§9)

Every construct that differs between `GqlAligned` and `OpenCypherV9`
goes through `DialectGate`. Do not scatter `if dialect == ...`
checks across the codebase; every gate is a named constant with its
own diagnostic code in the `E4000–E4999` range.

`Neo4jCurrent` is not in v1 (§9.3). If a bead asks for APOC, `EXISTS {}`
subqueries, `CALL { ... }`, `LOAD CSV`, `SHOW`, or `CYPHER` prefixes,
it is out of scope — reject the bead and point at §19 / §20.

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
  switching inside a bead — if a task needs nightly (fuzz, mutants,
  Miri), it runs in CI or an `xtask` subcommand.
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

Never, without explicit operator instruction in this session:

- `rm -rf` any path inside the workspace
- `git reset --hard`, `git clean -fd`, `git checkout -- <file>`
- `git push --force` (use `--force-with-lease` if truly necessary)
- Delete files that another agent has touched recently without
  checking Agent Mail / the recent commit log
- Overwrite `Cargo.lock`, `rust-toolchain.toml`, `LICENSE-*`, spec
  files

If you need to back out a change, prefer `git revert` over destructive
rewriting.

---

## 13. When you are stuck

Order of operations:

1. **Reread the bead.** Acceptance criteria answer most questions.
2. **Reread the relevant spec section.** Every bead cites one.
3. **Look at rust-analyzer / Biome / ruff** for the same problem in
   neighbouring code. The spec explicitly defers to their house style
   (§23).
4. **Ask the operator.** Do not improvise architecture; do not add
   scope "while you are there". The architecture is locked.

---

## 14. Reminders

- Re-read this file after every context compaction.
- Commit messages cite spec sections.
- Bead IDs appear in commit messages (`br-0042: implement §7.3`).
- Output text is for humans who cannot see your tool calls — say what
  you are doing, briefly, before you do it.

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
