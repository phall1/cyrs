#!/usr/bin/env sh
# cyrs pre-commit gate (spec 0001 §4.3).
# Invoked by .git/hooks/pre-commit. Runs the xtask gate on the
# workspace. Fails the commit on any non-zero exit.

set -eu

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

exec cargo xtask gate
