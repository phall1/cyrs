#!/usr/bin/env bash
# Pipe input.jsonl through cypher-agent and print responses one per line.
# Use `diff <(./run.sh) expected.jsonl` to sanity-check.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/../../target/release/cypher-agent"
if [[ ! -x "$BIN" ]]; then
    echo "build cypher-agent first: (cd ../.. && cargo build --release -p cypher-agent)" >&2
    exit 2
fi
"$BIN" < "$HERE/input.jsonl"
