//! Errors produced by HIR → Plan lowering (spec 0001 §12, bead cy-wlr).
//!
//! The [`lower_statement`] entry point in [`crate::lower`] validates its
//! post-resolve, post-desugar preconditions before walking the HIR. Any
//! violation surfaces as a [`PlanLowerError`] rather than a panic, so
//! fuzz and agent-facing callers can fail cleanly.
//!
//! [`lower_statement`]: crate::lower::lower_statement

use cypher_hir::HirSpan;
use smol_str::SmolStr;

/// A precondition violation detected by HIR → Plan lowering.
///
/// `lower_statement` requires its input to have been name-resolved
/// (`cypher-sema::resolve` / cy-b4b) and desugared
/// (`cypher_hir::desugar::desugar_statement` / cy-mla). The variants below
/// describe the kinds of precondition violation the entry point detects.
///
/// This enum is `#[non_exhaustive]`: callers must include a wildcard arm to
/// remain forward-compatible (spec cy-2i9.1 precedent).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanLowerError {
    /// An [`cypher_hir::Expr::Unresolved`] node survived into plan
    /// lowering — name resolution must run first (cy-b4b). The variable
    /// name is preserved for diagnostics.
    UnresolvedName {
        /// The identifier that was never resolved to a [`cypher_hir::VarId`].
        name: SmolStr,
        /// Approximate span of the offending clause. The HIR does not carry
        /// per-expression spans, so this is the clause span that contained
        /// the unresolved reference.
        span: HirSpan,
    },
    /// An expression that must be desugared before plan lowering
    /// survived into the entry point (cy-mla). Possible `kind` values
    /// are `"ListComprehension"`, `"MapProjection"`, and
    /// `"PatternPredicate"`.
    UndesugaredExpr {
        /// The name of the offending HIR expression variant.
        kind: &'static str,
        /// Approximate span of the offending clause (see
        /// [`Self::UnresolvedName::span`] for why this is clause-scoped).
        span: HirSpan,
    },
}

impl std::fmt::Display for PlanLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedName { name, .. } => write!(
                f,
                "cypher-plan: unresolved variable `{name}` in HIR → Plan lowering; \
                 run name resolution (cy-b4b) before calling lower_statement"
            ),
            Self::UndesugaredExpr { kind, .. } => write!(
                f,
                "cypher-plan: un-desugared `{kind}` expression in HIR → Plan lowering; \
                 run cypher_hir::desugar::desugar_statement (cy-mla) first"
            ),
        }
    }
}

impl std::error::Error for PlanLowerError {}
