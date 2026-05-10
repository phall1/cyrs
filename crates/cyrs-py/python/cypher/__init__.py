"""Python bindings for the Cyrs Cypher / GQL frontend.

Spec 0004 §6.  Thin wrapper re-exporting the Rust extension module's
public surface so callers can write ``import cypher`` instead of
``from cypher._cypher import ...``.  The Rust crate lives in
``crates/cyrs-py``; this file is shipped alongside the compiled
extension module inside the wheel (see ``pyproject.toml``'s
``[tool.maturin] python-source = "python"`` pin).
"""

from cypher._cypher import (  # noqa: F401  (re-export)
    PROTO_VERSION,
    CypherDatabase,
    Diagnostic,
)

__all__ = [
    "PROTO_VERSION",
    "CypherDatabase",
    "Diagnostic",
]
