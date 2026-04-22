//! Dialect-gate pass (spec 0001 §9).
//!
//! Every construct that differs between [`DialectMode::GqlAligned`] and
//! [`DialectMode::OpenCypherV9`] is represented by a named [`DialectGate`]
//! constant in this module. The constants are the single source of truth:
//! no `if dialect == …` checks are permitted to scatter across the codebase
//! (AGENTS §9).
//!
//! ## Code range
//!
//! All codes emitted here live in the `E4000–E4999` range (spec §10.2).
//!
//! ## v1 dialects (spec §9.1)
//!
//! | Variant             | Meaning                                        |
//! |---------------------|------------------------------------------------|
//! | `GqlAligned`        | Canonical; GQL-aligned Cypher per ISO/IEC 39075|
//! | `OpenCypherV9`      | Compatibility; openCypher v9 per TCK           |
//!
//! `Neo4jCurrent` (`cypher 5` / `cypher 25`) is **not in v1** (spec §9.3).
//! Passing it to [`check_dialect`] is an error (see §9.3, §19–§20 deferral).

use cypher_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cypher_hir::Statement;
use cypher_syntax::{TextRange, TextSize};

// ---------------------------------------------------------------------------
// Dialect mode
// ---------------------------------------------------------------------------

/// Dialect mode for semantic analysis (spec §9.1).
///
/// Mirrors `cypher_db::DialectMode`; defined here so that `cypher-sema`
/// (which sits below `cypher-db` in the crate graph) can be dialect-aware
/// without a circular dependency.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new dialects can land without
/// forcing a SemVer-major release.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DialectMode {
    /// Canonical: GQL-aligned Cypher per ISO/IEC 39075.
    #[default]
    GqlAligned,
    /// Compatibility: openCypher v9 per the openCypher spec + TCK.
    OpenCypherV9,
}

// ---------------------------------------------------------------------------
// Gate descriptor
// ---------------------------------------------------------------------------

/// A named, versioned gate for a single dialect-divergent construct.
///
/// Each gate pairs a stable `name` (used in messages) with a stable
/// diagnostic `code` and a list of dialects in which the construct is
/// permitted.  If `allowed_in` is empty the construct is allowed in no
/// dialect (reserved for deferred / v2 constructs).
#[derive(Debug, Clone, Copy)]
pub struct DialectGate {
    /// Stable machine-readable name (`snake_case`, no spaces).
    pub name: &'static str,
    /// Stable diagnostic code string (`"E4xxx"`).
    pub code: DiagCode,
    /// Dialects in which this construct is permitted.
    pub allowed_in: &'static [DialectMode],
}

impl DialectGate {
    /// Return `true` iff `dialect` is allowed to use this construct.
    #[must_use]
    pub const fn is_allowed(&self, dialect: DialectMode) -> bool {
        let mut i = 0;
        while i < self.allowed_in.len() {
            // Manual loop: `const fn` cannot use iterators in Rust 1.94.
            if matches!(
                (self.allowed_in[i], dialect),
                (DialectMode::GqlAligned, DialectMode::GqlAligned)
                    | (DialectMode::OpenCypherV9, DialectMode::OpenCypherV9)
            ) {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Gate registry (spec §9.2)
//
// Gate names derive from the spec's behaviour table (§9.2) plus the deferred
// items listed in §9.3.  Each gate has a unique E4xxx code registered in
// `cypher-diag/src/codes.rs`.
// ---------------------------------------------------------------------------

/// GQL-only label-negation syntax (`!(A)` in label expressions).
///
/// `GqlAligned` allows the `!` operator in label expressions; `OpenCypherV9`
/// does not (spec §9.2 row "`:` in label expressions").
pub const GATE_LABEL_NEGATION: DialectGate = DialectGate {
    name: "label_negation",
    code: DiagCode::E4010,
    allowed_in: &[DialectMode::GqlAligned],
};

/// Label-union (`A|B`) in node / relationship patterns.
///
/// Both dialects permit `A|B` union syntax (spec §9.2).  This gate is
/// therefore allowed in both and exists as an explicit named constant so
/// that the code-site can self-document its dialect origin.
pub const GATE_LABEL_UNION: DialectGate = DialectGate {
    name: "label_union",
    code: DiagCode::E4011,
    allowed_in: &[DialectMode::GqlAligned, DialectMode::OpenCypherV9],
};

/// Integer-division via `/` with integer operands (openCypher v9 semantics).
///
/// In `GqlAligned` mode, `/` always promotes to float; the `DIV` keyword
/// gives floor-divide.  In `OpenCypherV9` mode, `/` between integers does
/// floor-division (spec §9.2 row "Integer division promotion").
pub const GATE_INTEGER_DIVISION: DialectGate = DialectGate {
    name: "integer_division",
    code: DiagCode::E4012,
    allowed_in: &[DialectMode::OpenCypherV9],
};

/// `UNION` without `ALL` (set-semantics UNION).
///
/// Both dialects support `UNION ALL`; plain `UNION` (with deduplication)
/// exists in both (spec §9.2 does not restrict it).  Gate retained as an
/// explicit named constant for future divergence tracking.
pub const GATE_UNION_SET: DialectGate = DialectGate {
    name: "union_set",
    code: DiagCode::E4013,
    allowed_in: &[DialectMode::GqlAligned, DialectMode::OpenCypherV9],
};

/// Procedure `CALL` clause (both dialects).
///
/// Both `GqlAligned` and `OpenCypherV9` support the basic `CALL proc YIELD`
/// form (spec §9.2); gate is present for documentation and future
/// fine-grained divergence.
pub const GATE_CALL_PROCEDURE: DialectGate = DialectGate {
    name: "call_procedure",
    code: DiagCode::E4014,
    allowed_in: &[DialectMode::GqlAligned, DialectMode::OpenCypherV9],
};

/// `LOAD CSV` clause.
///
/// Deferred to a future spec version (§9.3 / §19).  Neither v1 dialect
/// supports it.
pub const GATE_LOAD_CSV: DialectGate = DialectGate {
    name: "load_csv",
    code: DiagCode::E4015,
    allowed_in: &[],
};

/// APOC / vendor-prefixed function names (e.g. `apoc.util.sleep`).
///
/// APOC is a Neo4j-specific extension, not part of either v1 dialect
/// (spec §9.3).
pub const GATE_APOC_FUNCTIONS: DialectGate = DialectGate {
    name: "apoc_functions",
    code: DiagCode::E4016,
    allowed_in: &[],
};

/// `EXISTS { }` subquery expression.
///
/// Full-subquery EXISTS is a Neo4j-current feature deferred to a future
/// spec version (§9.3).
pub const GATE_EXISTS_SUBQUERY: DialectGate = DialectGate {
    name: "exists_subquery",
    code: DiagCode::E4017,
    allowed_in: &[],
};

/// `CYPHER` version-prefix statement header (e.g. `CYPHER 5 MATCH …`).
///
/// Neo4j-specific; not part of v1 dialects (§9.3).
pub const GATE_CYPHER_PREFIX: DialectGate = DialectGate {
    name: "cypher_prefix",
    code: DiagCode::E4018,
    allowed_in: &[],
};

/// `CALL { } IN TRANSACTIONS` (call-in-transactions clause).
///
/// A Neo4j-current extension deferred to a future spec (§9.3).
pub const GATE_CALL_IN_TRANSACTIONS: DialectGate = DialectGate {
    name: "call_in_transactions",
    code: DiagCode::E4019,
    allowed_in: &[],
};

// ---------------------------------------------------------------------------
// Gate check helper
// ---------------------------------------------------------------------------

/// Check whether `dialect` may use the construct described by `gate`.
///
/// Returns `Ok(())` when allowed, or an `Err(Diagnostic)` with a stable
/// `E4xxx` code when the construct is disallowed.
///
/// # Usage
///
/// ```
/// use cypher_sema::dialect::{DialectMode, check, GATE_LABEL_NEGATION};
/// use cypher_syntax::{TextRange, TextSize};
///
/// let range = TextRange::empty(TextSize::new(0));
/// assert!(check(&GATE_LABEL_NEGATION, DialectMode::GqlAligned, range).is_ok());
/// assert!(check(&GATE_LABEL_NEGATION, DialectMode::OpenCypherV9, range).is_err());
/// ```
#[allow(clippy::result_large_err)]
pub fn check(gate: &DialectGate, dialect: DialectMode, span: TextRange) -> Result<(), Diagnostic> {
    if gate.is_allowed(dialect) {
        Ok(())
    } else {
        let dialect_name = match dialect {
            DialectMode::GqlAligned => "GqlAligned",
            DialectMode::OpenCypherV9 => "OpenCypherV9",
        };
        let allowed_list = if gate.allowed_in.is_empty() {
            "no v1 dialect (deferred construct, see spec §9.3)".to_owned()
        } else {
            gate.allowed_in
                .iter()
                .map(|d| match d {
                    DialectMode::GqlAligned => "GqlAligned",
                    DialectMode::OpenCypherV9 => "OpenCypherV9",
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        Err(Diagnostic::error(
            gate.code,
            span,
            format!(
                "construct `{}` is not allowed in dialect `{}`; \
                 allowed in: {}",
                gate.name, dialect_name, allowed_list,
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// Whole-statement dialect pass
// ---------------------------------------------------------------------------

/// Run the dialect-gate pass against a lowered HIR statement.
///
/// # Errors on `Neo4jCurrent`
///
/// `Neo4jCurrent` is not a v1 dialect (spec §9.3, §19–§20).  Because the
/// type is defined in this module, callers that try to construct it will
/// fail to compile; this function exists for documentation purposes and
/// future-proofing if the type grows a variant.
///
/// # Pass scope
///
/// In v1 the gate pass operates on the HIR's structural features rather
/// than requiring a full semantic resolution of dialect-sensitive surfaces
/// (most of which are parse-time or type-level, deferred to later beads).
/// The pass currently fires gates for constructs recorded in the HIR:
///
/// - `CALL` clauses with APOC-prefixed procedure names → `GATE_APOC_FUNCTIONS`
/// - `CALL` clauses in general → `GATE_CALL_PROCEDURE` (always allowed in v1)
///
/// Additional gates (LOAD CSV, EXISTS {}, `CYPHER` prefix, label negation,
/// integer division) are syntactic and will be fired from the parser or a
/// future lowering step; stubs are provided here for completeness.
pub fn check_dialect(stmt: &Statement, dialect: DialectMode, sink: &mut DiagnosticsSink) {
    use cypher_hir::Clause;

    for clause in &stmt.clauses {
        if let Clause::Call {
            procedure, span, ..
        } = clause
        {
            // Detect APOC-prefixed procedure names (Neo4j-specific, deferred §9.3).
            let lower = procedure.to_ascii_lowercase();
            if lower.starts_with("apoc.") || lower.starts_with("apoc ") {
                if let Err(d) = check(&GATE_APOC_FUNCTIONS, dialect, *span) {
                    sink.push(d);
                }
            } else {
                // Plain CALL — allowed in both v1 dialects.
                // check() will be a no-op because GATE_CALL_PROCEDURE allows all.
                if let Err(d) = check(&GATE_CALL_PROCEDURE, dialect, *span) {
                    sink.push(d);
                }
            }
        }
    }
}

/// A zero-width span at offset zero; used when a check is not tied to a
/// specific source range (e.g. the pass-entry `Neo4jCurrent` rejection).
fn zero_span() -> TextRange {
    TextRange::empty(TextSize::new(0))
}

/// Emit a diagnostic rejecting the `Neo4jCurrent` dialect at pass entry.
///
/// This is called by consumers that hold a `DialectMode`-equivalent enum
/// with a `Neo4jCurrent` variant and dispatch to this crate.  Because our
/// `DialectMode` has no `Neo4jCurrent` variant, this function is the
/// designated rejection point documented by spec §9.3.
pub fn reject_neo4j_current(sink: &mut DiagnosticsSink) {
    sink.push(Diagnostic::error(
        DiagCode::E4001,
        zero_span(),
        "dialect `Neo4jCurrent` (cypher 5 / cypher 25) is not supported in v1; \
         see spec §9.3 and §19–§20 for the deferral rationale",
    ));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use cypher_diag::DiagnosticsSink;
    use cypher_hir::{
        Binding, Clause, Expr, Pattern, PatternElement, PatternPart, Projection, Statement, VarId,
        VarKind,
    };
    use smol_str::SmolStr;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn zero_range() -> TextRange {
        TextRange::empty(TextSize::new(0))
    }

    fn dummy_syntax() -> cypher_syntax::SyntaxNode {
        cypher_syntax::parse("x").syntax()
    }

    fn alloc(stmt: &mut Statement) -> cypher_hir::HirId {
        stmt.alloc_id(dummy_syntax())
    }

    fn intern_var(stmt: &mut Statement, name: &str, kind: VarKind) -> VarId {
        for (id, b) in &stmt.bindings {
            if b.name.as_str() == name {
                return *id;
            }
        }
        let id = VarId(u32::try_from(stmt.bindings.len()).expect("overflow"));
        stmt.bindings.insert(
            id,
            Binding {
                id,
                name: SmolStr::new(name),
                kind,
                defined_at: zero_range(),
            },
        );
        id
    }

    fn run_gate(stmt: &Statement, dialect: DialectMode) -> String {
        let mut sink = DiagnosticsSink::new();
        check_dialect(stmt, dialect, &mut sink);
        let diags = sink.into_sorted();
        let mut out = String::new();
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
        }
        out
    }

    fn run_check(gate: &DialectGate, dialect: DialectMode) -> String {
        let result = check(gate, dialect, zero_range());
        let mut out = String::new();
        match result {
            Ok(()) => writeln!(out, "allowed").unwrap(),
            Err(d) => writeln!(out, "denied: {}: {}", d.code, d.message).unwrap(),
        }
        out
    }

    fn run_reject_neo4j() -> String {
        let mut sink = DiagnosticsSink::new();
        reject_neo4j_current(&mut sink);
        let diags = sink.into_sorted();
        let mut out = String::new();
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
        }
        out
    }

    // Helpers to build statements.
    fn call_stmt(procedure: &str) -> Statement {
        let mut stmt = Statement::new(zero_range());
        let id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Call {
            id,
            procedure: SmolStr::new(procedure),
            args: vec![],
            yields: vec![],
            span: zero_range(),
        });
        stmt
    }

    fn match_return_stmt() -> Statement {
        let mut stmt = Statement::new(zero_range());
        let nid = alloc(&mut stmt);
        let var = intern_var(&mut stmt, "n", VarKind::Node);
        let mid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(var),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let rid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: rid,
            projections: vec![Projection {
                expr: Expr::Var(var),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        stmt
    }

    // -----------------------------------------------------------------------
    // Gate unit tests: GATE_LABEL_NEGATION
    // -----------------------------------------------------------------------

    /// Label negation is allowed in `GqlAligned`.
    #[test]
    fn snap_label_negation_gql_ok() {
        insta::assert_snapshot!(
            "label_negation_gql_ok",
            run_check(&GATE_LABEL_NEGATION, DialectMode::GqlAligned)
        );
    }

    /// Label negation is NOT allowed in `OpenCypherV9`.
    #[test]
    fn snap_label_negation_oc_denied() {
        insta::assert_snapshot!(
            "label_negation_oc_denied",
            run_check(&GATE_LABEL_NEGATION, DialectMode::OpenCypherV9)
        );
    }

    // -----------------------------------------------------------------------
    // Gate unit tests: GATE_INTEGER_DIVISION
    // -----------------------------------------------------------------------

    /// Integer division via `/` is allowed in `OpenCypherV9`.
    #[test]
    fn snap_integer_division_oc_ok() {
        insta::assert_snapshot!(
            "integer_division_oc_ok",
            run_check(&GATE_INTEGER_DIVISION, DialectMode::OpenCypherV9)
        );
    }

    /// Integer division via `/` is NOT allowed in `GqlAligned` (use `DIV`).
    #[test]
    fn snap_integer_division_gql_denied() {
        insta::assert_snapshot!(
            "integer_division_gql_denied",
            run_check(&GATE_INTEGER_DIVISION, DialectMode::GqlAligned)
        );
    }

    // -----------------------------------------------------------------------
    // Gate unit tests: GATE_LOAD_CSV (deferred, no dialect allows it)
    // -----------------------------------------------------------------------

    /// `LOAD CSV` is denied in `GqlAligned` (not in v1).
    #[test]
    fn snap_load_csv_gql_denied() {
        insta::assert_snapshot!(
            "load_csv_gql_denied",
            run_check(&GATE_LOAD_CSV, DialectMode::GqlAligned)
        );
    }

    /// `LOAD CSV` is denied in `OpenCypherV9` (not in v1).
    #[test]
    fn snap_load_csv_oc_denied() {
        insta::assert_snapshot!(
            "load_csv_oc_denied",
            run_check(&GATE_LOAD_CSV, DialectMode::OpenCypherV9)
        );
    }

    // -----------------------------------------------------------------------
    // Gate unit tests: GATE_APOC_FUNCTIONS (deferred, no dialect allows it)
    // -----------------------------------------------------------------------

    /// APOC functions are denied in `GqlAligned`.
    #[test]
    fn snap_apoc_functions_gql_denied() {
        insta::assert_snapshot!(
            "apoc_functions_gql_denied",
            run_check(&GATE_APOC_FUNCTIONS, DialectMode::GqlAligned)
        );
    }

    /// APOC functions are denied in `OpenCypherV9`.
    #[test]
    fn snap_apoc_functions_oc_denied() {
        insta::assert_snapshot!(
            "apoc_functions_oc_denied",
            run_check(&GATE_APOC_FUNCTIONS, DialectMode::OpenCypherV9)
        );
    }

    // -----------------------------------------------------------------------
    // Whole-pass tests: check_dialect
    // -----------------------------------------------------------------------

    /// A plain `MATCH`/`RETURN` produces no diagnostics in `GqlAligned`.
    #[test]
    fn snap_pass_match_return_gql_clean() {
        insta::assert_snapshot!(
            "pass_match_return_gql_clean",
            run_gate(&match_return_stmt(), DialectMode::GqlAligned)
        );
    }

    /// A plain `MATCH`/`RETURN` produces no diagnostics in `OpenCypherV9`.
    #[test]
    fn snap_pass_match_return_oc_clean() {
        insta::assert_snapshot!(
            "pass_match_return_oc_clean",
            run_gate(&match_return_stmt(), DialectMode::OpenCypherV9)
        );
    }

    /// `CALL` with an APOC procedure fires `E4016` in `GqlAligned`.
    #[test]
    fn snap_pass_call_apoc_gql_denied() {
        insta::assert_snapshot!(
            "pass_call_apoc_gql_denied",
            run_gate(&call_stmt("apoc.util.sleep"), DialectMode::GqlAligned)
        );
    }

    /// `CALL` with an APOC procedure fires `E4016` in `OpenCypherV9`.
    #[test]
    fn snap_pass_call_apoc_oc_denied() {
        insta::assert_snapshot!(
            "pass_call_apoc_oc_denied",
            run_gate(&call_stmt("apoc.util.sleep"), DialectMode::OpenCypherV9)
        );
    }

    /// A plain (non-APOC) `CALL` is clean in `GqlAligned`.
    #[test]
    fn snap_pass_call_plain_gql_clean() {
        insta::assert_snapshot!(
            "pass_call_plain_gql_clean",
            run_gate(&call_stmt("db.labels"), DialectMode::GqlAligned)
        );
    }

    // -----------------------------------------------------------------------
    // Neo4jCurrent rejection
    // -----------------------------------------------------------------------

    /// `reject_neo4j_current()` always emits an `E4001` diagnostic.
    #[test]
    fn snap_reject_neo4j_current() {
        insta::assert_snapshot!("reject_neo4j_current", run_reject_neo4j());
    }
}
