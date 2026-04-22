#!/usr/bin/env sh
# Install the cypher pre-commit hook. Safe to re-run.
# Spec 0001 §4.3.

set -eu

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(git rev-parse --show-toplevel)"
hook_src="$script_dir/pre-commit.sh"
hook_dst="$repo_root/.git/hooks/pre-commit"

if [ ! -f "$hook_src" ]; then
  echo "pre-commit.sh not found at $hook_src" >&2
  exit 1
fi

mkdir -p "$repo_root/.git/hooks"
cp "$hook_src" "$hook_dst"
chmod +x "$hook_dst"
echo "Installed pre-commit hook -> $hook_dst"
