# Contributing to cyrs

Thanks for picking up a bead. This file is the short path from a fresh
clone to a green PR. Deeper norms live in [`AGENTS.md`](./AGENTS.md);
architecture lives in [`docs/specs/`](./docs/specs/).

---

## First 5 minutes

```sh
git clone https://github.com/phall1/cyrs.git
cd cyrs
bash scripts/install-hooks.sh
cargo xtask gate
```

That's the whole setup. The hook script wires `cargo xtask gate` into
`pre-commit` so every commit is checked before it lands. If the gate is
green, you are ready to edit.

See [`docs/getting-started.md`](./docs/getting-started.md) for the tour
of the crate graph and a first-change walkthrough.

---

## How work is tracked

Work is tracked in `.beads/` via the `br` CLI. Run `br ready` to see
unblocked beads; see [`AGENTS.md`](./AGENTS.md) §5 for the intake rules.

---

## Making a change

1. Pick a bead with `br ready`, or file one with `br create` if the work
   does not map to an existing bead. One bead = one responsibility.
2. Cite the spec section the bead implements — every bead points at
   [`docs/specs/0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md)
   or a later spec.
3. Write the failing test first. For a bug fix, add the test that would
   have caught the bug. For a feature, add unit + snapshot + UI fixture
   coverage as applicable (see *Testing bar* below).
4. Implement. Commits are small and each one compiles.
5. Run `cargo xtask gate`. The pre-commit hook runs it for you; if it
   fails, fix the cause — do not pass `--no-verify`.
6. Open a PR. The template prompts you for the bead ID, the spec
   section, and the test plan.

---

## Spec discipline

Every architecture decision lives in `docs/specs/`. Spec 0001 is
**locked** — amendments go via an operator-approved spec revision, not
inline edits. If implementation reveals the spec is wrong, stop, raise
it with the operator, and amend the spec. Do not silently diverge.

Details in [`AGENTS.md`](./AGENTS.md) §6.

---

## Diagnostic codes

Codes are stable. Once assigned, meaning never changes; retired codes
are never reused. Ranges live in [`AGENTS.md`](./AGENTS.md) §7 and the
registry itself lives at
[`crates/cypher-diag/src/codes.rs`](./crates/cypher-diag/src/codes.rs).

A new check means:

1. Pick the next free slot in the relevant range.
2. Add the constant + message to `codes.rs`.
3. Add a UI fixture under the owning crate's `tests/ui/`.
4. Add a docs page stub under `docs/diagnostics/`.

The gate rejects duplicate codes, unregistered emits, dead
registrations, and silent message mutations. See
[`docs/diagnostics.md`](./docs/diagnostics.md) for the full authoring
walkthrough.

---

## Testing bar

Rust-compiler-grade (spec §17). Every bead ships with:

- **Unit** tests for the touched module.
- **Snapshot** tests (`insta`) for anything shaped like output — CST,
  AST, HIR, diagnostics, plans, formatter output. Review with
  `cargo insta review`.
- **Property** tests (`proptest`) for the seven named properties in
  spec §17.3.
- **Compiletest** (UI) fixtures for anything that emits diagnostics.
  Regenerate with `cargo xtask bless`.
- **Fuzz** targets for lexer, parser, formatter, sema, plan. Any panic
  is a blocker.
- **TCK** conformance — `cypher-tck` tags in spec §17.5 must stay green.

The pre-commit gate covers unit, snapshot, format, clippy, deny, and
the denylist / diagnostic-code lints. Fuzz, Miri, mutants, and coverage
run in CI.

---

## Commit and PR conventions

Commits cite the bead ID and the spec section they implement. Match the
pattern visible in `git log`:

```
cy-XXX: spec §N — <what changed>
```

Examples from recent history:

```
cy-7lf: spec §6.1 — bare pattern predicates (WHERE (a)-->(b))
cy-zv0: spec §11 — TextEdit + incremental_reparse API
```

PRs follow the same shape. The PR template prompts for a one-paragraph
summary, a test plan, and the mandatory checkboxes (bead cited, spec
cited, gate green, denylist clean, no new crate without a spec
amendment).

---

## Destructive-ops policy

Never run `git reset --hard`, `git clean -fd`, `git checkout -- <file>`,
or `git push --force`. Never pass `--no-verify`. If you need to back
out, `git revert`; if you need to set aside work, `git stash` and
surface it.

Full list in [`AGENTS.md`](./AGENTS.md) §12.

---

## Where to look next

- [`AGENTS.md`](./AGENTS.md) — operating manual, invariants, tooling.
- [`docs/specs/0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md)
  — architecture, crate graph, testing bar.
- [`docs/architecture.md`](./docs/architecture.md) — crate-by-crate
  tour with entry points.
- [`docs/diagnostics.md`](./docs/diagnostics.md) — authoring a new
  diagnostic end-to-end.
- [`docs/getting-started.md`](./docs/getting-started.md) — first-change
  walkthrough.
