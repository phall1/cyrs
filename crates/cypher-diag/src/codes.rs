//! Diagnostic code registry (spec 0001 §10.2).
//!
//! Codes are **stable**. Once assigned, a code's meaning cannot change.
//! New checks get new codes; removed checks leave their code retired,
//! never reused. CI (§17.2 / §17.6) enforces uniqueness.
//!
//! Code ranges:
//!
//! | Range           | Meaning                                  |
//! | --------------- | ---------------------------------------- |
//! | `E0001..=E0999` | Syntax (lexer + parser)                  |
//! | `E1000..=E1999` | Name resolution                          |
//! | `E2000..=E2999` | Semantic — schema-free                   |
//! | `E3000..=E3999` | Semantic — schema-aware                  |
//! | `E4000..=E4999` | Dialect / compatibility                  |
//! | `E5000..=E5999` | Type system                              |
//! | `W6000..=W6999` | Style / lint warnings                    |
//! | `W7000..=W7999` | Performance warnings                     |
//! | `N8000..=N8999` | Informational notes                      |

use core::fmt;

/// Stable diagnostic code. Rendered as `E0001` / `W6001` / `N8001`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(non_camel_case_types)]
pub enum DiagCode {
    // --- syntax ------------------------------------------------------
    /// Reserved for tests; stands for "generic syntax error" until the
    /// grammar lands and carves specific codes.
    E0001 = 1,
    /// Unexpected token.
    E0002 = 2,
    /// Expected `<token>`, found `<token>`.
    E0003 = 3,
    /// Unclosed string literal.
    E0004 = 4,
    /// Unclosed block comment.
    E0005 = 5,
    /// Invalid numeric literal.
    E0006 = 6,

    // --- name resolution (1000..) ------------------------------------
    /// Unresolved variable.
    E1001 = 1001,
    /// Variable shadows an outer binding.
    E1002 = 1002,
    /// Variable used before binding.
    E1003 = 1003,
    /// Kind mismatch (e.g., path variable used as node).
    E1004 = 1004,
    /// Relationship variable repeated in same MATCH.
    E1005 = 1005,

    // --- semantic schema-free (2000..) -------------------------------
    /// Aggregation outside a projection context.
    E2001 = 2001,
    /// Nested aggregation.
    E2002 = 2002,
    /// Illegal clause ordering.
    E2003 = 2003,
    /// ORDER BY over invisible variable.
    E2004 = 2004,
    /// Parameter used with incompatible types.
    E2005 = 2005,
    /// `RETURN *` with empty scope.
    E2006 = 2006,

    // --- semantic schema-aware (3000..) ------------------------------
    /// Unknown label.
    E3001 = 3001,
    /// Unknown relationship type.
    E3002 = 3002,
    /// Unknown property on label.
    E3003 = 3003,
    /// Property type mismatch.
    E3004 = 3004,
    /// Relationship endpoint mismatch.
    E3005 = 3005,
    /// Unknown function.
    E3006 = 3006,
    /// Function arity mismatch.
    E3007 = 3007,
    /// Unknown procedure.
    E3008 = 3008,

    // --- dialect (4000..) --------------------------------------------
    /// Feature requires a different dialect mode.
    E4001 = 4001,
    /// Construct is deferred (spec §19 out-of-scope).
    E4002 = 4002,

    // --- type system (5000..) ----------------------------------------
    /// Structural type error (e.g., indexing a boolean).
    E5001 = 5001,
    /// Arithmetic on non-numeric operands.
    E5002 = 5002,

    // --- warnings (6000..) -------------------------------------------
    /// Dead WITH — projection with no downstream reader.
    W6001 = 6001,
    /// Identifier collides with a reserved keyword; needs backtick quoting.
    W6002 = 6002,
    /// Duplicate key in map literal — last write wins.
    W6003 = 6003,
    /// Variable bound but never read.
    W6004 = 6004,
    /// Redundant OPTIONAL MATCH — no bound variables escape the clause.
    W6005 = 6005,
    /// Pattern has no label or type restriction — will scan broadly.
    W6006 = 6006,
    /// Inconsistent keyword casing inside one query.
    W6007 = 6007,

    // --- performance (7000..) ----------------------------------------
    /// Cartesian product between disconnected MATCH components.
    W7001 = 7001,
    /// Expensive function call inside a row-wise filter.
    W7002 = 7002,
    /// Variable-length path without an upper bound.
    W7003 = 7003,
    /// Property access on an unindexed label in a selective filter.
    W7004 = 7004,

    // --- notes (8000..) ----------------------------------------------
    /// Informational — pattern normalised to canonical direction.
    N8001 = 8001,
    /// Informational — inferred type of an expression.
    N8002 = 8002,
    /// Informational — variable dropped from scope by this projection.
    N8003 = 8003,
}

impl DiagCode {
    /// Render as the stable wire-format string: `E0001`, `W6001`, `N8001`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E0001 => "E0001",
            Self::E0002 => "E0002",
            Self::E0003 => "E0003",
            Self::E0004 => "E0004",
            Self::E0005 => "E0005",
            Self::E0006 => "E0006",
            Self::E1001 => "E1001",
            Self::E1002 => "E1002",
            Self::E1003 => "E1003",
            Self::E1004 => "E1004",
            Self::E1005 => "E1005",
            Self::E2001 => "E2001",
            Self::E2002 => "E2002",
            Self::E2003 => "E2003",
            Self::E2004 => "E2004",
            Self::E2005 => "E2005",
            Self::E2006 => "E2006",
            Self::E3001 => "E3001",
            Self::E3002 => "E3002",
            Self::E3003 => "E3003",
            Self::E3004 => "E3004",
            Self::E3005 => "E3005",
            Self::E3006 => "E3006",
            Self::E3007 => "E3007",
            Self::E3008 => "E3008",
            Self::E4001 => "E4001",
            Self::E4002 => "E4002",
            Self::E5001 => "E5001",
            Self::E5002 => "E5002",
            Self::W6001 => "W6001",
            Self::W6002 => "W6002",
            Self::W6003 => "W6003",
            Self::W6004 => "W6004",
            Self::W6005 => "W6005",
            Self::W6006 => "W6006",
            Self::W6007 => "W6007",
            Self::W7001 => "W7001",
            Self::W7002 => "W7002",
            Self::W7003 => "W7003",
            Self::W7004 => "W7004",
            Self::N8001 => "N8001",
            Self::N8002 => "N8002",
            Self::N8003 => "N8003",
        }
    }

    /// Severity letter derived from the numeric range (spec §10.2).
    ///
    /// `0..=5999` → `'E'`, `6000..=7999` → `'W'`, `8000..=8999` → `'N'`.
    /// Panics if the discriminant falls outside any registered range —
    /// the [`ALL`](Self::ALL) invariants enforced by `tests/registry.rs`
    /// make this unreachable at runtime.
    #[must_use]
    pub const fn severity_char(self) -> char {
        match self as u32 {
            0..=5999 => 'E',
            6000..=7999 => 'W',
            8000..=8999 => 'N',
            _ => panic!("DiagCode discriminant outside any registered range"),
        }
    }

    /// Canonical enumeration of every registered diagnostic code, in
    /// numeric order. This is THE registry used by
    /// `tests/registry.rs` to enforce spec §10.2 invariants — every
    /// variant added to [`DiagCode`] must also be appended here.
    pub const ALL: &'static [DiagCode] = &[
        Self::E0001,
        Self::E0002,
        Self::E0003,
        Self::E0004,
        Self::E0005,
        Self::E0006,
        Self::E1001,
        Self::E1002,
        Self::E1003,
        Self::E1004,
        Self::E1005,
        Self::E2001,
        Self::E2002,
        Self::E2003,
        Self::E2004,
        Self::E2005,
        Self::E2006,
        Self::E3001,
        Self::E3002,
        Self::E3003,
        Self::E3004,
        Self::E3005,
        Self::E3006,
        Self::E3007,
        Self::E3008,
        Self::E4001,
        Self::E4002,
        Self::E5001,
        Self::E5002,
        Self::W6001,
        Self::W6002,
        Self::W6003,
        Self::W6004,
        Self::W6005,
        Self::W6006,
        Self::W6007,
        Self::W7001,
        Self::W7002,
        Self::W7003,
        Self::W7004,
        Self::N8001,
        Self::N8002,
        Self::N8003,
    ];
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::DiagCode;

    /// Every code rendered as a string has a unique textual form — the
    /// canonical registry uniqueness check (spec §10.2).
    #[test]
    fn codes_are_unique_strings() {
        let all = [
            DiagCode::E0001,
            DiagCode::E0002,
            DiagCode::E0003,
            DiagCode::E0004,
            DiagCode::E0005,
            DiagCode::E0006,
            DiagCode::E1001,
            DiagCode::E1002,
            DiagCode::E1003,
            DiagCode::E1004,
            DiagCode::E1005,
            DiagCode::E2001,
            DiagCode::E2002,
            DiagCode::E2003,
            DiagCode::E2004,
            DiagCode::E2005,
            DiagCode::E2006,
            DiagCode::E3001,
            DiagCode::E3002,
            DiagCode::E3003,
            DiagCode::E3004,
            DiagCode::E3005,
            DiagCode::E3006,
            DiagCode::E3007,
            DiagCode::E3008,
            DiagCode::E4001,
            DiagCode::E4002,
            DiagCode::E5001,
            DiagCode::E5002,
            DiagCode::W6001,
            DiagCode::W6002,
            DiagCode::W6003,
            DiagCode::W6004,
            DiagCode::W6005,
            DiagCode::W6006,
            DiagCode::W6007,
            DiagCode::W7001,
            DiagCode::W7002,
            DiagCode::W7003,
            DiagCode::W7004,
            DiagCode::N8001,
            DiagCode::N8002,
            DiagCode::N8003,
        ];
        let mut strs: Vec<_> = all.iter().map(|c| c.as_str()).collect();
        strs.sort_unstable();
        strs.dedup();
        assert_eq!(strs.len(), all.len(), "duplicate DiagCode string");
    }
}
