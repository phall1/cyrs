#!/usr/bin/env bash
# Tiny wrapper that points at the release binary built from the repo root.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/../../target/release/cypher"
if [[ ! -x "$BIN" ]]; then
    echo "build cypher-cli first: (cd ../.. && cargo build --release -p cypher-cli)" >&2
    exit 2
fi
"$BIN" check "$HERE/example.cyp"
