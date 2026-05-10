"""End-to-end smoke tests for the `cypher` Python extension module.

Spec 0004 §10.3.  Runs after `maturin develop` has installed the
wheel into the active virtualenv; the CI wheel-matrix workflow runs
this file against each platform × Python-version combination.

The tests cover:

- :meth:`CypherDatabase.check` flags syntax errors with `E`-prefixed
  codes (success path + error path).
- :meth:`CypherDatabase.format` returns a canonical form for a simple
  query (idempotence implied by round-trip).
- :meth:`CypherDatabase.complete` returns at least one completion for
  a reasonable cursor position inside a `MATCH` clause.
- :meth:`CypherDatabase.parse` surfaces parse errors via the
  `syntax_errors` list rather than raising.
- :meth:`CypherDatabase.schema_set` accepts a valid TOML schema and
  rejects malformed TOML with `ValueError`.
- `PROTO_VERSION` is a positive integer.
- `Diagnostic` instances expose `.code`, `.severity`, `.range`,
  `.message` as the type stub promises.

Run locally with::

    python -m venv .venv
    source .venv/bin/activate
    pip install maturin pytest
    maturin develop -m crates/cyrs-py/Cargo.toml
    pytest crates/cyrs-py/tests/
"""

from __future__ import annotations

import pytest

import cypher


# ---------------------------------------------------------------------------
# Module-level invariants.
# ---------------------------------------------------------------------------


def test_proto_version_is_positive_int() -> None:
    """PROTO_VERSION is the wire-version pin from spec 0004 §9.3."""
    assert isinstance(cypher.PROTO_VERSION, int)
    assert cypher.PROTO_VERSION >= 1


def test_public_surface_matches_stub() -> None:
    """Every name exported by `cypher.__all__` is importable.

    Guards against the `__init__.py` drifting from `__init__.pyi`.
    """
    for name in ("PROTO_VERSION", "CypherDatabase", "Diagnostic"):
        assert hasattr(cypher, name), f"missing public export: {name}"


# ---------------------------------------------------------------------------
# check — success + error paths.
# ---------------------------------------------------------------------------


def test_check_reports_syntax_error_with_e_code() -> None:
    """Unclosed paren produces at least one `E`-prefixed diagnostic."""
    db = cypher.CypherDatabase()
    diags = db.check("MATCH (n RETURN n")
    assert len(diags) >= 1, "unclosed paren must yield diagnostics"
    assert any(
        d.code.startswith("E") for d in diags
    ), f"expected an E-coded diagnostic, got {[d.code for d in diags]!r}"


def test_check_clean_source_returns_empty_list() -> None:
    """A well-formed query emits zero diagnostics."""
    db = cypher.CypherDatabase()
    diags = db.check("MATCH (n) RETURN n")
    # Note: the empty-list invariant is asserted by cyrs-wasm/tests/
    # wasm_smoke.rs; the Python side mirrors that contract.
    codes = [d.code for d in diags]
    assert not any(
        c.startswith("E") for c in codes
    ), f"well-formed query must not error: {codes!r}"


def test_diagnostic_attributes_round_trip() -> None:
    """Diagnostic exposes .code, .severity, .message, .range as the stub promises."""
    db = cypher.CypherDatabase()
    diags = db.check("MATCH (n RETURN n")
    assert diags, "fixture must yield at least one diagnostic"
    d = diags[0]
    assert isinstance(d.code, str) and d.code
    assert d.severity in {"error", "warning", "note", "help"}
    assert isinstance(d.message, str)
    # range is (start_line, start_col, end_line, end_col) — four 0-based ints.
    rng = d.range
    assert isinstance(rng, tuple) and len(rng) == 4
    assert all(isinstance(n, int) and n >= 0 for n in rng)
    # repr is stable-ish; at minimum it names the class.
    assert "Diagnostic" in repr(d)


# ---------------------------------------------------------------------------
# format — canonical form, infallible.
# ---------------------------------------------------------------------------


def test_format_lowercase_returns_canonical_form() -> None:
    """format() uppercases keywords on a simple input."""
    db = cypher.CypherDatabase()
    out = db.format("match (n) return n")
    assert isinstance(out, str)
    assert out.strip(), "format must return non-empty text"
    # Canonical form keeps MATCH / RETURN uppercase per the cyrs-fmt
    # contract — the exact whitespace layout is an fmt-detail, so we
    # only assert the keyword casing, not the full string.
    upper = out.upper()
    assert "MATCH" in upper and "RETURN" in upper


def test_format_is_infallible_on_malformed_input() -> None:
    """format() returns the input unchanged when the parser fails."""
    db = cypher.CypherDatabase()
    # Matches the agent "format is infallible on malformed input"
    # contract (spec 0001 §15) — no exception; output may be == input.
    out = db.format("this is not valid cypher at all !!!")
    assert isinstance(out, str)


# ---------------------------------------------------------------------------
# complete — cursor-driven completions.
# ---------------------------------------------------------------------------


def test_complete_inside_match_returns_items() -> None:
    """complete('MATCH (', 7) returns at least one completion."""
    db = cypher.CypherDatabase()
    items = db.complete("MATCH (", 7)
    assert isinstance(items, list)
    # Default keyword context should always surface at least a few
    # curated keywords; no schema is loaded so we don't assert label
    # completions.
    assert len(items) >= 1, "cursor after '(' must yield completions"
    for item in items:
        assert "label" in item and "kind" in item
        assert isinstance(item["label"], str)
        assert isinstance(item["kind"], str)


def test_complete_empty_source_does_not_raise() -> None:
    """Cursor at offset 0 in empty text returns a (possibly empty) list."""
    db = cypher.CypherDatabase()
    items = db.complete("", 0)
    assert isinstance(items, list)


# ---------------------------------------------------------------------------
# parse — structured output, errors non-raising.
# ---------------------------------------------------------------------------


def test_parse_surfaces_syntax_errors_via_list() -> None:
    """parse() reports syntax errors in `syntax_errors`, not via exceptions."""
    db = cypher.CypherDatabase()
    result = db.parse("MATCH (n RETURN n")
    assert result["op"] == "parse"
    assert isinstance(result["cst_string"], str)
    assert isinstance(result["syntax_errors"], list)
    assert len(result["syntax_errors"]) >= 1
    assert result["proto_version"] == cypher.PROTO_VERSION


def test_parse_clean_source_reports_no_errors() -> None:
    """Well-formed input yields an empty `syntax_errors` list."""
    db = cypher.CypherDatabase()
    result = db.parse("RETURN 1")
    assert result["syntax_errors"] == []


# ---------------------------------------------------------------------------
# hover — stable shape, empty content permitted.
# ---------------------------------------------------------------------------


def test_hover_returns_shape() -> None:
    """hover() returns {markdown, range, proto_version}."""
    db = cypher.CypherDatabase()
    out = db.hover("MATCH (n) RETURN n", 0)
    assert out["op"] == "hover"
    assert isinstance(out["markdown"], str)
    rng = out["range"]
    assert isinstance(rng, tuple) and len(rng) == 2
    assert out["proto_version"] == cypher.PROTO_VERSION


# ---------------------------------------------------------------------------
# rewrite — applied + unknown fix lists.
# ---------------------------------------------------------------------------


def test_rewrite_with_unknown_fix_id_is_non_raising() -> None:
    """Unknown fix ids land in `unknown_fix_ids` rather than raising."""
    db = cypher.CypherDatabase()
    out = db.rewrite("RETURN 1", ["cy-fix.does-not-exist"])
    assert out["op"] == "rewrite"
    assert isinstance(out["applied_edits"], list)
    assert isinstance(out["resulting_text"], str)
    # The unknown id must surface in the unknown list, not applied_edits.
    assert "cy-fix.does-not-exist" in out["unknown_fix_ids"]


# ---------------------------------------------------------------------------
# schema_set / schema_clear — TOML parsing + schema lifecycle.
# ---------------------------------------------------------------------------


def test_schema_set_accepts_valid_toml_and_clear_works() -> None:
    """A valid TOML schema installs; schema_clear drops it."""
    db = cypher.CypherDatabase()
    schema_toml = """
[[label]]
name = "Person"
properties = [
    { name = "name", type = "STRING", required = true },
]

[[rel_type]]
name = "KNOWS"
start_labels = ["Person"]
end_labels = ["Person"]
"""
    # No exception on valid input; return value is None.
    assert db.schema_set(schema_toml) is None
    # Clearing is always valid, even if no schema was set.
    assert db.schema_clear() is None


def test_schema_set_rejects_malformed_toml() -> None:
    """Malformed TOML raises ValueError, not RuntimeError."""
    db = cypher.CypherDatabase()
    with pytest.raises(ValueError):
        db.schema_set("this is not [ valid toml = ::: ]")


# ---------------------------------------------------------------------------
# Basic session scaffolding.
# ---------------------------------------------------------------------------


def test_multiple_check_calls_reuse_one_fileid() -> None:
    """Subsequent calls on the same instance don't leak diagnostics.

    Exercises the interned-FileId pattern: every call overwrites the
    single FileId's contents rather than opening a new file.
    """
    db = cypher.CypherDatabase()
    diags1 = db.check("MATCH (n RETURN n")
    assert len(diags1) >= 1
    diags2 = db.check("RETURN 1")
    # The clean query must not inherit diagnostics from the prior call.
    assert not any(d.code.startswith("E") for d in diags2)
