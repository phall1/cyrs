#!/usr/bin/env bash
# One-shot publish script for cyrs 0.1.0.
#
# Prereqs:
#   1. cargo login   (uses ~/.cargo/credentials.toml — paste your token)
#   2. From repo root: ./scripts/publish-0.1.0.sh
#
# Behavior:
#   - Publishes 19 crates in topological dep order.
#   - cyrs-py uses --no-verify (PyO3 extension-module symbols don't link).
#   - Sleeps 20s between each publish so crates.io's sparse index can
#     pick up the new version before the next crate tries to resolve it.
#   - If a crate is already published at 0.1.0, cargo errors with
#     "crate version `0.1.0` is already uploaded"; this script treats
#     that as success and continues (idempotent re-run).
#   - On any other failure, stops immediately. Re-run after fixing.

set -euo pipefail

CRATES=(
  cyrs-syntax
  cyrs-diag
  cyrs-ast
  cyrs-schema
  cyrs-project
  cyrs-hir
  cyrs-sema
  cyrs-plan
  cyrs-fmt
  cyrs-db
  cyrs-lang-services
  cyrs-wasm
  cyrs-ffi
  cyrs-py          # --no-verify
  cyrs-tck
  cyrs-lsp
  cyrs-agent
  cyrs-cli
  cyrs-lang
)

# Sanity: make sure we're on main and clean.
branch=$(git rev-parse --abbrev-ref HEAD)
if [[ "$branch" != "main" ]]; then
  echo "error: not on main (on '$branch'). aborting." >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree dirty. commit or stash first." >&2
  exit 1
fi

# Sanity: token present.
if ! grep -q '^token = ' "${CARGO_HOME:-$HOME/.cargo}/credentials.toml" 2>/dev/null && \
   [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "error: no crates.io token found." >&2
  echo "       run 'cargo login' first (or set CARGO_REGISTRY_TOKEN)." >&2
  exit 1
fi

publish_one() {
  local crate=$1
  local extra=()
  if [[ "$crate" == "cyrs-py" ]]; then
    extra+=(--no-verify)
  fi

  echo
  echo "================================================================"
  echo "  publishing $crate"
  echo "================================================================"

  if cargo publish -p "$crate" "${extra[@]}" 2>&1 | tee /tmp/cyrs-publish-"$crate".log; then
    echo "  ✓ $crate published"
  else
    if grep -q "is already uploaded" /tmp/cyrs-publish-"$crate".log; then
      echo "  · $crate already at 0.1.0 (skipping)"
    else
      echo
      echo "error: cargo publish -p $crate failed. log at /tmp/cyrs-publish-$crate.log" >&2
      exit 1
    fi
  fi
}

for crate in "${CRATES[@]}"; do
  publish_one "$crate"
  # Skip the sleep after the last crate.
  if [[ "$crate" != "${CRATES[-1]}" ]]; then
    echo "  (waiting 20s for crates.io index to propagate...)"
    sleep 20
  fi
done

echo
echo "================================================================"
echo "  done. all 19 crates published at 0.1.0."
echo "  next: gh release create v0.1.0 ... (if not done) + vsce publish"
echo "================================================================"
