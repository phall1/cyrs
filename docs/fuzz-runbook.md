# Fuzz runbook

Operational companion to the `fuzz/` crate (spec 0001 §17.4). Covers
what the targets cover, how to refresh the dictionaries, how to add a
new target, and how to triage a crash.

For the overview of seed strategy and where artifacts go, see
[`fuzz/README.md`](../fuzz/README.md). This file answers the "what do
I do when…" questions.

## Targets

| Target | Input model | Oracles |
|---|---|---|
| `fuzz_lexer` | arbitrary bytes | no panic; `validate_tokens` stable |
| `fuzz_parser` | arbitrary bytes | no panic; lossless `parse(src).syntax().text() == src` |
| `fuzz_formatter` | arbitrary bytes | no panic; `fmt(fmt(s)) == fmt(s)` |
| `fuzz_sema` | arbitrary bytes | no panic; diagnostic spans lie within input |
| `fuzz_plan` | arbitrary bytes | no panic on HIR → plan lowering |
| `fuzz_structured_parse` | RNG seed → grammar generator | valid parse, fmt idempotence, parse(fmt) clean, HIR + sema no-panic |

The byte-level targets all receive `-dict=fuzz/dicts/cypher.dict`; the
structured target does not (its input is an RNG seed, not source text).

## Dictionaries

### What the dicts are

`fuzz/dicts/cypher.dict` is a libFuzzer dictionary: one C-string literal
per line, `#`-prefixed comments and blank lines ignored. libFuzzer uses
these tokens as splice candidates when mutating inputs, dramatically
shortening the path to syntactically interesting shapes.

The entry count at the top of the file is kept in sync with the file
body; if you edit the dict, update the header comment.

Current groups (cy-h07.1, 178 entries):

- Every `*_KW` variant in `crates/cypher-syntax/src/kind.rs`.
- Punctuation + multi-char operators (`<=`, `->`, `..`, …).
- Literal shapes: quoted strings, integers, floats (incl. scientific),
  `null`, bools, `$param`, backtick-escaped identifiers.
- Unicode edge cases: NUL, DEL, BOM, LTR/RTL marks, line separator,
  combining acute, emoji (2 flavours).
- Identifier pool matching the generator's (so the two fuzzers can
  find overlapping crashes).
- Common fragments (`MATCH (n)`, `RETURN n`, `-[:KNOWS]->`, …).

### Who pages when an entry becomes obsolete

When a keyword is **removed** from `kind.rs` (rare; the enum is
append-only per spec §10.2 policy):

1. Open a bead in `.beads/` pointing at the dict line and the commit
   that removed the keyword.
2. The commit that removes the keyword owns the dict update — put both
   in the same PR so the dict never references a dead token.
3. Tag the maintainer of the fuzz subsystem (see CODEOWNERS once that
   lands; until then, the bead's orchestrator).

For **additions** (new keywords, new operators) the dict should grow
in lockstep. CI does not enforce this automatically (cy-h07.2 is the
follow-up bead to add a `cargo xtask check-fuzz-dict` gate), so rely
on the grammar author to update the dict in the same PR that adds
the token.

### Regenerating the keyword block

There is no codegen script yet — the block is hand-maintained. To
manually refresh the keyword section:

```sh
rg '_KW(?:\s*=\s*\d+)?,?\s*$' crates/cypher-syntax/src/kind.rs \
  | rg -o '[A-Z][A-Z_]+(?=_KW)' \
  | sort -u
```

Diff the output against the `# Keywords` section of
`fuzz/dicts/cypher.dict`; add any newcomers, remove any
disappeared entries. A future bead (cy-h07.2) replaces this
manual step with `cargo xtask gen-fuzz-dict`.

## Adding a new fuzz target

Template: copy `fuzz/fuzz_targets/fuzz_parser.rs` and edit the oracle.
Checklist:

- [ ] File: `fuzz/fuzz_targets/fuzz_<name>.rs`, `#![no_main]`, uses
      `libfuzzer_sys::fuzz_target!`.
- [ ] `[[bin]]` entry in `fuzz/Cargo.toml`:
      ```
      [[bin]]
      name  = "fuzz_<name>"
      path  = "fuzz_targets/fuzz_<name>.rs"
      test  = false
      doc   = false
      bench = false
      ```
- [ ] Corpus dir: `fuzz/corpus/fuzz_<name>/.gitkeep` + at least one
      seed file. Filenames starting with `seed` are checked in per
      `.gitignore`; hex-named files are libFuzzer-generated and
      ignored.
- [ ] CI: add a `build` loop entry and a `run` step in `.github/workflows/ci.yml`
      under `fuzz-smoke`. Pass `-dict=fuzz/dicts/cypher.dict` for
      byte-level targets; omit the dict for structured / RNG-seeded
      targets.
- [ ] Table row in `docs/fuzz-runbook.md` (this file) and
      `fuzz/README.md`.
- [ ] Build locally: `cargo +nightly fuzz build fuzz_<name>`.
- [ ] Smoke-run: `cargo +nightly fuzz run fuzz_<name> -- -max_total_time=60`.

If your oracle needs valid Cypher rather than arbitrary bytes, use
`cyrs_fuzz::generator::random_valid_cypher` (see
`fuzz/src/generator.rs`) and treat the libFuzzer input as an RNG seed.

## Crash triage playbook

When a run crashes, libFuzzer writes the reproducer to
`fuzz/artifacts/fuzz_<target>/crash-<hash>` and exits with status 77.

1. **Reproduce deterministically.**

   ```sh
   cargo +nightly fuzz run fuzz_<target> \
     fuzz/artifacts/fuzz_<target>/crash-<hash>
   ```

   If the crash does not reproduce, it's likely a flaky oracle — treat
   as a P1 and investigate before re-running nightly.

2. **Minimise.**

   ```sh
   cargo +nightly fuzz tmin fuzz_<target> \
     fuzz/artifacts/fuzz_<target>/crash-<hash>
   ```

   This writes a `minimized-from-*` file next to the original. Use the
   minimised version as the regression fixture; the full reproducer
   can stay in `artifacts/` as a bonus.

3. **Classify the crash.**

   - **Panic from inside a crate:** read the stack trace. File the bug
     against the crate that panicked (e.g. `cypher-plan/src/lower.rs`),
     tag P0 in `.beads/`. Every panic on any input is a spec violation
     (§17.4).
   - **Oracle assertion:** if the oracle asserts an invariant and the
     invariant is violated, decide first whether the invariant is
     correct. If yes → P0 bug in the crate; if no → the target's
     assertion needs loosening (rare, but see cy-h07.1 where
     `fuzz_structured_parse` deliberately skips plan lowering).
   - **ASan / UBSan:** memory-safety and UB are P0 regardless of crate.

4. **Add a regression test.**

   Port the minimised reproducer into the relevant crate's `tests/ui`
   (`.cypher` + `.stderr`) or `tests/properties.rs` (proptest seed).
   `cargo xtask bless` accepts the new fixture once it passes.
   Commit the fixture *with* the fix so CI catches a re-regression.

5. **Promote to the corpus.**

   Move the artifact into
   `fuzz/corpus/fuzz_<target>/seed_<hash_short>_<slug>` so the next
   nightly run re-executes it first thing. libFuzzer only auto-replays
   files in the corpus dir, not in `artifacts/`.

## OSS-Fuzz onboarding

Deferred to a future bead (see `.beads/` for a `cy-oss-fuzz` parent
ticket when it's filed). Doing it well requires: a pinned image with
our nightly toolchain, a seed-corpus tarball published by CI, and
a coverage-feedback loop into `docs/specs/0001-cypher-frontend.md §17.4`.
None of that is needed for the PR-gate fuzz job today — the nightly
workflow at `.github/workflows/fuzz-nightly.yml` runs each target for
24h, which is the bar §17.4 sets.
