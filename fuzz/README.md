# `cyrs` fuzz targets — spec 0001 §17.4

Five libFuzzer / cargo-fuzz targets exercising each pipeline stage:

| Target | Exercises |
|---|---|
| `fuzz_lexer` | `cypher_syntax::lex` — token stream from arbitrary bytes. |
| `fuzz_parser` | `cypher_syntax::parse` — recovering parser on arbitrary bytes. |
| `fuzz_formatter` | `cypher_fmt::format` — idempotence + no-panic on arbitrary input. |
| `fuzz_sema` | `cypher_hir::lower::lower_statement` + `cypher_sema::resolve` — name-resolution + kind checks. |
| `fuzz_plan` | `cypher_plan::lower::lower_statement` — HIR → plan lowering. |

## Running

```sh
# Single 5-minute PR-gate-style run.
cargo fuzz run fuzz_parser -- -max_total_time=300

# Shortcut: the xtask wraps this so the PR gate can re-use it.
cargo xtask fuzz fuzz_parser
```

24h nightly runs live in `.github/workflows/fuzz-nightly.yml`.

## Corpus layout

`fuzz/corpus/<target>/` holds the seed inputs for each target.
libFuzzer will mutate these into new inputs as it runs; durable
crash reproducers should be added under `fuzz/artifacts/<target>/`.

**Current seed strategy** (spec §17.4, bead cy-5tk): every `.cypher`
file under `crates/*/tests/ui/**` is copied into every target's
corpus.  These fixtures already exercise the grammar paths the
compiletest suite cares about, so libFuzzer starts with coverage of
every shipping clause + expression shape and mutates from there.

## Re-seeding from test fixtures

When new compiletest fixtures land, refresh the seeds with:

```sh
find crates -name '*.cypher' -not -name '*.formatted.*' -not -path '*/target/*' \
  | while IFS= read -r fixture; do
      name="seed_$(basename "$fixture" .cypher)"
      for target in fuzz_lexer fuzz_parser fuzz_formatter fuzz_sema fuzz_plan; do
        cp "$fixture" "fuzz/corpus/$target/$name"
      done
    done
```

The seed-filename prefix (`seed_…`) keeps the fixture-derived seeds
distinct from the `seed0`, `seed1`, `seedN` adversarial inputs the
targets were originally shipped with.

`.formatted.cypher` sidecars are the expected formatter output for
a given input — not meaningful fuzz seeds on their own, so the
loop above excludes them.

## Adding hand-written seeds

If a new target (or a new clause) needs targeted coverage that the
compiletest fixtures don't provide, drop the input into
`fuzz/corpus/<target>/my_seed_name`.  libFuzzer picks up any file
in the corpus directory — the filename is arbitrary.

## When fuzz finds a crash

1. libFuzzer writes the reproducer to `fuzz/artifacts/<target>/`
   with a `crash-<hash>` filename.
2. Commit the reproducer into `fuzz/artifacts/<target>/` so the
   next run regresses on it specifically (libFuzzer re-feeds any
   artifact it finds on startup).
3. File a **P0** bead referencing the crash hash, and attach a
   minimised input via `cargo fuzz tmin`.
