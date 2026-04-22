//! Schema-free type-inference pass (spec 0001 §7.4).
//!
//! Walks the resolved, desugared HIR and assigns a [`Type`] to each
//! expression using the unification engine from [`mod@crate::unify`]. Where
//! unification fails or a structural constraint is violated, a diagnostic
//! is emitted into the [`DiagnosticsSink`].
//!
//! # Scope of this pass (schema-free only)
//!
//! This pass implements the schema-free rules from §7.1 / §7.4:
//!
//! | Rule | Code |
//! |------|------|
//! | Arithmetic operands must unify with `Num` | [`E2009`] |
//! | String-operator operands must be `String` | [`E2010`] |
//! | Boolean-operator operands must be `Bool` | [`E2011`] |
//! | `IN` list element type unification | [`E2012`] |
//! | `IN` right-hand side must be a list | [`E2013`] |
//! | `IS NULL` / `IS NOT NULL` result is `Bool` (no type constraint) | — |
//! | Literal type assignment | — |
//!
//! **Deferred (see spec §20):**
//! - Function-call arity and return-type checks are deferred until a
//!   `StandardLibrary` / `SchemaProvider` is available (cy-36u). Without
//!   schema, all calls return [`Type::Any`].
//! - Parameter-type unification across sites (E2005) is deferred; this pass
//!   yields [`Type::Any`] for every [`Expr::Param`].
//!
//! # No schema-aware checks
//!
//! This pass deliberately does **not** validate against label schemas,
//! property types, or function catalogs. Those checks live in the
//! schema-aware pass (cy-36u, E3xxx codes).
//!
//! [`E2009`]: cypher_diag::DiagCode::E2009
//! [`E2010`]: cypher_diag::DiagCode::E2010
//! [`E2011`]: cypher_diag::DiagCode::E2011
//! [`E2012`]: cypher_diag::DiagCode::E2012
//! [`E2013`]: cypher_diag::DiagCode::E2013

use cypher_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cypher_hir::{
    BinOp, Binding, Clause, Expr, HirOffset, HirSpan, MapProjectionItem, Pattern, PatternElement,
    SetItem, Statement, UnaryOp, VarId,
};
use indexmap::IndexMap;

use crate::ty::Type;
use crate::unify::unify;

/// Run the schema-free type-inference pass on a resolved HIR statement.
///
/// The pass walks every expression in every clause, infers types bottom-up,
/// and emits diagnostics for structural type violations.
///
/// # No short-circuiting
///
/// Per spec §10.4, this pass never short-circuits: all expressions are
/// visited even after errors, so as many diagnostics as possible are
/// reported in a single pass.
pub fn infer_types(stmt: &Statement, sink: &mut DiagnosticsSink) {
    let mut ctx = InferCtx {
        bindings: &stmt.bindings,
        sink,
    };
    ctx.check_statement(stmt);
}

// ---------------------------------------------------------------------------
// Internal context
// ---------------------------------------------------------------------------

struct InferCtx<'a> {
    bindings: &'a IndexMap<VarId, Binding>,
    sink: &'a mut DiagnosticsSink,
}

impl InferCtx<'_> {
    fn check_statement(&mut self, stmt: &Statement) {
        for clause in &stmt.clauses {
            self.check_clause(clause);
        }
    }

    fn check_clause(&mut self, clause: &Clause) {
        match clause {
            Clause::Match { pattern, .. }
            | Clause::Create { pattern, .. }
            | Clause::Merge { pattern, .. } => {
                self.check_pattern(pattern);
            }
            Clause::Where { predicate, .. } => {
                self.infer_expr(predicate);
            }
            Clause::With {
                projections,
                filter,
                ..
            } => {
                for proj in projections {
                    self.infer_expr(&proj.expr);
                }
                if let Some(f) = filter {
                    self.infer_expr(f);
                }
            }
            Clause::Return { projections, .. } => {
                for proj in projections {
                    self.infer_expr(&proj.expr);
                }
            }
            Clause::Unwind { list, .. } => {
                self.infer_expr(list);
            }
            Clause::Set { items, .. } => {
                for item in items {
                    match item {
                        SetItem::Property { target, value, .. } => {
                            self.infer_expr(target);
                            self.infer_expr(value);
                        }
                        SetItem::AssignMap { map, .. } => {
                            self.infer_expr(map);
                        }
                        SetItem::Labels { .. } => {}
                    }
                }
            }
            Clause::Remove { items, .. } => {
                use cypher_hir::RemoveItem;
                for item in items {
                    match item {
                        RemoveItem::Property { target, .. } => {
                            self.infer_expr(target);
                        }
                        RemoveItem::Labels { .. } => {}
                    }
                }
            }
            Clause::Delete { targets, .. } => {
                for expr in targets {
                    self.infer_expr(expr);
                }
            }
            Clause::Call { args, .. } => {
                for arg in args {
                    self.infer_expr(arg);
                }
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Pattern) {
        for part in &pattern.parts {
            for elem in &part.elements {
                match elem {
                    PatternElement::Node { props, .. } | PatternElement::Rel { props, .. } => {
                        if let Some(p) = props {
                            self.infer_expr(p);
                        }
                    }
                }
            }
        }
    }

    /// Infer the type of an expression, emitting diagnostics for violations.
    ///
    /// Returns the inferred [`Type`]. [`Type::Unknown`] is returned when
    /// inference fails so that cascading errors are suppressed.
    fn infer_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            // ---------------------------------------------------------------
            // Literals — types assigned directly (spec §7.2).
            // ---------------------------------------------------------------
            Expr::Null => Type::Null,
            Expr::Bool(_) => Type::Bool,
            Expr::Int(_) => Type::Int,
            Expr::Float(_) => Type::Float,
            Expr::String(_) => Type::String,

            // ---------------------------------------------------------------
            // Variables — look up from bindings.
            // Node/Rel/Path VarKinds map to their structural types; Value
            // bindings can hold any type so we return Any.
            // ---------------------------------------------------------------
            Expr::Var(var) => self.var_type(*var),

            // ---------------------------------------------------------------
            // Parameters — schema-free: Any (type inferred from context or
            // externally supplied hints; cross-site unification deferred).
            // ---------------------------------------------------------------
            Expr::Param(_) => Type::Any,

            // ---------------------------------------------------------------
            // Unresolved — already an error from name resolution; propagate
            // Unknown silently so we don't cascade.
            // ---------------------------------------------------------------
            Expr::Unresolved(_) => Type::Unknown,

            // ---------------------------------------------------------------
            // Property access — result type is Any in schema-free mode
            // (schema-free cannot know the property's type). The target
            // must support property access (non-Path), which kinds.rs checks.
            // ---------------------------------------------------------------
            Expr::Prop { target, .. } => {
                self.infer_expr(target);
                Type::Any
            }

            // ---------------------------------------------------------------
            // Subscript index — target should be a list or map; result Any.
            // ---------------------------------------------------------------
            Expr::Index { target, index } => {
                self.infer_expr(target);
                self.infer_expr(index);
                Type::Any
            }

            // ---------------------------------------------------------------
            // Slice — visit target + bounds so their type errors surface;
            // the element-type lattice work lives in the cy-7s6.1 sema
            // commit. Result is `Any` here; the kinds pass narrows to
            // `LIST<T>` when the target is list-shaped.
            // ---------------------------------------------------------------
            Expr::Slice { target, start, end } => {
                self.infer_expr(target);
                if let Some(s) = start {
                    self.infer_expr(s);
                }
                if let Some(e) = end {
                    self.infer_expr(e);
                }
                Type::Any
            }

            // ---------------------------------------------------------------
            // List literal — element types unified to find element type.
            // ---------------------------------------------------------------
            Expr::List(items) => {
                if items.is_empty() {
                    return Type::List(Box::new(Type::Any));
                }
                let mut elem_ty = self.infer_expr(&items[0]);
                for item in &items[1..] {
                    let t = self.infer_expr(item);
                    elem_ty = match unify(&elem_ty, &t) {
                        Ok(unified) => unified,
                        Err(_) => Type::Any,
                    };
                }
                Type::List(Box::new(elem_ty))
            }

            // ---------------------------------------------------------------
            // Map literal — structural map type.
            // ---------------------------------------------------------------
            Expr::Map(pairs) => {
                let mut fields = std::collections::BTreeMap::new();
                for (key, val_expr) in pairs {
                    let vt = self.infer_expr(val_expr);
                    fields.insert(key.clone(), vt);
                }
                Type::Map(fields)
            }

            // ---------------------------------------------------------------
            // Function calls — deferred (no catalog in schema-free mode).
            // All args are still visited so their errors are reported.
            // Returns Any.
            // ---------------------------------------------------------------
            Expr::Call { args, .. } => {
                for arg in args {
                    self.infer_expr(arg);
                }
                // Function arity / return-type checks deferred to schema-aware
                // pass (cy-36u). See spec §20.
                Type::Any
            }

            // ---------------------------------------------------------------
            // Binary operators (spec §7.4).
            // ---------------------------------------------------------------
            Expr::BinOp { op, lhs, rhs } => self.infer_binop(*op, lhs, rhs),

            // ---------------------------------------------------------------
            // Unary operators.
            // ---------------------------------------------------------------
            Expr::UnaryOp { op, operand } => self.infer_unaryop(*op, operand),

            // ---------------------------------------------------------------
            // CASE — result is the union of arm result types.
            // ---------------------------------------------------------------
            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => {
                if let Some(s) = scrutinee {
                    self.infer_expr(s);
                }
                let mut arm_types: Vec<Type> = Vec::new();
                for (cond, result) in arms {
                    let ct = self.infer_expr(cond);
                    // Conditions must be boolean.
                    if scrutinee.is_none() {
                        self.require_bool(&ct, dummy_span());
                    }
                    arm_types.push(self.infer_expr(result));
                }
                if let Some(o) = otherwise {
                    arm_types.push(self.infer_expr(o));
                }
                Type::union(arm_types)
            }

            // ---------------------------------------------------------------
            // IS NULL / IS NOT NULL — no type constraint on operand;
            // result is always Bool.
            // ---------------------------------------------------------------
            Expr::IsNull { operand, .. } => {
                self.infer_expr(operand);
                Type::Bool
            }

            // ---------------------------------------------------------------
            // IN list — operand type must unify with element type of list.
            // ---------------------------------------------------------------
            Expr::InList { operand, list } => {
                let op_ty = self.infer_expr(operand);
                let list_ty = self.infer_expr(list);
                self.check_in_list(&op_ty, &list_ty, dummy_span());
                Type::Bool
            }

            // ---------------------------------------------------------------
            // Pattern predicate — always Bool.
            // ---------------------------------------------------------------
            Expr::PatternPredicate(pat) => {
                self.check_pattern(pat);
                Type::Bool
            }

            // ---------------------------------------------------------------
            // List comprehension — result is List(map_expr_type).
            // ---------------------------------------------------------------
            Expr::ListComprehension {
                iterable,
                filter,
                map_expr,
                ..
            } => {
                self.infer_expr(iterable);
                if let Some(f) = filter {
                    self.infer_expr(f);
                }
                let elem_ty = self.infer_expr(map_expr);
                Type::List(Box::new(elem_ty))
            }

            // ---------------------------------------------------------------
            // Map projection — result is Map.
            // ---------------------------------------------------------------
            Expr::MapProjection { base, items } => {
                self.infer_expr(base);
                for item in items {
                    match item {
                        MapProjectionItem::Computed { value, .. }
                        | MapProjectionItem::Aliased { value, .. } => {
                            self.infer_expr(value);
                        }
                        MapProjectionItem::PropCopy { .. }
                        | MapProjectionItem::VarShorthand { .. } => {}
                    }
                }
                Type::Map(std::collections::BTreeMap::new())
            }
        }
    }

    // -----------------------------------------------------------------------
    // Binary operator inference (spec §7.4)
    // -----------------------------------------------------------------------

    fn infer_binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Type {
        let lt = self.infer_expr(lhs);
        let rt = self.infer_expr(rhs);
        let span = dummy_span();

        match op {
            // Arithmetic: operands must unify to Num; result is the unified
            // numeric type (Int + Int → Int; Int + Float → Num; etc.).
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                let lt2 = self.require_numeric(&lt, span);
                let rt2 = self.require_numeric(&rt, span);
                // Unify the two (now-confirmed) numeric types.
                match unify(&lt2, &rt2) {
                    Ok(t) => t,
                    Err(_) => Type::Unknown,
                }
            }

            // Comparison: operands should be comparable scalars; result Bool.
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                // Schema-free: we don't enforce a specific type for
                // comparisons (Any is fine), just require they unify.
                let _ = unify(&lt, &rt); // non-unifying is OK at schema-free level
                Type::Bool
            }

            // Equality: no type constraint schema-free; result Bool.
            BinOp::Eq | BinOp::Neq => Type::Bool,

            // Boolean operators: operands must be Bool.
            BinOp::And | BinOp::Or | BinOp::Xor => {
                self.require_bool(&lt, span);
                self.require_bool(&rt, span);
                Type::Bool
            }

            // String operators (CONTAINS, STARTS WITH, ENDS WITH) and regex:
            // both sides must be String.
            BinOp::StartsWith | BinOp::EndsWith | BinOp::Contains | BinOp::RegexMatch => {
                self.require_string(&lt, span);
                self.require_string(&rt, span);
                Type::Bool
            }

            // String / list concatenation: Any → Any (permissive schema-free).
            BinOp::Concat => Type::Any,
        }
    }

    // -----------------------------------------------------------------------
    // Unary operator inference
    // -----------------------------------------------------------------------

    fn infer_unaryop(&mut self, op: UnaryOp, operand: &Expr) -> Type {
        let span = dummy_span();
        let ot = self.infer_expr(operand);
        match op {
            UnaryOp::Neg => self.require_numeric(&ot, span),
            UnaryOp::Not => {
                self.require_bool(&ot, span);
                Type::Bool
            }
        }
    }

    // -----------------------------------------------------------------------
    // Type-requirement helpers
    // -----------------------------------------------------------------------

    /// Require that `ty` is numeric (Int, Float, or Num); emit E2009 if not.
    /// Returns the confirmed numeric type (or Unknown on error).
    fn require_numeric(&mut self, ty: &Type, span: HirSpan) -> Type {
        match ty {
            // Already numeric, or Any/Unknown — pass through without error.
            Type::Int | Type::Float | Type::Num | Type::Any | Type::Unknown => ty.clone(),
            // Non-numeric: emit error, return Unknown so we don't cascade.
            _ => {
                self.sink.push(Diagnostic::error(
                    DiagCode::E2009,
                    span,
                    format!(
                        "arithmetic operand must be numeric (`Int`, `Float`, or `Num`) \
                         but found `{}`",
                        type_name(ty),
                    ),
                ));
                Type::Unknown
            }
        }
    }

    /// Require that `ty` is a string; emit E2010 if not.
    fn require_string(&mut self, ty: &Type, span: HirSpan) {
        match ty {
            Type::String | Type::Any | Type::Unknown => {}
            _ => {
                self.sink.push(Diagnostic::error(
                    DiagCode::E2010,
                    span,
                    format!(
                        "string operator requires `String` operands but found `{}`",
                        type_name(ty),
                    ),
                ));
            }
        }
    }

    /// Require that `ty` is Bool; emit E2011 if not.
    fn require_bool(&mut self, ty: &Type, span: HirSpan) {
        match ty {
            Type::Bool | Type::Any | Type::Unknown => {}
            _ => {
                self.sink.push(Diagnostic::error(
                    DiagCode::E2011,
                    span,
                    format!(
                        "boolean operator requires `Bool` operands but found `{}`",
                        type_name(ty),
                    ),
                ));
            }
        }
    }

    /// Check that the `IN` operand is compatible with the list's element type.
    fn check_in_list(&mut self, op_ty: &Type, list_ty: &Type, span: HirSpan) {
        match list_ty {
            Type::List(elem_ty) => {
                // Element type must unify with operand type.
                if unify(op_ty, elem_ty).is_err() {
                    // Only emit if neither side is Any/Unknown (to avoid noise).
                    if !op_ty.is_any()
                        && !op_ty.is_unknown()
                        && !elem_ty.is_any()
                        && !elem_ty.is_unknown()
                    {
                        self.sink.push(Diagnostic::error(
                            DiagCode::E2012,
                            span,
                            format!(
                                "`IN` operand type `{}` is not compatible with list element \
                                 type `{}`",
                                type_name(op_ty),
                                type_name(elem_ty),
                            ),
                        ));
                    }
                }
            }
            // Any / Unknown / Null: no constraint.
            Type::Any | Type::Unknown | Type::Null => {}
            // Something that is definitely not a list.
            _ => {
                self.sink.push(Diagnostic::error(
                    DiagCode::E2013,
                    span,
                    format!(
                        "`IN` right-hand side must be a list but found `{}`",
                        type_name(list_ty),
                    ),
                ));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Variable type helper
    // -----------------------------------------------------------------------

    fn var_type(&self, var: VarId) -> Type {
        use cypher_hir::VarKind;
        match self.bindings.get(&var).map(|b| b.kind) {
            Some(VarKind::Node) => Type::Node(None),
            Some(VarKind::Relationship) => Type::Relationship(None),
            Some(VarKind::Path) => Type::Path,
            Some(VarKind::Value) | None => Type::Any,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Human-readable name for a type (for diagnostic messages).
fn type_name(ty: &Type) -> &'static str {
    match ty {
        Type::Any => "Any",
        Type::Null => "Null",
        Type::Bool => "Bool",
        Type::Int => "Int",
        Type::Float => "Float",
        Type::Num => "Num",
        Type::String => "String",
        Type::Date => "Date",
        Type::Datetime => "Datetime",
        Type::List(_) => "List",
        Type::Map(_) => "Map",
        Type::Node(_) => "Node",
        Type::Relationship(_) => "Relationship",
        Type::Path => "Path",
        Type::Union(_) => "Union",
        Type::Unknown => "Unknown",
    }
}

/// Placeholder span — used when the HIR does not yet carry per-expression
/// spans. Once HIR expressions carry spans, callers should pass them directly.
fn dummy_span() -> HirSpan {
    HirSpan::empty(HirOffset::new(0))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use cypher_diag::DiagnosticsSink;
    use cypher_hir::{
        BinOp, Binding, Clause, Direction, Expr, HirOffset, HirSpan, Pattern, PatternElement,
        PatternPart, Projection, RelLength, Statement, UnaryOp, VarId, VarKind,
    };
    use smol_str::SmolStr;

    // -----------------------------------------------------------------------
    // Test infrastructure
    // -----------------------------------------------------------------------

    fn zero_range() -> HirSpan {
        HirSpan::empty(HirOffset::new(0))
    }

    fn intern_var(stmt: &mut Statement, name: &str, kind: VarKind) -> VarId {
        for (id, b) in &stmt.bindings {
            if b.name.as_str() == name {
                return *id;
            }
        }
        let id = VarId(u32::try_from(stmt.bindings.len()).expect("overflow"));
        stmt.bindings.insert(
            id,
            Binding {
                id,
                name: SmolStr::new(name),
                kind,
                defined_at: zero_range(),
            },
        );
        id
    }

    fn dummy_syntax() -> cypher_syntax::SyntaxNode {
        cypher_syntax::parse("x").syntax()
    }

    fn alloc(stmt: &mut Statement) -> cypher_hir::HirId {
        stmt.alloc_id(dummy_syntax())
    }

    fn run(stmt: &Statement) -> String {
        let mut sink = DiagnosticsSink::new();
        infer_types(stmt, &mut sink);
        let diags = sink.into_sorted();
        let mut out = String::new();
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
        }
        out
    }

    // Helper: build a single RETURN statement with one expression.
    fn return_stmt(expr: Expr) -> Statement {
        let mut stmt = Statement::new(zero_range());
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr,
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        stmt
    }

    // -----------------------------------------------------------------------
    // 1. Literal type assignment — no errors.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_int_literal_ok() {
        let stmt = return_stmt(Expr::Int(42));
        insta::assert_snapshot!("infer_int_literal_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_float_literal_ok() {
        let stmt = return_stmt(Expr::Float(2.5));
        insta::assert_snapshot!("infer_float_literal_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_string_literal_ok() {
        let stmt = return_stmt(Expr::String(SmolStr::new("hello")));
        insta::assert_snapshot!("infer_string_literal_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_bool_literal_ok() {
        let stmt = return_stmt(Expr::Bool(true));
        insta::assert_snapshot!("infer_bool_literal_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_null_literal_ok() {
        let stmt = return_stmt(Expr::Null);
        insta::assert_snapshot!("infer_null_literal_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 2. Arithmetic — OK cases.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_int_add_int_ok() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Int(1)),
            rhs: Box::new(Expr::Int(2)),
        });
        insta::assert_snapshot!("infer_int_add_int_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_float_mul_int_ok() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::Mul,
            lhs: Box::new(Expr::Float(2.0)),
            rhs: Box::new(Expr::Int(3)),
        });
        insta::assert_snapshot!("infer_float_mul_int_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 3. Arithmetic — error: non-numeric operand → E2009.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_string_add_int_error() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::String(SmolStr::new("x"))),
            rhs: Box::new(Expr::Int(1)),
        });
        insta::assert_snapshot!("infer_string_add_int_error", run(&stmt));
    }

    #[test]
    fn snap_infer_bool_sub_int_error() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::Sub,
            lhs: Box::new(Expr::Bool(true)),
            rhs: Box::new(Expr::Int(1)),
        });
        insta::assert_snapshot!("infer_bool_sub_int_error", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 4. String operators — OK.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_contains_strings_ok() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::Contains,
            lhs: Box::new(Expr::String(SmolStr::new("hello world"))),
            rhs: Box::new(Expr::String(SmolStr::new("world"))),
        });
        insta::assert_snapshot!("infer_contains_strings_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_starts_with_strings_ok() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::StartsWith,
            lhs: Box::new(Expr::String(SmolStr::new("hello"))),
            rhs: Box::new(Expr::String(SmolStr::new("he"))),
        });
        insta::assert_snapshot!("infer_starts_with_strings_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 5. String operators — error: non-string operand → E2010.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_contains_int_error() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::Contains,
            lhs: Box::new(Expr::Int(42)),
            rhs: Box::new(Expr::String(SmolStr::new("x"))),
        });
        insta::assert_snapshot!("infer_contains_int_error", run(&stmt));
    }

    #[test]
    fn snap_infer_ends_with_bool_error() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::EndsWith,
            lhs: Box::new(Expr::String(SmolStr::new("foo"))),
            rhs: Box::new(Expr::Bool(true)),
        });
        insta::assert_snapshot!("infer_ends_with_bool_error", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 6. Boolean operators — OK.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_and_bools_ok() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Bool(true)),
            rhs: Box::new(Expr::Bool(false)),
        });
        insta::assert_snapshot!("infer_and_bools_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 7. Boolean operators — error: non-bool operand → E2011.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_and_int_error() {
        let stmt = return_stmt(Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Int(1)),
            rhs: Box::new(Expr::Bool(false)),
        });
        insta::assert_snapshot!("infer_and_int_error", run(&stmt));
    }

    #[test]
    fn snap_infer_not_string_error() {
        let stmt = return_stmt(Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::String(SmolStr::new("yes"))),
        });
        insta::assert_snapshot!("infer_not_string_error", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 8. IS NULL / IS NOT NULL — no type constraint; result Bool.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_is_null_ok() {
        let stmt = return_stmt(Expr::IsNull {
            operand: Box::new(Expr::Int(1)),
            negated: false,
        });
        insta::assert_snapshot!("infer_is_null_ok", run(&stmt));
    }

    #[test]
    fn snap_infer_is_not_null_ok() {
        let stmt = return_stmt(Expr::IsNull {
            operand: Box::new(Expr::String(SmolStr::new("x"))),
            negated: true,
        });
        insta::assert_snapshot!("infer_is_not_null_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 9. IN list — OK.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_in_list_ok() {
        // 1 IN [1, 2, 3]
        let stmt = return_stmt(Expr::InList {
            operand: Box::new(Expr::Int(1)),
            list: Box::new(Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)])),
        });
        insta::assert_snapshot!("infer_in_list_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 10. IN list — error: element type mismatch → E2012.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_in_list_type_mismatch_error() {
        // true IN [1, 2, 3]  — Bool IN List<Int>
        let stmt = return_stmt(Expr::InList {
            operand: Box::new(Expr::Bool(true)),
            list: Box::new(Expr::List(vec![Expr::Int(1), Expr::Int(2)])),
        });
        insta::assert_snapshot!("infer_in_list_type_mismatch_error", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 11. IN list — error: right side not a list → E2013.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_in_not_a_list_error() {
        // 1 IN 42  — right side is Int, not List
        let stmt = return_stmt(Expr::InList {
            operand: Box::new(Expr::Int(1)),
            list: Box::new(Expr::Int(42)),
        });
        insta::assert_snapshot!("infer_in_not_a_list_error", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 12. Unary negation — OK.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_neg_int_ok() {
        let stmt = return_stmt(Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Int(5)),
        });
        insta::assert_snapshot!("infer_neg_int_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 13. Unary negation — error: non-numeric → E2009.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_neg_string_error() {
        let stmt = return_stmt(Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::String(SmolStr::new("x"))),
        });
        insta::assert_snapshot!("infer_neg_string_error", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 14. Variable with Node kind — no type error in RETURN.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_node_var_in_return_ok() {
        let mut stmt = Statement::new(zero_range());
        let n = intern_var(&mut stmt, "n", VarKind::Node);
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(n),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(n),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("infer_node_var_in_return_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 15. Any operand in arithmetic — no error (e.g. property access).
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_any_add_int_ok() {
        // n.prop + 1  — n.prop is Any in schema-free mode; should be OK.
        let mut stmt = Statement::new(zero_range());
        let n = intern_var(&mut stmt, "n", VarKind::Node);
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: Some(n),
                        labels: vec![],
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Prop {
                        target: Box::new(Expr::Var(n)),
                        prop: SmolStr::new("prop"),
                    }),
                    rhs: Box::new(Expr::Int(1)),
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("infer_any_add_int_ok", run(&stmt));
    }

    // -----------------------------------------------------------------------
    // 16. Relationship variable in RETURN — no type error.
    // -----------------------------------------------------------------------
    #[test]
    fn snap_infer_rel_var_in_return_ok() {
        let mut stmt = Statement::new(zero_range());
        let a = intern_var(&mut stmt, "a", VarKind::Node);
        let r = intern_var(&mut stmt, "r", VarKind::Relationship);
        let b = intern_var(&mut stmt, "b", VarKind::Node);
        let mid = alloc(&mut stmt);
        let nid_a = alloc(&mut stmt);
        let rid = alloc(&mut stmt);
        let nid_b = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: Some(a),
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: Some(r),
                            types: vec![],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: Some(b),
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            span: zero_range(),
        });
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Var(r),
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
        });
        insta::assert_snapshot!("infer_rel_var_in_return_ok", run(&stmt));
    }
}
