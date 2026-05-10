//! Errors produced by HIR → Plan lowering (spec 0001 §12, bead cy-wlr).
//!
//! The [`lower_statement`] entry point in [`crate::lower`] validates its
//! post-resolve, post-desugar preconditions before walking the HIR. Any
//! violation surfaces as a [`PlanLowerError`] rather than a panic, so
//! fuzz and agent-facing callers can fail cleanly.
//!
//! [`lower_statement`]: crate::lower::lower_statement

use cyrs_hir::HirSpan;
use smol_str::SmolStr;

/// A precondition violation detected by HIR → Plan lowering.
///
/// `lower_statement` requires its input to have been name-resolved
/// (`cyrs-sema::resolve` / cy-b4b) and desugared
/// (`cyrs_hir::desugar::desugar_statement` / cy-mla). The variants below
/// describe the kinds of precondition violation the entry point detects.
///
/// This enum is `#[non_exhaustive]`: callers must include a wildcard arm to
/// remain forward-compatible (spec cy-2i9.1 precedent).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanLowerError {
    /// An [`cyrs_hir::Expr::Unresolved`] node survived into plan
    /// lowering — name resolution must run first (cy-b4b). The variable
    /// name is preserved for diagnostics.
    UnresolvedName {
        /// The identifier that was never resolved to a [`cyrs_hir::VarId`].
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
    /// A [`cyrs_hir::PatternPart`] reached the lowerer with no
    /// [`cyrs_hir::PatternElement`]s, or with a malformed element
    /// sequence the Source + Expand translation cannot walk (cy-f2t).
    ///
    /// The parser's error-recovery pass can emit such pattern parts for
    /// inputs where the pattern is syntactically incomplete (e.g. a bare
    /// `MATCH` keyword, or an open `MATCH (` with no close). The
    /// previous lowerer assumed every part starts with a [`cyrs_hir::PatternElement::Node`]
    /// and panicked on the empty / leading-`Rel` case; this variant
    /// surfaces the recovery-produced shape as a clean error.
    EmptyPatternPart {
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
                "cyrs-plan: unresolved variable `{name}` in HIR → Plan lowering; \
                 run name resolution (cy-b4b) before calling lower_statement"
            ),
            Self::UndesugaredExpr { kind, .. } => write!(
                f,
                "cyrs-plan: un-desugared `{kind}` expression in HIR → Plan lowering; \
                 run cyrs_hir::desugar::desugar_statement (cy-mla) first"
            ),
            Self::EmptyPatternPart { .. } => write!(
                f,
                "cyrs-plan: empty or malformed pattern part in HIR → Plan lowering; \
                 parser error-recovery produced a pattern the lowerer cannot walk \
                 (e.g. bare `MATCH`)"
            ),
        }
    }
}

impl std::error::Error for PlanLowerError {}
