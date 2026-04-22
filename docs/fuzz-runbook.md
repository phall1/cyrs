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
| `fmt_parse_roundtrip` | arbitrary UTF-8 | P17.3.4 structural equality of CST modulo trivia |

Byte-level targets receive their per-target dictionary at
`fuzz/dicts/<target>.dict`; the structured target does not (its input
is an RNG seed, not source text).

## Dictionaries

### What the dicts are

libFuzzer dictionaries: one C-string literal per line, `#`-prefixed
comments and blank lines ignored. libFuzzer uses these tokens as splice
candidates when mutating inputs, dramatically shortening the path to
syntactically interesting shapes.

**Layout (cy-h07):**

- `fuzz/dicts/cypher.dict` — shared base, hand-maintained.
- `fuzz/dicts/extras/<target>.dict` — target-specific extras.
- `fuzz/dicts/<target>.dict` — auto-generated concatenation of the
  above by `fuzz/dicts/regen.sh`. Do NOT edit by hand. Each file is
  > 200 entries; the acceptance floor per target is 100.
- `fuzz/dicts/regen.sh` — shellcheck-clean regeneration script. Run
  it after editing the base or any extras file.

Current groups (shared base, cy-h07):

- Every `*_KW` variant in `crates/cypher-syntax/src/kind.rs`.
- Punctuation + multi-char operators (`<=`, `->`, `<->`, `..`, …).
- Literal shapes: quoted strings, integers, floats (incl. scientific),
  `null`, bools, `$param`, backtick-escaped identifiers.
- Unicode edge cases: NUL, DEL, BOM, LTR/RTL marks, line separator,
  combining acute, emoji (2 flavours).
- Identifier pool matching the generator's (so the two fuzzers can
  find overlapping crashes).
- Common fragments (`MATCH (n)`, `RETURN n`, `-[:KNOWS]->`, …).

Per-target extras (cy-h07):

- `fuzz_lexer.dict`: string escapes, numeric edge shapes, whitespace /
  comment shapes, max-munch identifier-adjacency traps, bare sigils.
- `fuzz_parser.dict`: clause-order bait, unbalanced-delimiter recovery
  stress, pattern-comprehension + quantified-pattern shapes, subquery
  forms that must reject cleanly per §9 v1 scope.
- `fuzz_formatter.dict`: trivia-only inputs, comment placements that
  break many formatters, operator-spacing edges, UTF-8 whitespace.
- `fuzz_sema.dict`: undeclared / shadowed variables, aggregation +
  grouping bait, WITH-pipeline scope rules, label expressions.
- `fuzz_plan.dict`: scan / expand / filter / project / aggregate / sort
  seeds, UNION, optional-match, write-side ops (SET / MERGE / DELETE).
- `fmt_parse_roundtrip.dict`: well-formed fragments covering every
  clause the formatter handles — the oracle is strongest when libFuzzer
  splices valid fragments into the input stream.

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
disappeared entries.

After editing the base dict or any per-target extras:

```sh
fuzz/dicts/regen.sh
```

regenerates all seven `fuzz/dicts/<target>.dict` files. Commit the
regenerated outputs in the same PR as the base edit so the committed
tree always matches what CI passes to libFuzzer.

A future bead (cy-h07.2) replaces the manual keyword-refresh step with
`cargo xtask gen-fuzz-dict`.

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
- [ ] Dictionary: add `fuzz/dicts/extras/<name>.dict` with
      target-specific splice shapes, update the `targets=(…)` array in
      `fuzz/dicts/regen.sh`, and run the script. Commit the regenerated
      `fuzz/dicts/<name>.dict` alongside the extras.
- [ ] CI: add a `build` loop entry and a `run` step in `.github/workflows/ci.yml`
      under `fuzz-smoke`. Pass `-dict=fuzz/dicts/<name>.dict` for
      byte-level targets; omit the dict for structured / RNG-seeded
      targets.
- [ ] Table row in `docs/fuzz-runbook.md` (this file) and
      `fuzz/README.md`.
- [ ] Build locally: `cargo +nightly fuzz build fuzz_<name>`.
- [ ] Smoke-run: `cargo +nightly fuzz run fuzz_<name> -- -max_total_time=60`.

If your oracle needs valid Cypher rather than arbitrary bytes, use
`cyrs_fuzz::generator::random_valid_cypher` (see
`fuzz/src/generator.rs`) and treat the libFuzzer input as an RNG seed.

## Known pre-existing panics (cy-h07 smoke findings)

Cy-h07's per-target dictionary expansion drove two pre-existing crashes
into the PR-gate smoke window. Both existed at the bead's base
(`c3c246b`); both are blocking bugs per §17.4 and need dedicated
follow-up beads before the fuzz smoke gate can turn blocking.

1. **`fuzz_plan` — pattern-part empty-element panic.** — **FIXED (cy-f2t).**
   - Reproducer: 5 bytes `MATCH` (bare keyword; parser recovers, HIR
     lowerer produces a pattern with zero elements).
   - Previously panicked at `crates/cypher-plan/src/lower.rs:682` with
     `pattern part must have at least one element`.
   - Fix: `precheck_statement` now rejects empty / leading-`Rel`
     pattern parts as `PlanLowerError::EmptyPatternPart`; the in-body
     `.expect(…)` sites were replaced with graceful fallbacks.
   - Minimised seed lives at
     `fuzz/corpus/fuzz_plan/seed_empty_match_bare` so libFuzzer
     regresses on it at every startup.

2. **`fuzz_formatter` — non-idempotent on newline inside string literal.**
   - Reproducer: 6 bytes `'\n'\nN` (a `'\n'` token followed by a
     newline and the letter `N`).
   - `fmt(fmt(s)) != fmt(s)`: the second run drops one newline.
   - Root cause: the formatter's whitespace-canonicalisation pass
     treats a trailing newline adjacent to a string literal differently
     on the first vs second pass.
   - Expected fix: inside `cypher-fmt::format`, ensure the
     trivia-rewriting pass is a true fixed point on any input.
   - Repro kept OUT of `fuzz/corpus/fuzz_formatter/` for the same
     reason as above.

The `fuzz_plan` finding was filed + fixed as cy-f2t (see the seed noted
above). The `fuzz_formatter` finding remains open. Until it lands, the
PR-gate fuzz-smoke CI step has `continue-on-error: true` (see
`.github/workflows/ci.yml`) so unrelated PRs don't get blocked.

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

Scaffolding lives at `oss-fuzz/` (bead cy-h07):

- `oss-fuzz/project.yaml` — manifest for the upstream `google/oss-fuzz`
  submission (contacts, sanitizers, engine, architectures).
- `oss-fuzz/Dockerfile` — builder image extending
  `gcr.io/oss-fuzz-base/base-builder-rust`.
- `oss-fuzz/build.sh` — builds every target, copies binaries +
  per-target dictionaries + zipped seed corpora into `$OUT`.
- `oss-fuzz/README.md` — submission flow + corpus-sync instructions.

**Submission is operator-gated.** Do NOT open the upstream PR without
explicit approval; see `oss-fuzz/README.md` §Submitting for the exact
flow (fork, mirror, local sanity pass, PR against
`google/oss-fuzz:master`).

### Corpus auto-pull from OSS-Fuzz

Once the project is live on OSS-Fuzz, the ClusterFuzz bucket is the
canonical corpus. Pull the current state into the local tree for
reproduction and expanded nightly runs:

```sh
# Requires gsutil + Google auth with read access to the public bucket.
for target in fuzz_lexer fuzz_parser fuzz_formatter fuzz_sema fuzz_plan \
              fuzz_structured_parse fmt_parse_roundtrip; do
    gsutil -m rsync -d \
        "gs://cyrs-corpus.clusterfuzz-external.appspot.com/libFuzzer/cyrs_${target}/" \
        "fuzz/corpus/${target}/"
done
```

Commit only the files that start with `seed` (or that we author by hand
as reproducers). ClusterFuzz names mutated entries by content hash;
those are covered by the `fuzz/corpus/**/[0-9a-f]{8}*` ignore rule in
`.gitignore` and should NOT be committed — they would inflate repo
size without adding signal a nightly rebuild can't produce.

### Regression minimisation from an OSS-Fuzz report

ClusterFuzz emails the primary contact when a new crash is confirmed,
with a link to the reproducer. The standard minimisation flow:

```sh
# Download the reproducer ClusterFuzz flags.
curl -o /tmp/crash "$REPRODUCER_URL"

# Feed it back into libFuzzer to confirm it still reproduces.
cargo +nightly fuzz run "$target" /tmp/crash

# Minimise.
cargo +nightly fuzz tmin "$target" /tmp/crash
# → writes fuzz/artifacts/$target/minimized-from-* next to the input.

# Promote the minimised reproducer into the corpus so the next nightly
# regresses on it deterministically.
mv fuzz/artifacts/$target/minimized-from-* \
   "fuzz/corpus/$target/seed_$(date +%Y%m%d)_${target}_oss_fuzz"
```

Then follow the `## Crash triage playbook` flow above (classify →
regression test → fix → commit both).
