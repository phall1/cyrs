//! Errors produced by AST → HIR lowering (spec 0001 §6.1, bead cy-cfi).
//!
//! The [`lower_statement`] and [`lower_parse`] entry points in
//! [`crate::lower`] return [`Result<Statement, HirLowerError>`] so an
//! embedder (notably pgGraph) has a typed, non-panicking failure channel
//! instead of having to wrap calls in `catch_unwind` (feat-request §4.1).
//!
//! [`lower_statement`]: crate::lower::lower_statement
//! [`lower_parse`]: crate::lower::lower_parse
//! [`Result<Statement, HirLowerError>`]: crate::Statement

use cyrs_syntax::SyntaxError;

/// A failure detected by AST → HIR lowering.
///
/// [`lower_statement`] returns [`Self::ParseFailed`] when the parser
/// reported one or more [`SyntaxError`]s for the input it was given —
/// previously those errors were silently swallowed and a best-effort
/// partial [`Statement`] returned. [`lower_parse`] borrows an
/// already-computed [`cyrs_syntax::Parse`], so it does not run the parser
/// and never yields [`Self::ParseFailed`]; it reserves
/// [`Self::Invariant`] for any lowering-invariant violation. Lowering is
/// genuinely infallible today, so `lower_parse` always returns `Ok` — the
/// variant exists so the typed channel is forward-compatible without a
/// breaking signature change.
///
/// This enum is `#[non_exhaustive]`: callers must include a wildcard arm
/// to remain forward-compatible (spec cy-2i9.1 precedent).
///
/// `HirLowerError` is intentionally not `PartialEq`/`Eq`:
/// [`Self::ParseFailed`] carries [`SyntaxError`]s, which `cyrs-syntax`
/// does not make comparable. Tests match on the variant and inspect the
/// payload rather than comparing whole values.
///
/// [`lower_statement`]: crate::lower::lower_statement
/// [`lower_parse`]: crate::lower::lower_parse
/// [`Statement`]: crate::Statement
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum HirLowerError {
    /// The parser reported syntax errors for the input passed to
    /// [`lower_statement`]. The recovered errors are preserved in source
    /// order so the embedder can surface a precise diagnostic rather than
    /// a generic failure.
    ///
    /// [`lower_statement`]: crate::lower::lower_statement
    ParseFailed {
        /// The parser's accumulated [`SyntaxError`]s, in source order.
        errors: Vec<SyntaxError>,
    },
    /// A lowering-invariant violation detected while walking the AST.
    ///
    /// Reserved for failure modes that AST → HIR lowering may grow in the
    /// future; lowering is infallible today, so this variant is currently
    /// never produced. The `detail` string carries a human-readable
    /// description for diagnostics.
    Invariant {
        /// Human-readable description of the violated invariant.
        detail: String,
    },
}

impl std::fmt::Display for HirLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed { errors } => {
                write!(
                    f,
                    "cyrs-hir: cannot lower input with {} syntax error(s)",
                    errors.len()
                )?;
                if let Some(first) = errors.first() {
                    write!(f, "; first: {first}")?;
                }
                Ok(())
            }
            Self::Invariant { detail } => {
                write!(f, "cyrs-hir: lowering invariant violated: {detail}")
            }
        }
    }
}

impl std::error::Error for HirLowerError {}
