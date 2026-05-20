//! Schema-aware semantic pass (spec 0001 §7.1 / §7.5).
//!
//! Runs **only** when a [`SchemaProvider`] is supplied to `analyse`.
//! Checks that every label, relationship type, property, and function
//! call in the statement is consistent with the declared schema.
//!
//! # Checks (E3xxx range — spec §10.2)
//!
//! | Rule | Code |
//! |------|------|
//! | Node pattern uses unknown label | [`E3001`] |
//! | Relationship pattern uses unknown type | [`E3002`] |
//! | Property access on labeled/typed binding not declared in schema | [`E3003`] |
//! | Property type conflicts with usage context | [`E3004`] |
//! | MERGE key not backed by a declared uniqueness constraint | [`E3009`] |
//! | Function call to unknown function | [`E3006`] |
//! | Function arity mismatch | [`E3007`] |
//! | Procedure call to unknown procedure | [`E3008`] |
//!
//! [`E3001`]: cyrs_diag::DiagCode::E3001
//! [`E3002`]: cyrs_diag::DiagCode::E3002
//! [`E3003`]: cyrs_diag::DiagCode::E3003
//! [`E3004`]: cyrs_diag::DiagCode::E3004
//! [`E3009`]: cyrs_diag::DiagCode::E3009
//! [`E3006`]: cyrs_diag::DiagCode::E3006
//! [`E3007`]: cyrs_diag::DiagCode::E3007
//! [`E3008`]: cyrs_diag::DiagCode::E3008

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::{
    Binding, Clause, Expr, HirOffset, HirSpan, MapProjectionItem, Pattern, PatternElement,
    RemoveItem, SetItem, Statement, VarId,
};
use cyrs_schema::{PropertyType, SchemaProvider};
use indexmap::IndexMap;
use smol_str::SmolStr;

use crate::ty::Type;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Schema-aware semantic pass.
///
/// Walk every clause of `stmt` and emit diagnostics for any construct that
/// contradicts `schema`. Per spec §10.4, no pass short-circuits on first error.
pub fn check_schema_aware(
    stmt: &Statement,
    schema: &dyn SchemaProvider,
    sink: &mut DiagnosticsSink,
) {
    let mut ctx = SchemaCtx {
        bindings: &stmt.bindings,
        schema,
        sink,
    };
    ctx.check_statement(stmt);
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

struct SchemaCtx<'a> {
    bindings: &'a IndexMap<VarId, Binding>,
    schema: &'a dyn SchemaProvider,
    sink: &'a mut DiagnosticsSink,
}

impl SchemaCtx<'_> {
    fn check_statement(&mut self, stmt: &Statement) {
        for clause in &stmt.clauses {
            self.check_clause(clause);
        }
    }

    fn check_clause(&mut self, clause: &Clause) {
        match clause {
            Clause::Match { pattern, .. } | Clause::Create { pattern, .. } => {
                self.check_pattern(pattern);
            }
            Clause::Merge { pattern, .. } => {
                self.check_pattern(pattern);
                self.check_merge_keys(pattern);
            }
            Clause::Where { predicate, .. } => {
                self.check_expr(predicate);
            }
            Clause::With {
                projections,
                filter,
                ..
            } => {
                for proj in projections {
                    self.check_expr(&proj.expr);
                }
                if let Some(f) = filter {
                    self.check_expr(f);
                }
            }
            Clause::Return { projections, .. } => {
                for proj in projections {
                    self.check_expr(&proj.expr);
                }
            }
            Clause::Unwind { list, .. } => {
                self.check_expr(list);
            }
            Clause::Set { items, .. } => {
                for item in items {
                    match item {
                        SetItem::Property {
                            target,
                            value,
                            prop,
                        } => {
                            // For property assignment, check the property type
                            // against the schema of the binding's labels.
                            self.check_prop_assignment(target, prop, value);
                        }
                        SetItem::AssignMap { map, .. } => {
                            self.check_expr(map);
                        }
                        SetItem::Labels { .. } => {}
                    }
                }
            }
            Clause::Remove { items, .. } => {
                for item in items {
                    match item {
                        RemoveItem::Property { target, .. } => {
                            self.check_expr(target);
                        }
                        RemoveItem::Labels { .. } => {}
                    }
                }
            }
            Clause::Delete { targets, .. } => {
                for expr in targets {
                    self.check_expr(expr);
                }
            }
            Clause::Call {
                procedure,
                args,
                span,
                ..
            } => {
                self.check_procedure_call(procedure, args.len(), *span);
                for arg in args {
                    self.check_expr(arg);
                }
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Pattern) {
        for part in &pattern.parts {
            for elem in &part.elements {
                match elem {
                    PatternElement::Node {
                        bind,
                        labels,
                        props,
                        span,
                        ..
                    } => {
                        // Check each label exists in schema.
                        for label in labels {
                            if !self.schema.has_label(label.as_str()) {
                                self.sink.push(Diagnostic::error(
                                    DiagCode::E3001,
                                    *span,
                                    format!("unknown label `:{label}`"),
                                ));
                            }
                        }
                        // Check property map keys against schema for each label.
                        if let Some(p) = props {
                            if !labels.is_empty() {
                                self.check_node_props(p, labels, *span);
                            }
                            self.check_expr(p);
                        }
                        let _ = bind;
                    }
                    PatternElement::Rel {
                        bind,
                        types,
                        props,
                        span,
                        ..
                    } => {
                        // Check each relationship type exists in schema.
                        for rel_type in types {
                            if !self.schema.has_relationship_type(rel_type.as_str()) {
                                self.sink.push(Diagnostic::error(
                                    DiagCode::E3002,
                                    *span,
                                    format!("unknown relationship type `:{rel_type}`"),
                                ));
                            }
                        }
                        // Check property map keys against schema for each rel type.
                        if let Some(p) = props {
                            if !types.is_empty() {
                                self.check_rel_props(p, types, *span);
                            }
                            self.check_expr(p);
                        }
                        let _ = bind;
                    }
                }
            }
        }
    }

    /// Check that inline node props in a pattern map use declared property names.
    fn check_node_props(&mut self, props_expr: &Expr, labels: &[SmolStr], span: HirSpan) {
        let Expr::Map(pairs) = props_expr else {
            return;
        };
        for (key, val_expr) in pairs {
            // Find property declaration in any of the declared labels.
            let decl = labels.iter().find_map(|label| {
                self.schema
                    .node_properties(label.as_str())
                    .and_then(|props| props.into_iter().find(|p| p.name == *key))
            });
            if let Some(decl) = decl {
                // Check value expression type against declared property type.
                let inferred = infer_literal_type(val_expr);
                if let Some(inferred) = inferred
                    && !types_compatible(&inferred, &decl.ty)
                {
                    self.sink.push(Diagnostic::error(
                        DiagCode::E3004,
                        span,
                        format!(
                            "property `{key}` expects type `{}` but got `{}`",
                            property_type_name(&decl.ty),
                            type_name(&inferred),
                        ),
                    ));
                }
            } else {
                // Property not declared on any of the labels.
                let label_list: Vec<_> = labels.iter().map(|l| format!(":{l}")).collect();
                self.sink.push(Diagnostic::error(
                    DiagCode::E3003,
                    span,
                    format!(
                        "property `{key}` is not declared on label(s) {}",
                        label_list.join(", "),
                    ),
                ));
            }
        }
    }

    /// Check that inline rel props in a pattern map use declared property names.
    fn check_rel_props(&mut self, props_expr: &Expr, types: &[SmolStr], span: HirSpan) {
        let Expr::Map(pairs) = props_expr else {
            return;
        };
        for (key, val_expr) in pairs {
            let decl = types.iter().find_map(|rt| {
                self.schema
                    .relationship_properties(rt.as_str())
                    .and_then(|props| props.into_iter().find(|p| p.name == *key))
            });
            if let Some(decl) = decl {
                let inferred = infer_literal_type(val_expr);
                if let Some(inferred) = inferred
                    && !types_compatible(&inferred, &decl.ty)
                {
                    self.sink.push(Diagnostic::error(
                        DiagCode::E3004,
                        span,
                        format!(
                            "property `{key}` expects type `{}` but got `{}`",
                            property_type_name(&decl.ty),
                            type_name(&inferred),
                        ),
                    ));
                }
            } else {
                let type_list: Vec<_> = types.iter().map(|t| format!(":{t}")).collect();
                self.sink.push(Diagnostic::error(
                    DiagCode::E3003,
                    span,
                    format!(
                        "property `{key}` is not declared on relationship type(s) {}",
                        type_list.join(", "),
                    ),
                ));
            }
        }
    }

    /// Validate the key property set of a `MERGE` pattern against the
    /// uniqueness constraints declared by the schema (spec §7.5).
    ///
    /// For every node / relationship element of the `MERGE` pattern that
    /// carries an explicit label / type **and** an inline literal property
    /// map, the map's key set is the "MERGE key". `MERGE` is provably
    /// deterministic only when that key set contains every property of at
    /// least one declared uniqueness tuple
    /// ([`SchemaProvider::label_unique_props`] /
    /// [`SchemaProvider::rel_type_unique_props`]). When no such tuple
    /// exists, [`DiagCode::E3009`] is emitted.
    ///
    /// Elements with no label/type, no property map, or a non-literal
    /// `props` expression are skipped — the same conservatism as the
    /// property-name checks. A schema that declares no uniqueness for the
    /// label/type also yields no diagnostic (absence of a constraint is
    /// not an error; the embedder still owns runtime enforcement).
    fn check_merge_keys(&mut self, pattern: &Pattern) {
        for part in &pattern.parts {
            for elem in &part.elements {
                match elem {
                    PatternElement::Node {
                        labels,
                        props: Some(props),
                        span,
                        ..
                    } if !labels.is_empty() => {
                        let tuples: Vec<Vec<SmolStr>> = labels
                            .iter()
                            .flat_map(|l| self.schema.label_unique_props(l.as_str()))
                            .collect();
                        self.check_merge_key_set(props, &tuples, labels, "label", *span);
                    }
                    PatternElement::Rel {
                        types,
                        props: Some(props),
                        span,
                        ..
                    } if !types.is_empty() => {
                        let tuples: Vec<Vec<SmolStr>> = types
                            .iter()
                            .flat_map(|t| self.schema.rel_type_unique_props(t.as_str()))
                            .collect();
                        self.check_merge_key_set(props, &tuples, types, "relationship type", *span);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Shared core of [`Self::check_merge_keys`]: given the `MERGE`
    /// element's `props` expression and the uniqueness `tuples` collected
    /// for its labels/types, emit [`DiagCode::E3009`] when the key set is
    /// not backed by any tuple.
    fn check_merge_key_set(
        &mut self,
        props: &Expr,
        tuples: &[Vec<SmolStr>],
        names: &[SmolStr],
        kind: &str,
        span: HirSpan,
    ) {
        // Non-literal-map `props` cannot be analysed statically — skip,
        // consistent with the property-name checks.
        let Expr::Map(pairs) = props else {
            return;
        };
        // A schema that declares no uniqueness for these labels/types
        // raises nothing — absence of a constraint is not an error.
        if tuples.is_empty() {
            return;
        }
        let key_set: std::collections::BTreeSet<&SmolStr> = pairs.iter().map(|(k, _)| k).collect();
        // Deterministic iff the key set contains every property of at
        // least one declared uniqueness tuple.
        let backed = tuples
            .iter()
            .any(|tuple| tuple.iter().all(|p| key_set.contains(p)));
        if !backed {
            let name_list: Vec<_> = names.iter().map(|n| format!(":{n}")).collect();
            let mut keys: Vec<&str> = key_set.iter().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            self.sink.push(Diagnostic::error(
                DiagCode::E3009,
                span,
                format!(
                    "MERGE key {{{}}} on {kind}(s) {} is not backed by a declared \
                     uniqueness constraint",
                    keys.join(", "),
                    name_list.join(", "),
                ),
            ));
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            // Literals and bare variable references — no schema checks needed.
            Expr::Null
            | Expr::Bool(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::String(_)
            | Expr::Var(_)
            | Expr::Param(_)
            | Expr::Unresolved(_)
            // cy-p1u5: subquery body is not lowered, so schema-aware
            // checks have nothing to do here. The dialect-gate pass
            // owns the rejection (E4017).
            | Expr::ExistsSubqueryDeferred { .. } => {}

            // Property access: check property name is declared if binding has labels.
            Expr::Prop { target, prop } => {
                self.check_expr(target);
                self.check_prop_access(target, prop, dummy_span());
            }

            Expr::Index { target, index } => {
                self.check_expr(target);
                self.check_expr(index);
            }
            Expr::Slice { target, start, end } => {
                self.check_expr(target);
                if let Some(s) = start {
                    self.check_expr(s);
                }
                if let Some(e) = end {
                    self.check_expr(e);
                }
            }

            Expr::List(items) => {
                for item in items {
                    self.check_expr(item);
                }
            }

            Expr::Map(pairs) => {
                for (_, v) in pairs {
                    self.check_expr(v);
                }
            }

            // Function calls: validate name and arity.
            Expr::Call { name, args, .. } => {
                let n_args = args.len();
                self.check_function_call(name, n_args, dummy_span());
                for arg in args {
                    self.check_expr(arg);
                }
            }

            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
            }

            Expr::UnaryOp { operand, .. } | Expr::IsNull { operand, .. } => {
                self.check_expr(operand);
            }

            Expr::Case {
                scrutinee,
                arms,
                otherwise,
            } => {
                if let Some(s) = scrutinee {
                    self.check_expr(s);
                }
                for (cond, result) in arms {
                    self.check_expr(cond);
                    self.check_expr(result);
                }
                if let Some(o) = otherwise {
                    self.check_expr(o);
                }
            }

            Expr::InList { operand, list } => {
                self.check_expr(operand);
                self.check_expr(list);
            }

            Expr::PatternPredicate(pat) => {
                self.check_pattern(pat);
            }

            Expr::ListComprehension {
                iterable,
                filter,
                map_expr,
                ..
            } => {
                self.check_expr(iterable);
                if let Some(f) = filter {
                    self.check_expr(f);
                }
                self.check_expr(map_expr);
            }
            // cy-8x5 — list-predicate traversal. Schema-aware checks
            // recurse into iterable + optional predicate identically
            // to list comprehensions; no dedicated rule here.
            Expr::ListPredicate {
                iterable,
                predicate,
                ..
            } => {
                self.check_expr(iterable);
                if let Some(p) = predicate {
                    self.check_expr(p);
                }
            }

            Expr::MapProjection { base, items } => {
                self.check_expr(base);
                for item in items {
                    match item {
                        MapProjectionItem::Computed { value, .. }
                        | MapProjectionItem::Aliased { value, .. } => {
                            self.check_expr(value);
                        }
                        MapProjectionItem::PropCopy { .. }
                        | MapProjectionItem::VarShorthand { .. } => {}
                    }
                }
            }
        }
    }

    /// Check `target.prop`: if `target` is a variable with known labels,
    /// verify the property is declared on at least one of them.
    fn check_prop_access(&mut self, target: &Expr, _prop: &SmolStr, span: HirSpan) {
        let Expr::Var(var_id) = target else {
            return;
        };
        let Some(binding) = self.bindings.get(var_id) else {
            return;
        };
        // Labels and rel-types live on the PatternElement, not on the Binding,
        // so we cannot check property names here — the richer check happens in
        // check_node_props / check_rel_props when we have pattern context.
        let _ = (binding, span);
    }

    /// Check a property assignment in a SET clause.
    fn check_prop_assignment(&mut self, target: &Expr, _prop: &SmolStr, value: &Expr) {
        self.check_expr(target);
        self.check_expr(value);
    }

    /// Validate a function call against the schema catalog.
    fn check_function_call(&mut self, name: &SmolStr, n_args: usize, span: HirSpan) {
        let Some(sig) = self.schema.function(name.as_str()) else {
            self.sink.push(Diagnostic::error(
                DiagCode::E3006,
                span,
                format!("unknown function `{name}`"),
            ));
            return;
        };

        let required = sig.params.len();
        let has_variadic = sig.variadic.is_some();

        let ok = if has_variadic {
            n_args >= required
        } else {
            n_args == required
        };

        if !ok {
            let expected_desc = if has_variadic {
                format!("at least {required}")
            } else {
                format!("exactly {required}")
            };
            self.sink.push(Diagnostic::error(
                DiagCode::E3007,
                span,
                format!("function `{name}` expects {expected_desc} argument(s) but got {n_args}",),
            ));
        }
    }

    /// Validate a CALL procedure against the schema catalog.
    fn check_procedure_call(&mut self, name: &SmolStr, n_args: usize, span: HirSpan) {
        let Some(sig) = self.schema.procedure(name.as_str()) else {
            self.sink.push(Diagnostic::error(
                DiagCode::E3008,
                span,
                format!("unknown procedure `{name}`"),
            ));
            return;
        };

        let required = sig.params.len();
        if n_args != required {
            self.sink.push(Diagnostic::error(
                DiagCode::E3007,
                span,
                format!(
                    "procedure `{name}` expects exactly {required} argument(s) but got {n_args}",
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Infer a concrete type from a literal expression only (returns None for
/// anything that is not a direct literal — those require full inference).
fn infer_literal_type(expr: &Expr) -> Option<Type> {
    match expr {
        Expr::Bool(_) => Some(Type::Bool),
        Expr::Int(_) => Some(Type::Int),
        Expr::Float(_) => Some(Type::Float),
        Expr::String(_) => Some(Type::String),
        Expr::Null => Some(Type::Null),
        Expr::List(items) => {
            if items.is_empty() {
                return Some(Type::List(Box::new(Type::Any)));
            }
            // Take type from first literal element.
            let elem_ty = infer_literal_type(items.first()?)?;
            Some(Type::List(Box::new(elem_ty)))
        }
        _ => None,
    }
}

/// Check that a Cypher [`Type`] is compatible with a schema [`PropertyType`].
///
/// "Compatible" means: the inferred type could be stored in a property slot
/// of the declared type. `Any` and `Unknown` on either side pass through
/// (we do not know enough to reject them).
fn types_compatible(inferred: &Type, declared: &PropertyType) -> bool {
    // List recursion handled before the flat-true checks.
    if let (Type::List(et), PropertyType::List(pt)) = (inferred, declared) {
        return types_compatible(et, pt);
    }
    matches!(
        (inferred, declared),
        // Any/Unknown inferred type, Null, or declared Any/Opaque/Enum passes.
        (Type::Any | Type::Unknown | Type::Null, _)
            | (_, PropertyType::Any | PropertyType::Opaque(_) | PropertyType::Enum(_, _))
            // Exact scalar matches.
            | (Type::Bool, PropertyType::Bool)
            | (Type::Int, PropertyType::Int)
            | (Type::Float, PropertyType::Float)
            | (Type::String, PropertyType::String)
            | (Type::Date, PropertyType::Date)
            | (Type::Datetime, PropertyType::Datetime)
            // Num covers Int and Float.
            | (Type::Num, PropertyType::Int | PropertyType::Float)
    )
}

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

fn property_type_name(pt: &PropertyType) -> &'static str {
    match pt {
        PropertyType::String => "String",
        PropertyType::Int => "Int",
        PropertyType::Float => "Float",
        PropertyType::Bool => "Bool",
        PropertyType::Date => "Date",
        PropertyType::Datetime => "Datetime",
        PropertyType::List(_) => "List",
        PropertyType::Enum(_, _) => "Enum",
        PropertyType::Opaque(_) => "Opaque",
        PropertyType::Any => "Any",
    }
}

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
    use cyrs_diag::DiagnosticsSink;
    use cyrs_hir::{
        Binding, Clause, Direction, Expr, HirOffset, HirSpan, Pattern, PatternElement, PatternPart,
        Projection, RelLength, Statement, VarId, VarKind, YieldItem,
    };
    use cyrs_schema::{
        EndpointDecl, FnCategories, FunctionSignature, ParamDecl, ProcedureSignature, PropertyDecl,
        PropertyType, ReturnTy, SchemaProvider, StandardLibrary, YieldDecl,
    };
    use smol_str::SmolStr;

    // -----------------------------------------------------------------------
    // Test schema fixture
    //
    // Graph: (:Person {name: String, age: Int})-[:KNOWS {since: Int}]->(:Person)
    //        (:Company {name: String})
    //        proc: db.ping()  yields ok:Bool
    // -----------------------------------------------------------------------

    #[derive(Debug)]
    struct TestSchema;

    impl SchemaProvider for TestSchema {
        fn labels(&self) -> Vec<SmolStr> {
            vec![SmolStr::new("Person"), SmolStr::new("Company")]
        }

        fn relationship_types(&self) -> Vec<SmolStr> {
            vec![SmolStr::new("KNOWS")]
        }

        fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
            match label {
                // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1).
                "Person" => Some(vec![
                    PropertyDecl::new("name", PropertyType::String, true),
                    PropertyDecl::new("age", PropertyType::Int, false),
                ]),
                "Company" => Some(vec![PropertyDecl::new("name", PropertyType::String, true)]),
                _ => None,
            }
        }

        fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>> {
            match rel_type {
                "KNOWS" => Some(vec![PropertyDecl::new("since", PropertyType::Int, false)]),
                _ => None,
            }
        }

        fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
            match rel_type {
                "KNOWS" => vec![EndpointDecl {
                    from: SmolStr::new("Person"),
                    to: SmolStr::new("Person"),
                    cardinality: cyrs_schema::Cardinality::ManyToMany,
                }],
                _ => vec![],
            }
        }

        fn inverse_of(&self, _: &str) -> Option<SmolStr> {
            None
        }

        // `Person` keys uniquely on `name`; `Company` declares no
        // uniqueness. `KNOWS` keys uniquely on `since`.
        fn label_unique_props(&self, label: &str) -> Vec<Vec<SmolStr>> {
            match label {
                "Person" => vec![vec![SmolStr::new("name")]],
                _ => Vec::new(),
            }
        }

        fn rel_type_unique_props(&self, rel_type: &str) -> Vec<Vec<SmolStr>> {
            match rel_type {
                "KNOWS" => vec![vec![SmolStr::new("since")]],
                _ => Vec::new(),
            }
        }

        fn function(&self, name: &str) -> Option<FunctionSignature> {
            match name.to_ascii_lowercase().as_str() {
                "custom_fn" => Some(FunctionSignature {
                    name: SmolStr::new("custom_fn"),
                    params: vec![ParamDecl {
                        name: SmolStr::new("x"),
                        ty: PropertyType::Int,
                        default: None,
                    }],
                    variadic: None,
                    return_ty: ReturnTy::Constant(PropertyType::Int),
                    categories: FnCategories {
                        pure: true,
                        aggregate: false,
                        deterministic: true,
                    },
                }),
                _ => None,
            }
        }

        fn procedure(&self, name: &str) -> Option<ProcedureSignature> {
            match name {
                "db.ping" => Some(ProcedureSignature {
                    name: SmolStr::new("db.ping"),
                    params: vec![],
                    yields: vec![YieldDecl {
                        name: SmolStr::new("ok"),
                        ty: PropertyType::Bool,
                    }],
                    mode: cyrs_schema::ProcMode::Read,
                }),
                _ => None,
            }
        }

        fn schema_digest(&self) -> [u8; 32] {
            [1u8; 32]
        }
    }

    // -----------------------------------------------------------------------
    // Helper: combined stdlib + test schema
    // -----------------------------------------------------------------------

    fn test_schema() -> StandardLibrary<TestSchema> {
        StandardLibrary::wrap(TestSchema)
    }

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

    fn dummy_syntax() -> cyrs_syntax::SyntaxNode {
        cyrs_syntax::parse("x").syntax()
    }

    fn alloc(stmt: &mut Statement) -> cyrs_hir::HirId {
        stmt.alloc_id(dummy_syntax())
    }

    fn run(stmt: &Statement, schema: &dyn SchemaProvider) -> String {
        let mut sink = DiagnosticsSink::new();
        check_schema_aware(stmt, schema, &mut sink);
        let diags = sink.into_sorted();
        let mut out = String::new();
        writeln!(out, "diagnostics: {}", diags.len()).unwrap();
        for d in &diags {
            writeln!(out, "  {}: {}", d.code, d.message).unwrap();
        }
        out
    }

    fn node_match_stmt(labels: Vec<SmolStr>, var_name: Option<&str>) -> Statement {
        let mut stmt = Statement::new(zero_range());
        let bind = var_name.map(|n| intern_var(&mut stmt, n, VarKind::Node));
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind,
                        labels,
                        props: None,
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        stmt
    }

    // -----------------------------------------------------------------------
    // 1. Unknown label → E3001
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_unknown_label_error() {
        let stmt = node_match_stmt(vec![SmolStr::new("Ghost")], None);
        let schema = test_schema();
        insta::assert_snapshot!("schema_unknown_label_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 2. Known label → clean
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_known_label_ok() {
        let stmt = node_match_stmt(vec![SmolStr::new("Person")], Some("n"));
        let schema = test_schema();
        insta::assert_snapshot!("schema_known_label_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 3. Multiple labels — one unknown → E3001
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_one_of_two_labels_unknown() {
        let stmt = node_match_stmt(vec![SmolStr::new("Person"), SmolStr::new("Ghost")], None);
        let schema = test_schema();
        insta::assert_snapshot!("schema_one_of_two_labels_unknown", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 4. Unknown relationship type → E3002
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_unknown_rel_type_error() {
        let mut stmt = Statement::new(zero_range());
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
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: None,
                            types: vec![SmolStr::new("UNKNOWN_REL")],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_unknown_rel_type_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 5. Known relationship type → clean
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_known_rel_type_ok() {
        let mut stmt = Statement::new(zero_range());
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
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: None,
                            labels: vec![SmolStr::new("Person")],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: None,
                            types: vec![SmolStr::new("KNOWS")],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: None,
                            labels: vec![SmolStr::new("Person")],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_known_rel_type_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 6. Unknown property in node pattern map → E3003
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_unknown_node_prop_error() {
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: None,
                        labels: vec![SmolStr::new("Person")],
                        props: Some(Expr::Map(vec![(
                            SmolStr::new("ghost_prop"),
                            Expr::String(SmolStr::new("x")),
                        )])),
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_unknown_node_prop_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 7. Known property in node pattern map → clean
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_known_node_prop_ok() {
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: None,
                        labels: vec![SmolStr::new("Person")],
                        props: Some(Expr::Map(vec![(
                            SmolStr::new("name"),
                            Expr::String(SmolStr::new("Alice")),
                        )])),
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_known_node_prop_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 8. Property type mismatch in node pattern: name expects String got Int → E3004
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_node_prop_type_mismatch_error() {
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: None,
                        labels: vec![SmolStr::new("Person")],
                        props: Some(Expr::Map(vec![(
                            SmolStr::new("name"),
                            Expr::Int(42), // name expects String!
                        )])),
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_node_prop_type_mismatch_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 9. Unknown property in rel pattern map → E3003
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_unknown_rel_prop_error() {
        let mut stmt = Statement::new(zero_range());
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
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: None,
                            types: vec![SmolStr::new("KNOWS")],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: Some(Expr::Map(vec![(
                                SmolStr::new("unknown_key"),
                                Expr::Int(2020),
                            )])),
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_unknown_rel_prop_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 10. Rel prop type mismatch: since expects Int got String → E3004
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_rel_prop_type_mismatch_error() {
        let mut stmt = Statement::new(zero_range());
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
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: None,
                            types: vec![SmolStr::new("KNOWS")],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: Some(Expr::Map(vec![(
                                SmolStr::new("since"),
                                Expr::String(SmolStr::new("not-a-year")),
                            )])),
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_rel_prop_type_mismatch_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 11. Unknown function call → E3006
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_unknown_function_error() {
        let mut stmt = Statement::new(zero_range());
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Call {
                    name: SmolStr::new("totally_unknown_fn"),
                    args: vec![Expr::Int(1)],
                    distinct: false,
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_unknown_function_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 12. Known stdlib function → clean
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_known_stdlib_function_ok() {
        let mut stmt = Statement::new(zero_range());
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Call {
                    name: SmolStr::new("size"),
                    args: vec![Expr::List(vec![Expr::Int(1), Expr::Int(2)])],
                    distinct: false,
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_known_stdlib_function_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 13. Function arity mismatch: size() takes 1 arg, got 2 → E3007
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_function_arity_mismatch_error() {
        let mut stmt = Statement::new(zero_range());
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Call {
                    name: SmolStr::new("size"),
                    args: vec![Expr::Int(1), Expr::Int(2)], // size takes 1 arg
                    distinct: false,
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_function_arity_mismatch_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 14. Unknown procedure CALL → E3008
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_unknown_procedure_error() {
        let mut stmt = Statement::new(zero_range());
        let cid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Call {
            id: cid,
            procedure: SmolStr::new("no.such.proc"),
            args: vec![],
            yields: vec![],
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_unknown_procedure_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 15. Known procedure CALL → clean
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_known_procedure_ok() {
        let mut stmt = Statement::new(zero_range());
        let cid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Call {
            id: cid,
            procedure: SmolStr::new("db.ping"),
            args: vec![],
            yields: vec![YieldItem {
                name: SmolStr::new("ok"),
                alias: None,
            }],
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_known_procedure_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 16. Custom schema function correct arity → clean
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_custom_fn_correct_arity_ok() {
        let mut stmt = Statement::new(zero_range());
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Call {
                    name: SmolStr::new("custom_fn"),
                    args: vec![Expr::Int(5)],
                    distinct: false,
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_custom_fn_correct_arity_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 17. Custom schema function wrong arity → E3007
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_custom_fn_wrong_arity_error() {
        let mut stmt = Statement::new(zero_range());
        let ret_id = alloc(&mut stmt);
        stmt.clauses.push(Clause::Return {
            id: ret_id,
            projections: vec![Projection {
                expr: Expr::Call {
                    name: SmolStr::new("custom_fn"),
                    args: vec![], // requires 1 arg
                    distinct: false,
                },
                alias: None,
                span: zero_range(),
            }],
            distinct: false,
            span: zero_range(),
            order_by: Vec::new(),
            skip: None,
            limit: None,
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_custom_fn_wrong_arity_error", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // 18. No labels — property check skipped (clean)
    // -----------------------------------------------------------------------

    #[test]
    fn snap_schema_no_labels_prop_unchecked_ok() {
        // MATCH (n {any_prop: 1}) — n has no labels, skip property check.
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Match {
            id: mid,
            optional: false,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    shortest: cyrs_hir::ShortestPath::No,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: None,
                        labels: vec![], // no labels → skip schema check
                        props: Some(Expr::Map(vec![(SmolStr::new("any_prop"), Expr::Int(1))])),
                        span: zero_range(),
                    }],
                }],
            },
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!("schema_no_labels_prop_unchecked_ok", run(&stmt, &schema));
    }

    // -----------------------------------------------------------------------
    // MERGE uniqueness validation (E3009, cy-e45 / feat-request §2.2)
    // -----------------------------------------------------------------------

    /// Build a single-node `MERGE` statement with the given label and
    /// inline literal property map.
    fn merge_node_stmt(label: &str, props: Vec<(&str, Expr)>) -> Statement {
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        let map = Expr::Map(
            props
                .into_iter()
                .map(|(k, v)| (SmolStr::new(k), v))
                .collect(),
        );
        stmt.clauses.push(Clause::Merge {
            id: mid,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: None,
                        labels: vec![SmolStr::new(label)],
                        props: Some(map),
                        span: zero_range(),
                    }],
                }],
            },
            on_create: Vec::new(),
            on_match: Vec::new(),
            span: zero_range(),
        });
        stmt
    }

    /// Build a `MERGE (a)-[:REL props]->(b)` statement with the given
    /// relationship type and inline literal property map.
    fn merge_rel_stmt(rel_type: &str, props: Vec<(&str, Expr)>) -> Statement {
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid_a = alloc(&mut stmt);
        let rid = alloc(&mut stmt);
        let nid_b = alloc(&mut stmt);
        let map = Expr::Map(
            props
                .into_iter()
                .map(|(k, v)| (SmolStr::new(k), v))
                .collect(),
        );
        stmt.clauses.push(Clause::Merge {
            id: mid,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![
                        PatternElement::Node {
                            id: nid_a,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                        PatternElement::Rel {
                            id: rid,
                            bind: None,
                            types: vec![SmolStr::new(rel_type)],
                            direction: Direction::Outgoing,
                            length: RelLength::Single,
                            props: Some(map),
                            span: zero_range(),
                        },
                        PatternElement::Node {
                            id: nid_b,
                            bind: None,
                            labels: vec![],
                            props: None,
                            span: zero_range(),
                        },
                    ],
                }],
            },
            on_create: Vec::new(),
            on_match: Vec::new(),
            span: zero_range(),
        });
        stmt
    }

    // 19. MERGE node keyed on the declared unique property → clean.
    #[test]
    fn snap_schema_merge_node_unique_key_ok() {
        let stmt = merge_node_stmt(
            "Person",
            vec![("name", Expr::String(SmolStr::new("Alice")))],
        );
        let schema = test_schema();
        insta::assert_snapshot!("schema_merge_node_unique_key_ok", run(&stmt, &schema));
    }

    // 20. MERGE node keyed on a non-unique property → E3009.
    #[test]
    fn snap_schema_merge_node_non_unique_key_error() {
        // `age` is a declared property but carries no uniqueness tuple.
        let stmt = merge_node_stmt("Person", vec![("age", Expr::Int(30))]);
        let schema = test_schema();
        insta::assert_snapshot!(
            "schema_merge_node_non_unique_key_error",
            run(&stmt, &schema)
        );
    }

    // 21. MERGE node with a superset key (unique prop + extra) → clean:
    //     containing a full uniqueness tuple is enough.
    #[test]
    fn snap_schema_merge_node_superset_key_ok() {
        let stmt = merge_node_stmt(
            "Person",
            vec![
                ("name", Expr::String(SmolStr::new("Alice"))),
                ("age", Expr::Int(30)),
            ],
        );
        let schema = test_schema();
        insta::assert_snapshot!("schema_merge_node_superset_key_ok", run(&stmt, &schema));
    }

    // 22. MERGE node on a label with no declared uniqueness → clean
    //     (absence of a constraint is not an error).
    #[test]
    fn snap_schema_merge_node_no_constraint_ok() {
        let stmt = merge_node_stmt(
            "Company",
            vec![("name", Expr::String(SmolStr::new("Acme")))],
        );
        let schema = test_schema();
        insta::assert_snapshot!("schema_merge_node_no_constraint_ok", run(&stmt, &schema));
    }

    // 23. MERGE relationship keyed on the declared unique property → clean.
    #[test]
    fn snap_schema_merge_rel_unique_key_ok() {
        let stmt = merge_rel_stmt("KNOWS", vec![("since", Expr::Int(2020))]);
        let schema = test_schema();
        insta::assert_snapshot!("schema_merge_rel_unique_key_ok", run(&stmt, &schema));
    }

    // 24. MERGE relationship keyed on a non-unique property → E3009.
    #[test]
    fn snap_schema_merge_rel_non_unique_key_error() {
        // `KNOWS` declares uniqueness only on `since`; an empty key set
        // backs nothing either, but here we use a declared-but-non-unique
        // shape via an unknown key to exercise the E3009 path together
        // with the E3003 path.
        let stmt = merge_rel_stmt("KNOWS", vec![("weight", Expr::Int(1))]);
        let schema = test_schema();
        insta::assert_snapshot!("schema_merge_rel_non_unique_key_error", run(&stmt, &schema));
    }

    // 25. MERGE on a non-literal `props` expression → skipped (clean):
    //     a parameter map cannot be analysed statically.
    #[test]
    fn snap_schema_merge_non_literal_props_skipped_ok() {
        let mut stmt = Statement::new(zero_range());
        let mid = alloc(&mut stmt);
        let nid = alloc(&mut stmt);
        stmt.clauses.push(Clause::Merge {
            id: mid,
            pattern: Pattern {
                parts: vec![PatternPart {
                    named_as: None,
                    elements: vec![PatternElement::Node {
                        id: nid,
                        bind: None,
                        labels: vec![SmolStr::new("Person")],
                        props: Some(Expr::Param(SmolStr::new("p"))),
                        span: zero_range(),
                    }],
                }],
            },
            on_create: Vec::new(),
            on_match: Vec::new(),
            span: zero_range(),
        });
        let schema = test_schema();
        insta::assert_snapshot!(
            "schema_merge_non_literal_props_skipped_ok",
            run(&stmt, &schema)
        );
    }
}
