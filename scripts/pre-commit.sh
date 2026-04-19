#!/usr/bin/env sh
# cypher pre-commit gate (spec 0001 §4.3).
# Invoked by .git/hooks/pre-commit. Runs the xtask gate on the cypher
# workspace. Fails the commit on any non-zero exit.

set -eu

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root/cypher"

exec cargo xtask gate
