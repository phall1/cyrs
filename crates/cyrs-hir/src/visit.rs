//! HIR visitor surface (cy-emb2).
//!
//! A [`Visitor`] trait + family of `walk_*` free functions, modelled on
//! `syn::visit::Visit` and `rustc_ast::visit::Visitor`. Every default
//! `visit_*` method delegates to the matching `walk_*` free function so
//! impls only override the productions they care about; recursion into
//! sub-nodes happens automatically.
//!
//! This is the trait surface embedders need to walk an HIR
//! [`Statement`] without hand-rolling a `match` tree over every variant.
//! See `docs/embedder-issues/0002-hir-walk-traits.md` for the embedder
//! migration story.
//!
//! # Style note
//!
//! The `walk_*` functions intentionally keep one arm per HIR variant
//! even when several arms have identical bodies. The shape mirrors the
//! grammar one-to-one so a future variant addition produces a
//! `non_exhaustive` compile error at exactly the right line, and so
//! per-variant overrides remain easy to grep for.
//!
//! # Mutation
//!
//! This module is **read-only**: `&` references everywhere. A
//! mutating `MutVisitor` / `walk_*_mut` family doubles the public
//! surface and is intentionally deferred to a follow-up bead.
//!
//! # Example
//!
//! ```
//! use cyrs_hir::{Expr, VarId, lower::lower_statement};
//! use cyrs_hir::visit::{Visitor, walk_expr};
//!
//! /// Collects every variable reference (`Expr::Var`) reachable from a
//! /// statement. Models the kind of pass an embedder needs when migrating
//! /// off a hand-rolled AST walker.
//! #[derive(Default)]
//! struct VarCollector {
//!     vars: Vec<VarId>,
//! }
//!
//! impl Visitor for VarCollector {
//!     fn visit_expr(&mut self, e: &Expr) {
//!         if let Expr::Var(v) = e {
//!             self.vars.push(*v);
//!         }
//!         walk_expr(self, e);
//!     }
//! }
//!
//! let stmt = lower_statement("MATCH (a) RETURN a");
//! let mut c = VarCollector::default();
//! c.visit_statement(&stmt);
//! assert!(!c.vars.is_empty());
//! ```

// Walk functions deliberately keep one arm per grammar production even
// when bodies coincide — see the module-level "Style note".
#![allow(clippy::match_same_arms)]

use crate::{
    Clause, Expr, MapProjectionItem, Pattern, PatternElement, PatternPart, Projection, RemoveItem,
    SetItem, Statement, YieldItem,
};

/// Read-only HIR visitor.
///
/// Default method bodies dispatch to the corresponding `walk_*` free
/// function, which in turn calls back into the visitor. Override only
/// the methods whose productions you care about; recursion into the
/// rest of the tree happens automatically.
///
/// To stop recursion at a node, override the relevant `visit_*` method
/// and do **not** call the matching `walk_*`.
pub trait Visitor: Sized {
    fn visit_statement(&mut self, stmt: &Statement) {
        walk_statement(self, stmt);
    }

    fn visit_clause(&mut self, clause: &Clause) {
        walk_clause(self, clause);
    }

    fn visit_pattern(&mut self, pattern: &Pattern) {
        walk_pattern(self, pattern);
    }

    fn visit_pattern_part(&mut self, part: &PatternPart) {
        walk_pattern_part(self, part);
    }

    fn visit_pattern_element(&mut self, element: &PatternElement) {
        walk_pattern_element(self, element);
    }

    fn visit_projection(&mut self, projection: &Projection) {
        walk_projection(self, projection);
    }

    fn visit_set_item(&mut self, item: &SetItem) {
        walk_set_item(self, item);
    }

    fn visit_remove_item(&mut self, item: &RemoveItem) {
        walk_remove_item(self, item);
    }

    fn visit_yield_item(&mut self, item: &YieldItem) {
        walk_yield_item(self, item);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }

    fn visit_map_projection_item(&mut self, item: &MapProjectionItem) {
        walk_map_projection_item(self, item);
    }
}

// ---------------------------------------------------------------------------
// walk_* free functions
// ---------------------------------------------------------------------------

pub fn walk_statement<V: Visitor>(v: &mut V, s: &Statement) {
    for clause in &s.clauses {
        v.visit_clause(clause);
    }
}

pub fn walk_clause<V: Visitor>(v: &mut V, c: &Clause) {
    match c {
        Clause::Match { pattern, .. } => v.visit_pattern(pattern),
        Clause::Where { predicate, .. } => v.visit_expr(predicate),
        Clause::With {
            projections,
            filter,
            ..
        } => {
            for p in projections {
                v.visit_projection(p);
            }
            if let Some(f) = filter {
                v.visit_expr(f);
            }
        }
        Clause::Return { projections, .. } => {
            for p in projections {
                v.visit_projection(p);
            }
        }
        Clause::Unwind { list, .. } => v.visit_expr(list),
        Clause::Create { pattern, .. } => v.visit_pattern(pattern),
        Clause::Merge {
            pattern,
            on_create,
            on_match,
            ..
        } => {
            v.visit_pattern(pattern);
            for it in on_create {
                v.visit_set_item(it);
            }
            for it in on_match {
                v.visit_set_item(it);
            }
        }
        Clause::Set { items, .. } => {
            for it in items {
                v.visit_set_item(it);
            }
        }
        Clause::Remove { items, .. } => {
            for it in items {
                v.visit_remove_item(it);
            }
        }
        Clause::Delete { targets, .. } => {
            for t in targets {
                v.visit_expr(t);
            }
        }
        Clause::Call { args, yields, .. } => {
            for a in args {
                v.visit_expr(a);
            }
            for y in yields {
                v.visit_yield_item(y);
            }
        }
    }
}

pub fn walk_pattern<V: Visitor>(v: &mut V, p: &Pattern) {
    for part in &p.parts {
        v.visit_pattern_part(part);
    }
}

pub fn walk_pattern_part<V: Visitor>(v: &mut V, p: &PatternPart) {
    for e in &p.elements {
        v.visit_pattern_element(e);
    }
}

pub fn walk_pattern_element<V: Visitor>(v: &mut V, e: &PatternElement) {
    match e {
        PatternElement::Node { props, .. } | PatternElement::Rel { props, .. } => {
            if let Some(props) = props {
                v.visit_expr(props);
            }
        }
    }
}

pub fn walk_projection<V: Visitor>(v: &mut V, p: &Projection) {
    v.visit_expr(&p.expr);
}

pub fn walk_set_item<V: Visitor>(v: &mut V, item: &SetItem) {
    match item {
        SetItem::Property { target, value, .. } => {
            v.visit_expr(target);
            v.visit_expr(value);
        }
        SetItem::Labels { .. } => {}
        SetItem::AssignMap { map, .. } => {
            v.visit_expr(map);
        }
    }
}

pub fn walk_remove_item<V: Visitor>(v: &mut V, item: &RemoveItem) {
    match item {
        RemoveItem::Property { target, .. } => v.visit_expr(target),
        RemoveItem::Labels { .. } => {}
    }
}

pub fn walk_yield_item<V: Visitor>(_v: &mut V, _item: &YieldItem) {
    // YieldItem carries only names; nothing to recurse into.
}

pub fn walk_expr<V: Visitor>(v: &mut V, e: &Expr) {
    match e {
        // Leaf expressions — nothing to recurse into.
        Expr::Null
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::Param(_)
        | Expr::Unresolved(_) => {}

        Expr::Prop { target, .. } => v.visit_expr(target),
        Expr::Index { target, index } => {
            v.visit_expr(target);
            v.visit_expr(index);
        }
        Expr::Slice { target, start, end } => {
            v.visit_expr(target);
            if let Some(s) = start {
                v.visit_expr(s);
            }
            if let Some(e) = end {
                v.visit_expr(e);
            }
        }
        Expr::List(items) => {
            for it in items {
                v.visit_expr(it);
            }
        }
        Expr::Map(pairs) => {
            for (_k, val) in pairs {
                v.visit_expr(val);
            }
        }
        Expr::Call { args, .. } => {
            for a in args {
                v.visit_expr(a);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            v.visit_expr(lhs);
            v.visit_expr(rhs);
        }
        Expr::UnaryOp { operand, .. } => v.visit_expr(operand),
        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            if let Some(s) = scrutinee {
                v.visit_expr(s);
            }
            for (cond, body) in arms {
                v.visit_expr(cond);
                v.visit_expr(body);
            }
            if let Some(o) = otherwise {
                v.visit_expr(o);
            }
        }
        Expr::IsNull { operand, .. } => v.visit_expr(operand),
        Expr::InList { operand, list } => {
            v.visit_expr(operand);
            v.visit_expr(list);
        }
        Expr::PatternPredicate(pat) => v.visit_pattern(pat),
        Expr::ListComprehension {
            iterable,
            filter,
            map_expr,
            ..
        } => {
            v.visit_expr(iterable);
            if let Some(f) = filter {
                v.visit_expr(f);
            }
            v.visit_expr(map_expr);
        }
        Expr::ListPredicate {
            iterable,
            predicate,
            ..
        } => {
            v.visit_expr(iterable);
            if let Some(p) = predicate {
                v.visit_expr(p);
            }
        }
        Expr::MapProjection { base, items } => {
            v.visit_expr(base);
            for it in items {
                v.visit_map_projection_item(it);
            }
        }
    }
}

pub fn walk_map_projection_item<V: Visitor>(v: &mut V, item: &MapProjectionItem) {
    match item {
        MapProjectionItem::PropCopy { .. } | MapProjectionItem::VarShorthand { .. } => {}
        MapProjectionItem::Computed { value, .. } | MapProjectionItem::Aliased { value, .. } => {
            v.visit_expr(value);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Binding, Clause, Direction, Expr, HirId, Pattern, PatternElement, PatternPart, Projection,
        RelLength, Statement, VarId, VarKind,
    };
    use cyrs_syntax::parse;
    use smol_str::SmolStr;

    fn fixture_stmt() -> Statement {
        // Manually-constructed HIR for `MATCH (a:Person) WHERE a.age > 30 RETURN a`,
        // exercising Match + Where + Return clauses with nested expression structure.
        let root = parse("MATCH (a:Person) WHERE a.age > 30 RETURN a").syntax();
        let mut stmt = Statement::new(root.text_range());

        let var = VarId(0);
        stmt.bindings.insert(
            var,
            Binding {
                id: var,
                name: SmolStr::new("a"),
                kind: VarKind::Node,
                defined_at: root.text_range(),
            },
        );

        let match_id = stmt.alloc_id(root.clone());
        let node_id = stmt.alloc_id(root.clone());
        stmt.clauses.push(Clause::Match {
            id: match_id,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: node_id,
                        bind: Some(var),
                        labels: vec![SmolStr::new("Person")],
                        props: None,
                        span: root.text_range(),
                    }],
                }],
            },
            span: root.text_range(),
        });

        let where_id = stmt.alloc_id(root.clone());
        stmt.clauses.push(Clause::Where {
            id: where_id,
            predicate: Expr::BinOp {
                op: crate::BinOp::Gt,
                lhs: Box::new(Expr::Prop {
                    target: Box::new(Expr::Var(var)),
                    prop: SmolStr::new("age"),
                }),
                rhs: Box::new(Expr::Int(30)),
            },
            span: root.text_range(),
        });

        let return_id = stmt.alloc_id(root.clone());
        stmt.clauses.push(Clause::Return {
            id: return_id,
            projections: vec![Projection {
                expr: Expr::Var(var),
                alias: None,
                span: root.text_range(),
            }],
            distinct: false,
            order_by: Vec::new(),
            skip: None,
            limit: None,
            span: root.text_range(),
        });

        // Force-use Direction / RelLength so the imports are referenced in tests.
        let _ = Direction::Outgoing;
        let _ = RelLength::Single;
        let _ = HirId::DUMMY;
        stmt
    }

    #[derive(Default)]
    struct ClauseCounter {
        matches: usize,
        wheres: usize,
        returns: usize,
        other: usize,
    }

    impl Visitor for ClauseCounter {
        fn visit_clause(&mut self, c: &Clause) {
            match c {
                Clause::Match { .. } => self.matches += 1,
                Clause::Where { .. } => self.wheres += 1,
                Clause::Return { .. } => self.returns += 1,
                _ => self.other += 1,
            }
            walk_clause(self, c);
        }
    }

    #[test]
    fn visitor_counts_clause_variants() {
        let stmt = fixture_stmt();
        let mut c = ClauseCounter::default();
        c.visit_statement(&stmt);
        assert_eq!(c.matches, 1);
        assert_eq!(c.wheres, 1);
        assert_eq!(c.returns, 1);
        assert_eq!(c.other, 0);
    }

    #[derive(Default)]
    struct VarCollector {
        vars: Vec<VarId>,
    }

    impl Visitor for VarCollector {
        fn visit_expr(&mut self, e: &Expr) {
            if let Expr::Var(v) = e {
                self.vars.push(*v);
            }
            walk_expr(self, e);
        }
    }

    #[test]
    fn visitor_descends_into_nested_expressions() {
        // Expr::BinOp { lhs: Prop { Var(a) }, rhs: Int(30) } in WHERE,
        // plus Var(a) in RETURN. Three Var refs total via Prop/BinOp + projection.
        let stmt = fixture_stmt();
        let mut c = VarCollector::default();
        c.visit_statement(&stmt);
        assert_eq!(c.vars.len(), 2, "expected 2 Var refs, got {:?}", c.vars);
        assert!(c.vars.iter().all(|v| *v == VarId(0)));
    }

    #[test]
    fn default_visit_methods_recurse() {
        // A Visitor that overrides nothing should still recurse (and do
        // nothing), proving the default-method delegation chain is intact.
        struct Noop;
        impl Visitor for Noop {}

        let stmt = fixture_stmt();
        let mut n = Noop;
        n.visit_statement(&stmt);
    }
}
