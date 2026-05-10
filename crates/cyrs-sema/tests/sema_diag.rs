//! Insta snapshot tests for rendered sema diagnostics (spec 0001 §17.2).
//!
//! Each test runs the full pipeline and snapshots the rendered diagnostic
//! output produced by [`cyrs_diag::render_text_string`].  The snapshots
//! cover one representative case per diagnostic cluster:
//!
//!  snap_e1001_unresolved_variable       — E1001 name-resolution error
//!  snap_e2007_node_in_arithmetic        — E2007 kind mismatch
//!  snap_e2009_arithmetic_type_mismatch  — E2009 type inference error
//!  snap_e2010_string_op_non_string      — E2010 string op type error
//!  snap_e2011_bool_op_non_bool          — E2011 boolean op type error
//!  snap_e2013_in_not_a_list             — E2013 IN not a list
//!  snap_schema_node_label_error         — E3001 unknown label
//!  snap_schema_rel_type_error           — E3002 unknown rel type
//!  snap_schema_unknown_fn_error         — E3006 unknown function
//!  snap_schema_fn_arity_error           — E3007 function arity mismatch

#![allow(clippy::too_many_lines)]
#![allow(clippy::doc_markdown)]

use cyrs_diag::{DiagnosticsSink, render_text_string};
use cyrs_hir::{desugar::desugar_statement, lower::lower_statement};
use cyrs_schema::{
    EndpointDecl, FnCategories, FunctionSignature, ParamDecl, ProcedureSignature, PropertyDecl,
    PropertyType, ReturnTy, SchemaProvider,
};
use cyrs_sema::{SemaOptions, analyse};
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run the full sema pipeline on `src` with no schema and return rendered
/// diagnostic strings joined together.
fn render_diags(src: &str, label: &str) -> String {
    let stmt = lower_statement(src);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let opts = SemaOptions::default();
    analyse(&stmt, None, &opts, &mut sink);
    let diags = sink.into_sorted();
    if diags.is_empty() {
        return String::from("(no diagnostics)");
    }
    diags
        .iter()
        .map(|d| render_text_string(label, src, d))
        .collect::<String>()
}

fn render_diags_with_schema(src: &str, label: &str, schema: &dyn SchemaProvider) -> String {
    let stmt = lower_statement(src);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let opts = SemaOptions::default();
    analyse(&stmt, Some(schema), &opts, &mut sink);
    let diags = sink.into_sorted();
    if diags.is_empty() {
        return String::from("(no diagnostics)");
    }
    diags
        .iter()
        .map(|d| render_text_string(label, src, d))
        .collect::<String>()
}

// ---------------------------------------------------------------------------
// Minimal test schema
// ---------------------------------------------------------------------------

/// A minimal `SchemaProvider` for snapshot tests.
struct MinimalSchema {
    labels: Vec<SmolStr>,
    rel_types: Vec<SmolStr>,
    functions: Vec<FunctionSignature>,
    procedures: Vec<ProcedureSignature>,
}

impl MinimalSchema {
    fn labels_only(labels: &[&str]) -> Self {
        MinimalSchema {
            labels: labels.iter().map(|s| SmolStr::new(*s)).collect(),
            rel_types: Vec::new(),
            functions: Vec::new(),
            procedures: Vec::new(),
        }
    }

    fn rel_types_only(rel_types: &[&str]) -> Self {
        MinimalSchema {
            labels: Vec::new(),
            rel_types: rel_types.iter().map(|s| SmolStr::new(*s)).collect(),
            functions: Vec::new(),
            procedures: Vec::new(),
        }
    }

    fn with_function(name: &str, param_count: usize) -> Self {
        let params = (0..param_count)
            .map(|i| ParamDecl {
                name: SmolStr::new(format!("p{i}")),
                ty: PropertyType::Any,
                default: None,
            })
            .collect();
        MinimalSchema {
            labels: Vec::new(),
            rel_types: Vec::new(),
            functions: vec![FunctionSignature {
                name: SmolStr::new(name),
                params,
                variadic: None,
                return_ty: ReturnTy::Constant(PropertyType::Any),
                categories: FnCategories {
                    pure: true,
                    aggregate: false,
                    deterministic: true,
                },
            }],
            procedures: Vec::new(),
        }
    }
}

impl SchemaProvider for MinimalSchema {
    fn labels(&self) -> Vec<SmolStr> {
        self.labels.clone()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.rel_types.clone()
    }

    fn node_properties(&self, _label: &str) -> Option<Vec<PropertyDecl>> {
        None
    }

    fn relationship_properties(&self, _rel_type: &str) -> Option<Vec<PropertyDecl>> {
        None
    }

    fn relationship_endpoints(&self, _rel_type: &str) -> Vec<EndpointDecl> {
        Vec::new()
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<SmolStr> {
        None
    }

    fn function(&self, name: &str) -> Option<FunctionSignature> {
        self.functions
            .iter()
            .find(|f| f.name.as_str() == name)
            .cloned()
    }

    fn procedure(&self, name: &str) -> Option<ProcedureSignature> {
        self.procedures
            .iter()
            .find(|p| p.name.as_str() == name)
            .cloned()
    }

    fn schema_digest(&self) -> [u8; 32] {
        [0u8; 32]
    }
}

// ---------------------------------------------------------------------------
// E1001 — unresolved variable
// ---------------------------------------------------------------------------

#[test]
fn snap_e1001_unresolved_variable() {
    let src = "MATCH (n) RETURN z";
    insta::assert_snapshot!(
        "snap_e1001_unresolved_variable",
        render_diags(src, "e1001_unresolved_variable.cypher")
    );
}

// ---------------------------------------------------------------------------
// E2007 — kind mismatch: node variable in arithmetic
// ---------------------------------------------------------------------------

#[test]
fn snap_e2007_node_in_arithmetic() {
    let src = "MATCH (n) RETURN n + 1";
    insta::assert_snapshot!(
        "snap_e2007_node_in_arithmetic",
        render_diags(src, "e2007_node_in_arithmetic.cypher")
    );
}

// ---------------------------------------------------------------------------
// E2009 — arithmetic operand type mismatch
// ---------------------------------------------------------------------------

#[test]
fn snap_e2009_arithmetic_type_mismatch() {
    let src = "RETURN true + 1";
    insta::assert_snapshot!(
        "snap_e2009_arithmetic_type_mismatch",
        render_diags(src, "e2009_arithmetic_type_mismatch.cypher")
    );
}

// ---------------------------------------------------------------------------
// E2010 — string operation on non-string
// ---------------------------------------------------------------------------

#[test]
fn snap_e2010_string_op_non_string() {
    let src = "RETURN 42 STARTS WITH 'hello'";
    insta::assert_snapshot!(
        "snap_e2010_string_op_non_string",
        render_diags(src, "e2010_string_op_non_string.cypher")
    );
}

// ---------------------------------------------------------------------------
// E2011 — boolean operation on non-boolean
// ---------------------------------------------------------------------------

#[test]
fn snap_e2011_bool_op_non_bool() {
    let src = "RETURN NOT 42";
    insta::assert_snapshot!(
        "snap_e2011_bool_op_non_bool",
        render_diags(src, "e2011_bool_op_non_bool.cypher")
    );
}

// ---------------------------------------------------------------------------
// E2013 — IN operand is not a list
// ---------------------------------------------------------------------------

#[test]
fn snap_e2013_in_not_a_list() {
    let src = "RETURN 1 IN 'not-a-list'";
    insta::assert_snapshot!(
        "snap_e2013_in_not_a_list",
        render_diags(src, "e2013_in_not_a_list.cypher")
    );
}

// ---------------------------------------------------------------------------
// E3001 — unknown node label (schema-aware)
// ---------------------------------------------------------------------------

#[test]
fn snap_schema_node_label_error() {
    let src = "MATCH (n:Ghost) RETURN n";
    let schema = MinimalSchema::labels_only(&["Person"]);
    insta::assert_snapshot!(
        "snap_schema_node_label_error",
        render_diags_with_schema(src, "e3001_unknown_label.cypher", &schema)
    );
}

// ---------------------------------------------------------------------------
// E3002 — unknown relationship type (schema-aware)
// ---------------------------------------------------------------------------

#[test]
fn snap_schema_rel_type_error() {
    let src = "MATCH (a)-[r:GHOST_REL]->(b) RETURN r";
    let schema = MinimalSchema::rel_types_only(&["KNOWS"]);
    insta::assert_snapshot!(
        "snap_schema_rel_type_error",
        render_diags_with_schema(src, "e3002_unknown_rel_type.cypher", &schema)
    );
}

// ---------------------------------------------------------------------------
// E3006 — unknown function (schema-aware)
// ---------------------------------------------------------------------------

#[test]
fn snap_schema_unknown_fn_error() {
    // Only "toUpper" is registered; calling "ghost.fn" should emit E3006.
    let src = "RETURN ghost.fn('hello')";
    let schema = MinimalSchema::with_function("toUpper", 1);
    insta::assert_snapshot!(
        "snap_schema_unknown_fn_error",
        render_diags_with_schema(src, "e3006_unknown_function.cypher", &schema)
    );
}

// ---------------------------------------------------------------------------
// E3007 — function arity mismatch (schema-aware)
// ---------------------------------------------------------------------------

#[test]
fn snap_schema_fn_arity_error() {
    // "size" takes exactly 1 argument; call with 2 → E3007.
    let src = "RETURN size('hello', 'extra')";
    let schema = MinimalSchema::with_function("size", 1);
    insta::assert_snapshot!(
        "snap_schema_fn_arity_error",
        render_diags_with_schema(src, "e3007_function_arity_mismatch.cypher", &schema)
    );
}
