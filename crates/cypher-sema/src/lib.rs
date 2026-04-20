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
//! The type system (§7.2) is defined in [`ty`]: a small enum over Cypher
//! value types with helper constructors and Union canonicalisation.
//! `Any` is the universal super-type: queries without schema produce Any-
//! typed property reads; only structural errors surface.
//!
//! Name resolution (§6.2) lives in [`resolve`]: it produces a
//! [`ScopeGraph`] + [`ResolvedNames`] table and emits `E1001`/`E1002`
//! diagnostics.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-sema/0.0.1")]

pub mod infer;
pub mod kinds;
pub mod resolve;
pub mod ty;
pub mod unify;

pub use infer::infer_types;
pub use kinds::check_kinds;
pub use resolve::{ResolveResult, resolve};
pub use ty::{LabelSet, Type};
pub use unify::{TypeMismatch, unify};

use cypher_diag::DiagnosticsSink;
use cypher_hir::Statement;
use cypher_schema::SchemaProvider;
use smol_str::SmolStr;

/// Knobs exposed to consumers. Added rather than changed; never breaks.
#[derive(Debug, Default, Clone)]
pub struct SemaOptions {
    pub parameter_hints: Vec<(SmolStr, Type)>,
    pub warn_shadowing: bool,
}

/// Entry point. Runs all passes against the statement, emitting into the
/// sink. No pass short-circuits on first error (spec §10.4).
///
/// Pass order: name resolution (§6.2) → kind-consistency (§6.3) →
/// schema-free type inference (§7.4).
pub fn analyse(
    stmt: &Statement,
    _schema: Option<&dyn SchemaProvider>,
    options: &SemaOptions,
    sink: &mut DiagnosticsSink,
) {
    // Pass 1: name resolution (§6.2).
    let _result = resolve::resolve(stmt, options.warn_shadowing, sink);

    // Pass 2: kind-consistency (§6.3).
    kinds::check_kinds(stmt, sink);

    // Pass 3: schema-free type inference (§7.4).
    infer::infer_types(stmt, sink);
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
