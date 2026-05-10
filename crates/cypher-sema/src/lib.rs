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
//! Name resolution (§6.2) lives in [`mod@resolve`]: it produces a
//! `ScopeGraph` + `ResolvedNames` table and emits `E1001`/`E1002`
//! diagnostics.
//!
//! # Stability
//!
//! The [`cypher_schema::SchemaProvider`] trait — re-exported and
//! consumed throughout this crate — is **SemVer-locked**. Adding,
//! removing, or changing the signature of any required method is a
//! major-version bump for both `cypher-schema` and `cypher-sema`.
//! New methods land as either default-implemented additions (so
//! existing downstream `impl SchemaProvider` blocks keep compiling)
//! or behind a new `#[non_exhaustive]` extension point. Embedders
//! can therefore upgrade `cypher-sema` minor versions without
//! touching their adapter code. The migration template at
//! `crates/cypher-sema/examples/embedder_adapter.rs` is part of the
//! public API surface and is held to the same compatibility bar.
//! See [`docs/sema-checks.md`](../../../docs/sema-checks.md) for the
//! semantic-check contract that pairs with this trait.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-sema/0.0.1")]

pub mod builtins;
pub mod dialect;
pub mod infer;
pub mod kinds;
pub mod resolve;
pub mod schema_aware;
pub mod ty;
pub mod unify;

pub use dialect::{DialectMode, check_dialect, reject_neo4j_current};
pub use infer::infer_types;
pub use kinds::check_kinds;
pub use resolve::{ResolveResult, resolve};
pub use schema_aware::check_schema_aware;
pub use ty::{LabelSet, Type};
pub use unify::{TypeMismatch, unify};

use cypher_diag::DiagnosticsSink;
use cypher_hir::Statement;
use cypher_schema::SchemaProvider;
use smol_str::SmolStr;

/// Knobs exposed to consumers. Added rather than changed; never breaks.
#[derive(Debug, Default, Clone)]
pub struct SemaOptions {
    /// Type hints supplied by the caller for `$param` references.
    /// Unresolved parameters default to `Type::Any`.
    pub parameter_hints: Vec<(SmolStr, Type)>,
    /// When `true`, re-binding a name already visible in scope emits
    /// `E1002` instead of silently shadowing.
    pub warn_shadowing: bool,
    /// Dialect to use for gate checks (§9). Defaults to [`DialectMode::GqlAligned`].
    pub dialect: DialectMode,
}

/// Entry point. Runs all passes against the statement, emitting into the
/// sink. No pass short-circuits on first error (spec §10.4).
///
/// Pass order: name resolution (§6.2) → kind-consistency (§6.3) →
/// dialect gates (§9) → schema-free type inference (§7.4) →
/// schema-aware checks (§7.5, only when `schema` is `Some`).
pub fn analyse(
    stmt: &Statement,
    schema: Option<&dyn SchemaProvider>,
    options: &SemaOptions,
    sink: &mut DiagnosticsSink,
) {
    // Pass 1: name resolution (§6.2).
    let _result = resolve::resolve(stmt, options.warn_shadowing, sink);

    // Pass 2: kind-consistency (§6.3).
    kinds::check_kinds(stmt, sink);

    // Pass 3: dialect gates (§9) — after resolution, before schema-aware.
    dialect::check_dialect(stmt, options.dialect, sink);

    // Pass 4: schema-free type inference (§7.4).
    infer::infer_types(stmt, sink);

    // Pass 5: schema-aware checks (§7.1 / §7.5) — only when a schema is
    // provided. E3xxx codes. Must run after schema-free so propagated types
    // are available (currently only literal types are used).
    if let Some(s) = schema {
        schema_aware::check_schema_aware(stmt, s, sink);
    }
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
