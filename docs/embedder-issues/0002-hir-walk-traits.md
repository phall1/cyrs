# 0002 — HIR walk traits matching a legacy AST visitor surface

**Severity:** high (blocks stage 2 of the migration)
**Discovered:** embedder plan/builder survey

## Problem

The embedder's `plan/builder.rs` is ~3700 lines of match arms over the
legacy parser's AST shape — `match clause { Match(_) => …, Where(_) => …, … }`.
`semantic.rs` is another ~1200 lines of the same pattern. To migrate
these to cyrs, embedders need a HIR walk surface that:

1. Is exhaustive (every grammar production reachable).
2. Carries spans for diagnostic mapping.
3. Has a `Visitor` and `MutVisitor` trait, or at minimum stable
   `match`-able enums whose variants don't change without a major bump.

Today `cypher-hir` exposes `Statement`, `Clause`, `Expr`, etc., as
public enums (good!) — but there is no documented walk trait. Embedders
must reinvent visitor patterns and accept the churn.

## Proposed shape

```rust
// in cypher-hir
pub trait Visitor {
    fn visit_statement(&mut self, stmt: &Statement) { walk_statement(self, stmt) }
    fn visit_clause(&mut self, clause: &Clause)    { walk_clause(self, clause) }
    fn visit_expr(&mut self, expr: &Expr)          { walk_expr(self, expr) }
    // … per production
}

pub fn walk_statement<V: Visitor + ?Sized>(v: &mut V, s: &Statement) { … }
```

Modeled after `syn::visit::Visit` / `rustc_ast::visit::Visitor`.

## Stretch: codegen the walker

Since the typed AST is already codegen'd from `cypher.ungrammar`, the
HIR walker could be too. That eliminates drift between grammar evolution
and visitor coverage.

## Why it matters for the embedder

Without this, stage 2 (porting plan/builder.rs and semantic.rs) requires
hand-writing recursive `match` arms across every HIR variant — duplicating
work the cyrs project will eventually need to do anyway for
`cypher-sema` and `cypher-fmt`.
