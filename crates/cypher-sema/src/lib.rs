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
//!
//! Name resolution (§6.2) lives in [`resolve`]: it produces a
//! [`ScopeGraph`] + [`ResolvedNames`] table and emits `E1001`/`E1002`
//! diagnostics.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-sema/0.0.1")]

pub mod resolve;

pub use resolve::{ResolveResult, resolve};

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
///
/// Currently runs: name resolution (§6.2).
pub fn analyse(
    stmt: &Statement,
    _schema: Option<&dyn SchemaProvider>,
    options: &SemaOptions,
    sink: &mut DiagnosticsSink,
) {
    // Pass 1: name resolution (§6.2).
    let _result = resolve::resolve(stmt, options.warn_shadowing, sink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cypher_hir::Statement as HirStmt;
    use cypher_syntax::{TextRange, TextSize};

    fn empty_stmt() -> HirStmt {
        HirStmt::new(TextRange::new(TextSize::new(0), TextSize::new(0)))
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
