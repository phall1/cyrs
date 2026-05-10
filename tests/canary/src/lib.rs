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

// ── cypher-plan ───────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cypher_plan::Direction`].
pub fn touch_plan_direction(d: &cypher_plan::Direction) {
    use cypher_plan::Direction;
    match d {
        Direction::Outgoing => (),
        Direction::Incoming => (),
        Direction::Undirected => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_plan::RelLength`].
pub fn touch_plan_rel_length(r: &cypher_plan::RelLength) {
    use cypher_plan::RelLength;
    match r {
        RelLength::Single => (),
        RelLength::Variable { .. } => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_plan::UnionKind`].
pub fn touch_plan_union_kind(u: &cypher_plan::UnionKind) {
    use cypher_plan::UnionKind;
    match u {
        UnionKind::All => (),
        UnionKind::Distinct => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_plan::SortDir`].
pub fn touch_plan_sort_dir(s: &cypher_plan::SortDir) {
    use cypher_plan::SortDir;
    match s {
        SortDir::Asc => (),
        SortDir::Desc => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_plan::BinOp`].
pub fn touch_plan_bin_op(b: &cypher_plan::BinOp) {
    use cypher_plan::BinOp;
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

/// Wildcard-tolerant exhaustive match on [`cypher_plan::UnaryOp`].
pub fn touch_plan_unary_op(u: &cypher_plan::UnaryOp) {
    use cypher_plan::UnaryOp;
    match u {
        UnaryOp::Neg => (),
        UnaryOp::Not => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_plan::ListPredKind`].
pub fn touch_plan_list_pred_kind(k: &cypher_plan::ListPredKind) {
    use cypher_plan::ListPredKind;
    match k {
        ListPredKind::Any => (),
        ListPredKind::All => (),
        ListPredKind::None => (),
        ListPredKind::Single => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_plan::ReadOp`].
pub fn touch_plan_read_op(op: &cypher_plan::ReadOp) {
    use cypher_plan::ReadOp;
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

/// Wildcard-tolerant exhaustive match on [`cypher_plan::WriteOp`].
pub fn touch_plan_write_op(op: &cypher_plan::WriteOp) {
    use cypher_plan::WriteOp;
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

/// Wildcard-tolerant exhaustive match on [`cypher_plan::Expr`].
pub fn touch_plan_expr(e: &cypher_plan::Expr) {
    use cypher_plan::Expr;
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

// ── cypher-hir ────────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cypher_hir::Direction`].
pub fn touch_hir_direction(d: &cypher_hir::Direction) {
    use cypher_hir::Direction;
    match d {
        Direction::Outgoing => (),
        Direction::Incoming => (),
        Direction::Undirected => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_hir::RelLength`].
pub fn touch_hir_rel_length(r: &cypher_hir::RelLength) {
    use cypher_hir::RelLength;
    match r {
        RelLength::Single => (),
        RelLength::Variable { .. } => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_hir::ListPredKind`].
pub fn touch_hir_list_pred_kind(k: &cypher_hir::ListPredKind) {
    use cypher_hir::ListPredKind;
    match k {
        ListPredKind::Any => (),
        ListPredKind::All => (),
        ListPredKind::None => (),
        ListPredKind::Single => (),
        _ => (),
    }
}

// ── cypher-diag ───────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cypher_diag::Severity`].
pub fn touch_diag_severity(s: &cypher_diag::Severity) {
    use cypher_diag::Severity;
    match s {
        Severity::Error => (),
        Severity::Warning => (),
        Severity::Note => (),
        Severity::Help => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_diag::Applicability`].
pub fn touch_diag_applicability(a: &cypher_diag::Applicability) {
    use cypher_diag::Applicability;
    match a {
        Applicability::MachineApplicable => (),
        Applicability::MaybeIncorrect => (),
        Applicability::HasPlaceholders => (),
        Applicability::Unspecified => (),
        _ => (),
    }
}

// ── cypher-db ─────────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cypher_db::DialectMode`].
pub fn touch_db_dialect_mode(d: &cypher_db::DialectMode) {
    use cypher_db::DialectMode;
    match d {
        DialectMode::GqlAligned => (),
        DialectMode::OpenCypherV9 => (),
        _ => (),
    }
}

// ── cypher-lang-services ──────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on
/// [`cypher_lang_services::CompletionItemKind`].
pub fn touch_completion_item_kind(k: &cypher_lang_services::CompletionItemKind) {
    use cypher_lang_services::CompletionItemKind;
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
/// [`cypher_lang_services::SymbolKind`].
pub fn touch_symbol_kind(k: &cypher_lang_services::SymbolKind) {
    use cypher_lang_services::SymbolKind;
    match k {
        SymbolKind::Label => (),
        SymbolKind::RelType => (),
        SymbolKind::Param => (),
        SymbolKind::NamedPath => (),
        _ => (),
    }
}

// ── cypher-schema ─────────────────────────────────────────────────────────────

/// Wildcard-tolerant exhaustive match on [`cypher_schema::Cardinality`].
pub fn touch_schema_cardinality(c: &cypher_schema::Cardinality) {
    use cypher_schema::Cardinality;
    match c {
        Cardinality::OneToOne => (),
        Cardinality::OneToMany => (),
        Cardinality::ManyToOne => (),
        Cardinality::ManyToMany => (),
        _ => (),
    }
}

/// Wildcard-tolerant exhaustive match on [`cypher_schema::ProcMode`].
pub fn touch_schema_proc_mode(m: &cypher_schema::ProcMode) {
    use cypher_schema::ProcMode;
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
        touch_plan_direction(&cypher_plan::Direction::Outgoing);
        touch_plan_rel_length(&cypher_plan::RelLength::Single);
        touch_plan_union_kind(&cypher_plan::UnionKind::All);
        touch_plan_sort_dir(&cypher_plan::SortDir::Asc);
        touch_plan_bin_op(&cypher_plan::BinOp::Add);
        touch_plan_unary_op(&cypher_plan::UnaryOp::Neg);
        touch_plan_list_pred_kind(&cypher_plan::ListPredKind::Any);
        touch_hir_direction(&cypher_hir::Direction::Outgoing);
        touch_hir_rel_length(&cypher_hir::RelLength::Single);
        touch_hir_list_pred_kind(&cypher_hir::ListPredKind::Any);
        touch_diag_severity(&cypher_diag::Severity::Error);
        touch_diag_applicability(&cypher_diag::Applicability::Unspecified);
        touch_db_dialect_mode(&cypher_db::DialectMode::GqlAligned);
        touch_completion_item_kind(&cypher_lang_services::CompletionItemKind::Keyword);
        touch_symbol_kind(&cypher_lang_services::SymbolKind::Label);
        touch_schema_cardinality(&cypher_schema::Cardinality::OneToOne);
        touch_schema_proc_mode(&cypher_schema::ProcMode::Read);
    }
}
