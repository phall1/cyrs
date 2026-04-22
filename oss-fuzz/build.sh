#!/usr/bin/env bash
# OSS-Fuzz build entry point for cyrs.
#
# This script is executed inside the OSS-Fuzz builder container
# (gcr.io/oss-fuzz-base/base-builder-rust). It must:
#
#   1. Build every fuzz target with the sanitizer OSS-Fuzz is asking for
#      (ASan or UBSan; passed via $SANITIZER).
#   2. Copy each resulting binary into $OUT so ClusterFuzz picks it up.
#   3. Copy each target's seed corpus (zipped) and dictionary into $OUT
#      as well — filenames follow the OSS-Fuzz convention
#      `<fuzzer>_seed_corpus.zip` and `<fuzzer>.dict`.
#
# Docs: https://google.github.io/oss-fuzz/getting-started/new-project-guide/rust-lang/
#
# NOTE: this script is NOT used by cyrs's own CI. Our PR gate and nightly
# runs invoke cargo-fuzz directly (see .github/workflows/{ci,fuzz-nightly}.yml).
# The file exists so the operator can drop it into google/oss-fuzz under
# projects/cyrs/build.sh without further editing.

set -euo pipefail

# OSS-Fuzz sets these; fail fast locally if someone runs this script by hand.
: "${OUT:?OUT env var not set (OSS-Fuzz builder sets this)}"
: "${SRC:?SRC env var not set (OSS-Fuzz builder sets this)}"

# List of fuzz targets — keep in sync with fuzz/Cargo.toml [[bin]] entries.
# Each target name corresponds to:
#   fuzz/fuzz_targets/<target>.rs
#   fuzz/corpus/<target>/
#   fuzz/dicts/<target>.dict   (except fuzz_structured_parse, see below)
readonly TARGETS=(
    fuzz_lexer
    fuzz_parser
    fuzz_formatter
    fuzz_sema
    fuzz_plan
    fuzz_structured_parse
    fmt_parse_roundtrip
)

cd "$SRC/cyrs/fuzz"

# cargo-fuzz is pre-installed in the base-builder-rust image; double-check
# so builds fail with a clear message rather than a cryptic "no such command".
if ! command -v cargo-fuzz >/dev/null; then
    echo "build.sh: cargo-fuzz not found on PATH" >&2
    exit 1
fi

# cargo-fuzz honours $RUSTFLAGS from OSS-Fuzz which injects the sanitizer
# and coverage flags. We only need to kick off the build.
for target in "${TARGETS[@]}"; do
    echo "==> building $target"
    cargo fuzz build "$target"

    # cargo-fuzz stores binaries under fuzz/target/<triple>/release/.
    # Copy each into $OUT/ with the bare target name, which is what
    # ClusterFuzz expects.
    bin=$(find "target" -type f -name "$target" -executable -print -quit)
    if [[ -z ${bin:-} ]]; then
        echo "build.sh: could not locate built binary for $target" >&2
        exit 1
    fi
    cp "$bin" "$OUT/$target"

    # Per-target dictionary (skip the RNG-seeded structured target).
    if [[ $target != "fuzz_structured_parse" ]]; then
        cp "dicts/$target.dict" "$OUT/$target.dict"
    fi

    # Zip the seed corpus if it has any entries.
    if [[ -d "corpus/$target" ]] && compgen -G "corpus/$target/*" >/dev/null; then
        (cd "corpus/$target" && zip -qr "$OUT/${target}_seed_corpus.zip" .)
    fi
done

echo "==> build.sh done; artifacts in $OUT"
