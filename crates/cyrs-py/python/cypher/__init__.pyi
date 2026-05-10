"""Type stubs for the `cypher` Python extension module.

Spec 0004 §6.3.  Hand-written, `mypy --strict`-clean.  Mirrors the
Rust-side `#[pyclass]` / `#[pymethods]` surface in `crates/cyrs-py/
src/lib.rs`; drift is caught by the CI gate that runs `mypy --strict`
against the built wheel.

Enums over Rust `#[non_exhaustive]` types (diagnostic code / severity /
completion kind) are exposed as plain `str` — downstream consumers
match on the string values, never a closed `Enum`, so spec 0004 §6.3's
"stubs track non_exhaustive" rule is satisfied by construction.
"""

from typing import Final, List, Optional, Tuple, TypedDict

__all__ = [
    "PROTO_VERSION",
    "CypherDatabase",
    "Diagnostic",
    "ParseResult",
    "HoverResult",
    "RewriteResult",
    "CompletionItem",
    "EditRecord",
]

# ---------------------------------------------------------------------------
# Module-level constants.
# ---------------------------------------------------------------------------

PROTO_VERSION: Final[int]
"""Wire protocol version (spec 0004 §9.3).  Callers should verify this
matches the value their code was built against before dispatching any
op; a mismatch is a hard-stop per the stability policy."""

# ---------------------------------------------------------------------------
# Structured return types.  `TypedDict`s keep mypy honest about what
# keys live in the returned dicts without freezing the Python side on a
# closed class — matches the `dict[str, Any]` shape the Rust code
# actually produces.
# ---------------------------------------------------------------------------

class CompletionItem(TypedDict, total=False):
    """Single completion proposal.

    * ``label`` — human-visible label shown in the completion list.
    * ``kind`` — one of ``"keyword" | "label" | "relationship_type" |
      "parameter" | "property" | "other"``.  Non-exhaustive — new
      kinds may appear in minor releases; do not `match` exhaustively
      (spec 0004 §6.3).
    * ``detail`` — optional right-aligned detail string; present only
      when the upstream engine supplied one.
    """

    label: str
    kind: str
    detail: str

class ParseResult(TypedDict):
    """Return shape of :meth:`CypherDatabase.parse`."""

    op: str
    cst_string: str
    syntax_errors: List[str]
    proto_version: int

class HoverResult(TypedDict):
    """Return shape of :meth:`CypherDatabase.hover`."""

    op: str
    markdown: str
    range: Tuple[int, int]
    proto_version: int

class EditRecord(TypedDict):
    """One applied edit inside :data:`RewriteResult.applied_edits`."""

    fix_id: str
    range: Tuple[int, int]
    replacement: str

class RewriteResult(TypedDict):
    """Return shape of :meth:`CypherDatabase.rewrite`."""

    op: str
    applied_edits: List[EditRecord]
    resulting_text: str
    unknown_fix_ids: List[str]
    proto_version: int

# ---------------------------------------------------------------------------
# Diagnostic — frozen `#[pyclass]` in Rust, immutable from Python.
# ---------------------------------------------------------------------------

class Diagnostic:
    """A single diagnostic produced by the analysis pipeline.

    Instances are constructed only by :meth:`CypherDatabase.check`;
    Python callers treat them as read-only value objects (spec 0004
    §6).  The underlying ``#[pyclass(frozen)]`` blocks mutation from
    Python.
    """

    @property
    def code(self) -> str:
        """Stable diagnostic code, e.g. ``"E0001"`` (spec 0001 §10).

        The registry lives in ``cyrs-diag/src/codes.rs`` and is
        ``#[non_exhaustive]`` — new codes may appear in minor releases.
        """

    @property
    def severity(self) -> str:
        """Lower-case severity tag.

        One of ``"error" | "warning" | "help" | "note"``.  Non-
        exhaustive — new severities may appear in minor releases
        (spec 0004 §6.3).
        """

    @property
    def message(self) -> str:
        """Human-readable diagnostic message."""

    @property
    def range(self) -> Tuple[int, int, int, int]:
        """Primary span as ``(start_line, start_col, end_line, end_col)``.

        Columns are 0-based UTF-8 offsets (spec 0001 §4.5).
        """

    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# CypherDatabase — the sole Python-facing handle.
# ---------------------------------------------------------------------------

class CypherDatabase:
    """Python handle wrapping one Cyrs language-services engine.

    Each instance owns a single interned ``FileId`` reused across
    calls, so the analysis cache is warm after the first query.  The
    object is *not* thread-safe — callers that want parallel analysis
    should instantiate one ``CypherDatabase`` per thread.

    Every method mirrors the agent v1 op of the same name (spec 0001
    §15 / spec 0004 §6).  See the crate-level README for a fuller
    walkthrough.
    """

    def __init__(self) -> None:
        """Construct a fresh database with no interned file and no schema."""

    def parse(self, text: str) -> ParseResult:
        """Parse ``text`` into a lossless CST.

        Returns a dict with the CST pretty-print and a list of raw
        parser error messages.  Malformed source is *not* an
        exception — it is surfaced via ``syntax_errors``.  Raises
        ``RuntimeError`` only on database failure (which indicates a
        bug upstream in ``cyrs-db``).
        """

    def check(self, text: str) -> List[Diagnostic]:
        """Run the full analysis pipeline (parse + sema) and return all diagnostics.

        An empty list means "source is clean".  Diagnostic codes start
        with ``'E'`` (error), ``'W'`` (warning), ``'N'`` (note), or
        ``'H'`` (help).
        """

    def complete(self, text: str, offset: int) -> List[CompletionItem]:
        """Return completion items at byte offset ``offset`` in ``text``.

        ``offset`` is a 0-based UTF-8 byte offset, not a code-point or
        character index.  Out-of-range offsets return an empty list.
        """

    def hover(self, text: str, offset: int) -> HoverResult:
        """Return the hover markdown + byte range at ``offset``.

        An empty ``"markdown"`` value means "no content at cursor".
        ``offset`` is a 0-based UTF-8 byte offset.
        """

    def format(self, text: str) -> str:
        """Return the canonically-formatted source text.

        Formatter failure is infallible from Python's perspective: on
        malformed input the returned value is ``text`` unchanged (same
        contract as the agent wire API, spec 0001 §15).  Callers that
        want the error message should call :meth:`check` instead.
        """

    def rewrite(self, text: str, fix_ids: List[str]) -> RewriteResult:
        """Apply the listed ``fix_ids`` to ``text``.

        ``fix_ids`` are the strings attached to ``Diagnostic.fixes`` —
        any unknown id lands in the returned ``unknown_fix_ids`` list
        instead of raising.
        """

    def schema_set(self, toml: str) -> None:
        """Install a schema from a TOML string.

        The TOML shape matches the CLI / agent — delegate to
        ``cyrs_schema::file::load_from_toml_str`` on the Rust side.
        Raises :class:`ValueError` with the parser's error message on
        malformed input.
        """

    def schema_clear(self) -> None:
        """Drop the in-session schema, reverting to "no schema loaded"."""
