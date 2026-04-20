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
    // --- syntax (E0001..=E0999) --------------------------------------
    /// Generic / unclassified syntax error.
    ///
    /// Docs: `docs/errors/E0001.md`
    E0001 = 1,
    /// Unexpected token encountered.
    ///
    /// Docs: `docs/errors/E0002.md`
    E0002 = 2,
    /// Expected `<token>`, found `<token>`.
    ///
    /// Docs: `docs/errors/E0003.md`
    E0003 = 3,
    /// Unclosed string literal (missing closing quote).
    ///
    /// Docs: `docs/errors/E0004.md`
    E0004 = 4,
    /// Unclosed block comment (missing `*/`).
    ///
    /// Docs: `docs/errors/E0005.md`
    E0005 = 5,
    /// Invalid numeric literal (bad digits or suffix).
    ///
    /// Docs: `docs/errors/E0006.md`
    E0006 = 6,
    /// Expected a statement (clause keyword) but found something else.
    ///
    /// Docs: `docs/errors/E0007.md`
    E0007 = 7,
    /// Expected `;` or end of input after statement.
    ///
    /// Docs: `docs/errors/E0008.md`
    E0008 = 8,
    /// Expected `(` to start a node pattern.
    ///
    /// Docs: `docs/errors/E0009.md`
    E0009 = 9,
    /// Expected a node pattern after a relationship pattern.
    ///
    /// Docs: `docs/errors/E0010.md`
    E0010 = 10,
    /// Expected `)` to close a node pattern.
    ///
    /// Docs: `docs/errors/E0011.md`
    E0011 = 11,
    /// Expected `-` at the start of a relationship pattern.
    ///
    /// Docs: `docs/errors/E0012.md`
    E0012 = 12,
    /// Expected `-` to close a left-arrow relationship pattern.
    ///
    /// Docs: `docs/errors/E0013.md`
    E0013 = 13,
    /// Expected `-` or `->` to close a relationship pattern.
    ///
    /// Docs: `docs/errors/E0014.md`
    E0014 = 14,
    /// Expected `]` to close a relationship detail block.
    ///
    /// Docs: `docs/errors/E0015.md`
    E0015 = 15,
    /// Expected a label name after `:`.
    ///
    /// Docs: `docs/errors/E0016.md`
    E0016 = 16,
    /// Expected a relationship type name after `:`.
    ///
    /// Docs: `docs/errors/E0017.md`
    E0017 = 17,
    /// Expected `}` to close a property map.
    ///
    /// Docs: `docs/errors/E0018.md`
    E0018 = 18,
    /// Expected a property key identifier.
    ///
    /// Docs: `docs/errors/E0019.md`
    E0019 = 19,
    /// Expected `:` separating a property key from its value.
    ///
    /// Docs: `docs/errors/E0020.md`
    E0020 = 20,
    /// Expected an expression for a property value.
    ///
    /// Docs: `docs/errors/E0021.md`
    E0021 = 21,
    /// Expected an identifier.
    ///
    /// Docs: `docs/errors/E0022.md`
    E0022 = 22,
    /// Expression nesting depth exceeds the parser limit.
    ///
    /// Docs: `docs/errors/E0023.md`
    E0023 = 23,
    /// Expected an operand after a unary operator.
    ///
    /// Docs: `docs/errors/E0024.md`
    E0024 = 24,
    /// Expected `NULL` after `IS` (or `IS NOT`).
    ///
    /// Docs: `docs/errors/E0025.md`
    E0025 = 25,
    /// Expected a right-hand side operand for a binary expression.
    ///
    /// Docs: `docs/errors/E0026.md`
    E0026 = 26,
    /// Expected an expression inside parentheses.
    ///
    /// Docs: `docs/errors/E0027.md`
    E0027 = 27,
    /// Expected `)` to close a parenthesised expression.
    ///
    /// Docs: `docs/errors/E0028.md`
    E0028 = 28,
    /// Expected `WITH` after `STARTS` (i.e. `STARTS WITH`).
    ///
    /// Docs: `docs/errors/E0029.md`
    E0029 = 29,
    /// Expected `WITH` after `ENDS` (i.e. `ENDS WITH`).
    ///
    /// Docs: `docs/errors/E0030.md`
    E0030 = 30,
    /// Expected a property key name after `.`.
    ///
    /// Docs: `docs/errors/E0031.md`
    E0031 = 31,
    /// Expected an index expression inside `[…]`.
    ///
    /// Docs: `docs/errors/E0032.md`
    E0032 = 32,
    /// Expected `]` to close a subscript / index expression.
    ///
    /// Docs: `docs/errors/E0033.md`
    E0033 = 33,
    /// Expected `)` to close a function call argument list.
    ///
    /// Docs: `docs/errors/E0034.md`
    E0034 = 34,
    /// Expected a function call argument expression.
    ///
    /// Docs: `docs/errors/E0035.md`
    E0035 = 35,
    /// Expected an expression in a `RETURN` item.
    ///
    /// Docs: `docs/errors/E0036.md`
    E0036 = 36,
    /// Expected an identifier after `AS` (alias).
    ///
    /// Docs: `docs/errors/E0037.md`
    E0037 = 37,
    /// Expected `BY` after `ORDER` (i.e. `ORDER BY`).
    ///
    /// Docs: `docs/errors/E0038.md`
    E0038 = 38,
    /// Expected an expression in an `ORDER BY` item.
    ///
    /// Docs: `docs/errors/E0039.md`
    E0039 = 39,
    /// Expected an expression after `SKIP`.
    ///
    /// Docs: `docs/errors/E0040.md`
    E0040 = 40,
    /// Expected an expression after `LIMIT`.
    ///
    /// Docs: `docs/errors/E0041.md`
    E0041 = 41,
    /// Expected `MATCH` after `OPTIONAL`.
    ///
    /// Docs: `docs/errors/E0042.md`
    E0042 = 42,
    /// Expected an expression after `WHERE`.
    ///
    /// Docs: `docs/errors/E0043.md`
    E0043 = 43,
    /// Clause keyword encountered that is not yet implemented (deferred construct).
    ///
    /// Docs: `docs/errors/E0044.md`
    E0044 = 44,
    /// Expected a clause keyword (`MATCH`, `WITH`, `RETURN`, …).
    ///
    /// Docs: `docs/errors/E0045.md`
    E0045 = 45,
    /// Invalid escape sequence in a string literal.
    ///
    /// Docs: `docs/errors/E0046.md`
    E0046 = 46,

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
    /// Variable used in arithmetic / numeric context but has non-Value kind
    /// (Node, Relationship, or Path). Schema-free kind-consistency check
    /// (spec §6.3).
    ///
    /// Docs: `docs/errors/E2007.md`
    E2007 = 2007,
    /// Variable of incorrect kind used in pattern position (spec §6.3).
    /// E.g., a `Value` variable where a node-pattern binder is expected, or
    /// a `Node` variable where a path binder is expected.
    ///
    /// Docs: `docs/errors/E2008.md`
    E2008 = 2008,

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
    /// Type mismatch in unification — two incompatible concrete types cannot
    /// be unified (spec §7.2, §7.3).
    ///
    /// Docs: `docs/errors/E5003.md`
    E5003 = 5003,

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
            Self::E0007 => "E0007",
            Self::E0008 => "E0008",
            Self::E0009 => "E0009",
            Self::E0010 => "E0010",
            Self::E0011 => "E0011",
            Self::E0012 => "E0012",
            Self::E0013 => "E0013",
            Self::E0014 => "E0014",
            Self::E0015 => "E0015",
            Self::E0016 => "E0016",
            Self::E0017 => "E0017",
            Self::E0018 => "E0018",
            Self::E0019 => "E0019",
            Self::E0020 => "E0020",
            Self::E0021 => "E0021",
            Self::E0022 => "E0022",
            Self::E0023 => "E0023",
            Self::E0024 => "E0024",
            Self::E0025 => "E0025",
            Self::E0026 => "E0026",
            Self::E0027 => "E0027",
            Self::E0028 => "E0028",
            Self::E0029 => "E0029",
            Self::E0030 => "E0030",
            Self::E0031 => "E0031",
            Self::E0032 => "E0032",
            Self::E0033 => "E0033",
            Self::E0034 => "E0034",
            Self::E0035 => "E0035",
            Self::E0036 => "E0036",
            Self::E0037 => "E0037",
            Self::E0038 => "E0038",
            Self::E0039 => "E0039",
            Self::E0040 => "E0040",
            Self::E0041 => "E0041",
            Self::E0042 => "E0042",
            Self::E0043 => "E0043",
            Self::E0044 => "E0044",
            Self::E0045 => "E0045",
            Self::E0046 => "E0046",
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
            Self::E2007 => "E2007",
            Self::E2008 => "E2008",
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
            Self::E5003 => "E5003",
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
        Self::E0007,
        Self::E0008,
        Self::E0009,
        Self::E0010,
        Self::E0011,
        Self::E0012,
        Self::E0013,
        Self::E0014,
        Self::E0015,
        Self::E0016,
        Self::E0017,
        Self::E0018,
        Self::E0019,
        Self::E0020,
        Self::E0021,
        Self::E0022,
        Self::E0023,
        Self::E0024,
        Self::E0025,
        Self::E0026,
        Self::E0027,
        Self::E0028,
        Self::E0029,
        Self::E0030,
        Self::E0031,
        Self::E0032,
        Self::E0033,
        Self::E0034,
        Self::E0035,
        Self::E0036,
        Self::E0037,
        Self::E0038,
        Self::E0039,
        Self::E0040,
        Self::E0041,
        Self::E0042,
        Self::E0043,
        Self::E0044,
        Self::E0045,
        Self::E0046,
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
        Self::E2007,
        Self::E2008,
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
        Self::E5003,
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
            DiagCode::E0007,
            DiagCode::E0008,
            DiagCode::E0009,
            DiagCode::E0010,
            DiagCode::E0011,
            DiagCode::E0012,
            DiagCode::E0013,
            DiagCode::E0014,
            DiagCode::E0015,
            DiagCode::E0016,
            DiagCode::E0017,
            DiagCode::E0018,
            DiagCode::E0019,
            DiagCode::E0020,
            DiagCode::E0021,
            DiagCode::E0022,
            DiagCode::E0023,
            DiagCode::E0024,
            DiagCode::E0025,
            DiagCode::E0026,
            DiagCode::E0027,
            DiagCode::E0028,
            DiagCode::E0029,
            DiagCode::E0030,
            DiagCode::E0031,
            DiagCode::E0032,
            DiagCode::E0033,
            DiagCode::E0034,
            DiagCode::E0035,
            DiagCode::E0036,
            DiagCode::E0037,
            DiagCode::E0038,
            DiagCode::E0039,
            DiagCode::E0040,
            DiagCode::E0041,
            DiagCode::E0042,
            DiagCode::E0043,
            DiagCode::E0044,
            DiagCode::E0045,
            DiagCode::E0046,
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
            DiagCode::E2007,
            DiagCode::E2008,
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
            DiagCode::E5003,
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
