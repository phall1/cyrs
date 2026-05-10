# Vendored openCypher TCK — Pin Record

This directory contains a vendored copy of the openCypher Technology
Compatibility Kit (TCK).  It is the upstream corpus the `cyrs-tck`
`full-tck` Cargo feature runs against.

See spec 0001 §17.5 and bead cy-p5q (cy-7s6.9) for the reasoning.

## Upstream source

- Repository: <https://github.com/opencypher/openCypher>
- Path within repo: `tck/`
- Pinned revision: **`677cbafabb8c3c5eed458fd3b1ec0daec8d67d23`**
  (tag `2024.3`, 2026-03-20)

## What was copied

| Vendored path              | Upstream path             |
| -------------------------- | ------------------------- |
| `full/features/clauses/`   | `tck/features/clauses/`   |
| `full/features/expressions/` | `tck/features/expressions/` |
| `full/features/useCases/`  | `tck/features/useCases/`  |
| `full/graphs/`             | `tck/graphs/`             |
| `full/README.adoc`         | `tck/README.adoc`         |
| `full/index.adoc`          | `tck/index.adoc`          |
| `full/LICENSE`             | `LICENSE` (repo root)     |
| `full/NOTICE`              | `NOTICE` (repo root)      |

- 220 Gherkin `.feature` files, 1339 scenarios, ~1.9 MB uncompressed.

## Vendoring method

**Direct vendoring** (not a `git submodule`).  The corpus totals ~1.9 MB,
well under the 50 MB threshold in the bead brief, and direct vendoring
keeps `cargo test -p cyrs-tck --features full-tck` working without a
submodule init step (relevant to CI sandboxing, spec §17).

## License

The openCypher TCK is distributed under the Apache License 2.0, which is
compatible with cyrs' dual MIT / Apache-2.0 licensing.  The original
`LICENSE` and `NOTICE` files from the upstream repo are preserved
alongside the corpus.

Each vendored `.feature` file still carries the upstream per-file
Apache-2.0 header and the "Attribution Notice under the terms of the
Apache License 2.0" paragraph that names the openCypher Implementers
Group — do not strip these headers.

## Updating the pin

To refresh against a newer upstream release:

1. Clone `opencypher/openCypher` at the target tag / commit.
2. Replace `full/features/`, `full/graphs/`, `full/README.adoc`,
   `full/index.adoc` with the upstream `tck/*` equivalents.
3. Re-copy `LICENSE` and `NOTICE` from the upstream repo root.
4. Update the "Pinned revision" line above.
5. Re-run `cargo xtask tck-baseline` to regenerate
   `../full-baseline.md` with the new pass-rate snapshot.
6. Commit everything together.

Do **not** edit vendored feature files in place; file upstream PRs
instead.
