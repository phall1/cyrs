//! HIR for GQL `SESSION SET …` top-level statements (cy-lp3y).
//!
//! `SESSION SET` is a top-level statement category disjoint from the
//! query `STATEMENT` (ISO/IEC 39075:2024 §14.15, spec 0001 §0 amendment
//! dated 2026-05-19 (cy-5e3f); parser + AST landed in cy-9kzx). Because
//! the construct has no clauses, no name-resolution-bearing surface,
//! and never participates in `UNION`, it is represented as a sibling
//! HIR datum carried on [`crate::Statement::session_set`] rather than
//! a [`crate::Clause`] variant — see the bead notes for the rationale.
//!
//! Lowering preserves the [`crate::HirId`] discipline: the outer
//! `SESSION_SET_STMT` node is allocated against `Statement::node_map`
//! so diagnostics can point back at the originating concrete syntax.
//!
//! Plan IR mapping (cy-plan-catalog-session) and session-state evaluation
//! are explicitly out of scope here — the embedder owns the session
//! model. The HIR only carries the syntactic shape needed by sema's
//! dialect-gate pass and any downstream consumer that needs to walk
//! the statement without re-parsing.

// --- cy-lp3y SESSION SET HIR ---

use cyrs_syntax::TextRange;
use smol_str::SmolStr;

use crate::{Expr, HirId};

/// Lowered `SESSION SET …` top-level statement (ISO §14.15).
///
/// Sibling to the query [`crate::Statement::clauses`] vector; a
/// [`crate::Statement`] carries at most one of these (when present,
/// `clauses` is empty).
#[derive(Debug, Clone)]
pub struct SessionSetHir {
    /// [`HirId`] of the outer `SESSION_SET_STMT` node.
    pub id: HirId,
    /// The four sub-forms of `SESSION SET` collapsed into a sum.
    pub variant: SessionSetVariantHir,
    /// Source span covering the entire statement.
    pub span: TextRange,
}

/// Discriminant for [`SessionSetHir::variant`].
///
/// Marked `#[non_exhaustive]` so future additions to the ISO §14.15
/// production (e.g. `SESSION SET SCHEMA`) can land without forcing a
/// SemVer-major release downstream.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SessionSetVariantHir {
    /// `SESSION SET GRAPH <ref>` or `SESSION SET PROPERTY GRAPH <ref>`.
    Graph {
        /// Graph reference token text (e.g. `CURRENT_GRAPH`, `g`,
        /// `"My Graph"`). Empty when the parser recovered without
        /// producing a reference.
        graph_ref: SmolStr,
        /// `true` iff the source said `SESSION SET PROPERTY GRAPH …`.
        is_property_graph: bool,
        /// Source span of the variant.
        span: TextRange,
    },
    /// `SESSION SET TIME ZONE <string-literal>`.
    TimeZone {
        /// The string-literal token text *including quotes* — not
        /// content-decoded. HIR keeps the literal verbatim so embedders
        /// can perform their own zone-string validation (in scope: none
        /// here; ISO §14.15 leaves zone-string format implementation-
        /// defined).
        time_zone: SmolStr,
        /// Source span of the variant.
        span: TextRange,
    },
    /// `SESSION SET VALUE [IF NOT EXISTS] $param = <expr>`.
    Value {
        /// Parameter name **including** the leading `$` (matches the
        /// `PARAM` token text). Empty when the parser recovered without
        /// producing a parameter.
        param: SmolStr,
        /// `true` iff the `IF NOT EXISTS` guard was present.
        if_not_exists: bool,
        /// Lowered RHS value expression. `Expr::Null` is the recovery
        /// fallback when the parser left an unrecognised expression shape.
        value: Expr,
        /// Source span of the variant.
        span: TextRange,
    },
}

impl SessionSetHir {
    /// Return a stable, machine-readable name for the variant
    /// discriminant. Used by diagnostics and test snapshots.
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match &self.variant {
            SessionSetVariantHir::Graph { .. } => "graph",
            SessionSetVariantHir::TimeZone { .. } => "time_zone",
            SessionSetVariantHir::Value { .. } => "value",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::lower_statement;

    #[test]
    fn lowers_session_set_graph() {
        let stmt = lower_statement("SESSION SET GRAPH CURRENT_GRAPH");
        let ss = stmt
            .session_set
            .as_ref()
            .expect("SESSION SET should lower to session_set");
        assert!(stmt.clauses.is_empty(), "no clauses for SESSION SET");
        assert_eq!(ss.variant_name(), "graph");
        match &ss.variant {
            SessionSetVariantHir::Graph {
                graph_ref,
                is_property_graph,
                ..
            } => {
                assert_eq!(graph_ref.as_str(), "CURRENT_GRAPH");
                assert!(!is_property_graph);
            }
            _ => panic!("expected Graph variant"),
        }
    }

    #[test]
    fn lowers_session_set_property_graph() {
        let stmt = lower_statement("SESSION SET PROPERTY GRAPH CURRENT_PROPERTY_GRAPH");
        let ss = stmt.session_set.as_ref().expect("session_set");
        match &ss.variant {
            SessionSetVariantHir::Graph {
                graph_ref,
                is_property_graph,
                ..
            } => {
                assert_eq!(graph_ref.as_str(), "CURRENT_PROPERTY_GRAPH");
                assert!(is_property_graph);
            }
            _ => panic!("expected Graph(property) variant"),
        }
    }

    #[test]
    fn lowers_session_set_time_zone() {
        let stmt = lower_statement("SESSION SET TIME ZONE \"utc\"");
        let ss = stmt.session_set.as_ref().expect("session_set");
        assert_eq!(ss.variant_name(), "time_zone");
        match &ss.variant {
            SessionSetVariantHir::TimeZone { time_zone, .. } => {
                // Literal text is preserved verbatim (quotes included).
                assert_eq!(time_zone.as_str(), "\"utc\"");
            }
            _ => panic!("expected TimeZone variant"),
        }
    }

    #[test]
    fn lowers_session_set_value_if_not_exists() {
        let stmt = lower_statement("SESSION SET VALUE IF NOT EXISTS $foo = {a: 1}");
        let ss = stmt.session_set.as_ref().expect("session_set");
        assert_eq!(ss.variant_name(), "value");
        match &ss.variant {
            SessionSetVariantHir::Value {
                param,
                if_not_exists,
                value,
                ..
            } => {
                assert_eq!(param.as_str(), "$foo");
                assert!(if_not_exists);
                // RHS lowered to a map expression.
                assert!(matches!(value, Expr::Map(_)));
            }
            _ => panic!("expected Value variant"),
        }
    }

    #[test]
    fn lowers_session_set_value_without_guard() {
        let stmt = lower_statement("SESSION SET VALUE $bar = {x: 'hi'}");
        let ss = stmt.session_set.as_ref().expect("session_set");
        match &ss.variant {
            SessionSetVariantHir::Value {
                param,
                if_not_exists,
                ..
            } => {
                assert_eq!(param.as_str(), "$bar");
                assert!(!if_not_exists);
            }
            _ => panic!("expected Value variant"),
        }
    }

    #[test]
    fn query_statement_has_no_session_set() {
        let stmt = lower_statement("MATCH (n) RETURN n");
        assert!(stmt.session_set.is_none());
        assert!(!stmt.clauses.is_empty());
    }

    #[test]
    fn session_set_records_node_map_entry() {
        let stmt = lower_statement("SESSION SET GRAPH g");
        let ss = stmt.session_set.as_ref().expect("session_set");
        // The outer SESSION_SET_STMT node has a recorded HirId.
        assert!(stmt.syntax_for(ss.id).is_some());
    }
}

// --- end cy-lp3y ---
