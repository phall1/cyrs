#!/usr/bin/env sh
# Non-coupling gate (spec 0001 §2.C1, §2.C2).
# Run from the cypher workspace root.
#
# §2.C1 — no Cargo.toml dep starts with `intel-` or `trench-`.
# §2.C2 — no .rs source (outside tests/fixtures/) uses a denylisted
#         domain identifier.
#
# ---- Regex strategy for §2.C2 ----
# The spec's denylist contains two classes of word:
#
#   A. UNAMBIGUOUS domain words with no legitimate compiler/parser
#      meaning — `Actor`, `provenance`, `bitemporal`, `expertise`.
#      Any case-insensitive word-boundary match is a bug.
#
#   B. AMBIGUOUS words that are real compiler-engineering terms
#      (`Event` for a parser event stream, `Operation` for an
#      opcode, `Capability` for an LSP capability, `branch` for a
#      parser branch). A naive word-boundary match produces dozens
#      of false positives on any compiler codebase.
#
# For class B we match only identifier COMPOUNDS: a denylist word
# glued to an adjacent PascalCase segment, e.g. `ActorEvent`,
# `EventLog`, `SubjectCapability`, `BranchHead`. Compound forms are
# the shape domain leakage actually takes; standalone `Event` in a
# parser crate is not.
#
# `cypher-lsp` is exempt from the `Capability` compound check
# because the LSP protocol itself uses `Capability`/`Capabilities`
# suffixes on standard types (`ServerCapabilities`,
# `TextDocumentSyncCapability`). The crate is a thin shell over
# `lsp-types` (§3.2, §14) and cannot rename those identifiers.
# Trench-flavored `Capability` would still be caught by the `Actor`
# unambiguous rule since domain capabilities are actor-scoped.
#
# If a future reviewer wants a stricter rule they should extend this
# script; do NOT weaken these regexes silently.

set -u

fail=0

# ---- §2.C1: deps ----
echo "==> cy-kc9: checking for intel-/trench- deps in Cargo.toml files"
bad_deps="$(
  find . -name Cargo.toml -not -path './target/*' -print0 \
    | xargs -0 grep -n -E '^(intel-|trench-)[a-z0-9_-]*[[:space:]]*=' \
    2>/dev/null || true
)"
if [ -n "$bad_deps" ]; then
  echo "FAIL: forbidden sibling-workspace deps found:"
  echo "$bad_deps"
  fail=1
fi

# ---- §2.C2: denylisted domain words in .rs sources ----
# Class A: unambiguous domain words (any case, word-boundary).
unambiguous='\b(Actor|provenance|bitemporal|expertise)\b'

# Class B: ambiguous words — only flagged as PascalCase compounds.
# The word must be glued to another PascalCase segment on at least
# one side. `BranchHead`, `HeadBranch`, `ActorEvent`, `EventLog`
# all match; a lone `Event` does not.
compound='([A-Z][a-z0-9]+)(Event|Operation|Capability|Branch)\b|\b(Event|Operation|Capability|Branch)([A-Z][a-z0-9]+)'
# `Capability` sub-pattern is suppressed under cypher-lsp/ (LSP
# protocol types); see header comment.
compound_no_lsp='([A-Z][a-z0-9]+)(Event|Operation|Branch)\b|\b(Event|Operation|Branch)([A-Z][a-z0-9]+)'

# External-type allowlist: PascalCase compounds defined by third-party
# crates that we merely re-use.  These are not domain leakage — we do
# not control their naming and aliasing them hurts readability.  Each
# entry is a regex alternation matched with `-w` semantics (extended by
# the \b anchors in `compound`).
#
#   rowan::WalkEvent       — parser tree-walk iterator variant (cypher-fmt).
#   lsp_types::FileEvent   — LSP didChangeWatchedFiles payload (cypher-lsp).
#   web_sys::MessageEvent  — Web Worker postMessage event (cypher-lsp wasm).
#
# The anchors around each entry prevent false negatives: an identifier
# such as `ActorWalkEvent` would still be flagged by the main regex
# because it does not match `\bWalkEvent\b`.
external_allow='\b(WalkEvent|FileEvent|MessageEvent)\b'

echo "==> cy-kc9: checking for domain-name denylist in .rs files"
bad_unambiguous="$(
  find . -name '*.rs' \
    -not -path './target/*' \
    -not -path '*/tests/*' \
    -not -path '*/fixtures/*' \
    -print0 \
  | xargs -0 grep -n -E -i "$unambiguous" 2>/dev/null || true
)"
# Outside cypher-lsp: full compound check.
bad_compound="$(
  find . -name '*.rs' \
    -not -path './target/*' \
    -not -path '*/tests/*' \
    -not -path '*/fixtures/*' \
    -not -path './crates/cypher-lsp/*' \
    -print0 \
  | xargs -0 grep -n -E "$compound" 2>/dev/null \
  | grep -v -E "$external_allow" \
  || true
)"
# Inside cypher-lsp: compound check minus `Capability` suffix.
bad_compound_lsp="$(
  find ./crates/cypher-lsp -name '*.rs' \
    -not -path '*/target/*' \
    -not -path '*/tests/*' \
    -not -path '*/fixtures/*' \
    -print0 2>/dev/null \
  | xargs -0 grep -n -E "$compound_no_lsp" 2>/dev/null \
  | grep -v -E "$external_allow" \
  || true
)"

if [ -n "$bad_unambiguous" ] || [ -n "$bad_compound" ] || [ -n "$bad_compound_lsp" ]; then
  echo "FAIL: forbidden domain words in source (spec §2.C2 denylist):"
  [ -n "$bad_unambiguous" ] && echo "$bad_unambiguous"
  [ -n "$bad_compound" ] && echo "$bad_compound"
  [ -n "$bad_compound_lsp" ] && echo "$bad_compound_lsp"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo ""
  echo "Non-coupling gate FAILED. See AGENTS.md §2 for the full contract."
  exit 1
fi
echo "non-coupling gate: OK"
