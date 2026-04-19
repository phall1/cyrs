//! `cypher-sema` — semantic analysis and type system (spec 0001 §7).
//!
//! Two modes (§7.1):
//!
//! - **Schema-free.** Always run. Unresolved variables, kind mismatches,
//!   aggregation scope, clause ordering, parameter discipline, structural
//!   type errors.
//! - **Schema-aware.** Run when a [`cypher_schema::SchemaProvider`] is
//!   supplied. Adds unknown labels/types/properties, endpoint mismatches,
//!   function arity / type mismatches.
//!
//! The type system (§7.2) is a small unification engine over [`Type`].
//! `Any` is the universal subtype: queries without schema produce Any-
//! typed property reads; only structural errors surface.

#![doc(html_root_url = "https://docs.rs/cypher-sema/0.0.1")]

use cypher_diag::DiagnosticsSink;
use cypher_hir::Statement;
use cypher_schema::SchemaProvider;
use smol_str::SmolStr;
use std::collections::BTreeMap;

/// The Cypher value-level type system. Spec §7.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Any,
    Null,
    Bool,
    Int,
    Float,
    /// Numeric — `Int | Float`. Produced by arithmetic inference.
    Num,
    String,
    Date,
    Datetime,
    List(Box<Type>),
    Map(BTreeMap<SmolStr, Type>),
    Node(Option<LabelSet>),
    Relationship(Option<SmolStr>),
    Path,
    Union(Vec<Type>),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelSet(pub Vec<SmolStr>);

/// Knobs exposed to consumers. Added rather than changed; never breaks.
#[derive(Debug, Default, Clone)]
pub struct SemaOptions {
    pub parameter_hints: Vec<(SmolStr, Type)>,
    pub warn_shadowing: bool,
}

/// Entry point. Runs all passes against the statement, emitting into the
/// sink. No pass short-circuits on first error (spec §10.4).
pub fn analyse(
    _stmt: &Statement,
    _schema: Option<&dyn SchemaProvider>,
    _options: &SemaOptions,
    sink: &mut DiagnosticsSink,
) {
    // Passes land with the grammar. Until then, `analyse` is a no-op; the
    // signature is committed so callers may depend on it.
    let _ = sink;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cypher_hir::Statement as HirStmt;
    use cypher_syntax::{TextRange, TextSize};

    fn empty_stmt() -> HirStmt {
        HirStmt {
            clauses: Vec::new(),
            bindings: indexmap::IndexMap::new(),
            span: TextRange::new(TextSize::new(0), TextSize::new(0)),
        }
    }

    #[test]
    fn analyse_empty_statement_is_clean() {
        let stmt = empty_stmt();
        let mut sink = DiagnosticsSink::new();
        let opts = SemaOptions::default();
        analyse(&stmt, None, &opts, &mut sink);
        assert!(sink.is_empty());
    }
}
