//! `cyrs-canary` — downstream-consumer canary for cy-2i9.1
//! `#[non_exhaustive]` coverage.
//!
//! This crate impersonates a downstream `cyrs` consumer. It depends on
//! the same public crates a real consumer would (`cyrs-plan`,
//! `cyrs-hir`, `cyrs-diag`, `cyrs-db`, `cyrs-lang-services`,
//! `cyrs-schema`) and exercises every enum that cy-2i9.1 marked
//! `#[non_exhaustive]` with a match that:
//!
//! 1. lists every currently-known variant (so `cargo build` breaks if a
//!    *removal* slips through — removing a variant is a SemVer-major
//!    event we want to catch), and
//! 2. ends with a wildcard arm (`_ => ()`) — proof that the enum is
//!    reachable from outside its defining crate without an exhaustive
//!    pattern, the property `#[non_exhaustive]` is supposed to provide.
//!
//! `#![deny(unreachable_patterns)]` turns the wildcard's reachability
//! into a load-bearing assertion: a `non_exhaustive`-attributed enum
//! makes `_` reachable from another crate, so the deny lint stays
//! silent. If a future change drops `#[non_exhaustive]` from any of
//! these enums, the wildcard arm becomes unreachable (we already cover
//! every known variant) and the canary refuses to compile — that is
//! the regression signal cy-2i9 promised.
//!
//! Adding a *new* variant in any of the source crates is non-breaking
//! by design: the wildcard absorbs it. The canary still builds, and
//! that is exactly the consumer-facing stability guarantee in
//! `docs/stability.md`.
//!
//! See bead cy-e3h for the acceptance contract.

#![forbid(unsafe_code)]
#![deny(unreachable_patterns)]
#![deny(missing_docs)]

// ── cyrs-plan ───────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::Direction`].
pub fn touch_plan_direction(d: &cyrs_plan::Direction) {
    use cyrs_plan::Direction;
    match d {
        Direction::Outgoing => (),
        Direction::Incoming => (),
        Direction::Undirected => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::RelLength`].
pub fn touch_plan_rel_length(r: &cyrs_plan::RelLength) {
    use cyrs_plan::RelLength;
    match r {
        RelLength::Single => (),
        RelLength::Variable { .. } => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::UnionKind`].
pub fn touch_plan_union_kind(u: &cyrs_plan::UnionKind) {
    use cyrs_plan::UnionKind;
    match u {
        UnionKind::All => (),
        UnionKind::Distinct => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::SortDir`].
pub fn touch_plan_sort_dir(s: &cyrs_plan::SortDir) {
    use cyrs_plan::SortDir;
    match s {
        SortDir::Asc => (),
        SortDir::Desc => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::BinOp`].
pub fn touch_plan_bin_op(b: &cyrs_plan::BinOp) {
    use cyrs_plan::BinOp;
    match b {
        BinOp::Add => (),
        BinOp::Sub => (),
        BinOp::Mul => (),
        BinOp::Div => (),
        BinOp::Mod => (),
        BinOp::Pow => (),
        BinOp::Eq => (),
        BinOp::Neq => (),
        BinOp::Lt => (),
        BinOp::Le => (),
        BinOp::Gt => (),
        BinOp::Ge => (),
        BinOp::And => (),
        BinOp::Or => (),
        BinOp::Xor => (),
        BinOp::In => (),
        BinOp::StartsWith => (),
        BinOp::EndsWith => (),
        BinOp::Contains => (),
        BinOp::RegexMatch => (),
        BinOp::Concat => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::UnaryOp`].
pub fn touch_plan_unary_op(u: &cyrs_plan::UnaryOp) {
    use cyrs_plan::UnaryOp;
    match u {
        UnaryOp::Neg => (),
        UnaryOp::Not => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::ListPredKind`].
pub fn touch_plan_list_pred_kind(k: &cyrs_plan::ListPredKind) {
    use cyrs_plan::ListPredKind;
    match k {
        ListPredKind::Any => (),
        ListPredKind::All => (),
        ListPredKind::None => (),
        ListPredKind::Single => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::ReadOp`].
pub fn touch_plan_read_op(op: &cyrs_plan::ReadOp) {
    use cyrs_plan::ReadOp;
    match op {
        ReadOp::Source { .. } => (),
        ReadOp::Expand { .. } => (),
        ReadOp::Filter { .. } => (),
        ReadOp::Project { .. } => (),
        ReadOp::Aggregate { .. } => (),
        ReadOp::OrderBy { .. } => (),
        ReadOp::Skip { .. } => (),
        ReadOp::Limit { .. } => (),
        ReadOp::Distinct { .. } => (),
        ReadOp::Unwind { .. } => (),
        ReadOp::Union { .. } => (),
        ReadOp::With { .. } => (),
        ReadOp::OptionalJoin { .. } => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::WriteOp`].
pub fn touch_plan_write_op(op: &cyrs_plan::WriteOp) {
    use cyrs_plan::WriteOp;
    match op {
        WriteOp::CreateNode { .. } => (),
        WriteOp::CreateRel { .. } => (),
        WriteOp::MergeNode { .. } => (),
        WriteOp::MergeRel { .. } => (),
        WriteOp::SetProperty { .. } => (),
        WriteOp::SetLabels { .. } => (),
        WriteOp::RemoveProperty { .. } => (),
        WriteOp::RemoveLabels { .. } => (),
        WriteOp::Delete { .. } => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_plan::Expr`].
pub fn touch_plan_expr(e: &cyrs_plan::Expr) {
    use cyrs_plan::Expr;
    match e {
        Expr::Null => (),
        Expr::Bool(_) => (),
        Expr::Int(_) => (),
        Expr::Float(_) => (),
        Expr::String(_) => (),
        Expr::Var(_) => (),
        Expr::Prop { .. } => (),
        Expr::Index { .. } => (),
        Expr::Slice { .. } => (),
        Expr::List(_) => (),
        Expr::Map(_) => (),
        Expr::Call { .. } => (),
        Expr::BinOp { .. } => (),
        Expr::UnaryOp { .. } => (),
        Expr::Case { .. } => (),
        Expr::IsNull { .. } => (),
        Expr::InList { .. } => (),
        Expr::ListPredicate { .. } => (),
        Expr::Param { .. } => (),
        Expr::Exists { .. } => (),
        _ => (),
    }
}

// ── cyrs-hir ────────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cyrs_hir::Direction`].
pub fn touch_hir_direction(d: &cyrs_hir::Direction) {
    use cyrs_hir::Direction;
    match d {
        Direction::Outgoing => (),
        Direction::Incoming => (),
        Direction::Undirected => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_hir::RelLength`].
pub fn touch_hir_rel_length(r: &cyrs_hir::RelLength) {
    use cyrs_hir::RelLength;
    match r {
        RelLength::Single => (),
        RelLength::Variable { .. } => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_hir::ListPredKind`].
pub fn touch_hir_list_pred_kind(k: &cyrs_hir::ListPredKind) {
    use cyrs_hir::ListPredKind;
    match k {
        ListPredKind::Any => (),
        ListPredKind::All => (),
        ListPredKind::None => (),
        ListPredKind::Single => (),
        _ => (),
    }
}

// ── cyrs-diag ───────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cyrs_diag::Severity`].
pub fn touch_diag_severity(s: &cyrs_diag::Severity) {
    use cyrs_diag::Severity;
    match s {
        Severity::Error => (),
        Severity::Warning => (),
        Severity::Note => (),
        Severity::Help => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_diag::Applicability`].
pub fn touch_diag_applicability(a: &cyrs_diag::Applicability) {
    use cyrs_diag::Applicability;
    match a {
        Applicability::MachineApplicable => (),
        Applicability::MaybeIncorrect => (),
        Applicability::HasPlaceholders => (),
        Applicability::Unspecified => (),
        _ => (),
    }
}

// ── cyrs-db ─────────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cyrs_db::DialectMode`].
pub fn touch_db_dialect_mode(d: &cyrs_db::DialectMode) {
    use cyrs_db::DialectMode;
    match d {
        DialectMode::GqlAligned => (),
        DialectMode::OpenCypherV9 => (),
        _ => (),
    }
}

// ── cyrs-lang-services ──────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on
/// [`cyrs_lang_services::CompletionItemKind`].
pub fn touch_completion_item_kind(k: &cyrs_lang_services::CompletionItemKind) {
    use cyrs_lang_services::CompletionItemKind;
    match k {
        CompletionItemKind::Keyword => (),
        CompletionItemKind::Label => (),
        CompletionItemKind::RelationshipType => (),
        CompletionItemKind::Parameter => (),
        CompletionItemKind::Property => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on
/// [`cyrs_lang_services::SymbolKind`].
pub fn touch_symbol_kind(k: &cyrs_lang_services::SymbolKind) {
    use cyrs_lang_services::SymbolKind;
    match k {
        SymbolKind::Label => (),
        SymbolKind::RelType => (),
        SymbolKind::Param => (),
        SymbolKind::NamedPath => (),
        _ => (),
    }
}

// ── cyrs-schema ─────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cyrs_schema::Cardinality`].
pub fn touch_schema_cardinality(c: &cyrs_schema::Cardinality) {
    use cyrs_schema::Cardinality;
    match c {
        Cardinality::OneToOne => (),
        Cardinality::OneToMany => (),
        Cardinality::ManyToOne => (),
        Cardinality::ManyToMany => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cyrs_schema::ProcMode`].
pub fn touch_schema_proc_mode(m: &cyrs_schema::ProcMode) {
    use cyrs_schema::ProcMode;
    match m {
        ProcMode::Read => (),
        ProcMode::Write => (),
        ProcMode::Schema => (),
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    //! Smoke tests: feed one variant of each enum into the canary
    //! matcher to prove the surface is reachable. The deny-lints in the
    //! crate root do the load-bearing work; these tests just keep the
    //! matchers from being optimised out of `cargo build`.

    use super::*;

    #[test]
    fn matchers_run_without_panicking() {
        touch_plan_direction(&cyrs_plan::Direction::Outgoing);
        touch_plan_rel_length(&cyrs_plan::RelLength::Single);
        touch_plan_union_kind(&cyrs_plan::UnionKind::All);
        touch_plan_sort_dir(&cyrs_plan::SortDir::Asc);
        touch_plan_bin_op(&cyrs_plan::BinOp::Add);
        touch_plan_unary_op(&cyrs_plan::UnaryOp::Neg);
        touch_plan_list_pred_kind(&cyrs_plan::ListPredKind::Any);
        touch_hir_direction(&cyrs_hir::Direction::Outgoing);
        touch_hir_rel_length(&cyrs_hir::RelLength::Single);
        touch_hir_list_pred_kind(&cyrs_hir::ListPredKind::Any);
        touch_diag_severity(&cyrs_diag::Severity::Error);
        touch_diag_applicability(&cyrs_diag::Applicability::Unspecified);
        touch_db_dialect_mode(&cyrs_db::DialectMode::GqlAligned);
        touch_completion_item_kind(&cyrs_lang_services::CompletionItemKind::Keyword);
        touch_symbol_kind(&cyrs_lang_services::SymbolKind::Label);
        touch_schema_cardinality(&cyrs_schema::Cardinality::OneToOne);
        touch_schema_proc_mode(&cyrs_schema::ProcMode::Read);
    }
}
